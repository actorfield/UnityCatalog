//! Log-structured body for repos::external_location.
//! Signatures identical to external_location.rs.

use crate::models::external_location::ExternalLocationRow;
use crate::store::action::{Action, EntityKind};
use crate::store::row::Row;
use crate::store::Store;
use uc_errors::{ErrorCode, UcError};
use uuid::Uuid;

fn row_of(v: &Row) -> Result<ExternalLocationRow, UcError> {
    crate::typed_row!(v, Row::ExternalLocation, "external location")
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
        crate::pagination::over_fetch(max_results),
    );
    let rows: Vec<ExternalLocationRow> = found.into_iter().map(row_of).collect::<Result<_, _>>()?;
    let (rows, next) = crate::pagination::page(rows, max_results, |r| r.name.clone());
    Ok((rows, next))
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
    candidates.sort_by(|a, b| b.url.len().cmp(&a.url.len()).then_with(|| a.id.cmp(&b.id)));
    Ok(candidates.into_iter().next())
}

/// Patch an external location in place. `None` leaves a field alone.
#[allow(clippy::too_many_arguments)]
pub async fn update(
    store: &Store,
    id: Uuid,
    new_name: Option<&str>,
    url: Option<&str>,
    comment: Option<&str>,
    owner: Option<&str>,
    credential_id: Option<Uuid>,
    updated_at: i64,
    updated_by: Option<&str>,
) -> Result<(), UcError> {
    store
        .commit("UPDATE EXTERNAL LOCATION", |snap| {
            // Zero rows matched is success in the SQL; preserved.
            let Some(current) = snap.get(EntityKind::ExternalLocation, id) else {
                return Ok((vec![], ()));
            };
            let mut row = row_of(current)?;

            if let Some(target) = new_name {
                if target != row.name
                    && snap
                        .get_by_natural_key(EntityKind::ExternalLocation, target)
                        .is_some()
                {
                    return Err(UcError::new(
                        ErrorCode::ExternalLocationAlreadyExists,
                        format!("External location '{}' already exists", target),
                    ));
                }
                row.name = target.to_string();
            }
            if let Some(u) = url {
                row.url = u.to_string();
            }
            if let Some(c) = comment {
                row.comment = Some(c.to_string());
            }
            if let Some(o) = owner {
                row.owner = Some(o.to_string());
            }
            if let Some(c) = credential_id {
                row.credential_id = c;
            }
            row.updated_at = Some(updated_at);
            row.updated_by = updated_by.map(str::to_owned);

            let body = serde_json::to_value(&row)
                .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
            Ok((
                vec![Action::Upsert {
                    kind: EntityKind::ExternalLocation,
                    id,
                    body,
                }],
                (),
            ))
        })
        .await
}
