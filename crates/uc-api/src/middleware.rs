use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use std::sync::Arc;
use uc_auth::{decode_oidc_sub, jwt::decode_token, UcClaims};
use uc_db::repos::UserRepo;
use uc_errors::{error_into_response, ErrorFormat, UcError};
use uc_types::TokenType;

use crate::state::AppState;

/// Paths that bypass JWT authentication.
const AUTH_BYPASS_PATHS: &[&str] = &[
    "/api/1.0/unity-control/auth/tokens",
    "/.well-known/jwks.json",
];

/// JWT auth middleware: extracts Bearer token, validates it, checks user state,
/// and inserts Arc<UcClaims> into request extensions.
pub async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();

    // Bypass auth for public endpoints
    if AUTH_BYPASS_PATHS.iter().any(|p| path == *p) {
        return next.run(req).await;
    }

    // When auth is disabled, inject a dummy service claims so handlers can still extract them
    if !state.auth_enabled {
        let dummy = Arc::new(UcClaims {
            sub: "anonymous@unitycatalog.io".to_string(),
            iss: "internal".to_string(),
            iat: 0,
            jti: "disabled".to_string(),
            token_type: uc_types::TokenType::Service,
        });
        req.extensions_mut().insert(dummy);
        return next.run(req).await;
    }

    // Extract token from Authorization header or UC_TOKEN cookie
    let token = extract_token(&req);

    let token = match token {
        Some(t) => t,
        None => {
            let err = UcError::unauthenticated("No authentication token provided");
            return error_into_response(err, ErrorFormat::Catalog);
        }
    };

    // Decode and validate JWT — try UC-issued first, then OIDC issuer if configured
    let claims = match decode_token(&state.jwt_config, &token) {
        Ok(td) => td.claims,
        Err(uc_err) => {
            // OIDC-validated tokens map to a per-caller principal so distinct
            // identities (e.g. each K8s ServiceAccount) aren't collapsed into a
            // single shared admin principal. Each external `sub` resolves to its
            // own (lazily-created, zero-grant) uc_users row.
            if let Some(oidc) = &state.oidc_config {
                match decode_oidc_sub(oidc, &token) {
                    Ok(sub) => match UserRepo::find_or_create_by_external_id(&state.pool, &sub).await {
                        Ok(row) => UcClaims {
                            sub: row.email.unwrap_or_else(|| row.external_id.clone().unwrap_or(sub)),
                            iss: oidc.issuer.clone(),
                            iat: chrono::Utc::now().timestamp(),
                            jti: uuid::Uuid::now_v7().to_string(),
                            token_type: TokenType::Service,
                        },
                        Err(e) => return error_into_response(e, ErrorFormat::Catalog),
                    },
                    Err(_) => return error_into_response(uc_err, ErrorFormat::Catalog),
                }
            } else {
                return error_into_response(uc_err, ErrorFormat::Catalog);
            }
        }
    };

    // SERVICE tokens bypass user DB lookup — they represent the server itself
    if claims.token_type != uc_types::TokenType::Service {
        match UserRepo::get_by_email(&state.pool, &claims.sub).await {
            Ok(Some(user)) if user.is_enabled() => {}
            Ok(Some(_)) => {
                let err = UcError::unauthenticated("User account is disabled");
                return error_into_response(err, ErrorFormat::Catalog);
            }
            Ok(None) => {
                let err = UcError::unauthenticated(format!("User '{}' not found", claims.sub));
                return error_into_response(err, ErrorFormat::Catalog);
            }
            Err(e) => return error_into_response(e, ErrorFormat::Catalog),
        }
    }

    req.extensions_mut().insert(Arc::new(claims));
    next.run(req).await
}

fn extract_token(req: &Request) -> Option<String> {
    // Authorization: Bearer <token>
    if let Some(auth) = req.headers().get("Authorization") {
        if let Ok(val) = auth.to_str() {
            if let Some(token) = val.strip_prefix("Bearer ") {
                return Some(token.trim().to_string());
            }
        }
    }

    // Cookie: UC_TOKEN=<token>
    if let Some(cookie_header) = req.headers().get("Cookie") {
        if let Ok(val) = cookie_header.to_str() {
            for cookie in val.split(';') {
                let cookie = cookie.trim();
                if let Some(token) = cookie.strip_prefix("UC_TOKEN=") {
                    return Some(token.to_string());
                }
            }
        }
    }

    None
}
