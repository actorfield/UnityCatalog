use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct FunctionRow {
    pub id: Uuid,
    pub schema_id: Uuid,
    pub name: String,
    pub comment: Option<String>,
    pub owner: Option<String>,
    pub created_at: Option<i64>,
    pub created_by: Option<String>,
    pub updated_at: Option<i64>,
    pub updated_by: Option<String>,
    pub data_type: Option<String>,
    pub full_data_type: Option<String>,
    pub external_language: Option<String>,
    pub is_deterministic: Option<bool>,
    pub is_null_call: Option<bool>,
    pub parameter_style: Option<String>,
    pub routine_body: Option<String>,
    pub routine_definition: Option<String>,
    pub sql_data_access: Option<String>,
    pub security_type: Option<String>,
    pub specific_name: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
pub struct FunctionParamRow {
    pub id: Uuid,
    pub function_id: Uuid,
    pub name: String,
    pub input_or_return: i16, // 0=INPUT, 1=RETURN
    pub ordinal_position: i32,
    pub type_text: Option<String>,
    pub type_json: Option<String>,
    pub type_name: Option<String>,
    pub type_precision: Option<i32>,
    pub type_scale: Option<i32>,
    pub type_interval_type: Option<String>,
    pub comment: Option<String>,
    pub parameter_mode: Option<String>,
    pub parameter_default: Option<String>,
}
