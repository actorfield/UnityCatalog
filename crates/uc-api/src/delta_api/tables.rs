use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use uc_db::{
    models::staging::StagingTableRow,
    repos::{schema, staging, table},
};
use uc_errors::UcError;
use uc_openapi::delta::{
    DeltaCreateStagingTableRequest, DeltaCreateTableRequest, DeltaLoadTableResponse,
    DeltaStagingTableResponse, DeltaTableMetadata, DeltaUpdateTableRequest,
};
use uuid::Uuid;

pub async fn create_staging_table(
    State(state): State<AppState>,
    Path((catalog, schema)): Path<(String, String)>,
    Json(req): Json<DeltaCreateStagingTableRequest>,
) -> Result<Json<DeltaStagingTableResponse>, UcError> {
    let schema_row = schema::get_by_full_name(&state.pool, &catalog, &schema).await?;
    let id = Uuid::now_v7();
    let now = chrono::Utc::now().timestamp_millis();
    let loc = format!("file:///tmp/uc/staging/{}", id);
    let row = StagingTableRow {
        id,
        schema_id: schema_row.id,
        name: req.name,
        staging_location: loc.clone(),
        created_at: now,
        created_by: None,
        accessed_at: now,
        stage_committed: false,
        stage_committed_at: None,
        purge_state: 0,
        num_cleanup_retries: 0,
        last_cleanup_at: None,
    };
    staging::create(&state.pool, &row).await?;
    Ok(Json(DeltaStagingTableResponse {
        table_id: id,
        table_type: Some("MANAGED".to_string()),
        location: Some(loc),
        storage_credentials: None,
        required_protocol: None,
        suggested_protocol: None,
        required_properties: None,
        suggested_properties: None,
    }))
}

pub async fn create_table(
    State(state): State<AppState>,
    Path((catalog, schema)): Path<(String, String)>,
    Json(req): Json<DeltaCreateTableRequest>,
) -> Result<Json<DeltaLoadTableResponse>, UcError> {
    let schema_row = schema::get_by_full_name(&state.pool, &catalog, &schema).await?;
    let id = Uuid::now_v7();
    let now = chrono::Utc::now().timestamp_millis();
    let table_type = req
        .table_type
        .as_deref()
        .unwrap_or("EXTERNAL")
        .to_uppercase();
    let row = uc_db::models::table::TableRow {
        id,
        schema_id: schema_row.id,
        name: req.name.clone(),
        r#type: table_type,
        owner: None,
        created_at: now,
        created_by: None,
        updated_at: None,
        updated_by: None,
        data_source_format: Some("DELTA".into()),
        comment: None,
        url: req.location.clone(),
        column_count: req.columns.as_ref().map(|c| i32::try_from(c.fields.len()).unwrap_or(i32::MAX)),
        view_definition: None,
        uniform_iceberg_metadata_location: None,
        uniform_iceberg_converted_delta_version: None,
        uniform_iceberg_converted_delta_timestamp: None,
    };
    table::create(&state.pool, &row).await?;
    let metadata = build_metadata(id, &req);
    Ok(Json(DeltaLoadTableResponse {
        metadata,
        commits: None,
        uniform: None,
        latest_table_version: Some(0),
    }))
}

