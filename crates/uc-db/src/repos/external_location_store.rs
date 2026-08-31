//! Log-structured body for repos::external_location.
//! Signatures identical to external_location.rs.

use crate::models::external_location::ExternalLocationRow;
use crate::store::action::{Action, EntityKind};
use crate::store::Store;
use uc_errors::{ErrorCode, UcError};

fn row_of(v: &serde_json::Value) -> Result<ExternalLocationRow, UcError> {
    serde_json::from_value(v.clone()).map_err(|e| {
        UcError::new(
            ErrorCode::Internal,
            format!("corrupt external location row: {e}"),
        )
    })
}

pub async fn create(
    store: &Store,
    row: &ExternalLocationRow,
) -> Result<ExternalLocationRow, UcError> {
    let row = row.clone();
    store
        .commit("CREATE EXTERNAL LOCATION", |snap| {
            if snap
                .get_by_natural_key(EntityKind::ExternalLocation, &row.name)
                .is_some()
            {
                return Err(UcError::new(
                    ErrorCode::ExternalLocationAlreadyExists,
                    format!("External location '{}' already exists", row.name),
                ));
            }
            let body = serde_json::to_value(&row)
                .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
            Ok((
                vec![Action::Upsert {
                    kind: EntityKind::ExternalLocation,
                    id: row.id,
                    body,
                }],
                row.clone(),
            ))
        })
        .await
}

pub async fn get_by_name(store: &Store, name: &str) -> Result<ExternalLocationRow, UcError> {
    let snap = store.snapshot().await;
    snap.get_by_natural_key(EntityKind::ExternalLocation, name)
        .ok_or_else(|| {
            UcError::new(
                ErrorCode::NotFound,
                format!("External location '{}' not found", name),
            )
        })
        .and_then(row_of)
}

pub async fn list(
    store: &Store,
    page_token: Option<&str>,
    max_results: i64,
) -> Result<(Vec<ExternalLocationRow>, Option<String>), UcError> {
    let snap = store.snapshot().await;
    let found = snap.scan(
        EntityKind::ExternalLocation,
        page_token,
        max_results as usize + 1,
    );
    let rows: Vec<ExternalLocationRow> =
        found.into_iter().map(row_of).collect::<Result<_, _>>()?;
    let next = if rows.len() as i64 > max_results {
        rows.get(max_results as usize - 1).map(|r| r.name.clone())
    } else {
        None
    };
    Ok((rows.into_iter().take(max_results as usize).collect(), next))
}

pub async fn delete(store: &Store, name: &str) -> Result<(), UcError> {
    store
        .commit("DROP EXTERNAL LOCATION", |snap| {
            let current = snap
                .get_by_natural_key(EntityKind::ExternalLocation, name)
                .ok_or_else(|| {
                    UcError::new(
                        ErrorCode::NotFound,
                        format!("External location '{}' not found", name),
                    )
                })?;
            let row = row_of(current)?;
            Ok((
                vec![Action::Remove {
                    kind: EntityKind::ExternalLocation,
                    id: row.id,
                }],
                (),
            ))
        })
        .await
}

/// Longest-prefix match, replacing
/// `WHERE $1 LIKE (url || '%') ORDER BY LENGTH(url) DESC LIMIT 1`.
///
/// Ties on url length were resolved arbitrarily by SQLite; broken by id here so
/// the same path always resolves to the same location.
pub async fn find_by_path_prefix(
    store: &Store,
    path: &str,
) -> Result<Option<ExternalLocationRow>, UcError> {
    let snap = store.snapshot().await;
    let mut candidates: Vec<ExternalLocationRow> = snap
        .iter(EntityKind::ExternalLocation)
        .map(row_of)
        .collect::<Result<Vec<_>, _>>()?;
    candidates.retain(|l| path.starts_with(&l.url));
    candidates.sort_by(|a, b| {
        b.url
            .len()
            .cmp(&a.url.len())
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(candidates.into_iter().next())
}
