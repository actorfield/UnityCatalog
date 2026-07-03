//! Types derived from api/delta.yaml — Delta Lake CCv2 protocol.
//!
//! Key serde decisions:
//! - DeltaTableUpdate uses #[serde(tag = "action")] with explicit per-variant renames
//! - DeltaTableRequirement uses #[serde(tag = "type")]
//! - DeltaDataType uses #[serde(untagged)] with compound variants BEFORE Primitive(String)
//! - Delta field names: per-field #[serde(rename = "...")] — no rename_all at struct level
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// ── Delta Config ──────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct DeltaCatalogConfig {
    pub endpoints: Vec<String>,
    #[serde(rename = "protocol-version")]
    pub protocol_version: String,
}

// ── Delta Protocol ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaProtocol {
    #[serde(rename = "minReaderVersion")]
    pub min_reader_version: i32,
    #[serde(rename = "minWriterVersion")]
    pub min_writer_version: i32,
    #[serde(rename = "readerFeatures", skip_serializing_if = "Option::is_none")]
    pub reader_features: Option<Vec<String>>,
    #[serde(rename = "writerFeatures", skip_serializing_if = "Option::is_none")]
    pub writer_features: Option<Vec<String>>,
}

// ── Delta Schema Types (recursive) ───────────────────────────────────────────

/// Delta Lake column data type.
/// Wire format: bare string for primitives ("long", "string", "decimal(10,2)"),
/// or a JSON object for compound types.
/// Compound variants are listed BEFORE Primitive so serde's untagged matching
/// tries object deserialization first (untagged tries variants in declaration order).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DeltaDataType {
    Struct(DeltaStructType),
    Array(DeltaArrayType),
    Map(DeltaMapType),
    /// Primitive type string: "long", "string", "boolean", "decimal(10,2)", etc.
    Primitive(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaStructType {
    #[serde(rename = "type")]
    pub type_tag: String,
    pub fields: Vec<DeltaStructField>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaStructField {
    pub name: String,
    #[serde(rename = "type")]
    pub field_type: Box<DeltaDataType>,
    pub nullable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaArrayType {
    #[serde(rename = "type")]
    pub type_tag: String,
    #[serde(rename = "elementType")]
    pub element_type: Box<DeltaDataType>,
    #[serde(rename = "containsNull")]
    pub contains_null: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaMapType {
    #[serde(rename = "type")]
    pub type_tag: String,
    #[serde(rename = "keyType")]
    pub key_type: Box<DeltaDataType>,
    #[serde(rename = "valueType")]
    pub value_type: Box<DeltaDataType>,
    #[serde(rename = "valueContainsNull")]
    pub value_contains_null: bool,
}

// ── Delta Commit ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaCommit {
    pub version: i64,
    pub timestamp: i64,
    #[serde(rename = "file-name")]
    pub file_name: String,
    #[serde(rename = "file-size")]
    pub file_size: i64,
    #[serde(rename = "file-modification-timestamp")]
    pub file_modification_timestamp: i64,
}

// ── Delta Uniform (Delta + Iceberg metadata) ──────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaUniformIcebergMetadata {
    #[serde(rename = "metadata-location", skip_serializing_if = "Option::is_none")]
    pub metadata_location: Option<String>,
    #[serde(
        rename = "converted-delta-version",
        skip_serializing_if = "Option::is_none"
    )]
    pub converted_delta_version: Option<i64>,
    #[serde(
        rename = "converted-delta-timestamp",
        skip_serializing_if = "Option::is_none"
    )]
    pub converted_delta_timestamp: Option<i64>,
    #[serde(
        rename = "base-converted-delta-version",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_converted_delta_version: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaUniformMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iceberg: Option<DeltaUniformIcebergMetadata>,
}

