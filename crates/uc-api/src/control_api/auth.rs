//! What remains of the control-plane auth surface.
//!
//! Token exchange and the JWKS endpoint are gone: UC signs no tokens, so it has
//! no key to publish and nothing to exchange for. Callers present a token from
//! the configured OIDC issuer and it is validated against that issuer's JWKS.

use crate::state::AppState;
use axum::{extract::State, http::StatusCode};

/// Kept as a no-op so clients that call it on sign-out get a 200 rather than a
/// 404. There is no server-side session to end.
pub async fn logout(State(_state): State<AppState>) -> StatusCode {
    StatusCode::OK
}