pub async fn load_table(
    State(state): State<AppState>,
    Path((catalog, schema, table)): Path<(String, String, String)>,
) -> Result<Json<DeltaLoadTableResponse>, UcError> {
    let schema_row = schema::get_by_full_name(&state.pool, &catalog, &schema).await?;
    let row = table::get_by_schema_and_name(&state.pool, schema_row.id, &table).await?;
    // A table with no commits reports version 0, matching what `create` returns
    // just above — otherwise creating a table and immediately loading it gives
    // two different versions.
    //
    // Under SQLite this was accidental: `SELECT MAX(commit_version)` over no
    // rows decoded as 0, so the `-1` branch was unreachable here and the
    // inconsistency never showed. The log store returns None honestly, which is
    // what surfaced it.
    let latest = uc_db::repos::delta::latest_version(&state.pool, row.id)
        .await?
        .unwrap_or(0);
    let metadata = DeltaTableMetadata {
        etag: Some(row.id.to_string()),
        table_type: Some(row.r#type.clone()),
        table_uuid: Some(row.id),
        location: row.url.clone(),
        created_time: Some(row.created_at),
        updated_time: row.updated_at,
        columns: None,
        partition_columns: None,
        properties: None,
        last_commit_version: Some(latest),
        last_commit_timestamp_ms: None,
    };
    Ok(Json(DeltaLoadTableResponse {
        metadata,
        commits: None,
        uniform: None,
        latest_table_version: Some(latest),
    }))
}

pub async fn table_exists(
    State(state): State<AppState>,
    Path((catalog, schema, table)): Path<(String, String, String)>,
) -> Result<StatusCode, UcError> {
    let schema_row = schema::get_by_full_name(&state.pool, &catalog, &schema).await?;
    table::get_by_schema_and_name(&state.pool, schema_row.id, &table).await?;
    Ok(StatusCode::OK)
}

pub async fn update_table(
    State(state): State<AppState>,
    Path((catalog, schema, table)): Path<(String, String, String)>,
    Json(req): Json<DeltaUpdateTableRequest>,
) -> Result<Json<DeltaLoadTableResponse>, UcError> {
    let schema_row = schema::get_by_full_name(&state.pool, &catalog, &schema).await?;
    let row = table::get_by_schema_and_name(&state.pool, schema_row.id, &table).await?;
    let now = chrono::Utc::now().timestamp_millis();
    // -1 here, unlike the load path above: this is the *last committed*
    // version, so an empty table must read as "one before zero" for the next
    // commit to land at 0. SQLite's MAX-over-nothing returned 0 and would have
    // pushed the first commit to version 1.
    let mut latest = uc_db::repos::delta::latest_version(&state.pool, row.id)
        .await?
        .unwrap_or(-1);

    // Validate DeltaTableRequirement assertions before applying updates
    if let Some(ref requirements) = req.requirements {
        for requirement in requirements {
            use uc_openapi::delta::DeltaTableRequirement;
            match requirement {
                DeltaTableRequirement::AssertTableUuid { uuid } => {
                    if *uuid != row.id {
                        return Err(UcError::new(
                            uc_errors::ErrorCode::UpdateRequirementConflict,
                            format!(
                                "assert-table-uuid failed: expected {} but got {}",
                                uuid, row.id
                            ),
                        ));
                    }
                }
                DeltaTableRequirement::AssertEtag { etag } => {
                    // Our etag is the table UUID string
                    if etag != &row.id.to_string() {
                        return Err(UcError::new(
                            uc_errors::ErrorCode::UpdateRequirementConflict,
                            format!(
                                "assert-etag failed: expected {} but current etag is {}",
                                etag, row.id
                            ),
                        ));
                    }
                }
            }
        }
    }

    // Process all CCv2 update types
    for update in &req.updates {
        use uc_openapi::delta::DeltaTableUpdate;
        match update {
            DeltaTableUpdate::AddCommit { commit, .. } => {
                if commit.version <= latest {
                    return Err(UcError::new(
                        uc_errors::ErrorCode::CommitVersionConflict,
                        format!(
                            "Commit version {} already exists (latest: {})",
                            commit.version, latest
                        ),
                    ));
                }
                let commit_row = uc_db::models::delta::DeltaCommitRow {
                    id: Uuid::now_v7(),
                    table_id: row.id,
                    commit_version: commit.version,
                    commit_filename: commit.file_name.clone(),
                    commit_filesize: commit.file_size,
                    commit_file_modification_timestamp: commit.file_modification_timestamp,
                    commit_timestamp: commit.timestamp,
                    is_backfilled_latest_commit: false,
                };
                uc_db::repos::delta::insert(&state.pool, &commit_row).await?;
                latest = commit.version;
            }
            DeltaTableUpdate::SetProperties { updates } => {
                uc_db::repos::property::replace(&state.pool, row.id, "table", updates).await?;
            }
            DeltaTableUpdate::RemoveProperties { removals } => {
                for key in removals {
                    uc_db::repos::property::delete_key(&state.pool, row.id, "table", key).await?;
                }
            }
            DeltaTableUpdate::SetColumns { columns } => {
                // Persist the new column schema as JSON in uc_columns
                let col_json = serde_json::to_string(columns).unwrap_or_default();
                table::patch(
                    &state.pool,
                    row.id,
                    Some(i32::try_from(columns.fields.len()).unwrap_or(i32::MAX)),
                    None,
                    None,
                    None,
                    now,
                )
                .await?;
                // Store schema JSON as a property for retrieval
                uc_db::repos::property::set(
                    &state.pool,
                    row.id,
                    "table",
                    "__delta_schema__",
                    &col_json,
                )
                .await?;
            }
            DeltaTableUpdate::SetTableComment { comment } => {
                table::patch(&state.pool, row.id, None, Some(comment), None, None, now).await?;
            }
            DeltaTableUpdate::SetPartitionColumns { partition_columns } => {
                let json = serde_json::to_string(partition_columns).unwrap_or_default();
                uc_db::repos::property::set(
                    &state.pool,
                    row.id,
                    "table",
                    "__delta_partition_cols__",
                    &json,
                )
                .await?;
            }
            DeltaTableUpdate::SetProtocol { protocol } => {
                // Store protocol as properties
                uc_db::repos::property::set(
                    &state.pool,
                    row.id,
                    "table",
                    "delta.minReaderVersion",
                    &protocol.min_reader_version.to_string(),
                )
                .await?;
                uc_db::repos::property::set(
                    &state.pool,
                    row.id,
                    "table",
                    "delta.minWriterVersion",
                    &protocol.min_writer_version.to_string(),
                )
                .await?;
            }
            DeltaTableUpdate::SetDomainMetadata { updates } => {
                let json = serde_json::to_string(updates).unwrap_or_default();
                uc_db::repos::property::set(
                    &state.pool,
                    row.id,
                    "table",
                    "__delta_domain_metadata__",
                    &json,
                )
                .await?;
            }
            DeltaTableUpdate::RemoveDomainMetadata { domains } => {
                for domain in domains {
                    uc_db::repos::property::delete_key(
                        &state.pool,
                        row.id,
                        "table",
                        &format!("__delta_domain__{}", domain),
                    )
                    .await?;
                }
            }
            DeltaTableUpdate::SetLatestBackfilledVersion {
                latest_published_version,
            } => {
                // Mark the commit at this version as the backfilled latest
                uc_db::repos::delta::mark_backfilled(&state.pool, row.id, *latest_published_version)
                    .await?;
            }
            DeltaTableUpdate::UpdateMetadataSnapshotVersion {
                last_commit_version,
                last_commit_timestamp_ms,
            } => {
                // Update the table's metadata snapshot version tracking
                table::patch(
                    &state.pool,
                    row.id,
                    None,
                    None,
                    Some(*last_commit_version),
                    Some(*last_commit_timestamp_ms),
                    now,
                )
                .await?;
            }
        }
    }

    let metadata = DeltaTableMetadata {
        etag: Some(row.id.to_string()),
        table_type: Some(row.r#type),
        table_uuid: Some(row.id),
        location: row.url,
        created_time: Some(row.created_at),
        updated_time: Some(now),
        columns: None,
        partition_columns: None,
        properties: None,
        last_commit_version: Some(latest),
        last_commit_timestamp_ms: None,
    };
    Ok(Json(DeltaLoadTableResponse {
        metadata,
        commits: None,
        uniform: None,
        latest_table_version: Some(latest),
    }))
}

pub async fn delete_table(
    State(state): State<AppState>,
    Path((catalog, schema, table)): Path<(String, String, String)>,
) -> Result<StatusCode, UcError> {
    let schema_row = schema::get_by_full_name(&state.pool, &catalog, &schema).await?;
    let row = table::get_by_schema_and_name(&state.pool, schema_row.id, &table).await?;
    table::delete_columns(&state.pool, row.id).await?;
    uc_db::repos::property::delete_for_entity(&state.pool, row.id, "table").await?;
    table::delete(&state.pool, row.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn rename_table(
    State(state): State<AppState>,
    Path((catalog, schema, table)): Path<(String, String, String)>,
    Json(req): Json<uc_openapi::delta::DeltaRenameTableRequest>,
) -> Result<Json<DeltaLoadTableResponse>, UcError> {
    let schema_row = schema::get_by_full_name(&state.pool, &catalog, &schema).await?;
    let row = table::get_by_schema_and_name(&state.pool, schema_row.id, &table).await?;
    let now = chrono::Utc::now().timestamp_millis();
    table::rename(&state.pool, row.id, &req.new_name, now).await?;
    let updated = table::get_by_id(&state.pool, row.id).await?;
    let metadata = DeltaTableMetadata {
        etag: Some(updated.id.to_string()),
        table_type: Some(updated.r#type),
        table_uuid: Some(updated.id),
        location: updated.url,
        created_time: Some(updated.created_at),
        updated_time: Some(now),
        columns: None,
        partition_columns: None,
        properties: None,
        last_commit_version: None,
        last_commit_timestamp_ms: None,
    };
    Ok(Json(DeltaLoadTableResponse {
        metadata,
        commits: None,
        uniform: None,
        latest_table_version: None,
    }))
}

pub async fn report_metrics(
    State(_state): State<AppState>,
    Path(_p): Path<(String, String, String)>,
) -> StatusCode {
    StatusCode::OK
}

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
