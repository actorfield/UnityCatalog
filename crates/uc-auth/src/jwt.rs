use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use uc_errors::{ErrorCode, UcError};
use uc_types::TokenType;

/// Mirrors Java SecurityContext: RSA 2048-bit, RS512 algorithm.
/// JWT claims — mirrors Java JwtClaim + SecurityContext fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UcClaims {
    /// Subject: user email or service identifier
    pub sub: String,
    /// Issuer: always "internal"
    pub iss: String,
    /// Issued-at: epoch seconds
    pub iat: i64,
    /// JWT ID: unique per token
    pub jti: String,
    /// Token type: ACCESS or SERVICE
    pub token_type: TokenType,
}

// ── OIDC / external JWKS validation ──────────────────────────────────────────

/// Issuer config fetched from `{issuer}/.well-known/openid-configuration`.
/// Used to validate K8s SA projected tokens (or any OIDC-issued token).
pub struct OidcConfig {
    pub issuer: String,
    pub jwks: JwkSet,
}

/// Validate an externally-issued JWT against the OIDC JWK set.
/// Returns the `sub` claim on success.
///
/// Tokens from the configured issuer are treated as service identities —
/// callers map the returned subject to a local principal as needed.
pub fn decode_oidc_sub(config: &OidcConfig, token: &str) -> Result<String, UcError> {
    let header = jsonwebtoken::decode_header(token).map_err(|e| {
        UcError::new(
            ErrorCode::Unauthenticated,
            format!("OIDC token header: {e}"),
        )
    })?;

    let kid = header.kid.as_deref();

    for jwk in &config.jwks.keys {
        // Skip keys whose kid doesn't match the token's kid header (when present)
        if let Some(token_kid) = kid {
            if jwk.common.key_id.as_deref() != Some(token_kid) {
                continue;
            }
        }
        let Ok(decoding_key) = DecodingKey::from_jwk(jwk) else {
            continue;
        };
        let mut validation = Validation::new(header.alg);
        validation.set_issuer(&[&config.issuer]);
        validation.validate_aud = false; // audience enforced by NetworkPolicy
        validation.validate_exp = true;
        if let Ok(td) = jsonwebtoken::decode::<serde_json::Value>(token, &decoding_key, &validation)
        {
            let sub = td
                .claims
                .get("sub")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("oidc-principal")
                .to_string();
            return Ok(sub);
        }
    }
    Err(UcError::new(
        ErrorCode::Unauthenticated,
        "OIDC token validation failed against all JWKS keys",
    ))
}

#[cfg(test)]
mod tests {
    // Tests panic on purpose; see the note in the crate-level modules.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;

    // ── OIDC helpers ──────────────────────────────────────────────────────────

    fn hs_jwks(secret: &[u8], kid: Option<&str>) -> JwkSet {
        let k = URL_SAFE_NO_PAD.encode(secret);
        let mut key = serde_json::json!({ "kty": "oct", "k": k, "alg": "HS256" });
        if let Some(id) = kid {
            key["kid"] = serde_json::Value::String(id.to_string());
        }
        serde_json::from_value(serde_json::json!({ "keys": [key] })).unwrap()
    }

    fn hs_token(secret: &[u8], issuer: &str, sub: &str, exp_delta: i64) -> String {
        hs_token_kid(secret, issuer, sub, exp_delta, None)
    }

    fn hs_token_kid(
        secret: &[u8],
        issuer: &str,
        sub: &str,
        exp_delta: i64,
        kid: Option<&str>,
    ) -> String {
        use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
        let now = chrono::Utc::now().timestamp();
        let claims = serde_json::json!({
            "sub": sub, "iss": issuer,
            "iat": now, "exp": now + exp_delta
        });
        let mut h = Header::new(Algorithm::HS256);
        h.kid = kid.map(|s| s.to_string());
        encode(&h, &claims, &EncodingKey::from_secret(secret)).unwrap()
    }

    // ── decode_oidc_sub ───────────────────────────────────────────────────────

    #[test]
    fn oidc_valid_token_returns_sub() {
        let secret = b"test-oidc-secret-must-be-long-enough-32b";
        let issuer = "https://kubernetes.default.svc";
        let sub = "system:serviceaccount:example:sa-cp-demo";
        let token = hs_token(secret, issuer, sub, 3600);
        let config = OidcConfig {
            issuer: issuer.to_string(),
            jwks: hs_jwks(secret, None),
        };
        assert_eq!(decode_oidc_sub(&config, &token).unwrap(), sub);
    }

    #[test]
    fn oidc_wrong_issuer_rejected() {
        let secret = b"test-oidc-secret-must-be-long-enough-32b";
        let token = hs_token(secret, "https://wrong.issuer", "sa", 3600);
        let config = OidcConfig {
            issuer: "https://kubernetes.default.svc".to_string(),
            jwks: hs_jwks(secret, None),
        };
        assert!(decode_oidc_sub(&config, &token).is_err());
    }

    #[test]
    fn oidc_empty_jwks_fails() {
        let secret = b"test-oidc-secret-must-be-long-enough-32b";
        let issuer = "https://kubernetes.default.svc";
        let token = hs_token(secret, issuer, "sa", 3600);
        let empty_jwks: JwkSet = serde_json::from_value(serde_json::json!({ "keys": [] })).unwrap();
        let config = OidcConfig {
            issuer: issuer.to_string(),
            jwks: empty_jwks,
        };
        assert!(decode_oidc_sub(&config, &token).is_err());
    }

    #[test]
    fn oidc_expired_token_rejected() {
        let secret = b"test-oidc-secret-must-be-long-enough-32b";
        let issuer = "https://kubernetes.default.svc";
        let token = hs_token(secret, issuer, "sa", -300); // expired 5 min ago, outside 60s leeway
        let config = OidcConfig {
            issuer: issuer.to_string(),
            jwks: hs_jwks(secret, None),
        };
        assert!(decode_oidc_sub(&config, &token).is_err());
    }

    #[test]
    fn oidc_kid_match_accepted() {
        let secret = b"test-oidc-secret-must-be-long-enough-32b";
        let issuer = "https://kubernetes.default.svc";
        let kid = "k8s-key-1";
        let token = hs_token_kid(secret, issuer, "sa-cp-demo", 3600, Some(kid));
        let config = OidcConfig {
            issuer: issuer.to_string(),
            jwks: hs_jwks(secret, Some(kid)),
        };
        assert_eq!(decode_oidc_sub(&config, &token).unwrap(), "sa-cp-demo");
    }

    #[test]
    fn oidc_kid_mismatch_rejected() {
        let secret = b"test-oidc-secret-must-be-long-enough-32b";
        let issuer = "https://kubernetes.default.svc";
        // Token says kid=key-A but JWKS only has kid=key-B → no matching key tried
        let token = hs_token_kid(secret, issuer, "sa", 3600, Some("key-A"));
        let config = OidcConfig {
            issuer: issuer.to_string(),
            jwks: hs_jwks(secret, Some("key-B")),
        };
        assert!(decode_oidc_sub(&config, &token).is_err());
    }

    #[test]
    fn oidc_garbage_token_rejected() {
        let config = OidcConfig {
            issuer: "https://kubernetes.default.svc".to_string(),
            jwks: hs_jwks(b"secret-key-long-enough-32-bytes!!", None),
        };
        assert!(decode_oidc_sub(&config, "not.a.jwt").is_err());
    }
}
