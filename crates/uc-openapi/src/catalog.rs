//! Types derived from api/all.yaml — the main Unity Catalog REST API.
use serde::{de, Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Deserialize `properties` as either a `HashMap<String,String>` or a JSON string containing one.
/// The Java client sends `properties: "{}"` (a string), not `properties: {}` (an object).
pub fn deser_props_or_string<'de, D>(deserializer: D) -> Result<Option<HashMap<String, String>>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrMap {
        Map(HashMap<String, String>),
        Str(String),
        Null,
    }
    match StringOrMap::deserialize(deserializer)? {
        StringOrMap::Map(m) => Ok(Some(m)),
        StringOrMap::Str(s) => {
            if s.trim().is_empty() || s.trim() == "{}" || s.trim() == "\"{}\"" {
                Ok(Some(HashMap::new()))
            } else {
                serde_json::from_str::<HashMap<String, String>>(&s)
                    .map(Some)
                    .map_err(de::Error::custom)
            }
        }
        StringOrMap::Null => Ok(None),
    }
}

// ── Metastore ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct MetastoreSummary {
    pub metastore_id: Uuid,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
}

// ── Catalogs ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct CatalogInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_location: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateCatalog {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_root: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateCatalog {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListCatalogsResponse {
    pub catalogs: Vec<CatalogInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

