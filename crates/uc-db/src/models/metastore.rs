use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct MetastoreRow {
    pub id: Uuid,
    pub name: String,
}
