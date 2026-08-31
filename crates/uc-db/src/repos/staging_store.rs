//! Log-structured body for repos::staging. Signatures identical to staging.rs.

use crate::models::staging::StagingTableRow;
use crate::store::action::{Action, EntityKind};
use crate::store::Store;
use uc_errors::{ErrorCode, UcError};
use uuid::Uuid;

fn row_of(v: &serde_json::Value) -> Result<StagingTableRow, UcError> {
    serde_json::from_value(v.clone())
        .map_err(|e| UcError::new(ErrorCode::Internal, format!("corrupt staging row: {e}")))
}

pub async fn create(store: &Store, row: &StagingTableRow) -> Result<StagingTableRow, UcError> {
    let row = row.clone();
    store
        .commit("CREATE STAGING TABLE", |_| {
            // uc_staging_tables declares no UNIQUE, so the SQL insert cannot
            // conflict either. No precondition to check.
            let body = serde_json::to_value(&row)
                .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
            Ok((
                vec![Action::Upsert {
                    kind: EntityKind::StagingTable,
                    id: row.id,
                    body,
                }],
                row.clone(),
            ))
        })
        .await
}

pub async fn get_by_id(store: &Store, id: Uuid) -> Result<StagingTableRow, UcError> {
    let snap = store.snapshot().await;
    snap.get(EntityKind::StagingTable, id)
        .ok_or_else(|| {
            UcError::new(
                ErrorCode::NotFound,
                format!("Staging table '{}' not found", id),
            )
        })
        .and_then(row_of)
}

/// Find a staging table by its storage location (used during MANAGED table
/// commit).
///
/// `staging_location` has an INDEX but no UNIQUE, so duplicates are
/// representable and there is no natural-key index to use — this scans. The
/// SQL's `fetch_one` returned an arbitrary row among duplicates; sorting by id
/// makes the choice stable, which matters here because the caller commits data
/// against whichever row comes back.
pub async fn get_by_location(store: &Store, location: &str) -> Result<StagingTableRow, UcError> {
    let snap = store.snapshot().await;
    let mut hits: Vec<StagingTableRow> = snap
        .iter(EntityKind::StagingTable)
        .map(row_of)
        .collect::<Result<Vec<_>, _>>()?;
    hits.retain(|s| s.staging_location == location);
    hits.sort_by_key(|s| s.id);
    hits.into_iter().next().ok_or_else(|| {
        UcError::new(
            ErrorCode::NotFound,
            format!("Staging table at '{}' not found", location),
        )
    })
}

/// Deleting nothing is not an error, matching the SQL's unchecked
/// rows_affected.
pub async fn mark_committed(
    store: &Store,
    id: Uuid,
    committed_at: i64,
) -> Result<(), UcError> {
    store
        .commit("COMMIT STAGING TABLE", |snap| {
            let Some(current) = snap.get(EntityKind::StagingTable, id) else {
                // The SQL UPDATE matches zero rows and reports success.
                return Ok((vec![], ()));
            };
            let mut row = row_of(current)?;
            row.stage_committed = true;
            row.stage_committed_at = Some(committed_at);
            let body = serde_json::to_value(&row)
                .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
            Ok((
                vec![Action::Upsert {
                    kind: EntityKind::StagingTable,
                    id,
                    body,
                }],
                (),
            ))
        })
        .await
}
