use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct TableRow {
    pub id: Uuid,
    pub schema_id: Uuid,
    pub name: String,
    pub r#type: String,
    pub owner: Option<String>,
    pub created_at: i64,
    pub created_by: Option<String>,
    pub updated_at: Option<i64>,
    pub updated_by: Option<String>,
    pub data_source_format: Option<String>,
    pub comment: Option<String>,
    pub url: Option<String>,
    pub column_count: Option<i32>,
    pub view_definition: Option<String>,
    pub uniform_iceberg_metadata_location: Option<String>,
    pub uniform_iceberg_converted_delta_version: Option<i64>,
    pub uniform_iceberg_converted_delta_timestamp: Option<i64>,
}

#[derive(Debug, Clone, FromRow)]
pub struct ColumnRow {
    pub id: Uuid,
    pub table_id: Uuid,
    pub name: String,
    pub ordinal_position: i32,
    pub type_text: String,
    pub type_json: String,
    pub type_name: String,
    pub type_precision: Option<i32>,
    pub type_scale: Option<i32>,
    pub type_interval_type: Option<String>,
    pub nullable: bool,
    pub comment: Option<String>,
    pub partition_index: Option<i32>,
}
