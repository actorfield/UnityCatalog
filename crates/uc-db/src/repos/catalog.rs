//! Worked example: repos/catalog.rs ported to the log-structured store.
//!
//! Sketch only — sits alongside the SQL version so the two can be diffed. The
//! real change replaces the body of repos/catalog.rs in place.
//!
//! Every signature is byte-identical to the SQL version. That is the whole
//! argument for this seam: `AnyPool` becomes `Store`, and uc-api does not move.

use crate::models::catalog::CatalogRow;
use crate::store::action::{Action, EntityKind};
use crate::store::row::Row;
use crate::store::Store;
use uc_errors::{ErrorCode, UcError};
use uuid::Uuid;

fn row_of(v: &Row) -> Result<CatalogRow, UcError> {
    crate::typed_row!(v, Row::Catalog, "catalog")
}

#[allow(clippy::too_many_arguments)]
pub async fn create(
    store: &Store,
    id: Uuid,
    name: &str,
    comment: Option<&str>,
    owner: Option<&str>,
    created_by: Option<&str>,
    storage_root: Option<&str>,
    created_at: i64,
) -> Result<CatalogRow, UcError> {
    let row = CatalogRow {
        id,
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
        .commit("CREATE CATALOG", |snap| {
            // Re-evaluated on every attempt. This is where the UNIQUE(name)
            // constraint now lives, and why a lost race must re-run rather
            // than blindly retry the PUT.
            if snap.get_by_natural_key(EntityKind::Catalog, name).is_some() {
                return Err(UcError::new(
                    ErrorCode::CatalogAlreadyExists,
                    format!("Catalog '{}' already exists", name),
                ));
            }
            let body = serde_json::to_value(&row)
                .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
            Ok((
                vec![Action::Upsert {
                    kind: EntityKind::Catalog,
                    id,
                    body,
                }],
                row.clone(),
            ))
        })
        .await
}

pub async fn get_by_name(store: &Store, name: &str) -> Result<CatalogRow, UcError> {
    let snap = store.snapshot().await;
    snap.get_by_natural_key(EntityKind::Catalog, name)
        .ok_or_else(|| {
            UcError::new(
                ErrorCode::CatalogNotFound,
                format!("Catalog '{}' not found", name),
            )
        })
        .and_then(row_of)
}

pub async fn get_by_id(store: &Store, id: Uuid) -> Result<CatalogRow, UcError> {
    let snap = store.snapshot().await;
    snap.get(EntityKind::Catalog, id)
        .ok_or_else(|| {
            UcError::new(
                ErrorCode::CatalogNotFound,
                format!("Catalog '{}' not found", id),
            )
        })
        .and_then(row_of)
}

/// `SELECT * FROM uc_catalogs WHERE name > $1 ORDER BY name LIMIT $2` becomes a
/// BTreeMap range scan. The over-fetch-by-one and the off-by-one in the token
/// (`rows[max_results - 1]`) are preserved verbatim from the SQL version --
/// clients are paging against that behaviour today, quirk included.
pub async fn list(
    store: &Store,
    page_token: Option<&str>,
    max_results: i64,
) -> Result<(Vec<CatalogRow>, Option<String>), UcError> {
    let snap = store.snapshot().await;
    let found = snap.scan(
        EntityKind::Catalog,
        page_token,
        crate::pagination::over_fetch(max_results),
    );
    let rows: Vec<CatalogRow> = found.into_iter().map(row_of).collect::<Result<_, _>>()?;

    let (rows, next_token) = crate::pagination::page(rows, max_results, |r| r.name.clone());
    Ok((rows, next_token))
}

pub async fn update(
    store: &Store,
    name: &str,
    new_name: Option<&str>,
    comment: Option<&str>,
    owner: Option<&str>,
    updated_by: Option<&str>,
    updated_at: i64,
) -> Result<CatalogRow, UcError> {
    store
        .commit("UPDATE CATALOG", |snap| {
            let current = snap
                .get_by_natural_key(EntityKind::Catalog, name)
                .ok_or_else(|| {
                    UcError::new(
                        ErrorCode::CatalogNotFound,
                        format!("Catalog '{}' not found", name),
                    )
                })?;
            let mut row = row_of(current)?;

            // A rename onto an occupied name was a unique violation in SQL. It
            // surfaced as a 500 there, because only `create` mapped
            // is_unique_violation. Preserving that would be preserving a bug,
            // so this returns the domain error -- calling it out because it is
            // a deliberate behaviour change, not an accident of the port.
            if let Some(target) = new_name {
                if target != name
                    && snap
                        .get_by_natural_key(EntityKind::Catalog, target)
                        .is_some()
                {
                    return Err(UcError::new(
                        ErrorCode::CatalogAlreadyExists,
                        format!("Catalog '{}' already exists", target),
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
                vec![Action::Upsert {
                    kind: EntityKind::Catalog,
                    id: row.id,
                    body,
                }],
                row,
            ))
        })
        .await
}

pub async fn delete(store: &Store, name: &str) -> Result<(), UcError> {
    store
        .commit("DROP CATALOG", |snap| {
            let current = snap
                .get_by_natural_key(EntityKind::Catalog, name)
                .ok_or_else(|| {
                    UcError::new(
                        ErrorCode::CatalogNotFound,
                        format!("Catalog '{}' not found", name),
                    )
                })?;
            let row = row_of(current)?;
            // No cascade: SQLite has FK enforcement off (no PRAGMA
            // foreign_keys=ON), so orphaned schemas are the behaviour today.
            // Adding cascade here would be a behaviour change disguised as a
            // port. See docs/log-structured-metadata.md.
            Ok((
                vec![Action::Remove {
                    kind: EntityKind::Catalog,
                    id: row.id,
                }],
                (),
            ))
        })
        .await
}
