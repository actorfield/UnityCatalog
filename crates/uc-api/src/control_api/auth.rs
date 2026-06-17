use axum::{extract::State, http::StatusCode, Json};
use uc_auth::jwt::{encode_token, UcClaims};
use uc_db::repos::UserRepo;
use uc_errors::{ErrorCode, UcError};
use uc_openapi::control::OAuthTokenExchangeResponse;
use crate::state::AppState;

#[derive(serde::Deserialize)]
pub struct TokenForm {
    pub grant_type: Option<String>,
    pub subject_token: Option<String>,
    pub subject_token_type: Option<String>,
}

pub async fn token_exchange(
    State(state): State<AppState>,
    Json(form): Json<TokenForm>,
) -> Result<Json<OAuthTokenExchangeResponse>, UcError> {
    // Validate: must be token-exchange grant type
    let gt = form.grant_type.as_deref().unwrap_or("");
    if gt != "urn:ietf:params:oauth:grant-type:token-exchange" {
        return Err(UcError::new(ErrorCode::InvalidArgument, format!("Unsupported grant_type: {}", gt)));
    }

    let subject_token = form.subject_token.as_deref()
        .ok_or_else(|| UcError::new(ErrorCode::InvalidArgument, "subject_token required"))?;

    // For the simplest case: subject_token is a user email or an existing UC token.
    // Look up the user and issue an access token.
    let user = UserRepo::get_by_email(&state.pool, subject_token).await?
        .ok_or_else(|| UcError::unauthenticated(format!("User '{}' not found", subject_token)))?;

    if !user.is_enabled() {
        return Err(UcError::unauthenticated("User account is disabled"));
    }

    let claims = UcClaims::new_access(user.email.unwrap_or(user.name));
    let token = encode_token(&state.jwt_config, &claims)?;

    Ok(Json(OAuthTokenExchangeResponse {
        access_token: token,
        token_type: "Bearer".to_string(),
        expires_in: None,
        scope: None,
        issued_token_type: "urn:ietf:params:oauth:token-type:access_token".to_string(),
    }))
}

pub async fn logout(State(_state): State<AppState>) -> StatusCode {
    StatusCode::OK
}

pub async fn jwks(State(state): State<AppState>) -> Result<String, UcError> {
    // Read the JWKS file written by KeyManager
    let path = std::path::Path::new("./etc/conf/certs.json");
    std::fs::read_to_string(path)
        .map_err(|e| UcError::new(ErrorCode::Internal, format!("JWKS file not found: {}", e)))
}
