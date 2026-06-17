use uc_types::UriScheme;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum CredentialOperation {
    Read,
    ReadWrite,
}

#[derive(Debug, Clone)]
pub struct CredentialContext {
    pub scheme: UriScheme,
    pub locations: Vec<String>,
    pub operation: CredentialOperation,
    pub table_id: Option<Uuid>,
    /// JSON blob of the stored credential (from uc_credentials.credential column)
    pub credential_json: Option<String>,
    /// Role ARN for AWS AssumeRole
    pub role_arn: Option<String>,
    /// External ID for AWS AssumeRole
    pub external_id: Option<String>,
}
