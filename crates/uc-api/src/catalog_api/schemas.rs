use axum::{extract::{Path, Query, State}, http::StatusCode, Extension, Json};
use std::sync::Arc;
use uc_auth::UcClaims;
use uc_db::repos::{CatalogRepo, PropertyRepo, SchemaRepo};
use uc_errors::UcError;
use uc_openapi::catalog::{CreateSchema, ListSchemasResponse, SchemaInfo, UpdateSchema};
use uc_types::Privilege;
use uuid::Uuid;
use crate::state::AppState;
use crate::catalog_api::helpers::*;

#[derive(serde::Deserialize)]
pub struct ListParams {
    pub catalog_name: String,
    pub max_results: Option<i64>,
    pub page_token: Option<String>,
}

pub async fn create(
    State(state): State<AppState>,
    Extension(claims): Extension<Arc<UcClaims>>,
    Json(req): Json<CreateSchema>,
) -> Result<Json<SchemaInfo>, UcError> {
    let catalog = CatalogRepo::get_by_name(&state.pool, &req.catalog_name).await?;
    if state.auth_enabled {
        let user = get_user(&state, &claims.sub).await?;
        require(&state, user.id, catalog.id, Privilege::CreateSchema).await?;
    }
    validate_sql_name(&req.name)?;
    let id = Uuid::new_v4();
    let now = chrono::Utc::now().timestamp_millis();
    let row = SchemaRepo::create(&state.pool, id, catalog.id, &req.name, req.comment.as_deref(), None,
        auth_sub(&state, &claims), req.storage_root.as_deref(), now).await?;
    if let Some(ref props) = req.properties {
        PropertyRepo::replace(&state.pool, id, "schema", props).await?;
    }
    if state.auth_enabled {
        if let Ok(user) = get_user(&state, &claims.sub).await {
            state.authorizer.grant(user.id, id, Privilege::Owner).await?;
            state.authorizer.add_hierarchy_child(catalog.id, id).await?;
        }
    }
    let props = PropertyRepo::get_for_entity(&state.pool, id, "schema").await.ok();
    Ok(Json(to_schema_info(row, catalog.name, props)))
}

pub async fn list(
    State(state): State<AppState>,
    Extension(claims): Extension<Arc<UcClaims>>,
    Query(params): Query<ListParams>,
) -> Result<Json<ListSchemasResponse>, UcError> {
    let catalog = CatalogRepo::get_by_name(&state.pool, &params.catalog_name).await?;
    let max = params.max_results.unwrap_or(50).min(1000);
    let (rows, next_token) = SchemaRepo::list(&state.pool, catalog.id, params.page_token.as_deref(), max).await?;
    // #1105: filter to only schemas the caller can see
    let principal = if state.auth_enabled {
        get_user(&state, &claims.sub).await.ok().map(|u| u.id)
    } else { None };
    let visible_ids: std::collections::HashSet<uuid::Uuid> = if state.auth_enabled {
        filter_visible(&state, principal, rows.iter().map(|r| (r.id, ())).collect(),
            uc_types::Privilege::UseSchema).await?.into_iter().collect()
    } else {
        rows.iter().map(|r| r.id).collect()
    };
    let schemas = rows.into_iter().filter(|r| visible_ids.contains(&r.id))
        .map(|r| to_schema_info(r, catalog.name.clone(), None)).collect();
    Ok(Json(ListSchemasResponse { schemas, next_page_token: next_token }))
}

pub async fn get(
    State(state): State<AppState>,
    Path(full_name): Path<String>,
) -> Result<Json<SchemaInfo>, UcError> {
    let (cat, sch) = split2(&full_name)?;
    let row = SchemaRepo::get_by_full_name(&state.pool, cat, sch).await?;
    let props = PropertyRepo::get_for_entity(&state.pool, row.id, "schema").await.ok();
    Ok(Json(to_schema_info(row, cat.to_string(), props)))
}