// ── Schemas ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct SchemaInfo {
    pub name: String,
    pub catalog_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_location: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateSchema {
    pub name: String,
    pub catalog_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_root: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateSchema {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListSchemasResponse {
    pub schemas: Vec<SchemaInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

// ── Tables ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TableType {
    Managed,
    External,
    StreamingTable,
    MaterializedView,
    MetricView,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DataSourceFormat {
    Delta,
    Csv,
    Json,
    Avro,
    Parquet,
    Orc,
    Text,
    UnityCatalog,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ColumnTypeName {
    Boolean,
    Byte,
    Short,
    Int,
    Long,
    Float,
    Double,
    Date,
    Timestamp,
    TimestampNtz,
    String,
    Binary,
    Decimal,
    Interval,
    Array,
    Struct,
    Map,
    Variant,
    Char,
    Null,
    UserDefinedType,
    TableType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_json: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_name: Option<ColumnTypeName>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_precision: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_scale: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_interval_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nullable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partition_index: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TableInfo {
    pub name: String,
    pub catalog_name: String,
    pub schema_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table_type: Option<TableType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_source_format: Option<DataSourceFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub columns: Option<Vec<ColumnInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view_definition: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateTable {
    pub name: String,
    pub catalog_name: String,
    pub schema_name: String,
    pub table_type: TableType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_source_format: Option<DataSourceFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub columns: Option<Vec<ColumnInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view_definition: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListTablesResponse {
    pub tables: Vec<TableInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

// ── Volumes ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VolumeType {
    Managed,
    External,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VolumeInfo {
    pub catalog_name: String,
    pub schema_name: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_id: Option<Uuid>,
    pub volume_type: VolumeType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateVolumeRequestContent {
    pub catalog_name: String,
    pub schema_name: String,
    pub name: String,
    pub volume_type: VolumeType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateVolumeRequestContent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListVolumesResponseContent {
    pub volumes: Vec<VolumeInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

// ── Functions ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct FunctionParameterInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_json: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_name: Option<ColumnTypeName>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_precision: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_scale: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_interval_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameter_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameter_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameter_default: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FunctionParameterInfos {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Vec<FunctionParameterInfo>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FunctionInfo {
    pub name: String,
    pub catalog_name: String,
    pub schema_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_params: Option<FunctionParameterInfos>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_params: Option<FunctionParameterInfos>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_type: Option<ColumnTypeName>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_data_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub routine_body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub routine_definition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameter_style: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_deterministic: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sql_data_access: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_null_call: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub specific_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// Accepts either a JSON map {"k":"v"} or a legacy string "{}" from the Java client.
    #[serde(skip_serializing_if = "Option::is_none", default, deserialize_with = "deser_props_or_string")]
    pub properties: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_language: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateFunctionRequest {
    pub function_info: FunctionInfo,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListFunctionsResponse {
    pub functions: Vec<FunctionInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

// ── Registered Models ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct RegisteredModelInfo {
    pub name: String,
    pub catalog_name: String,
    pub schema_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateRegisteredModel {
    pub name: String,
    pub catalog_name: String,
    pub schema_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateRegisteredModel {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListRegisteredModelsResponse {
    pub registered_models: Vec<RegisteredModelInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

// ── Model Versions ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ModelVersionStatus {
    ModelVersionStatusUnknown,
    PendingRegistration,
    FailedRegistration,
    Ready,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModelVersionInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub catalog_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<ModelVersionStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateModelVersion {
    pub model_name: String,
    pub catalog_name: String,
    pub schema_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateModelVersion {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FinalizeModelVersion {
    pub status: ModelVersionStatus,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListModelVersionsResponse {
    pub model_versions: Vec<ModelVersionInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

// ── Credentials ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CredentialPurpose {
    ServicePrincipal,
    GcsServiceAccount,
    AwsIamRole,
    AzureManagedIdentity,
    AwsAssumeRole,
    DatabricksGcpServiceAccount,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AwsIamRoleRequest {
    pub role_arn: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unity_catalog_iam_arn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CredentialInfo {
    pub id: Uuid,
    pub name: String,
    pub purpose: CredentialPurpose,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aws_iam_role: Option<AwsIamRoleRequest>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateCredentialRequest {
    pub name: String,
    pub purpose: CredentialPurpose,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aws_iam_role: Option<AwsIamRoleRequest>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateCredentialRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aws_iam_role: Option<AwsIamRoleRequest>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListCredentialsResponse {
    pub credentials: Vec<CredentialInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

// ── External Locations ────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct ExternalLocationInfo {
    pub id: Uuid,
    pub name: String,
    pub url: String,
    pub credential_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateExternalLocation {
    pub name: String,
    pub url: String,
    pub credential_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateExternalLocation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListExternalLocationsResponse {
    pub external_locations: Vec<ExternalLocationInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

// ── Permissions ───────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct PrivilegeAssignment {
    pub principal: String,
    pub privileges: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PermissionsList {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub securable_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_name: Option<String>,
    pub privilege_assignments: Vec<PrivilegeAssignment>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PrivilegeAssignmentChange {
    pub principal: String,
    pub add: Vec<String>,
    pub remove: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdatePermissions {
    pub changes: Vec<PrivilegeAssignmentChange>,
}

// ── Temporary Credentials ─────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
#[derive(Clone)]
pub struct AwsCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expiration: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[derive(Clone)]
pub struct AzureUserDelegationSas {
    pub sas_token: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[derive(Clone)]
pub struct GcpOauthToken {
    pub oauth_token: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[derive(Clone)]
pub struct TemporaryCredentials {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aws_temp_credentials: Option<AwsCredentials>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub azure_user_delegation_sas: Option<AzureUserDelegationSas>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gcp_oauth_token: Option<GcpOauthToken>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expiration_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl Default for TemporaryCredentials {
    fn default() -> Self {
        Self {
            aws_temp_credentials: None,
            azure_user_delegation_sas: None,
            gcp_oauth_token: None,
            expiration_time: None,
            url: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CredentialOperation {
    Read,
    ReadWrite,
    ReadVolume,
    WriteVolume,
    ReadModelVersion,
    ReadWriteModelVersion,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GenerateTemporaryTableCredential {
    pub table_id: Uuid,
    pub operation: CredentialOperation,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GenerateTemporaryVolumeCredential {
    pub volume_id: Uuid,
    pub operation: CredentialOperation,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GenerateTemporaryModelVersionCredential {
    pub catalog_name: String,
    pub schema_name: String,
    pub model_name: String,
    pub version: i64,
    pub operation: CredentialOperation,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GenerateTemporaryPathCredential {
    pub url: String,
    pub operation: CredentialOperation,
}

// ── Staging Tables ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateStagingTable {
    pub name: String,
    pub catalog_name: String,
    pub schema_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StagingTableInfo {
    pub table_id: Uuid,
    pub staging_location: String,
    pub schema_name: String,
    pub catalog_name: String,
}

// ── Delta Commits ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct GetCommitsRequest {
    pub table_full_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starting_version: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ending_version: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CommitInfo {
    pub version: i64,
    pub timestamp: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_size: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_modification_timestamp: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetCommitsResponse {
    pub commits_info: Vec<CommitInfo>,
    pub latest_table_version: i64,
}
