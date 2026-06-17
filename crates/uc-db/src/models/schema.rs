use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
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
