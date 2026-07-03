/// Managed storage location resolution.
///
/// Mirrors Java's ExternalLocationUtils.getManagedStorageLocation():
///   schema.storage_root → catalog.storage_root → error
///
/// Derived locations follow the pattern:
///   <parent>/schemas/<schema_id>/tables/<table_id>
///   <parent>/schemas/<schema_id>/volumes/<volume_id>
///   <parent>/schemas/<schema_id>/models/<model_id>
use uc_errors::{ErrorCode, UcError};
use uuid::Uuid;

use crate::{
    models::{catalog::CatalogRow, schema::SchemaRow},
    pool::AnyPool,
    repos::{catalog, schema},
};

/// Resolve the managed storage root for a schema, walking up to catalog if needed.
/// Returns an error if neither schema nor catalog has a storage_root configured.
pub async fn resolve_storage_root(
    pool: &AnyPool,
    catalog_name: &str,
    schema_name: &str,
) -> Result<String, UcError> {
    let schema = schema::get_by_full_name(pool, catalog_name, schema_name).await?;
    let catalog = catalog::get_by_name(pool, catalog_name).await?;
    resolve_from_rows(&catalog, &schema)
}

pub fn resolve_from_rows(catalog: &CatalogRow, schema: &SchemaRow) -> Result<String, UcError> {
    // Schema storage_root takes priority over catalog
    if let Some(ref root) = schema.storage_root {
        if !root.is_empty() {
            return Ok(root.trim_end_matches('/').to_string());
        }
    }
    if let Some(ref root) = catalog.storage_root {
        if !root.is_empty() {
            return Ok(root.trim_end_matches('/').to_string());
        }
    }
    Err(UcError::new(
        ErrorCode::InvalidArgument,
        format!(
            "No managed storage configured for {}.{}. \
             Set storage_root on the catalog or schema, \
             or provide an explicit storage_location.",
            catalog.name, schema.name
        ),
    ))
}

/// Derive the storage location for a managed table.
/// Pattern: <storage_root>/schemas/<schema_id>/tables/<table_id>
pub fn managed_table_location(storage_root: &str, schema_id: Uuid, table_id: Uuid) -> String {
    format!(
        "{}/schemas/{}/tables/{}",
        storage_root.trim_end_matches('/'),
        schema_id,
        table_id
    )
}

/// Derive the storage location for a managed volume.
pub fn managed_volume_location(storage_root: &str, schema_id: Uuid, volume_id: Uuid) -> String {
    format!(
        "{}/schemas/{}/volumes/{}",
        storage_root.trim_end_matches('/'),
        schema_id,
        volume_id
    )
}

/// Derive the storage location for a registered model.
pub fn managed_model_location(storage_root: &str, schema_id: Uuid, model_id: Uuid) -> String {
    format!(
        "{}/schemas/{}/models/{}",
        storage_root.trim_end_matches('/'),
        schema_id,
        model_id
    )
}

/// Derive the storage location for a model version.
pub fn managed_model_version_location(model_location: &str, version: i32) -> String {
    format!(
        "{}/versions/{}",
        model_location.trim_end_matches('/'),
        version
    )
}

/// Derive the staging location for a staging table.
/// Pattern: <storage_root>/schemas/<schema_id>/staging/<staging_id>
pub fn staging_table_location(storage_root: &str, schema_id: Uuid, staging_id: Uuid) -> String {
    format!(
        "{}/schemas/{}/staging/{}",
        storage_root.trim_end_matches('/'),
        schema_id,
        staging_id
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_catalog(storage_root: Option<&str>) -> CatalogRow {
        CatalogRow {
            id: Uuid::new_v4(),
            name: "cat".into(),
            comment: None,
            owner: None,
            created_at: 0,
            created_by: None,
            updated_at: None,
            updated_by: None,
            storage_root: storage_root.map(String::from),
            storage_location: None,
        }
    }

    fn fake_schema(storage_root: Option<&str>) -> SchemaRow {
        SchemaRow {
            id: Uuid::new_v4(),
            catalog_id: Uuid::new_v4(),
            name: "sch".into(),
            comment: None,
            owner: None,
            created_at: 0,
            created_by: None,
            updated_at: None,
            updated_by: None,
            storage_root: storage_root.map(String::from),
            storage_location: None,
        }
    }

    #[test]
    fn schema_root_takes_priority() {
        let cat = fake_catalog(Some("s3://cat-bucket"));
        let sch = fake_schema(Some("s3://schema-bucket"));
        assert_eq!(resolve_from_rows(&cat, &sch).unwrap(), "s3://schema-bucket");
    }

    #[test]
    fn falls_back_to_catalog_root() {
        let cat = fake_catalog(Some("s3://cat-bucket"));
        let sch = fake_schema(None);
        assert_eq!(resolve_from_rows(&cat, &sch).unwrap(), "s3://cat-bucket");
    }

    #[test]
    fn error_when_no_root_configured() {
        let cat = fake_catalog(None);
        let sch = fake_schema(None);
        assert!(resolve_from_rows(&cat, &sch).is_err());
    }

    #[test]
    fn managed_locations_use_correct_pattern() {
        let root = "s3://bucket";
        let sid = Uuid::nil();
        let tid = Uuid::nil();
        assert!(managed_table_location(root, sid, tid).contains("/schemas/"));
        assert!(managed_table_location(root, sid, tid).contains("/tables/"));
        assert!(managed_volume_location(root, sid, tid).contains("/volumes/"));
        assert!(managed_model_location(root, sid, tid).contains("/models/"));
        assert!(staging_table_location(root, sid, tid).contains("/staging/"));
    }

    #[test]
    fn trailing_slash_stripped() {
        let root = "s3://bucket/";
        let sid = Uuid::nil();
        let tid = Uuid::nil();
        assert!(!managed_table_location(root, sid, tid).contains("//schemas"));
    }
}
