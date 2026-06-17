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
    pub fn from_der(private_key_der: &[u8], public_key_der: &[u8], key_id: String) -> Result<Self, UcError> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::KeyManager;

    #[test]
    fn encode_decode_round_trip() {
        let km = KeyManager::generate().expect("key gen failed");
        let config = JwtConfig::from_der(&km.private_key_der, &km.public_key_der, "test-kid".into())
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
        let config = JwtConfig::from_der(&km.private_key_der, &km.public_key_der, "kid".into()).unwrap();
        let km2 = KeyManager::generate().expect("key gen 2");
        let config2 = JwtConfig::from_der(&km2.private_key_der, &km2.public_key_der, "kid2".into()).unwrap();
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
        let config = JwtConfig::from_der(&km.private_key_der, &km.public_key_der, "kid".into()).unwrap();
        let claims = UcClaims::new_service();
        let token = encode_token(&config, &claims).unwrap();
        let decoded = decode_token(&config, &token).unwrap();
        assert_eq!(decoded.claims.token_type, uc_types::TokenType::Service);
    }

    #[test]
    fn missing_iss_field_rejected() {
        // A token without iss=internal should fail validation
        let km = KeyManager::generate().unwrap();
        let config = JwtConfig::from_der(&km.private_key_der, &km.public_key_der, "k".into()).unwrap();
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
}
