use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use std::sync::Arc;
use uc_auth::{decode_oidc_sub, UcClaims};
use uc_db::repos::user;
use uc_db::store::actor::{self as actor_scope, Actor};
use uc_errors::{error_into_response, ErrorFormat, UcError};
use uc_types::TokenType;

use crate::state::AppState;

/// Paths that bypass JWT authentication. Empty since UC stopped issuing tokens:
/// the exchange and JWKS endpoints it used to exempt no longer exist.
const AUTH_BYPASS_PATHS: &[&str] = &[];

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
            token_type: TokenType::Service,
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

    // Validate against the configured OIDC issuer. UC signs no tokens of its
    // own: it is a resource server, so the only trust root is the issuer's
    // JWKS. `oidc_config` is Some whenever auth is enabled -- the two are set
    // from the same flag, so there is no state where auth is on with no way to
    // authenticate.
    let Some(oidc) = &state.oidc_config else {
        let err = UcError::unauthenticated("No OIDC issuer configured");
        return error_into_response(err, ErrorFormat::Catalog);
    };

    // Each external `sub` resolves to its own lazily-created, zero-grant
    // uc_users row, so distinct identities (one per K8s ServiceAccount, say)
    // are not collapsed into a single shared principal.
    let (claims, actor) = match decode_oidc_sub(oidc, &token) {
        Ok(sub) => match user::find_or_create_by_external_id(&state.pool, &sub).await {
            Ok(row) => {
                let name = row
                    .email
                    .clone()
                    .unwrap_or_else(|| row.external_id.clone().unwrap_or_else(|| sub.clone()));
                let actor = Actor::new(Some(row.id), name.clone());
                (
                    UcClaims {
                        sub: name,
                        iss: oidc.issuer.clone(),
                        iat: chrono::Utc::now().timestamp(),
                        jti: uuid::Uuid::now_v7().to_string(),
                        token_type: TokenType::Service,
                    },
                    actor,
                )
            }
            Err(e) => return error_into_response(e, ErrorFormat::Catalog),
        },
        Err(e) => return error_into_response(e, ErrorFormat::Catalog),
    };

    // SERVICE tokens bypass user DB lookup — they represent the server itself
    if claims.token_type != TokenType::Service {
        match user::get_by_email(&state.pool, &claims.sub).await {
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
    // Every commit made while handling this request records who made it. This
    // is what gives deletes and grants an actor: they have no *_by column to
    // carry one, so the commit itself has to.
    actor_scope::scope(Some(actor), next.run(req)).await
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