// ── Delta Domain Metadata ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaClusteringDomainMetadata {
    #[serde(rename = "clusteringColumns")]
    pub clustering_columns: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaRowTrackingDomainMetadata {
    #[serde(rename = "rowIdHighWaterMark")]
    pub row_id_high_water_mark: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaDomainMetadataUpdates {
    #[serde(rename = "delta.clustering", skip_serializing_if = "Option::is_none")]
    pub clustering: Option<DeltaClusteringDomainMetadata>,
    #[serde(rename = "delta.rowTracking", skip_serializing_if = "Option::is_none")]
    pub row_tracking: Option<DeltaRowTrackingDomainMetadata>,
}

// ── Delta Table Table Metadata ────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct DeltaTableMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    #[serde(rename = "table-type", skip_serializing_if = "Option::is_none")]
    pub table_type: Option<String>,
    #[serde(rename = "table-uuid", skip_serializing_if = "Option::is_none")]
    pub table_uuid: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(rename = "created-time", skip_serializing_if = "Option::is_none")]
    pub created_time: Option<i64>,
    #[serde(rename = "updated-time", skip_serializing_if = "Option::is_none")]
    pub updated_time: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub columns: Option<DeltaStructType>,
    #[serde(rename = "partition-columns", skip_serializing_if = "Option::is_none")]
    pub partition_columns: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, String>>,
    #[serde(
        rename = "last-commit-version",
        skip_serializing_if = "Option::is_none"
    )]
    pub last_commit_version: Option<i64>,
    #[serde(
        rename = "last-commit-timestamp-ms",
        skip_serializing_if = "Option::is_none"
    )]
    pub last_commit_timestamp_ms: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeltaLoadTableResponse {
    pub metadata: DeltaTableMetadata,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commits: Option<Vec<DeltaCommit>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uniform: Option<DeltaUniformMetadata>,
    #[serde(
        rename = "latest-table-version",
        skip_serializing_if = "Option::is_none"
    )]
    pub latest_table_version: Option<i64>,
}

// ── Delta Create Table ────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct DeltaCreateTableRequest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(rename = "table-type", skip_serializing_if = "Option::is_none")]
    pub table_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub columns: Option<DeltaStructType>,
    #[serde(rename = "partition-columns", skip_serializing_if = "Option::is_none")]
    pub partition_columns: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<DeltaProtocol>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, String>>,
    #[serde(
        rename = "last-commit-timestamp-ms",
        skip_serializing_if = "Option::is_none"
    )]
    pub last_commit_timestamp_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uniform: Option<DeltaUniformMetadata>,
}

// ── Delta Staging Table ───────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct DeltaCreateStagingTableRequest {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeltaStagingTableResponse {
    #[serde(rename = "table-id")]
    pub table_id: Uuid,
    #[serde(rename = "table-type", skip_serializing_if = "Option::is_none")]
    pub table_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(
        rename = "storage-credentials",
        skip_serializing_if = "Option::is_none"
    )]
    pub storage_credentials: Option<Vec<DeltaStorageCredential>>,
    #[serde(rename = "required-protocol", skip_serializing_if = "Option::is_none")]
    pub required_protocol: Option<DeltaProtocol>,
    #[serde(rename = "suggested-protocol", skip_serializing_if = "Option::is_none")]
    pub suggested_protocol: Option<DeltaProtocol>,
    #[serde(
        rename = "required-properties",
        skip_serializing_if = "Option::is_none"
    )]
    pub required_properties: Option<HashMap<String, String>>,
    #[serde(
        rename = "suggested-properties",
        skip_serializing_if = "Option::is_none"
    )]
    pub suggested_properties: Option<HashMap<String, String>>,
}

// ── Delta Update Table (CCv2) ─────────────────────────────────────────────────

/// Optimistic concurrency requirements before applying updates.
/// Per-variant renames are explicit — no rename_all on the enum.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DeltaTableRequirement {
    #[serde(rename = "assert-table-uuid")]
    AssertTableUuid { uuid: Uuid },
    #[serde(rename = "assert-etag")]
    AssertEtag { etag: String },
}

/// Discriminated union of all update actions for a Delta table.
/// Tag field is "action". Variants use explicit renames (not rename_all).
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "action")]
pub enum DeltaTableUpdate {
    #[serde(rename = "set-properties")]
    SetProperties { updates: HashMap<String, String> },

    #[serde(rename = "remove-properties")]
    RemoveProperties { removals: Vec<String> },

    #[serde(rename = "set-columns")]
    SetColumns { columns: DeltaStructType },

    #[serde(rename = "set-table-comment")]
    SetTableComment { comment: String },

    #[serde(rename = "add-commit")]
    AddCommit {
        commit: DeltaCommit,
        #[serde(skip_serializing_if = "Option::is_none")]
        uniform: Option<DeltaUniformMetadata>,
    },

    #[serde(rename = "set-latest-backfilled-version")]
    SetLatestBackfilledVersion {
        #[serde(rename = "latest-published-version")]
        latest_published_version: i64,
    },

    #[serde(rename = "set-protocol")]
    SetProtocol { protocol: DeltaProtocol },

    #[serde(rename = "set-domain-metadata")]
    SetDomainMetadata { updates: DeltaDomainMetadataUpdates },

    #[serde(rename = "remove-domain-metadata")]
    RemoveDomainMetadata { domains: Vec<String> },

    #[serde(rename = "set-partition-columns")]
    SetPartitionColumns {
        #[serde(rename = "partition-columns")]
        partition_columns: Vec<String>,
    },

