use crate::{catalog_api::helpers::*, state::AppState};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Extension, Json,
};
use std::sync::Arc;
use uc_auth::UcClaims;
use uc_db::{
    managed_storage,
    models::table::{ColumnRow, TableRow},
    repos::{property, schema, staging, table},
};
use uc_errors::{ErrorCode, UcError};
use uc_openapi::catalog::{
    ColumnInfo, ColumnTypeName, CreateTable, DataSourceFormat, ListTablesResponse, TableInfo,
    TableType,
};
use uc_types::Privilege;
use uuid::Uuid;

#[derive(serde::Deserialize)]
pub struct ListParams {
    pub catalog_name: String,
    pub schema_name: String,
    pub max_results: Option<i64>,
    pub page_token: Option<String>,
}

pub async fn create(
    State(state): State<AppState>,
    Extension(claims): Extension<Arc<UcClaims>>,
    Json(req): Json<CreateTable>,
) -> Result<Json<TableInfo>, UcError> {
    let schema = schema::get_by_full_name(&state.pool, &req.catalog_name, &req.schema_name).await?;
    if state.auth_enabled {
        let user = get_user(&state, &claims.sub).await?;
        require(&state, user.id, schema.id, Privilege::CreateTable).await?;
    }

    validate_sql_name(&req.name)?;
    let now = now_ms();
    let caller = auth_sub(&state, &claims).map(String::from);

    // ── #1143: Managed table staging commit flow ──────────────────────────────
    let (id, storage_location) = match req.table_type {
        TableType::Managed => {
            let storage_loc = req.storage_location.as_deref().unwrap_or("");

            // If caller provided a staging location, find and commit the staging table
            let (table_id, resolved_loc) = if !storage_loc.is_empty() {
                // Commit the staging table — find by location, validate caller owns it
                let staging = staging::get_by_location(&state.pool, storage_loc)
                    .await
                    .map_err(|_| {
                        UcError::new(
                            ErrorCode::NotFound,
                            format!(
                                "No staging table found at location '{}'. \
                                 Create a staging table first via POST /staging-tables.",
                                storage_loc
                            ),
                        )
                    })?;

                if staging.stage_committed {
                    return Err(UcError::new(
                        ErrorCode::FailedPrecondition,
                        format!(
                            "Staging table at '{}' has already been committed.",
                            storage_loc
                        ),
                    ));
                }

                // Validate caller owns the staging table
                if state.auth_enabled {
                    let user = get_user(&state, &claims.sub).await?;
                    if !state
                        .authorizer
                        .authorize(user.id, staging.id, Privilege::Owner)
                        .await?
                    {
                        return Err(UcError::permission_denied(
                            "Only the staging table creator can commit it into a MANAGED table",
                        ));
                    }
                }

                // Mark staging as committed — use staging UUID as table ID (Java behaviour)
                staging::mark_committed(&state.pool, staging.id, now).await?;
                (staging.id, staging.staging_location.clone())
            } else {
                // No staging location provided — auto-derive from storage_root hierarchy
                let root = managed_storage::resolve_storage_root(
                    &state.pool,
                    &req.catalog_name,
                    &req.schema_name,
                )
                .await?;
                let new_id = Uuid::new_v4();
                let loc = managed_storage::managed_table_location(&root, schema.id, new_id);
                (new_id, loc)
            };

            (table_id, Some(resolved_loc))
        }
        _ => {
            // EXTERNAL / VIEW — use provided storage_location as-is
            (Uuid::new_v4(), req.storage_location.clone())
        }
    };

    let col_count = req.columns.as_ref().map(|c| c.len() as i32);
    let row = TableRow {
        id,
        schema_id: schema.id,
        name: req.name.clone(),
        r#type: format!("{:?}", req.table_type).to_uppercase(),
        owner: None,
        created_at: now,
        created_by: caller,
        updated_at: None,
        updated_by: None,
        data_source_format: req
            .data_source_format
            .as_ref()
            .map(|f| format!("{:?}", f).to_uppercase()),
        comment: req.comment.clone(),
        url: storage_location,
        column_count: col_count,
        view_definition: req.view_definition.clone(),
        uniform_iceberg_metadata_location: None,
        uniform_iceberg_converted_delta_version: None,
        uniform_iceberg_converted_delta_timestamp: None,
    };
    let created = table::create(&state.pool, &row).await?;
    // Insert columns
    if let Some(ref cols) = req.columns {
        let col_rows: Vec<ColumnRow> = cols
            .iter()
            .enumerate()
            .map(|(i, c)| ColumnRow {
                id: Uuid::new_v4(),
                table_id: id,
                name: c.name.clone(),
                ordinal_position: i as i32,
                type_text: c.type_text.clone().unwrap_or_default(),
                // #1053: derive type_json from type_text if absent
                type_json: c.type_json.clone().unwrap_or_else(|| {
                    c.type_text
                        .as_deref()
                        .map(|t| format!(r#"{{\"type\":\"{}\"}}"#, t))
                        .unwrap_or_default()
                }),
                type_name: c
                    .type_name
                    .as_ref()
                    .map(|t| format!("{:?}", t).to_uppercase())
                    .unwrap_or_default(),
                type_precision: c.type_precision,
                type_scale: c.type_scale,
                type_interval_type: c.type_interval_type.clone(),
                nullable: c.nullable.unwrap_or(true),
                comment: c.comment.clone(),
                partition_index: c.partition_index,
            })
            .collect();
        table::insert_columns(&state.pool, &col_rows).await?;
    }
    if let Some(ref props) = req.properties {
        property::replace(&state.pool, id, "table", props).await?;
    }
    if state.auth_enabled {
        if let Ok(user) = get_user(&state, &claims.sub).await {
            state
                .authorizer
                .grant(user.id, id, Privilege::Owner)
                .await?;
            state.authorizer.add_hierarchy_child(schema.id, id).await?;
        }
    }
    let columns = table::get_columns(&state.pool, id).await.ok();
    let props = property::get_for_entity(&state.pool, id, "table")
        .await
        .ok();
    Ok(Json(to_table_info(
        created,
        &req.catalog_name,
        &req.schema_name,
        columns,
        props,
    )))
}

pub async fn list(
    State(state): State<AppState>,
    Extension(claims): Extension<Arc<UcClaims>>,
    Query(params): Query<ListParams>,
) -> Result<Json<ListTablesResponse>, UcError> {
    let schema =
        schema::get_by_full_name(&state.pool, &params.catalog_name, &params.schema_name).await?;
    let max = params.max_results.unwrap_or(50).min(1000);
    let (rows, next_token) =
        table::list(&state.pool, schema.id, params.page_token.as_deref(), max).await?;
    // #1105: filter to only tables the caller can see when auth is enabled
    let principal = if state.auth_enabled {
        get_user(&state, &claims.sub).await.ok().map(|u| u.id)
    } else {
        None
    };
    let visible_ids: std::collections::HashSet<uuid::Uuid> = if state.auth_enabled {
        crate::catalog_api::helpers::filter_visible(
            &state,
            principal,
            rows.iter().map(|r| (r.id, ())).collect(),
            uc_types::Privilege::Select,
        )
        .await?
        .into_iter()
        .collect()
    } else {
        rows.iter().map(|r| r.id).collect()
    };
    let tables = rows
        .into_iter()
        .filter(|r| visible_ids.contains(&r.id))
        .map(|r| to_table_info(r, &params.catalog_name, &params.schema_name, None, None))
        .collect();
    Ok(Json(ListTablesResponse {
        tables,
        next_page_token: next_token,
    }))
}

pub async fn get(
    State(state): State<AppState>,
    Path(full_name): Path<String>,
) -> Result<Json<TableInfo>, UcError> {
    let (cat, sch, tbl) = split3(&full_name)?;
    let schema = schema::get_by_full_name(&state.pool, cat, sch).await?;
    let row = table::get_by_schema_and_name(&state.pool, schema.id, tbl).await?;
    let columns = table::get_columns(&state.pool, row.id).await.ok();
    let props = property::get_for_entity(&state.pool, row.id, "table")
        .await
        .ok();
    Ok(Json(to_table_info(row, cat, sch, columns, props)))
}

pub async fn delete(
    State(state): State<AppState>,
    Extension(claims): Extension<Arc<UcClaims>>,
    Path(full_name): Path<String>,
) -> Result<StatusCode, UcError> {
    let (cat, sch, tbl) = split3(&full_name)?;
    let schema = schema::get_by_full_name(&state.pool, cat, sch).await?;
    let row = table::get_by_schema_and_name(&state.pool, schema.id, tbl).await?;
    if state.auth_enabled {
        let user = get_user(&state, &claims.sub).await?;
        require(&state, user.id, row.id, Privilege::Owner).await?;
    }
    table::delete_columns(&state.pool, row.id).await?;
    property::delete_for_entity(&state.pool, row.id, "table").await?;
    state.authorizer.remove_hierarchy_children(row.id).await?;
    table::delete(&state.pool, row.id).await?;
    Ok(StatusCode::OK)
}

fn parse_table_type(s: &str) -> Option<TableType> {
    match s {
        "MANAGED" => Some(TableType::Managed),
        "EXTERNAL" => Some(TableType::External),
        "STREAMING_TABLE" => Some(TableType::StreamingTable),
        "MATERIALIZED_VIEW" => Some(TableType::MaterializedView),
        _ => None,
    }
}

fn parse_format(s: &str) -> Option<DataSourceFormat> {
    match s {
        "DELTA" => Some(DataSourceFormat::Delta),
        "CSV" => Some(DataSourceFormat::Csv),
        "JSON" => Some(DataSourceFormat::Json),
        "AVRO" => Some(DataSourceFormat::Avro),
        "PARQUET" => Some(DataSourceFormat::Parquet),
        "ORC" => Some(DataSourceFormat::Orc),
        "TEXT" => Some(DataSourceFormat::Text),
        _ => None,
    }
}

fn parse_col_type(s: &str) -> Option<ColumnTypeName> {
    match s {
        "BOOLEAN" => Some(ColumnTypeName::Boolean),
        "BYTE" => Some(ColumnTypeName::Byte),
        "SHORT" => Some(ColumnTypeName::Short),
        "INT" => Some(ColumnTypeName::Int),
        "LONG" => Some(ColumnTypeName::Long),
        "FLOAT" => Some(ColumnTypeName::Float),
        "DOUBLE" => Some(ColumnTypeName::Double),
        "DATE" => Some(ColumnTypeName::Date),
        "TIMESTAMP" => Some(ColumnTypeName::Timestamp),
        "TIMESTAMP_NTZ" => Some(ColumnTypeName::TimestampNtz),
        "STRING" => Some(ColumnTypeName::String),
        "BINARY" => Some(ColumnTypeName::Binary),
        "DECIMAL" => Some(ColumnTypeName::Decimal),
        "ARRAY" => Some(ColumnTypeName::Array),
        "STRUCT" => Some(ColumnTypeName::Struct),
        "MAP" => Some(ColumnTypeName::Map),
        "NULL" => Some(ColumnTypeName::Null),
        _ => None,
    }
}

/// Normalize a storage path to a file:// URI if it is an absolute local path.
fn normalize_location(url: Option<String>) -> Option<String> {
    url.map(|u| {
        if u.starts_with('/') {
            format!("file://{}", u)
        } else {
            u
        }
    })
}

fn to_table_info(
    r: TableRow,
    cat: &str,
    sch: &str,
    cols: Option<Vec<ColumnRow>>,
    props: Option<std::collections::HashMap<String, String>>,
) -> TableInfo {
    let full_name = format!("{}.{}.{}", cat, sch, r.name);
    let columns = cols.map(|cv| {
        cv.into_iter()
            .map(|c| ColumnInfo {
                name: c.name.clone(),
                type_text: Some(c.type_text.clone()),
                type_json: Some(c.type_json.clone()),
                type_name: parse_col_type(&c.type_name),
                type_precision: c.type_precision,
                type_scale: c.type_scale,
                type_interval_type: c.type_interval_type,
                position: Some(c.ordinal_position),
                comment: c.comment,
                nullable: Some(c.nullable),
                partition_index: c.partition_index,
            })
            .collect()
    });
    let table_type = parse_table_type(&r.r#type);
    let data_source_format = r.data_source_format.as_deref().and_then(parse_format);
    let storage_location = normalize_location(r.url);
    TableInfo {
        name: r.name,
        catalog_name: cat.to_string(),
        schema_name: sch.to_string(),
        table_type,
        data_source_format,
        columns,
        storage_location,
        comment: r.comment,
        properties: props,
        owner: r.owner,
        created_at: Some(r.created_at),
        created_by: r.created_by,
        updated_at: r.updated_at,
        updated_by: r.updated_by,
        table_id: Some(r.id),
        full_name: Some(full_name),
        view_definition: r.view_definition,
    }
}
