// TODO: implement full handler — stub returns 501
use axum::{extract::{Path, Query, State}, http::StatusCode, Json};
use crate::state::AppState;
use uc_errors::UcError;
pub async fn create(State(_s): State<AppState>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn list(State(_s): State<AppState>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn get(State(_s): State<AppState>, Path(_n): Path<String>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn update(State(_s): State<AppState>, Path(_n): Path<String>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn delete(State(_s): State<AppState>, Path(_n): Path<String>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