    #[serde(rename = "update-metadata-snapshot-version")]
    UpdateMetadataSnapshotVersion {
        #[serde(rename = "last-commit-version")]
        last_commit_version: i64,
        #[serde(rename = "last-commit-timestamp-ms")]
        last_commit_timestamp_ms: i64,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeltaUpdateTableRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requirements: Option<Vec<DeltaTableRequirement>>,
    pub updates: Vec<DeltaTableUpdate>,
}

// ── Delta Rename Table ────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct DeltaRenameTableRequest {
    #[serde(rename = "new-name")]
    pub new_name: String,
}

// ── Delta Credentials ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct DeltaAwsCredentialConfig {
    #[serde(rename = "awsAccessKey")]
    pub aws_access_key: String,
    #[serde(rename = "awsSecretKey")]
    pub aws_secret_key: String,
    #[serde(rename = "awsSessionToken", skip_serializing_if = "Option::is_none")]
    pub aws_session_token: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeltaStorageCredential {
    pub prefix: String,
    pub operation: DeltaCredentialOperation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,
    #[serde(rename = "expiration-time-ms", skip_serializing_if = "Option::is_none")]
    pub expiration_time_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DeltaCredentialOperation {
    Read,
    ReadWrite,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeltaCredentialsResponse {
    #[serde(rename = "storage-credentials")]
    pub storage_credentials: Vec<DeltaStorageCredential>,
}

// ── Delta Metrics ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct DeltaReportMetricsRequest {
    #[serde(rename = "table-id")]
    pub table_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report: Option<serde_json::Value>,
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_table_update_set_properties_round_trip() {
        let update = DeltaTableUpdate::SetProperties {
            updates: [("k".to_string(), "v".to_string())].into(),
        };
        let json = serde_json::to_string(&update).unwrap();
        assert!(json.contains(r#""action":"set-properties""#));
        let back: DeltaTableUpdate = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, DeltaTableUpdate::SetProperties { .. }));
    }

    #[test]
    fn delta_table_update_add_commit_round_trip() {
        let json = r#"{"action":"add-commit","commit":{"version":42,"timestamp":1000,"file-name":"abc","file-size":100,"file-modification-timestamp":999}}"#;
        let update: DeltaTableUpdate = serde_json::from_str(json).unwrap();
        assert!(matches!(update, DeltaTableUpdate::AddCommit { .. }));
    }

    #[test]
    fn delta_requirement_assert_uuid_round_trip() {
        let uuid = Uuid::new_v4();
        let req = DeltaTableRequirement::AssertTableUuid { uuid };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""type":"assert-table-uuid""#));
        let back: DeltaTableRequirement = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            back,
            DeltaTableRequirement::AssertTableUuid { .. }
        ));
    }

    #[test]
    fn delta_data_type_primitive_round_trip() {
        let dt: DeltaDataType = serde_json::from_str(r#""long""#).unwrap();
        assert!(matches!(dt, DeltaDataType::Primitive(ref s) if s == "long"));
        let back = serde_json::to_string(&dt).unwrap();
        assert_eq!(back, r#""long""#);
    }

    #[test]
    fn delta_data_type_struct_round_trip() {
        let json = r#"{"type":"struct","fields":[{"name":"id","type":"long","nullable":false}]}"#;
        let dt: DeltaDataType = serde_json::from_str(json).unwrap();
        assert!(matches!(dt, DeltaDataType::Struct(_)));
    }

    #[test]
    fn delta_data_type_array_round_trip() {
        let json = r#"{"type":"array","elementType":"string","containsNull":true}"#;
        let dt: DeltaDataType = serde_json::from_str(json).unwrap();
        assert!(matches!(dt, DeltaDataType::Array(_)));
    }

    #[test]
    fn delta_data_type_map_round_trip() {
        let json =
            r#"{"type":"map","keyType":"string","valueType":"long","valueContainsNull":false}"#;
        let dt: DeltaDataType = serde_json::from_str(json).unwrap();
        assert!(matches!(dt, DeltaDataType::Map(_)));
    }

    #[test]
    fn delta_update_table_full_request() {
        let json = r#"{
            "requirements": [{"type":"assert-etag","etag":"abc123"}],
            "updates": [
                {"action":"set-properties","updates":{"key":"val"}},
                {"action":"set-protocol","protocol":{"minReaderVersion":1,"minWriterVersion":2}}
            ]
        }"#;
        let req: DeltaUpdateTableRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.updates.len(), 2);
        assert_eq!(req.requirements.as_ref().unwrap().len(), 1);
    }
}
