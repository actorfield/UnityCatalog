use rsa::{
    pkcs1::{DecodeRsaPublicKey, EncodeRsaPrivateKey, EncodeRsaPublicKey},
    traits::PublicKeyParts,
    RsaPrivateKey, RsaPublicKey,
};
use std::path::Path;
use uc_errors::{ErrorCode, UcError};

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
        let private_key = RsaPrivateKey::new(&mut rng, RSA_BITS).map_err(|e| {
            UcError::new(
                ErrorCode::Internal,
                format!("RSA key generation failed: {}", e),
            )
        })?;
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

        Ok(Self {
            private_key_der: private_der,
            public_key_der: public_der,
            key_id,
        })
    }

    /// Serialise for storage as one object.
    ///
    /// Hex rather than raw DER so the object is inspectable and diffable, and
    /// so it survives anything that assumes text. The private key is in here:
    /// whatever holds this object is as sensitive as the keypair itself.
    pub fn encode(&self) -> Result<Vec<u8>, UcError> {
        let doc = serde_json::json!({
            "key_id": self.key_id,
            "private_key_der": hex::encode(&self.private_key_der),
            "public_key_der": hex::encode(&self.public_key_der),
        });
        serde_json::to_vec_pretty(&doc)
            .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, UcError> {
        let bad = |what: &str| {
            UcError::new(ErrorCode::Internal, format!("key material: {what}"))
        };
        let doc: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|e| bad(&format!("unparseable: {e}")))?;
        let field = |name: &str| -> Result<String, UcError> {
            doc.get(name)
                .and_then(|v| v.as_str())
                .map(str::to_owned)
                .ok_or_else(|| bad(&format!("missing {name}")))
        };
        let unhex = |name: &str, v: &str| -> Result<Vec<u8>, UcError> {
            hex::decode(v).map_err(|e| bad(&format!("{name} is not hex: {e}")))
        };
        let private_hex = field("private_key_der")?;
        let public_hex = field("public_key_der")?;
        Ok(Self {
            key_id: field("key_id")?,
            private_key_der: unhex("private_key_der", &private_hex)?,
            public_key_der: unhex("public_key_der", &public_hex)?,
        })
    }

    /// Load key material from a file — the way a mounted Kubernetes Secret
    /// arrives.
    ///
    /// This is the preferred source in any deployment that has a secret store.
    /// It never generates: a configured-but-missing key file is an error, not a
    /// cue to mint a fresh keypair. Silently generating is the failure that
    /// invalidates every issued token while looking like a clean start.
    ///
    /// A file rather than an environment variable on purpose. Env vars are
    /// inherited by child processes, surface in crash dumps and process
    /// listings, and are easy to log by accident; a mounted file is readable
    /// only by the process that opens it. Secrets mount as files.
    pub fn load_from_file(path: &Path) -> Result<Self, UcError> {
        let bytes = std::fs::read(path).map_err(|e| {
            UcError::new(
                ErrorCode::Internal,
                format!("key file {}: {e}", path.display()),
            )
        })?;
        Self::decode(&bytes)
    }

    /// Write key material in the format `load_from_file` reads, for generating
    /// the contents of a Secret out of band.
    pub fn write_to_file(&self, path: &Path) -> Result<(), UcError> {
        std::fs::write(path, self.encode()?).map_err(|e| {
            UcError::new(
                ErrorCode::Internal,
                format!("key file {}: {e}", path.display()),
            )
        })
    }

    /// Load from DER files, generating them if they do not exist.
    pub fn load_or_generate(config_dir: &Path) -> Result<Self, UcError> {
        let priv_path = config_dir.join("private_key.der");
        let pub_path = config_dir.join("public_key.der");
        let kid_path = config_dir.join("key_id.txt");

        if priv_path.exists() && pub_path.exists() && kid_path.exists() {
            let private_key_der = std::fs::read(&priv_path)
                .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
            let public_key_der = std::fs::read(&pub_path)
                .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
            let key_id = std::fs::read_to_string(&kid_path)
                .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?
                .trim()
                .to_string();
            return Ok(Self {
                private_key_der,
                public_key_der,
                key_id,
            });
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

/// The JWKS document for this keypair, per RFC 7517.
///
/// Derived on demand rather than read back from `certs.json`. The file was only
/// ever written on the generate path, so a server that loaded existing keys --
/// or whose certs.json was lost -- served 500s from /jwks forever.
pub fn jwks(km: &KeyManager) -> String {
    build_jwks(km)
}

fn build_jwks(km: &KeyManager) -> String {
    use base64::Engine as _;
    let kid = &km.key_id;

    // Parse the PKCS#1 DER public key to extract the RSA modulus (n) and exponent (e)
    // as base64url-encoded values per RFC 7517 (JWK format).
    match RsaPublicKey::from_pkcs1_der(&km.public_key_der) {
        Ok(pub_key) => {
            // n: RSA modulus (big-endian byte array, base64url-encoded, no padding)
            let n_bytes = pub_key.n().to_bytes_be();
            let n_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&n_bytes);
            // e: RSA public exponent (big-endian byte array, base64url-encoded, no padding)
            let e_bytes = pub_key.e().to_bytes_be();
            let e_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&e_bytes);
            format!(
                r#"{{"keys":[{{"kty":"RSA","use":"sig","alg":"RS512","kid":"{kid}","n":"{n_b64}","e":"{e_b64}"}}]}}"#
            )
        }
        Err(_) => {
            // Fallback: won't validate but at least returns a parseable JWKS
            let n_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&km.public_key_der);
            format!(
                r#"{{"keys":[{{"kty":"RSA","use":"sig","kid":"{kid}","n":"{n_b64}","e":"AQAB"}}]}}"#
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_produces_valid_der_bytes() {
        let km = KeyManager::generate().unwrap();
        assert!(!km.private_key_der.is_empty());
        assert!(!km.public_key_der.is_empty());
        assert!(!km.key_id.is_empty());
        assert_eq!(km.key_id.len(), 32, "key_id should be 32 hex chars");
    }

    #[test]
    fn load_or_generate_creates_files_on_first_run() {
        let dir = std::env::temp_dir().join(format!("uc_keys_test_{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();

        let km = KeyManager::load_or_generate(&dir).unwrap();
        assert!(dir.join("private_key.der").exists());
        assert!(dir.join("public_key.der").exists());
        assert!(dir.join("key_id.txt").exists());
        assert!(dir.join("certs.json").exists());

        // Load path: second call returns same key_id
        let km2 = KeyManager::load_or_generate(&dir).unwrap();
        assert_eq!(km.key_id, km2.key_id);
        assert_eq!(km.public_key_der, km2.public_key_der);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn certs_json_contains_valid_base64url_n() {
        let dir = std::env::temp_dir().join(format!("uc_keys_certs_{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        KeyManager::load_or_generate(&dir).unwrap();

        let certs = std::fs::read_to_string(dir.join("certs.json")).unwrap();
        assert!(certs.contains("\"kty\":\"RSA\""));
        // n should be base64url — no + or /
        let n_start = certs.find("\"n\":\"").unwrap() + 5;
        let n_end = certs[n_start..].find('"').unwrap() + n_start;
        let n_val = &certs[n_start..n_end];
        assert!(!n_val.contains('+'), "n must be base64url");
        assert!(!n_val.contains('/'), "n must be base64url");
        assert!(n_val.len() > 100, "n should be a long RSA modulus");

        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod key_material_tests {
    use super::*;

    #[test]
    fn encode_decode_round_trips() {
        let km = KeyManager::generate().unwrap();
        let bytes = km.encode().unwrap();
        let back = KeyManager::decode(&bytes).unwrap();
        assert_eq!(back.key_id, km.key_id);
        assert_eq!(back.private_key_der, km.private_key_der);
        assert_eq!(back.public_key_der, km.public_key_der);
    }

    #[test]
    fn decode_rejects_damaged_material_rather_than_half_loading() {
        assert!(KeyManager::decode(b"not json").is_err());
        assert!(KeyManager::decode(br#"{"key_id":"a"}"#).is_err());
        assert!(
            KeyManager::decode(
                br#"{"key_id":"a","private_key_der":"zz","public_key_der":"00"}"#
            )
            .is_err(),
            "non-hex must be refused, not silently truncated"
        );
    }

    #[test]
    fn jwks_is_derived_from_the_keypair_not_a_file() {
        let km = KeyManager::generate().unwrap();
        let doc = jwks(&km);
        assert!(doc.contains(&km.key_id), "kid must match the live key");
        assert!(doc.contains("\"kty\""), "must be a JWKS document: {doc}");
        // Stable across calls — the handler returns it per request.
        assert_eq!(doc, jwks(&km));
    }
}
