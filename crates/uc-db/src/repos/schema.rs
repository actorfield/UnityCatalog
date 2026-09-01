//! Log-structured body for repos::schema. Signatures identical to schema.rs.

use crate::models::schema::SchemaRow;
use crate::store::action::{Action, EntityKind};
use crate::store::{Snapshot, Store};
use uc_errors::{ErrorCode, UcError};
use uuid::Uuid;

fn row_of(v: &serde_json::Value) -> Result<SchemaRow, UcError> {
    serde_json::from_value(v.clone())
        .map_err(|e| UcError::new(ErrorCode::Internal, format!("corrupt schema row: {e}")))
}

/// UNIQUE(catalog_id, name), rendered as the store's natural key.
fn nk(catalog_id: Uuid, name: &str) -> String {
    format!("{catalog_id}\u{0}{name}")
}

/// Prefix covering every schema in one catalog.
fn prefix(catalog_id: Uuid) -> String {
    format!("{catalog_id}\u{0}")
}

#[allow(clippy::too_many_arguments)]
pub async fn create(
    store: &Store,
    id: Uuid,
    catalog_id: Uuid,
    name: &str,
    comment: Option<&str>,
    owner: Option<&str>,
    created_by: Option<&str>,
    storage_root: Option<&str>,
    created_at: i64,
) -> Result<SchemaRow, UcError> {
    let row = SchemaRow {
        id,
        catalog_id,
        name: name.to_string(),
        comment: comment.map(str::to_owned),
        owner: owner.map(str::to_owned),
        created_at,
        created_by: created_by.map(str::to_owned),
        updated_at: None,
        updated_by: None,
        storage_root: storage_root.map(str::to_owned),
        storage_location: None,
    };

    store
        .commit("CREATE SCHEMA", |snap| {
            if snap
                .get_by_natural_key(EntityKind::Schema, &nk(catalog_id, name))
                .is_some()
            {
                return Err(UcError::new(
                    ErrorCode::SchemaAlreadyExists,
                    format!("Schema '{}' already exists", name),
                ));
            }
            let body = serde_json::to_value(&row)
                .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
            Ok((
                vec![Action::Upsert { kind: EntityKind::Schema, id, body }],
                row.clone(),
            ))
        })
        .await
}

/// The only JOIN in the SQL repo layer: resolve the catalog by name, then the
/// schema by (catalog_id, name). Two index lookups instead of a join.
pub async fn get_by_full_name(
    store: &Store,
    catalog_name: &str,
    schema_name: &str,
) -> Result<SchemaRow, UcError> {
    let snap = store.snapshot().await;
    let not_found = || {
        UcError::new(
            ErrorCode::SchemaNotFound,
            format!("Schema '{}.{}' not found", catalog_name, schema_name),
        )
    };
    // A missing catalog surfaces as SchemaNotFound, matching the SQL: the join
    // produced zero rows either way, and callers depend on that code.
    let catalog = snap
        .get_by_natural_key(EntityKind::Catalog, catalog_name)
        .ok_or_else(not_found)?;
    let catalog_id: Uuid = serde_json::from_value(catalog["id"].clone())
        .map_err(|e| UcError::new(ErrorCode::Internal, format!("corrupt catalog row: {e}")))?;

    snap.get_by_natural_key(EntityKind::Schema, &nk(catalog_id, schema_name))
        .ok_or_else(not_found)
        .and_then(row_of)
}

pub async fn get_by_id(store: &Store, id: Uuid) -> Result<SchemaRow, UcError> {
    let snap = store.snapshot().await;
    snap.get(EntityKind::Schema, id)
        .ok_or_else(|| {
            UcError::new(
                ErrorCode::SchemaNotFound,
                format!("Schema '{}' not found", id),
            )
        })
        .and_then(row_of)
}

pub async fn list(
    store: &Store,
    catalog_id: Uuid,
    page_token: Option<&str>,
    max_results: i64,
) -> Result<(Vec<SchemaRow>, Option<String>), UcError> {
    let snap = store.snapshot().await;
    let found = snap.scan_prefix(
        EntityKind::Schema,
        &prefix(catalog_id),
        page_token,
        crate::pagination::over_fetch(max_results),
    );
    let rows: Vec<SchemaRow> = found.into_iter().map(row_of).collect::<Result<_, _>>()?;

    // Off-by-one preserved from the SQL version verbatim; clients page against
    // this behaviour today.
    let (rows, next_token) =
        crate::pagination::page(rows, max_results, |r| r.name.clone());
    Ok((rows, next_token))
}

pub async fn update(
    store: &Store,
    id: Uuid,
    new_name: Option<&str>,
    comment: Option<&str>,
    owner: Option<&str>,
    updated_by: Option<&str>,
    updated_at: i64,
) -> Result<SchemaRow, UcError> {
    store
        .commit("UPDATE SCHEMA", |snap: &Snapshot| {
            let current = snap.get(EntityKind::Schema, id).ok_or_else(|| {
                // The SQL used fetch_one, so a missing row became RowNotFound ->
                // Internal via sqlx_err, not SchemaNotFound. Preserving that
                // would be preserving a 500; flagged as a deliberate change.
                UcError::new(
                    ErrorCode::SchemaNotFound,
                    format!("Schema '{}' not found", id),
                )
            })?;
            let mut row = row_of(current)?;

            if let Some(target) = new_name {
                if target != row.name
                    && snap
                        .get_by_natural_key(EntityKind::Schema, &nk(row.catalog_id, target))
                        .is_some()
                {
                    return Err(UcError::new(
                        ErrorCode::SchemaAlreadyExists,
                        format!("Schema '{}' already exists", target),
                    ));
                }
                row.name = target.to_string();
            }
            // COALESCE semantics: None leaves the existing value alone.
            if let Some(c) = comment {
                row.comment = Some(c.to_string());
            }
            if let Some(o) = owner {
                row.owner = Some(o.to_string());
            }
            row.updated_at = Some(updated_at);
            row.updated_by = updated_by.map(str::to_owned);

            let body = serde_json::to_value(&row)
                .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
            Ok((
                vec![Action::Upsert { kind: EntityKind::Schema, id, body }],
                row,
            ))
        })
        .await
}

pub async fn delete(store: &Store, id: Uuid) -> Result<(), UcError> {
    store
        .commit("DROP SCHEMA", |snap| {
            if snap.get(EntityKind::Schema, id).is_none() {
                return Err(UcError::new(
                    ErrorCode::SchemaNotFound,
                    format!("Schema '{}' not found", id),
                ));
            }
            // No cascade: FK enforcement is off in the SQL path too.
            Ok((vec![Action::Remove { kind: EntityKind::Schema, id }], ()))
        })
        .await
}
