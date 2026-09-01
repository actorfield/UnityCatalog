//! Log-structured body for repos::metastore. Signatures identical to metastore.rs.

use crate::models::metastore::MetastoreRow;
use crate::store::action::{Action, EntityKind};
use crate::store::Store;
use uc_errors::{ErrorCode, UcError};
use uuid::Uuid;

fn row_of(v: &serde_json::Value) -> Result<MetastoreRow, UcError> {
    serde_json::from_value(v.clone())
        .map_err(|e| UcError::new(ErrorCode::Internal, format!("corrupt metastore row: {e}")))
}

/// Get the singleton metastore row, creating it if absent.
///
/// The SQL version is a read-then-insert with nothing between them, so two
/// uc-servers starting together can both see no row and both insert — there is
/// no UNIQUE on uc_metastore to stop them, and the loser is not detected
/// because `get` takes `LIMIT 1`. Here the check runs inside the commit
/// closure, so a replica that loses the race re-runs it, finds the row the
/// winner wrote, and returns that. Same signature, race removed.
pub async fn get_or_init(store: &Store, name: &str) -> Result<MetastoreRow, UcError> {
    store
        .commit("INIT METASTORE", |snap| {
            if let Some(existing) = snap.iter(EntityKind::Metastore).next() {
                // Already there: commit nothing, return what exists.
                return Ok((vec![], row_of(existing)?));
            }
            let row = MetastoreRow {
                id: Uuid::now_v7(),
                name: name.to_string(),
            };
            let body = serde_json::to_value(&row)
                .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
            Ok((
                vec![Action::Upsert {
                    kind: EntityKind::Metastore,
                    id: row.id,
                    body,
                }],
                row,
            ))
        })
        .await
}

pub async fn get(store: &Store) -> Result<MetastoreRow, UcError> {
    let snap = store.snapshot().await;
    let found = snap
        .iter(EntityKind::Metastore)
        .next()
        .ok_or_else(|| UcError::new(ErrorCode::Internal, "Metastore not initialised"))?;
    row_of(found)
}
