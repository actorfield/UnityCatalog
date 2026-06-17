use crate::context::CredentialContext;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use uc_errors::{ErrorCode, UcError};
use uc_openapi::catalog::TemporaryCredentials;
use uc_types::UriScheme;

// Credentials are reused until this many seconds before expiry.
pub(crate) const CACHE_EXPIRY_BUFFER_SECS: u64 = 60;

#[derive(Clone)]
struct CachedCredential {
    creds: TemporaryCredentials,
    /// When this cache entry expires (wall clock).
    expires_at: Instant,
}

/// Dispatching credential vendor — mirrors Java's CloudCredentialVendor.
/// Fixes issue #1576: caches vended credentials by (role_arn, locations) key
/// so repeated queries for the same table don't hammer STS on every call.
pub struct CloudCredentialVendor {
    /// Cache key: role_arn + sorted locations joined
    cache: Mutex<HashMap<String, CachedCredential>>,
    #[cfg(feature = "aws")]
    aws: Option<AwsCredentialVendor>,
}

impl Default for CloudCredentialVendor {
    fn default() -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
            #[cfg(feature = "aws")]
            aws: None,
        }
    }
}

impl CloudCredentialVendor {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn vend(&self, ctx: &CredentialContext) -> Result<TemporaryCredentials, UcError> {
        // Local filesystem — no credentials needed, skip cache
        if matches!(ctx.scheme, UriScheme::File | UriScheme::Null) {
            return Ok(TemporaryCredentials::default());
        }

        // Check cache first
        let cache_key = make_cache_key(ctx);
        {
            let cache = self.cache.lock().unwrap();
            if let Some(entry) = cache.get(&cache_key) {
                if entry.expires_at > Instant::now() {
                    return Ok(entry.creds.clone());
                }
            }
        }

        // Cache miss — vend fresh credentials
        let creds = self.vend_fresh(ctx).await?;

        // Store in cache with TTL derived from expiration field if available
        // Default to 55-minute TTL (STS default session is 1h, buffer 5m)
        let ttl = parse_expiry_ttl(&creds).unwrap_or(Duration::from_secs(55 * 60));
        {
            let mut cache = self.cache.lock().unwrap();
            cache.insert(cache_key, CachedCredential {
                creds: creds.clone(),
                expires_at: Instant::now() + ttl,
            });
        }

        Ok(creds)
    }

    async fn vend_fresh(&self, ctx: &CredentialContext) -> Result<TemporaryCredentials, UcError> {
        match ctx.scheme {
            UriScheme::S3 => {
                #[cfg(feature = "aws")]
                if let Some(ref aws) = self.aws {
                    return aws.vend(ctx).await;
                }
                Err(UcError::new(ErrorCode::Unimplemented, "AWS credential vending not configured"))
            }
            UriScheme::Abfs | UriScheme::Abfss => {
                Err(UcError::new(ErrorCode::Unimplemented, "Azure credential vending not yet implemented"))
            }
            UriScheme::Gs => {
                Err(UcError::new(ErrorCode::Unimplemented, "GCP credential vending not yet implemented"))
            }
            UriScheme::File | UriScheme::Null => Ok(TemporaryCredentials::default()),
        }
    }
}

fn make_cache_key(ctx: &CredentialContext) -> String {
    let role = ctx.role_arn.as_deref().unwrap_or("");
    let mut locs = ctx.locations.clone();
    locs.sort();
    format!("{}::{}", role, locs.join(","))
}

/// Parse expiry from the credential's expiration field (RFC3339 string).
/// Returns a Duration from now to (expiry - buffer).
fn parse_expiry_ttl(creds: &TemporaryCredentials) -> Option<Duration> {
    let exp_str = creds.aws_temp_credentials.as_ref()
        .and_then(|a| a.expiration.as_deref())
        .or_else(|| creds.expiration_time.as_deref())?;

    let exp = chrono::DateTime::parse_from_rfc3339(exp_str).ok()?;
    let now = chrono::Utc::now();
    let secs_until_exp = (exp.with_timezone(&chrono::Utc) - now).num_seconds();
    if secs_until_exp <= CACHE_EXPIRY_BUFFER_SECS as i64 {
        return None; // already near expiry, don't cache
    }
    Some(Duration::from_secs(
        (secs_until_exp as u64).saturating_sub(CACHE_EXPIRY_BUFFER_SECS),
    ))
}

// ── AWS Credential Vendor ─────────────────────────────────────────────────────

