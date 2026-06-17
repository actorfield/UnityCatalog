// TODO: implement OAuth2 token exchange and JWKS endpoint
use axum::{extract::State, http::StatusCode};
use crate::state::AppState;
use uc_errors::UcError;
pub async fn token_exchange(State(_s): State<AppState>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn logout(State(_s): State<AppState>) -> Result<StatusCode, UcError> { Ok(StatusCode::OK) }
pub async fn jwks(State(_s): State<AppState>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
