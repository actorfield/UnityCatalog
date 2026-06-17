use axum::{extract::{Path, State}, http::StatusCode};
use crate::state::AppState;
use uc_errors::UcError;
pub async fn get_table_credentials(State(_s): State<AppState>, Path(_p): Path<(String,String,String)>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn get_staging_credentials(State(_s): State<AppState>, Path(_id): Path<String>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn get_path_credentials(State(_s): State<AppState>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
