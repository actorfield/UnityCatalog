use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg_attr(feature = "sql", derive(sqlx::FromRow))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaRow {
    pub id: Uuid,
    pub catalog_id: Uuid,
    pub name: String,
    pub comment: Option<String>,
    pub owner: Option<String>,
    pub created_at: i64,
    pub created_by: Option<String>,
    pub updated_at: Option<i64>,
    pub updated_by: Option<String>,
    pub storage_root: Option<String>,
    pub storage_location: Option<String>,
}
