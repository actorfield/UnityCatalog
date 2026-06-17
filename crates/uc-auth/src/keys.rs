use rsa::{pkcs1::{EncodeRsaPrivateKey, EncodeRsaPublicKey}, RsaPrivateKey, RsaPublicKey};
use uc_errors::{ErrorCode, UcError};
use std::path::Path;

const RSA_BITS: usize = 2048;

pub struct KeyManager {
    pub private_key_der: Vec<u8>,
    pub public_key_der: Vec<u8>,
    pub key_id: String,
}

impl KeyManager {
    /// Generate a new RSA-2048 key pair.
    pub fn generate() -> Result<Self, UcError> {
        let mut rng = rand::thread_rng();
        let private_key = RsaPrivateKey::new(&mut rng, RSA_BITS)
            .map_err(|e| UcError::new(ErrorCode::Internal, format!("RSA key generation failed: {}", e)))?;
        let public_key = RsaPublicKey::from(&private_key);

        // jsonwebtoken 9 expects PKCS#1 DER format (not PKCS#8)
        let private_der = private_key
            .to_pkcs1_der()
            .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?
            .as_bytes()
            .to_vec();

        let public_der = public_key
            .to_pkcs1_der()
            .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?
            .as_bytes()
            .to_vec();

        let key_id = hex::encode(rand::random::<[u8; 16]>());

        Ok(Self { private_key_der: private_der, public_key_der: public_der, key_id })
    }

    /// Load from DER files, generating them if they do not exist.
    pub fn load_or_generate(config_dir: &Path) -> Result<Self, UcError> {
        let priv_path = config_dir.join("private_key.der");
        let pub_path  = config_dir.join("public_key.der");
        let kid_path  = config_dir.join("key_id.txt");

        if priv_path.exists() && pub_path.exists() && kid_path.exists() {
            let private_key_der = std::fs::read(&priv_path)
                .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
            let public_key_der = std::fs::read(&pub_path)
                .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
            let key_id = std::fs::read_to_string(&kid_path)
                .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?
                .trim()
                .to_string();
            return Ok(Self { private_key_der, public_key_der, key_id });
        }

        // Generate and persist
        let km = Self::generate()?;
        std::fs::create_dir_all(config_dir)
            .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
        std::fs::write(&priv_path, &km.private_key_der)
            .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
        std::fs::write(&pub_path, &km.public_key_der)
            .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
        std::fs::write(&kid_path, &km.key_id)
            .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;

        // Also write JWKS for clients
        let jwks = build_jwks(&km);
        std::fs::write(config_dir.join("certs.json"), &jwks)
            .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;

        Ok(km)
    }
}

fn build_jwks(km: &KeyManager) -> String {
    // Minimal JWKS with kid, kty=RSA, use=sig
    // Full n/e extraction would require parsing the DER — supply a placeholder for now;
    // real JWKS construction uses the rsa crate's public key components.
    let kid = &km.key_id;
    let n_b64 = base64::encode(&km.public_key_der); // placeholder: real impl extracts n & e
    format!(r#"{{"keys":[{{"kty":"RSA","use":"sig","kid":"{kid}","n":"{n_b64}","e":"AQAB"}}]}}"#)
}
