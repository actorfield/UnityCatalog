use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct ExternalLocationRow {
    pub id: Uuid,
    pub name: String,
    pub url: String,
    pub comment: Option<String>,
    pub owner: Option<String>,
    pub credential_id: Uuid,
    pub created_at: Option<i64>,
    pub created_by: Option<String>,
    pub updated_at: Option<i64>,
    pub updated_by: Option<String>,
}
