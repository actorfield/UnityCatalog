use axum::{extract::{Path, State}, http::StatusCode, Json};
use uc_db::{models::staging::StagingTableRow, repos::{SchemaRepo, StagingTableRepo, TableRepo}};
use uc_errors::UcError;
use uc_openapi::delta::{
    DeltaCreateStagingTableRequest, DeltaCreateTableRequest, DeltaLoadTableResponse, DeltaStagingTableResponse,
    DeltaTableMetadata, DeltaUpdateTableRequest,
};
use uuid::Uuid;
use crate::state::AppState;

pub async fn create_staging_table(
    State(state): State<AppState>,
    Path((catalog, schema)): Path<(String, String)>,
    Json(req): Json<DeltaCreateStagingTableRequest>,
) -> Result<Json<DeltaStagingTableResponse>, UcError> {
    let schema_row = SchemaRepo::get_by_full_name(&state.pool, &catalog, &schema).await?;
    let id = Uuid::new_v4();
    let now = chrono::Utc::now().timestamp_millis();
    let loc = format!("file:///tmp/uc/staging/{}", id);
    let row = StagingTableRow { id, schema_id: schema_row.id, name: req.name, staging_location: loc.clone(),
        created_at: now, created_by: None, accessed_at: now, stage_committed: false,
        stage_committed_at: None, purge_state: 0, num_cleanup_retries: 0, last_cleanup_at: None };
    StagingTableRepo::create(&state.pool, &row).await?;
    Ok(Json(DeltaStagingTableResponse {
        table_id: id, table_type: Some("MANAGED".to_string()), location: Some(loc),
        storage_credentials: None, required_protocol: None, suggested_protocol: None,
        required_properties: None, suggested_properties: None,
    }))
}

pub async fn create_table(
    State(state): State<AppState>,
    Path((catalog, schema)): Path<(String, String)>,
    Json(req): Json<DeltaCreateTableRequest>,
) -> Result<Json<DeltaLoadTableResponse>, UcError> {
    let schema_row = SchemaRepo::get_by_full_name(&state.pool, &catalog, &schema).await?;
    let id = Uuid::new_v4();
    let now = chrono::Utc::now().timestamp_millis();
    let table_type = req.table_type.as_deref().unwrap_or("EXTERNAL").to_uppercase();
    let row = uc_db::models::table::TableRow {
        id, schema_id: schema_row.id, name: req.name.clone(), r#type: table_type,
        owner: None, created_at: now, created_by: None, updated_at: None, updated_by: None,
        data_source_format: Some("DELTA".into()), comment: None, url: req.location.clone(),
        column_count: req.columns.as_ref().map(|c| c.fields.len() as i32),
        view_definition: None, uniform_iceberg_metadata_location: None,
        uniform_iceberg_converted_delta_version: None, uniform_iceberg_converted_delta_timestamp: None,
    };
    TableRepo::create(&state.pool, &row).await?;
    let metadata = build_metadata(id, &req);
    Ok(Json(DeltaLoadTableResponse { metadata, commits: None, uniform: None, latest_table_version: Some(0) }))
}

