use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg_attr(feature = "sql", derive(sqlx::FromRow))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisteredModelRow {
    pub id: Uuid,
    pub schema_id: Uuid,
    pub name: String,
    pub owner: Option<String>,
    pub created_at: Option<i64>,
    pub created_by: Option<String>,
    pub updated_at: Option<i64>,
    pub updated_by: Option<String>,
    pub comment: Option<String>,
    pub url: Option<String>,
    pub max_version_number: Option<i32>,
}

#[cfg_attr(feature = "sql", derive(sqlx::FromRow))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelVersionRow {
    pub id: Uuid,
    pub registered_model_id: Uuid,
    pub version: Option<i32>,
    pub source: Option<String>,
    pub run_id: Option<String>,
    pub status: Option<String>,
    pub owner: Option<String>,
    pub created_at: Option<i64>,
    pub created_by: Option<String>,
    pub updated_at: Option<i64>,
    pub updated_by: Option<String>,
    pub comment: Option<String>,
    pub url: Option<String>,
}
