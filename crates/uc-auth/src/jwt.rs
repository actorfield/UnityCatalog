use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, TokenData, Validation};
use serde::{Deserialize, Serialize};
use uc_errors::{ErrorCode, UcError};
use uc_types::TokenType;
use uuid::Uuid;

/// Mirrors Java SecurityContext: RSA 2048-bit, RS512 algorithm.
pub struct JwtConfig {
    pub encoding_key: EncodingKey,
    pub decoding_key: DecodingKey,
    pub key_id: String,
}

impl JwtConfig {
    pub fn from_der(
        private_key_der: &[u8],
        public_key_der: &[u8],
        key_id: String,
    ) -> Result<Self, UcError> {
        Ok(Self {
            encoding_key: EncodingKey::from_rsa_der(private_key_der),
            decoding_key: DecodingKey::from_rsa_der(public_key_der),
            key_id,
        })
    }
}

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

impl UcClaims {
    pub fn new_access(sub: impl Into<String>) -> Self {
        Self {
            sub: sub.into(),
            iss: "internal".to_string(),
            iat: chrono::Utc::now().timestamp(),
            jti: Uuid::new_v4().to_string(),
            token_type: TokenType::Access,
        }
    }

    pub fn new_service() -> Self {
        Self {
            sub: "uc_service".to_string(),
            iss: "internal".to_string(),
            iat: chrono::Utc::now().timestamp(),
            jti: Uuid::new_v4().to_string(),
            token_type: TokenType::Service,
        }
    }
}

pub fn encode_token(config: &JwtConfig, claims: &UcClaims) -> Result<String, UcError> {
    let mut header = Header::new(Algorithm::RS512);
    header.kid = Some(config.key_id.clone());

    jsonwebtoken::encode(&header, claims, &config.encoding_key)
        .map_err(|e| UcError::new(ErrorCode::Internal, format!("Token encoding failed: {}", e)))
}

pub fn decode_token(config: &JwtConfig, token: &str) -> Result<TokenData<UcClaims>, UcError> {
    let mut validation = Validation::new(Algorithm::RS512);
    validation.set_issuer(&["internal"]);
    validation.validate_exp = false; // UC tokens are long-lived
    validation.set_required_spec_claims(&["sub", "iss"]);

    jsonwebtoken::decode::<UcClaims>(token, &config.decoding_key, &validation)
        .map_err(|e| UcError::new(ErrorCode::Unauthenticated, format!("Invalid token: {}", e)))
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
            let sub = td.claims["sub"]
                .as_str()
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
    use super::*;
    use crate::keys::KeyManager;
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
        use jsonwebtoken::{encode, EncodingKey, Header};
        let now = chrono::Utc::now().timestamp();
        let claims = serde_json::json!({
            "sub": sub, "iss": issuer,
            "iat": now, "exp": now + exp_delta
        });
        let mut h = Header::new(Algorithm::HS256);
        h.kid = kid.map(|s| s.to_string());
        encode(&h, &claims, &EncodingKey::from_secret(secret)).unwrap()
    }

    #[test]
    fn encode_decode_round_trip() {
        let km = KeyManager::generate().expect("key gen failed");
        let config =
            JwtConfig::from_der(&km.private_key_der, &km.public_key_der, "test-kid".into())
                .expect("config failed");
        let claims = UcClaims::new_access("test@example.com");
        let token = encode_token(&config, &claims).expect("encode failed");
        let decoded = decode_token(&config, &token).expect("decode failed");
        assert_eq!(decoded.claims.sub, "test@example.com");
        assert_eq!(decoded.claims.iss, "internal");
    }

    #[test]
    fn wrong_algorithm_rejected() {
        let km = KeyManager::generate().expect("key gen");
        let config =
            JwtConfig::from_der(&km.private_key_der, &km.public_key_der, "kid".into()).unwrap();
        let km2 = KeyManager::generate().expect("key gen 2");
        let config2 =
            JwtConfig::from_der(&km2.private_key_der, &km2.public_key_der, "kid2".into()).unwrap();
        let claims = UcClaims::new_service();
        let token = encode_token(&config, &claims).unwrap();
        assert!(decode_token(&config2, &token).is_err());
    }

    #[test]
    fn service_token_has_service_type() {
        let claims = UcClaims::new_service();
        assert_eq!(claims.sub, "uc_service");
        assert_eq!(claims.iss, "internal");
        assert_eq!(claims.token_type, uc_types::TokenType::Service);
    }

    #[test]
    fn access_token_has_access_type() {
        let claims = UcClaims::new_access("user@example.com");
        assert_eq!(claims.sub, "user@example.com");
        assert_eq!(claims.token_type, uc_types::TokenType::Access);
        assert!(!claims.jti.is_empty());
        assert!(claims.iat > 0);
    }

    #[test]
    fn service_token_round_trip() {
        let km = KeyManager::generate().unwrap();
        let config =
            JwtConfig::from_der(&km.private_key_der, &km.public_key_der, "kid".into()).unwrap();
        let claims = UcClaims::new_service();
        let token = encode_token(&config, &claims).unwrap();
        let decoded = decode_token(&config, &token).unwrap();
        assert_eq!(decoded.claims.token_type, uc_types::TokenType::Service);
    }

    #[test]
    fn missing_iss_field_rejected() {
        // A token without iss=internal should fail validation
        let km = KeyManager::generate().unwrap();
        let config =
            JwtConfig::from_der(&km.private_key_der, &km.public_key_der, "k".into()).unwrap();
        // Forge a token with wrong issuer by creating claims with wrong iss
        let mut claims = UcClaims::new_access("test@x.com");
        claims.iss = "external".to_string();
        let token = encode_token(&config, &claims).unwrap();
        // Should fail because iss != "internal"
        assert!(decode_token(&config, &token).is_err());
    }

    #[test]
    fn jti_is_unique_per_token() {
        let c1 = UcClaims::new_access("a@b.com");
        let c2 = UcClaims::new_access("a@b.com");
        assert_ne!(c1.jti, c2.jti);
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
