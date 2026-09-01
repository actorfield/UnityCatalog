use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg_attr(feature = "sql", derive(sqlx::FromRow))]
#[derive(Debug, Clone, Serialize, Deserialize)]
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
