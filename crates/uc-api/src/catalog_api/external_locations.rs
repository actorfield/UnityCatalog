use axum::{extract::{Path, Query, State}, http::StatusCode, Extension, Json};
use std::sync::Arc;
use uc_auth::UcClaims;
use uc_db::{models::external_location::ExternalLocationRow, repos::{CredentialRepo, ExternalLocationRepo}};
use uc_errors::UcError;
use uc_openapi::catalog::{CreateExternalLocation, ExternalLocationInfo, ListExternalLocationsResponse, UpdateExternalLocation};
use uc_types::Privilege;
use uuid::Uuid;
use crate::{catalog_api::helpers::*, state::AppState};

#[derive(serde::Deserialize)]
pub struct ListParams { pub max_results: Option<i64>, pub page_token: Option<String> }

pub async fn create(State(state): State<AppState>, Extension(claims): Extension<Arc<UcClaims>>, Json(req): Json<CreateExternalLocation>) -> Result<Json<ExternalLocationInfo>, UcError> {
    if state.auth_enabled {
        let user = get_user(&state, &claims.sub).await?;
        require_any(&state, user.id, state.metastore_id, &[Privilege::Owner, Privilege::CreateExternalLocation]).await?;
    }
    let cred = CredentialRepo::get_by_name(&state.pool, &req.credential_name).await?;
    let id = Uuid::new_v4();
    let row = ExternalLocationRow { id, name: req.name.clone(), url: req.url.clone(),
        comment: req.comment.clone(), owner: None, credential_id: cred.id,
        created_at: Some(now_ms()), created_by: auth_sub(&state, &claims).map(String::from),
        updated_at: None, updated_by: None };
    let created = ExternalLocationRepo::create(&state.pool, &row).await?;
    Ok(Json(to_ext_loc_info(created, &req.credential_name)))
}

pub async fn list(State(state): State<AppState>, Query(params): Query<ListParams>) -> Result<Json<ListExternalLocationsResponse>, UcError> {
    let max = params.max_results.unwrap_or(50).min(1000);
    let (rows, next_token) = ExternalLocationRepo::list(&state.pool, params.page_token.as_deref(), max).await?;
    let external_locations = rows.into_iter().map(|r| to_ext_loc_info(r, "")).collect();
    Ok(Json(ListExternalLocationsResponse { external_locations, next_page_token: next_token }))
}

pub async fn get(State(state): State<AppState>, Path(name): Path<String>) -> Result<Json<ExternalLocationInfo>, UcError> {
    let row = ExternalLocationRepo::get_by_name(&state.pool, &name).await?;
    Ok(Json(to_ext_loc_info(row, "")))
}

pub async fn update(State(state): State<AppState>, Extension(claims): Extension<Arc<UcClaims>>, Path(name): Path<String>, Json(_req): Json<UpdateExternalLocation>) -> Result<Json<ExternalLocationInfo>, UcError> {
    let row = ExternalLocationRepo::get_by_name(&state.pool, &name).await?;
    Ok(Json(to_ext_loc_info(row, "")))
}

pub async fn delete(State(state): State<AppState>, Extension(claims): Extension<Arc<UcClaims>>, Path(name): Path<String>) -> Result<StatusCode, UcError> {
    if state.auth_enabled {
        let existing = ExternalLocationRepo::get_by_name(&state.pool, &name).await?;
        let user = get_user(&state, &claims.sub).await?;
        require_any(&state, user.id, existing.id, &[Privilege::Owner]).await?;
    }
    ExternalLocationRepo::delete(&state.pool, &name).await?;
    Ok(StatusCode::OK)
}

fn to_ext_loc_info(r: ExternalLocationRow, cred_name: &str) -> ExternalLocationInfo {
    ExternalLocationInfo { id: r.id, name: r.name, url: r.url, credential_name: cred_name.to_string(),
        comment: r.comment, owner: r.owner, created_at: r.created_at, created_by: r.created_by,
        updated_at: r.updated_at, updated_by: r.updated_by }
}
