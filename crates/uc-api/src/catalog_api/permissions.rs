use axum::{extract::{Path, State}, Json};
use uc_errors::UcError;
use uc_openapi::catalog::{PermissionsList, UpdatePermissions};
use crate::state::AppState;

pub async fn get(State(_state): State<AppState>, Path((_securable_type, _full_name)): Path<(String, String)>) -> Result<Json<PermissionsList>, UcError> {
    Ok(Json(PermissionsList { securable_type: None, full_name: None, privilege_assignments: vec![] }))
}

pub async fn update(State(_state): State<AppState>, Path((_securable_type, _full_name)): Path<(String, String)>, Json(_req): Json<UpdatePermissions>) -> Result<Json<PermissionsList>, UcError> {
    Ok(Json(PermissionsList { securable_type: None, full_name: None, privilege_assignments: vec![] }))
}
