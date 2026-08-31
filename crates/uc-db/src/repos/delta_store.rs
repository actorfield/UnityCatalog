//! Log-structured body for repos::delta. Signatures identical to delta.rs.
//!
//! This is the module partitioning was for. Each row is stored as the commit
//! file itself, in the table's own log, at the version it claims — so
//! UNIQUE(table_id, commit_version) is no longer a constraint the store has to
//! enforce, it is the object key. `insert` is a single conditional PUT: no
//! snapshot read, no commit loop, no retry, and no serialisation against
//! commits to any other table.

use crate::models::delta::DeltaCommitRow;
use crate::store::Store;
use uc_errors::{ErrorCode, UcError};
use uuid::Uuid;

fn decode(bytes: &[u8]) -> Result<DeltaCommitRow, UcError> {
    serde_json::from_slice(bytes)
        .map_err(|e| UcError::new(ErrorCode::Internal, format!("corrupt delta commit: {e}")))
}

pub async fn insert(store: &Store, row: &DeltaCommitRow) -> Result<DeltaCommitRow, UcError> {
    let body = serde_json::to_vec(row)
        .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
    // AlreadyExists is mapped to CommitVersionConflict inside DeltaLog::append,
    // preserving the 409 the unique-violation branch produced.
    store
        .delta
        .append(row.table_id, row.commit_version, body)
        .await?;
    Ok(row.clone())
}

pub async fn list_for_table(
    store: &Store,
    table_id: Uuid,
    starting_version: Option<i64>,
    ending_version: Option<i64>,
) -> Result<Vec<DeltaCommitRow>, UcError> {
    let versions = store
        .delta
        .versions(table_id, starting_version, ending_version)
        .await?;
    let mut rows = Vec::with_capacity(versions.len());
    for version in versions {
        // Listed but unreadable means a hole in the table's history. The SQL
        // would have returned a contiguous range or nothing; silently skipping
        // would hand a Delta client a gap it cannot detect.
        let bytes = store.delta.read(table_id, version).await?.ok_or_else(|| {
            UcError::new(
                ErrorCode::Internal,
                format!("delta log hole: version {version} listed but unreadable"),
            )
        })?;
        rows.push(decode(&bytes)?);
    }
    Ok(rows) // already ascending, matching ORDER BY commit_version
}

pub async fn latest_version(store: &Store, table_id: Uuid) -> Result<Option<i64>, UcError> {
    store.delta.latest_version(table_id).await
}
