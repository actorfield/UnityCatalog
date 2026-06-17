use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct VolumeRow {
    pub id: Uuid,
    pub schema_id: Uuid,
    pub name: String,
    pub comment: Option<String>,
    pub storage_location: Option<String>,
    pub owner: Option<String>,
    pub created_at: i64,
    pub created_by: Option<String>,
    pub updated_at: Option<i64>,
    pub updated_by: Option<String>,
    pub volume_type: String,
}
