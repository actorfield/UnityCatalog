use axum::{extract::{Path, Query, State}, http::StatusCode, Extension, Json};
use std::sync::Arc;
use uc_auth::UcClaims;
use uc_db::{models::table::{ColumnRow, TableRow}, repos::{PropertyRepo, SchemaRepo, TableRepo}};
use uc_errors::UcError;
use uc_openapi::catalog::{ColumnInfo, ColumnTypeName, CreateTable, DataSourceFormat, ListTablesResponse, TableInfo, TableType};
use uc_types::Privilege;
use uuid::Uuid;
use crate::{catalog_api::helpers::*, state::AppState};

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
    let schema = SchemaRepo::get_by_full_name(&state.pool, &req.catalog_name, &req.schema_name).await?;
    if state.auth_enabled {
        let user = get_user(&state, &claims.sub).await?;
        require_any(&state, user.id, schema.id, &[Privilege::Owner, Privilege::CreateTable]).await?;
    }
    let id = Uuid::new_v4();
    let now = now_ms();
    let col_count = req.columns.as_ref().map(|c| c.len() as i32);
    let row = TableRow {
        id, schema_id: schema.id, name: req.name.clone(),
        r#type: format!("{:?}", req.table_type).to_uppercase(),
        owner: None, created_at: now, created_by: auth_sub(&state, &claims).map(String::from),
        updated_at: None, updated_by: None,
        data_source_format: req.data_source_format.as_ref().map(|f| format!("{:?}", f).to_uppercase()),
        comment: req.comment.clone(), url: req.storage_location.clone(),
        column_count: col_count, view_definition: req.view_definition.clone(),
        uniform_iceberg_metadata_location: None,
        uniform_iceberg_converted_delta_version: None,
        uniform_iceberg_converted_delta_timestamp: None,
    };
    let created = TableRepo::create(&state.pool, &row).await?;
    // Insert columns
    if let Some(ref cols) = req.columns {
        let col_rows: Vec<ColumnRow> = cols.iter().enumerate().map(|(i, c)| ColumnRow {
            id: Uuid::new_v4(), table_id: id,
            name: c.name.clone(), ordinal_position: i as i32,
            type_text: c.type_text.clone().unwrap_or_default(),
            type_json: c.type_json.clone().unwrap_or_default(),
            type_name: c.type_name.as_ref().map(|t| format!("{:?}", t).to_uppercase()).unwrap_or_default(),
            type_precision: c.type_precision, type_scale: c.type_scale,
            type_interval_type: c.type_interval_type.clone(),
            nullable: c.nullable.unwrap_or(true),
            comment: c.comment.clone(), partition_index: c.partition_index,
        }).collect();
        TableRepo::insert_columns(&state.pool, &col_rows).await?;
    }
    if let Some(ref props) = req.properties {
        PropertyRepo::replace(&state.pool, id, "table", props).await?;
    }
    if state.auth_enabled {
        if let Some(user) = uc_db::repos::UserRepo::get_by_email(&state.pool, &claims.sub).await? {
            state.authorizer.grant(user.id, id, Privilege::Owner).await?;
            state.authorizer.add_hierarchy_child(schema.id, id).await?;
        }
    }
    let columns = TableRepo::get_columns(&state.pool, id).await.ok();
    let props = PropertyRepo::get_for_entity(&state.pool, id, "table").await.ok();
    Ok(Json(to_table_info(created, &req.catalog_name, &req.schema_name, columns, props)))
}

pub async fn list(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> Result<Json<ListTablesResponse>, UcError> {
    let schema = SchemaRepo::get_by_full_name(&state.pool, &params.catalog_name, &params.schema_name).await?;
    let max = params.max_results.unwrap_or(50).min(1000);
    let (rows, next_token) = TableRepo::list(&state.pool, schema.id, params.page_token.as_deref(), max).await?;
    let tables = rows.into_iter().map(|r| to_table_info(r, &params.catalog_name, &params.schema_name, None, None)).collect();
    Ok(Json(ListTablesResponse { tables, next_page_token: next_token }))
}

pub async fn get(
    State(state): State<AppState>,
    Path(full_name): Path<String>,
) -> Result<Json<TableInfo>, UcError> {
    let (cat, sch, tbl) = split3(&full_name)?;
    let schema = SchemaRepo::get_by_full_name(&state.pool, cat, sch).await?;
    let row = TableRepo::get_by_schema_and_name(&state.pool, schema.id, tbl).await?;
    let columns = TableRepo::get_columns(&state.pool, row.id).await.ok();
    let props = PropertyRepo::get_for_entity(&state.pool, row.id, "table").await.ok();
    Ok(Json(to_table_info(row, cat, sch, columns, props)))
}

pub async fn delete(
    State(state): State<AppState>,
    Extension(claims): Extension<Arc<UcClaims>>,
    Path(full_name): Path<String>,
) -> Result<StatusCode, UcError> {
    let (cat, sch, tbl) = split3(&full_name)?;
    let schema = SchemaRepo::get_by_full_name(&state.pool, cat, sch).await?;
    let row = TableRepo::get_by_schema_and_name(&state.pool, schema.id, tbl).await?;
    if state.auth_enabled {
        let user = get_user(&state, &claims.sub).await?;
        require_any(&state, user.id, row.id, &[Privilege::Owner]).await?;
    }
    TableRepo::delete_columns(&state.pool, row.id).await?;
    PropertyRepo::delete_for_entity(&state.pool, row.id, "table").await?;
    state.authorizer.remove_hierarchy_children(row.id).await?;
    TableRepo::delete(&state.pool, row.id).await?;
    Ok(StatusCode::OK)
}

fn to_table_info(r: TableRow, cat: &str, sch: &str, cols: Option<Vec<ColumnRow>>, props: Option<std::collections::HashMap<String,String>>) -> TableInfo {
    let full_name = format!("{}.{}.{}", cat, sch, r.name);
    let columns = cols.map(|cv| cv.into_iter().map(|c| ColumnInfo {
        name: c.name, type_text: Some(c.type_text), type_json: Some(c.type_json),
        type_name: None, type_precision: c.type_precision, type_scale: c.type_scale,
        type_interval_type: c.type_interval_type, position: Some(c.ordinal_position),
        comment: c.comment, nullable: Some(c.nullable), partition_index: c.partition_index,
    }).collect());
    TableInfo { name: r.name, catalog_name: cat.to_string(), schema_name: sch.to_string(),
        table_type: None, data_source_format: None, columns, storage_location: r.url,
        comment: r.comment, properties: props, owner: r.owner, created_at: Some(r.created_at),
        created_by: r.created_by, updated_at: r.updated_at, updated_by: r.updated_by,
        table_id: Some(r.id), full_name: Some(full_name), view_definition: r.view_definition }
}
