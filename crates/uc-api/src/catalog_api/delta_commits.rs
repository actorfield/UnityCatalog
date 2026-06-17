use axum::{extract::State, http::StatusCode};
use crate::state::AppState;
use uc_errors::UcError;
pub async fn get_commits(State(_s): State<AppState>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn commit(State(_s): State<AppState>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
