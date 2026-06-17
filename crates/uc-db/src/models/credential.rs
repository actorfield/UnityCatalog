use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct CredentialRow {
    pub id: Uuid,
    pub name: String,
    pub credential_type: String,
    /// JSON blob of the credential payload (AwsIamRole, AzureSP, GcpSA, etc.)
    pub credential: String,
    pub purpose: String,
    pub comment: Option<String>,
    pub owner: Option<String>,
    pub created_at: i64,
    pub created_by: Option<String>,
    pub updated_at: Option<i64>,
    pub updated_by: Option<String>,
}
