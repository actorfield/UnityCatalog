use axum::{extract::{Path, Query, State}, http::StatusCode, Extension, Json};
use std::sync::Arc;
use uc_auth::UcClaims;
use uc_db::repos::UserRepo;
use uc_errors::UcError;
use uc_openapi::control::{UserResource, UserResourceList};
use uuid::Uuid;
use crate::state::AppState;

#[derive(serde::Deserialize)]
pub struct ListParams { pub filter: Option<String>, pub start_index: Option<i32>, pub count: Option<i32> }

pub async fn create_user(State(state): State<AppState>, Json(req): Json<UserResource>) -> Result<Json<UserResource>, UcError> {
    let id = Uuid::new_v4(); let now = chrono::Utc::now().timestamp_millis();
    let email = req.emails.as_ref().and_then(|e| e.first()).map(|e| e.value.as_str());
    let row = UserRepo::create(&state.pool, id, &req.user_name, email, req.external_id.as_deref(),
        if req.active.unwrap_or(true) { "ENABLED" } else { "DISABLED" }, now).await?;
    Ok(Json(UserResource { id: Some(row.id.to_string()), user_name: row.name.clone(),
        display_name: None, emails: None, name: None, active: Some(row.is_enabled()),
        external_id: row.external_id }))
}

pub async fn list_users(State(state): State<AppState>, Query(params): Query<ListParams>) -> Result<Json<UserResourceList>, UcError> {
    let max = params.count.unwrap_or(50) as i64;
    let (rows, _) = UserRepo::list(&state.pool, None, max).await?;
    let resources = rows.into_iter().map(|r| UserResource {
        id: Some(r.id.to_string()), user_name: r.name.clone(), display_name: None, emails: None,
        name: None, active: Some(r.is_enabled()), external_id: r.external_id,
    }).collect::<Vec<_>>();
    let total = resources.len() as i32;
    Ok(Json(UserResourceList {
        resources, total_results: total, start_index: 1, items_per_page: total,
        schemas: vec!["urn:ietf:params:scim:api:messages:2.0:ListResponse".to_string()],
    }))
}

pub async fn get_user(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<UserResource>, UcError> {
    let uid: Uuid = id.parse().map_err(|_| UcError::invalid_argument("invalid user id"))?;
    let row = UserRepo::get_by_id(&state.pool, uid).await?;
    Ok(Json(UserResource { id: Some(row.id.to_string()), user_name: row.name.clone(), display_name: None,
        emails: None, name: None, active: Some(row.is_enabled()), external_id: row.external_id }))
}

pub async fn update_user(State(state): State<AppState>, Path(id): Path<String>, Json(req): Json<UserResource>) -> Result<Json<UserResource>, UcError> {
    let uid: Uuid = id.parse().map_err(|_| UcError::invalid_argument("invalid user id"))?;
    let state_str = if req.active.unwrap_or(true) { "ENABLED" } else { "DISABLED" };
    let row = UserRepo::update(&state.pool, uid, Some(&req.user_name), None, Some(state_str), chrono::Utc::now().timestamp_millis()).await?;
    Ok(Json(UserResource { id: Some(row.id.to_string()), user_name: row.name.clone(), display_name: None,
        emails: None, name: None, active: Some(row.is_enabled()), external_id: row.external_id }))
}

pub async fn delete_user(State(state): State<AppState>, Path(id): Path<String>) -> Result<StatusCode, UcError> {
    let uid: Uuid = id.parse().map_err(|_| UcError::invalid_argument("invalid user id"))?;
    UserRepo::delete(&state.pool, uid).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn patch_user(State(state): State<AppState>, Path(id): Path<String>) -> Result<StatusCode, UcError> {
    Ok(StatusCode::OK)
}

pub async fn get_me(State(state): State<AppState>, Extension(claims): Extension<Arc<UcClaims>>) -> Result<Json<UserResource>, UcError> {
    // Try DB lookup first; if no-auth mode and user doesn't exist, return synthetic response
    match UserRepo::get_by_email(&state.pool, &claims.sub).await? {
        Some(user) => Ok(Json(UserResource {
            id: Some(user.id.to_string()), user_name: user.name.clone(), display_name: None,
            emails: None, name: None, active: Some(user.is_enabled()), external_id: user.external_id,
        })),
        None if !state.auth_enabled => Ok(Json(UserResource {
            id: None, user_name: claims.sub.clone(), display_name: None,
            emails: None, name: None, active: Some(true), external_id: None,
        })),
        None => Err(UcError::not_found("User", &claims.sub)),
    }
}

pub async fn patch_me(State(_state): State<AppState>) -> StatusCode { StatusCode::OK }
