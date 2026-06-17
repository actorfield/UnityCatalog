use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct PropertyRow {
    pub id: Uuid,
    pub entity_id: Uuid,
    pub entity_type: String,
    pub property_key: String,
    pub property_value: String,
}
