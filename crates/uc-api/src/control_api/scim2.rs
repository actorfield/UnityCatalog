// TODO: implement SCIM2 user management
use axum::{extract::{Path, State}, http::StatusCode};
use crate::state::AppState;
use uc_errors::UcError;
pub async fn create_user(State(_s): State<AppState>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn list_users(State(_s): State<AppState>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn get_user(State(_s): State<AppState>, Path(_id): Path<String>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn update_user(State(_s): State<AppState>, Path(_id): Path<String>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn delete_user(State(_s): State<AppState>, Path(_id): Path<String>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn patch_user(State(_s): State<AppState>, Path(_id): Path<String>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn get_me(State(_s): State<AppState>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn patch_me(State(_s): State<AppState>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
