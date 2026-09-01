use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg_attr(feature = "sql", derive(sqlx::FromRow))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetastoreRow {
    pub id: Uuid,
    pub name: String,
}