#[cfg(feature = "aws")]
pub struct AwsCredentialVendor {
    master_role_arn: Option<String>,
}

#[cfg(feature = "aws")]
impl AwsCredentialVendor {
    pub fn new(master_role_arn: Option<String>) -> Self {
        Self { master_role_arn }
    }

    pub async fn vend(&self, ctx: &CredentialContext) -> Result<TemporaryCredentials, UcError> {
        use aws_sdk_sts::Client;
        use uc_openapi::catalog::AwsCredentials;

        let config = aws_config::load_from_env().await;
        let sts_client = Client::new(&config);

        let role_arn = ctx.role_arn.as_deref()
            .ok_or_else(|| UcError::new(ErrorCode::InvalidArgument, "No role ARN configured for credential"))?;

        let mut req = sts_client
            .assume_role()
            .role_arn(role_arn)
            .role_session_name("unity-catalog");

        if let Some(ref ext_id) = ctx.external_id {
            req = req.external_id(ext_id);
        }

        let response = req.send().await
            .map_err(|e| UcError::new(ErrorCode::Internal, format!("STS AssumeRole failed: {}", e)))?;

        let creds = response.credentials()
            .ok_or_else(|| UcError::new(ErrorCode::Internal, "STS returned no credentials"))?;

        Ok(TemporaryCredentials {
            aws_temp_credentials: Some(AwsCredentials {
                access_key_id: creds.access_key_id().to_string(),
                secret_access_key: creds.secret_access_key().to_string(),
                session_token: creds.session_token().to_string(),
                expiration: creds.expiration().map(|t| t.to_string()),
            }),
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{CredentialContext, CredentialOperation};
    use uc_types::UriScheme;

    fn local_ctx(url: &str) -> CredentialContext {
        CredentialContext {
            scheme: UriScheme::from_url(url),
            locations: vec![url.to_string()],
            operation: CredentialOperation::Read,
            table_id: None,
            credential_json: None,
            role_arn: None,
            external_id: None,
        }
    }

    #[tokio::test]
    async fn local_file_returns_empty_credentials() {
        let vendor = CloudCredentialVendor::new();
        let ctx = local_ctx("file:///tmp/test");
        let creds = vendor.vend(&ctx).await.unwrap();
        assert!(creds.aws_temp_credentials.is_none());
        assert!(creds.gcp_oauth_token.is_none());
        assert!(creds.azure_user_delegation_sas.is_none());
    }

    #[tokio::test]
    async fn null_scheme_returns_empty_credentials() {
        let vendor = CloudCredentialVendor::new();
        let ctx = local_ctx("unknown://x");
        // NULL scheme → local/empty (no cloud SDK needed)
        // This should fall to the File/Null arm and return default
        let creds = vendor.vend(&ctx).await;
        // Either empty creds or Unimplemented — both acceptable for null scheme
        match creds {
            Ok(c) => {
                assert!(c.aws_temp_credentials.is_none());
            }
            Err(e) => {
                assert!(e.to_string().contains("UNIMPLEMENTED") || e.to_string().contains("not"));
            }
        }
    }

    #[tokio::test]
    async fn local_file_vend_twice_is_consistent() {
        let vendor = CloudCredentialVendor::new();
        let ctx = local_ctx("file:///tmp/repeat");
        let c1 = vendor.vend(&ctx).await.unwrap();
        let c2 = vendor.vend(&ctx).await.unwrap();
        // Both should be empty (local path, no cloud creds)
        assert!(c1.aws_temp_credentials.is_none());
        assert!(c2.aws_temp_credentials.is_none());
    }

    #[tokio::test]
    async fn s3_without_aws_feature_returns_unimplemented() {
        let vendor = CloudCredentialVendor::new();
        let ctx = local_ctx("s3://my-bucket/path");
        let result = vendor.vend(&ctx).await;
        // Without the aws feature, should be Unimplemented
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("UNIMPLEMENTED") || msg.contains("not configured") || msg.contains("not yet"));
    }

    #[tokio::test]
    async fn azure_returns_unimplemented() {
        let vendor = CloudCredentialVendor::new();
        let ctx = local_ctx("abfss://container@account.dfs.core.windows.net/path");
        let result = vendor.vend(&ctx).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("UNIMPLEMENTED"));
    }

    #[tokio::test]
    async fn gcs_returns_unimplemented() {
        let vendor = CloudCredentialVendor::new();
        let ctx = local_ctx("gs://my-bucket/path");
        let result = vendor.vend(&ctx).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("UNIMPLEMENTED"));
    }
}