pub async fn update(
    State(state): State<AppState>,
    Extension(claims): Extension<Arc<UcClaims>>,
    Path(full_name): Path<String>,
    Json(req): Json<UpdateSchema>,
) -> Result<Json<SchemaInfo>, UcError> {
    let (cat, sch) = split2(&full_name)?;
    let existing = SchemaRepo::get_by_full_name(&state.pool, cat, sch).await?;
    if state.auth_enabled {
        let user = get_user(&state, &claims.sub).await?;
        require(&state, user.id, existing.id, Privilege::Owner).await?;
    }
    let now = chrono::Utc::now().timestamp_millis();
    let row = SchemaRepo::update(&state.pool, existing.id, req.new_name.as_deref(),
        req.comment.as_deref(), req.owner.as_deref(), auth_sub(&state, &claims), now).await?;
    if let Some(ref props) = req.properties {
        if !props.is_empty() {
        PropertyRepo::replace(&state.pool, row.id, "schema", props).await?;
        }
    }
    let props = PropertyRepo::get_for_entity(&state.pool, row.id, "schema").await.ok();
    Ok(Json(to_schema_info(row, cat.to_string(), props)))
}

#[derive(serde::Deserialize)]
pub struct DeleteParams { pub force: Option<bool> }

pub async fn delete(
    State(state): State<AppState>,
    Extension(claims): Extension<Arc<UcClaims>>,
    Path(full_name): Path<String>,
    Query(params): Query<DeleteParams>,
) -> Result<StatusCode, UcError> {
    let (cat, sch) = split2(&full_name)?;
    let existing = SchemaRepo::get_by_full_name(&state.pool, cat, sch).await?;
    if state.auth_enabled {
        let user = get_user(&state, &claims.sub).await?;
        require(&state, user.id, existing.id, Privilege::Owner).await?;
    }

    let force = params.force.unwrap_or(false);

    // Check for child objects before deletion
    use uc_db::repos::{TableRepo, VolumeRepo, FunctionRepo, ModelRepo};
    let (tables, _) = TableRepo::list(&state.pool, existing.id, None, 1).await?;
    let (volumes, _) = VolumeRepo::list(&state.pool, existing.id, None, 1).await?;
    let (funcs, _) = FunctionRepo::list(&state.pool, existing.id, None, 1).await?;
    let (models, _) = ModelRepo::list_models(&state.pool, existing.id, None, 1).await?;

    let has_children = !tables.is_empty() || !volumes.is_empty() || !funcs.is_empty() || !models.is_empty();
    if has_children {
        if !force {
            return Err(UcError::new(
                uc_errors::ErrorCode::FailedPrecondition,
                format!("Schema '{}' is not empty. Use force=true to force deletion.", full_name),
            ));
        }
        // force=true: delete all children
        let (all_tables, _) = TableRepo::list(&state.pool, existing.id, None, 10000).await?;
        for t in all_tables {
            TableRepo::delete_columns(&state.pool, t.id).await?;
            PropertyRepo::delete_for_entity(&state.pool, t.id, "table").await?;
            TableRepo::delete(&state.pool, t.id).await?;
        }
        let (all_volumes, _) = VolumeRepo::list(&state.pool, existing.id, None, 10000).await?;
        for v in all_volumes { VolumeRepo::delete(&state.pool, v.id).await?; }
        let (all_funcs, _) = FunctionRepo::list(&state.pool, existing.id, None, 10000).await?;
        for f in all_funcs { FunctionRepo::delete(&state.pool, f.id).await?; }
        let (all_models, _) = ModelRepo::list_models(&state.pool, existing.id, None, 10000).await?;
        for m in all_models { ModelRepo::delete_model(&state.pool, m.id).await?; }
    }

    PropertyRepo::delete_for_entity(&state.pool, existing.id, "schema").await?;
    state.authorizer.remove_hierarchy_children(existing.id).await?;
    SchemaRepo::delete(&state.pool, existing.id).await?;
    Ok(StatusCode::OK)
}

fn to_schema_info(r: uc_db::models::schema::SchemaRow, catalog_name: String, props: Option<std::collections::HashMap<String,String>>) -> SchemaInfo {
    let full_name = format!("{}.{}", catalog_name, r.name);
    SchemaInfo { name: r.name, catalog_name, comment: r.comment, properties: props,
        full_name: Some(full_name), owner: r.owner, created_at: Some(r.created_at),
        created_by: r.created_by, updated_at: r.updated_at, updated_by: r.updated_by,
        schema_id: Some(r.id), storage_root: r.storage_root, storage_location: r.storage_location }
}
