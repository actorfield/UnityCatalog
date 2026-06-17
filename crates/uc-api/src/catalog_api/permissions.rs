use axum::{extract::{Path, State}, http::StatusCode};
use crate::state::AppState;
use uc_errors::UcError;
pub async fn get(State(_s): State<AppState>, Path(_p): Path<(String,String)>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn update(State(_s): State<AppState>, Path(_p): Path<(String,String)>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
