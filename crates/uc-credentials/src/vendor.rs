use crate::context::CredentialContext;
use uc_errors::{ErrorCode, UcError};
use uc_openapi::catalog::TemporaryCredentials;
use uc_types::UriScheme;

/// Dispatching credential vendor — mirrors Java's CloudCredentialVendor.
pub struct CloudCredentialVendor {
    #[cfg(feature = "aws")]
    aws: Option<AwsCredentialVendor>,
}

impl Default for CloudCredentialVendor {
    fn default() -> Self {
        Self {
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
            UriScheme::File | UriScheme::Null => {
                // Local filesystem — return empty credentials (no cloud creds needed)
                Ok(TemporaryCredentials::default())
            }
        }
    }
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
