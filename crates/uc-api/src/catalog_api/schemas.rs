use axum::{extract::{Path, Query, State}, http::StatusCode, Extension, Json};
use std::sync::Arc;
use uc_auth::UcClaims;
use uc_db::repos::{CatalogRepo, PropertyRepo, SchemaRepo, UserRepo};
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
        require_any(&state, user.id, catalog.id, &[Privilege::Owner, Privilege::CreateSchema]).await?;
    }
    let id = Uuid::new_v4();
    let now = chrono::Utc::now().timestamp_millis();
    let row = SchemaRepo::create(&state.pool, id, catalog.id, &req.name, req.comment.as_deref(), None,
        auth_sub(&state, &claims), req.storage_root.as_deref(), now).await?;
    if let Some(ref props) = req.properties {
        PropertyRepo::replace(&state.pool, id, "schema", props).await?;
    }
    if state.auth_enabled {
        if let Some(user) = UserRepo::get_by_email(&state.pool, &claims.sub).await? {
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
        uc_db::repos::UserRepo::get_by_email(&state.pool, &claims.sub).await?.map(|u| u.id)
    } else { None };
    let visible_ids: std::collections::HashSet<uuid::Uuid> = if state.auth_enabled {
        filter_visible(&state, principal, rows.iter().map(|r| (r.id, ())).collect(),
            &[uc_types::Privilege::Owner, uc_types::Privilege::UseSchema]).await?.into_iter().collect()
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
        require_any(&state, user.id, existing.id, &[Privilege::Owner]).await?;
    }
    let now = chrono::Utc::now().timestamp_millis();
    let row = SchemaRepo::update(&state.pool, existing.id, req.new_name.as_deref(),
        req.comment.as_deref(), req.owner.as_deref(), auth_sub(&state, &claims), now).await?;
    if let Some(ref props) = req.properties {
        PropertyRepo::replace(&state.pool, row.id, "schema", props).await?;
    }
    let props = PropertyRepo::get_for_entity(&state.pool, row.id, "schema").await.ok();
    Ok(Json(to_schema_info(row, cat.to_string(), props)))
}

pub async fn delete(
    State(state): State<AppState>,
    Extension(claims): Extension<Arc<UcClaims>>,
    Path(full_name): Path<String>,
) -> Result<StatusCode, UcError> {
    let (cat, sch) = split2(&full_name)?;
    let existing = SchemaRepo::get_by_full_name(&state.pool, cat, sch).await?;
    if state.auth_enabled {
        let user = get_user(&state, &claims.sub).await?;
        require_any(&state, user.id, existing.id, &[Privilege::Owner]).await?;
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