pub async fn load_table(
    State(state): State<AppState>,
    Path((catalog, schema, table)): Path<(String, String, String)>,
) -> Result<Json<DeltaLoadTableResponse>, UcError> {
    let schema_row = SchemaRepo::get_by_full_name(&state.pool, &catalog, &schema).await?;
    let row = TableRepo::get_by_schema_and_name(&state.pool, schema_row.id, &table).await?;
    let latest = uc_db::repos::DeltaCommitRepo::latest_version(&state.pool, row.id).await?.unwrap_or(-1);
    let metadata = DeltaTableMetadata {
        etag: Some(row.id.to_string()), table_type: Some(row.r#type.clone()),
        table_uuid: Some(row.id), location: row.url.clone(), created_time: Some(row.created_at),
        updated_time: row.updated_at, columns: None, partition_columns: None,
        properties: None, last_commit_version: Some(latest), last_commit_timestamp_ms: None,
    };
    Ok(Json(DeltaLoadTableResponse { metadata, commits: None, uniform: None, latest_table_version: Some(latest) }))
}

pub async fn table_exists(
    State(state): State<AppState>,
    Path((catalog, schema, table)): Path<(String, String, String)>,
) -> Result<StatusCode, UcError> {
    let schema_row = SchemaRepo::get_by_full_name(&state.pool, &catalog, &schema).await?;
    TableRepo::get_by_schema_and_name(&state.pool, schema_row.id, &table).await?;
    Ok(StatusCode::OK)
}

pub async fn update_table(
    State(state): State<AppState>,
    Path((catalog, schema, table)): Path<(String, String, String)>,
    Json(req): Json<DeltaUpdateTableRequest>,
) -> Result<Json<DeltaLoadTableResponse>, UcError> {
    let schema_row = SchemaRepo::get_by_full_name(&state.pool, &catalog, &schema).await?;
    let row = TableRepo::get_by_schema_and_name(&state.pool, schema_row.id, &table).await?;
    let now = chrono::Utc::now().timestamp_millis();
    let mut latest = uc_db::repos::DeltaCommitRepo::latest_version(&state.pool, row.id).await?.unwrap_or(-1);

    // Process all CCv2 update types
    for update in &req.updates {
        use uc_openapi::delta::DeltaTableUpdate;
        match update {
            DeltaTableUpdate::AddCommit { commit, .. } => {
                if commit.version <= latest {
                    return Err(uc_errors::UcError::new(
                        uc_errors::ErrorCode::CommitVersionConflict,
                        format!("Commit version {} already exists (latest: {})", commit.version, latest),
                    ));
                }
                let commit_row = uc_db::models::delta::DeltaCommitRow {
                    id: Uuid::new_v4(), table_id: row.id, commit_version: commit.version,
                    commit_filename: commit.file_name.clone(), commit_filesize: commit.file_size,
                    commit_file_modification_timestamp: commit.file_modification_timestamp,
                    commit_timestamp: commit.timestamp, is_backfilled_latest_commit: false,
                };
                uc_db::repos::DeltaCommitRepo::insert(&state.pool, &commit_row).await?;
                latest = commit.version;
            }
            DeltaTableUpdate::SetProperties { updates } => {
                uc_db::repos::PropertyRepo::replace(&state.pool, row.id, "table", updates).await?;
            }
            DeltaTableUpdate::RemoveProperties { removals } => {
                for key in removals {
                    sqlx::query(
                        "DELETE FROM uc_properties WHERE entity_id=$1 AND entity_type='table' AND property_key=$2"
                    )
                    .bind(row.id).bind(key)
                    .execute(state.pool.as_ref()).await.map_err(crate::db_err)?;
                }
            }
            DeltaTableUpdate::SetColumns { columns } => {
                // Persist the new column schema as JSON in uc_columns
                let col_json = serde_json::to_string(columns).unwrap_or_default();
                sqlx::query(
                    "UPDATE uc_tables SET column_count=$1, updated_at=$2 WHERE id=$3"
                )
                .bind(columns.fields.len() as i32).bind(now).bind(row.id)
                .execute(state.pool.as_ref()).await.map_err(crate::db_err)?;
                // Store schema JSON as a property for retrieval
                sqlx::query(
                    "INSERT OR REPLACE INTO uc_properties (id, entity_id, entity_type, property_key, property_value) VALUES ($1,$2,'table','__delta_schema__',$3)"
                )
                .bind(Uuid::new_v4()).bind(row.id).bind(&col_json)
                .execute(state.pool.as_ref()).await.map_err(crate::db_err)?;
            }
            DeltaTableUpdate::SetTableComment { comment } => {
                sqlx::query("UPDATE uc_tables SET comment=$1, updated_at=$2 WHERE id=$3")
                    .bind(comment).bind(now).bind(row.id)
                    .execute(state.pool.as_ref()).await.map_err(crate::db_err)?;
            }
            DeltaTableUpdate::SetPartitionColumns { partition_columns } => {
                let json = serde_json::to_string(partition_columns).unwrap_or_default();
                sqlx::query(
                    "INSERT OR REPLACE INTO uc_properties (id, entity_id, entity_type, property_key, property_value) VALUES ($1,$2,'table','__delta_partition_cols__',$3)"
                )
                .bind(Uuid::new_v4()).bind(row.id).bind(&json)
                .execute(state.pool.as_ref()).await.map_err(crate::db_err)?;
            }
            DeltaTableUpdate::SetProtocol { protocol } => {
                // Store protocol as properties
                sqlx::query(
                    "INSERT OR REPLACE INTO uc_properties (id, entity_id, entity_type, property_key, property_value) VALUES ($1,$2,'table','delta.minReaderVersion',$3)"
                )
                .bind(Uuid::new_v4()).bind(row.id).bind(protocol.min_reader_version.to_string())
                .execute(state.pool.as_ref()).await.map_err(crate::db_err)?;
                sqlx::query(
                    "INSERT OR REPLACE INTO uc_properties (id, entity_id, entity_type, property_key, property_value) VALUES ($1,$2,'table','delta.minWriterVersion',$3)"
                )
                .bind(Uuid::new_v4()).bind(row.id).bind(protocol.min_writer_version.to_string())
                .execute(state.pool.as_ref()).await.map_err(crate::db_err)?;
            }
            DeltaTableUpdate::SetDomainMetadata { updates } => {
                let json = serde_json::to_string(updates).unwrap_or_default();
                sqlx::query(
                    "INSERT OR REPLACE INTO uc_properties (id, entity_id, entity_type, property_key, property_value) VALUES ($1,$2,'table','__delta_domain_metadata__',$3)"
                )
                .bind(Uuid::new_v4()).bind(row.id).bind(&json)
                .execute(state.pool.as_ref()).await.map_err(crate::db_err)?;
            }
            DeltaTableUpdate::RemoveDomainMetadata { domains } => {
                for domain in domains {
                    sqlx::query(
                        "DELETE FROM uc_properties WHERE entity_id=$1 AND entity_type='table' AND property_key=$2"
                    )
                    .bind(row.id).bind(format!("__delta_domain__{}", domain))
                    .execute(state.pool.as_ref()).await.map_err(crate::db_err)?;
                }
            }
            DeltaTableUpdate::SetLatestBackfilledVersion { latest_published_version } => {
                // Mark the commit at this version as the backfilled latest
                sqlx::query(
                    "UPDATE uc_delta_commits SET is_backfilled_latest_commit=1 WHERE table_id=$1 AND commit_version=$2"
                )
                .bind(row.id).bind(latest_published_version)
                .execute(state.pool.as_ref()).await.map_err(crate::db_err)?;
            }
            DeltaTableUpdate::UpdateMetadataSnapshotVersion { last_commit_version, last_commit_timestamp_ms } => {
                // Update the table's metadata snapshot version tracking
                sqlx::query(
                    "UPDATE uc_tables SET uniform_iceberg_converted_delta_version=$1, uniform_iceberg_converted_delta_timestamp=$2, updated_at=$3 WHERE id=$4"
                )
                .bind(last_commit_version).bind(last_commit_timestamp_ms).bind(now).bind(row.id)
                .execute(state.pool.as_ref()).await.map_err(crate::db_err)?;
            }
        }
    }

    let metadata = DeltaTableMetadata {
        etag: Some(row.id.to_string()), table_type: Some(row.r#type),
        table_uuid: Some(row.id), location: row.url, created_time: Some(row.created_at),
        updated_time: Some(now), columns: None, partition_columns: None,
        properties: None, last_commit_version: Some(latest), last_commit_timestamp_ms: None,
    };
    Ok(Json(DeltaLoadTableResponse { metadata, commits: None, uniform: None, latest_table_version: Some(latest) }))
}

pub async fn delete_table(
    State(state): State<AppState>,
    Path((catalog, schema, table)): Path<(String, String, String)>,
) -> Result<StatusCode, UcError> {
    let schema_row = SchemaRepo::get_by_full_name(&state.pool, &catalog, &schema).await?;
    let row = TableRepo::get_by_schema_and_name(&state.pool, schema_row.id, &table).await?;
    TableRepo::delete_columns(&state.pool, row.id).await?;
    uc_db::repos::PropertyRepo::delete_for_entity(&state.pool, row.id, "table").await?;
    TableRepo::delete(&state.pool, row.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn rename_table(
    State(state): State<AppState>,
    Path((catalog, schema, table)): Path<(String, String, String)>,
    Json(req): Json<uc_openapi::delta::DeltaRenameTableRequest>,
) -> Result<Json<DeltaLoadTableResponse>, UcError> {
    let schema_row = SchemaRepo::get_by_full_name(&state.pool, &catalog, &schema).await?;
    let row = TableRepo::get_by_schema_and_name(&state.pool, schema_row.id, &table).await?;
    let now = chrono::Utc::now().timestamp_millis();
    sqlx::query("UPDATE uc_tables SET name=$1, updated_at=$2 WHERE id=$3")
        .bind(&req.new_name).bind(now).bind(row.id)
        .execute(state.pool.as_ref()).await.map_err(crate::db_err)?;
    let updated = TableRepo::get_by_id(&state.pool, row.id).await?;
    let metadata = DeltaTableMetadata {
        etag: Some(updated.id.to_string()), table_type: Some(updated.r#type),
        table_uuid: Some(updated.id), location: updated.url, created_time: Some(updated.created_at),
        updated_time: Some(now), columns: None, partition_columns: None,
        properties: None, last_commit_version: None, last_commit_timestamp_ms: None,
    };
    Ok(Json(DeltaLoadTableResponse { metadata, commits: None, uniform: None, latest_table_version: None }))
}

pub async fn report_metrics(
    State(_state): State<AppState>,
    Path(_p): Path<(String, String, String)>,
) -> StatusCode { StatusCode::OK }

fn build_metadata(id: Uuid, req: &DeltaCreateTableRequest) -> DeltaTableMetadata {
    DeltaTableMetadata {
        etag: Some(id.to_string()),
        table_type: req.table_type.clone(),
        table_uuid: Some(id),
        location: req.location.clone(),
        created_time: Some(chrono::Utc::now().timestamp_millis()),
        updated_time: None,
        columns: req.columns.clone(),
        partition_columns: req.partition_columns.clone(),
        properties: req.properties.clone(),
        last_commit_version: Some(-1),
        last_commit_timestamp_ms: req.last_commit_timestamp_ms,
    }
}
