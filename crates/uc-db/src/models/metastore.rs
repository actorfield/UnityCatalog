use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetastoreRow {
    pub id: Uuid,
    pub name: String,
}
