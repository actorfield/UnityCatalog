//! The typed representation of a resident row.
//!
//! Replaces `serde_json::Value` as the snapshot's storage. `Value` was
//! convenient — one map served every kind without a variant per kind — but it
//! is expensive twice over: each row became a `Map<String, Value>` with a
//! heap-allocated `String` per field name, repeated for every row, and every
//! read paid a `from_value` deserialisation to get a typed struct back.
//!
//! The wire format is unchanged. Commits and checkpoints still carry `body` as
//! arbitrary JSON, so `Row` is a storage detail: it is constructed on the way
//! in and matched on the way out.

use super::action::EntityKind;
use crate::models::{
    casbin::CasbinRule,
    catalog::CatalogRow,
    credential::CredentialRow,
    external_location::ExternalLocationRow,
    function::{FunctionParamRow, FunctionRow},
    metastore::MetastoreRow,
    model::{ModelVersionRow, RegisteredModelRow},
    property::PropertyRow,
    schema::SchemaRow,
    staging::StagingTableRow,
    table::{ColumnRow, TableRow},
    user::UserRow,
    volume::VolumeRow,
};
use uc_errors::{ErrorCode, UcError};

/// One row, of whichever kind.
///
/// There is no `DeltaCommit` variant: delta commits live in per-table log
/// partitions where the version is the object key, and are never resident. Nor
/// a `Dependency` one — `uc_dependencies` had no repository and nothing ever
/// wrote it. Typing the storage is what made both obvious.
#[derive(Debug, Clone)]
pub enum Row {
    Metastore(MetastoreRow),
    Catalog(CatalogRow),
    Schema(SchemaRow),
    Table(TableRow),
    Column(ColumnRow),
    Volume(VolumeRow),
    Function(FunctionRow),
    FunctionParameter(FunctionParamRow),
    RegisteredModel(RegisteredModelRow),
    ModelVersion(ModelVersionRow),
    StagingTable(StagingTableRow),
    User(UserRow),
    Credential(CredentialRow),
    ExternalLocation(ExternalLocationRow),
    Property(PropertyRow),
    CasbinRule(CasbinRule),
}

/// Build a `Row` from a wire body.
///
/// A body that does not match its declared kind is an error rather than a
/// silently empty row: at replay it means the log and this build disagree, and
/// continuing would materialise state that was never committed.
pub fn from_body(kind: EntityKind, body: &serde_json::Value) -> Result<Row, UcError> {
    fn parse<T: serde::de::DeserializeOwned>(
        kind: EntityKind,
        body: &serde_json::Value,
    ) -> Result<T, UcError> {
        serde_json::from_value(body.clone())
            .map_err(|e| UcError::new(ErrorCode::Internal, format!("corrupt {kind:?} row: {e}")))
    }

    Ok(match kind {
        EntityKind::Metastore => Row::Metastore(parse(kind, body)?),
        EntityKind::Catalog => Row::Catalog(parse(kind, body)?),
        EntityKind::Schema => Row::Schema(parse(kind, body)?),
        EntityKind::Table => Row::Table(parse(kind, body)?),
        EntityKind::Column => Row::Column(parse(kind, body)?),
        EntityKind::Volume => Row::Volume(parse(kind, body)?),
        EntityKind::Function => Row::Function(parse(kind, body)?),
        EntityKind::FunctionParameter => Row::FunctionParameter(parse(kind, body)?),
        EntityKind::RegisteredModel => Row::RegisteredModel(parse(kind, body)?),
        EntityKind::ModelVersion => Row::ModelVersion(parse(kind, body)?),
        EntityKind::StagingTable => Row::StagingTable(parse(kind, body)?),
        EntityKind::User => Row::User(parse(kind, body)?),
        EntityKind::Credential => Row::Credential(parse(kind, body)?),
        EntityKind::ExternalLocation => Row::ExternalLocation(parse(kind, body)?),
        EntityKind::Property => Row::Property(parse(kind, body)?),
        EntityKind::CasbinRule => Row::CasbinRule(parse(kind, body)?),
    })
}

impl Row {
    /// Back to the wire body, for checkpoint encoding.
    pub fn to_body(&self) -> Result<serde_json::Value, UcError> {
        let v = |r: Result<serde_json::Value, serde_json::Error>| {
            r.map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))
        };
        match self {
            Row::Metastore(r) => v(serde_json::to_value(r)),
            Row::Catalog(r) => v(serde_json::to_value(r)),
            Row::Schema(r) => v(serde_json::to_value(r)),
            Row::Table(r) => v(serde_json::to_value(r)),
            Row::Column(r) => v(serde_json::to_value(r)),
            Row::Volume(r) => v(serde_json::to_value(r)),
            Row::Function(r) => v(serde_json::to_value(r)),
            Row::FunctionParameter(r) => v(serde_json::to_value(r)),
            Row::RegisteredModel(r) => v(serde_json::to_value(r)),
            Row::ModelVersion(r) => v(serde_json::to_value(r)),
            Row::StagingTable(r) => v(serde_json::to_value(r)),
            Row::User(r) => v(serde_json::to_value(r)),
            Row::Credential(r) => v(serde_json::to_value(r)),
            Row::ExternalLocation(r) => v(serde_json::to_value(r)),
            Row::Property(r) => v(serde_json::to_value(r)),
            Row::CasbinRule(r) => v(serde_json::to_value(r)),
        }
    }

    /// The kind this row belongs to.
    pub fn kind(&self) -> EntityKind {
        match self {
            Row::Metastore(_) => EntityKind::Metastore,
            Row::Catalog(_) => EntityKind::Catalog,
            Row::Schema(_) => EntityKind::Schema,
            Row::Table(_) => EntityKind::Table,
            Row::Column(_) => EntityKind::Column,
            Row::Volume(_) => EntityKind::Volume,
            Row::Function(_) => EntityKind::Function,
            Row::FunctionParameter(_) => EntityKind::FunctionParameter,
            Row::RegisteredModel(_) => EntityKind::RegisteredModel,
            Row::ModelVersion(_) => EntityKind::ModelVersion,
            Row::StagingTable(_) => EntityKind::StagingTable,
            Row::User(_) => EntityKind::User,
            Row::Credential(_) => EntityKind::Credential,
            Row::ExternalLocation(_) => EntityKind::ExternalLocation,
            Row::Property(_) => EntityKind::Property,
            Row::CasbinRule(_) => EntityKind::CasbinRule,
        }
    }
}

/// Extract a typed row, or report the mismatch.
///
/// Used by the repo layer in place of `serde_json::from_value`, so a read is a
/// clone rather than a deserialisation.
#[macro_export]
macro_rules! typed_row {
    ($row:expr, $variant:path, $what:literal) => {
        match $row {
            $variant(r) => Ok(r.clone()),
            other => Err(uc_errors::UcError::new(
                uc_errors::ErrorCode::Internal,
                format!("expected {} row, found {:?}", $what, other.kind()),
            )),
        }
    };
}
