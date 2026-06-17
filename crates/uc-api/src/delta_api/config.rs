use axum::{extract::State, http::StatusCode};
use crate::state::AppState;
use uc_errors::UcError;
pub async fn get_config(State(_s): State<AppState>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
