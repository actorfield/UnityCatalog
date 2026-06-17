// TODO: implement full handler — stub returns 501
use axum::{extract::{Path, State}, http::StatusCode};
use crate::state::AppState;
use uc_errors::UcError;
pub async fn create_model(State(_s): State<AppState>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn list_models(State(_s): State<AppState>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn get_model(State(_s): State<AppState>, Path(_n): Path<String>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn update_model(State(_s): State<AppState>, Path(_n): Path<String>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn delete_model(State(_s): State<AppState>, Path(_n): Path<String>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn create_version(State(_s): State<AppState>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn list_versions(State(_s): State<AppState>, Path(_n): Path<String>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn get_version(State(_s): State<AppState>, Path(_p): Path<(String,String)>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn update_version(State(_s): State<AppState>, Path(_p): Path<(String,String)>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn delete_version(State(_s): State<AppState>, Path(_p): Path<(String,String)>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn finalize_version(State(_s): State<AppState>, Path(_p): Path<(String,String)>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
