//! Object-store log: the commit protocol and replay.
//!
//! The only thing this design needs from the object store is atomic
//! create-if-absent. `ObjectLog` is the seam so that can be S3, MinIO, or an
//! in-memory fake in tests without the store knowing which.

use super::action::{
    checkpoint_key, commit_key, version_from_key, Commit, LAST_CHECKPOINT_KEY,
};
use uc_errors::{ErrorCode, UcError};

/// Outcome of a conditional create. Distinguishing "someone beat me" from a
/// transport error is the whole point — collapsing them into one error type is
/// how you end up retrying a 500 forever or surfacing a race as an outage.
#[derive(Debug, PartialEq, Eq)]
pub enum PutResult {
    Created,
    AlreadyExists,
}

#[async_trait::async_trait]
pub trait ObjectLog: Send + Sync {
    /// PUT with `If-None-Match: *`. MUST NOT overwrite an existing object, and
    /// MUST report AlreadyExists rather than succeeding. A backend that cannot
    /// guarantee this is not usable here — silent overwrite is undetectable
    /// data loss, not a degraded mode.
    async fn put_if_absent(&self, key: &str, body: Vec<u8>) -> Result<PutResult, UcError>;

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, UcError>;

    /// Keys under `prefix` that sort strictly after `start_after`, in
    /// lexicographic order. Order is load-bearing (see action::commit_key).
    async fn list_after(&self, prefix: &str, start_after: &str) -> Result<Vec<String>, UcError>;

    /// Unconditional overwrite. Only for `_last_checkpoint`, which is advisory.
    async fn put(&self, key: &str, body: Vec<u8>) -> Result<(), UcError>;
}

/// Bounded so a pathological write-storm surfaces as an error instead of
/// spinning. Hitting this is a signal worth alerting on, not a normal path.
pub const MAX_COMMIT_ATTEMPTS: usize = 8;

pub const CHECKPOINT_INTERVAL: u64 = 100;

/// Read the commits strictly after `from_version`, in order.
pub async fn read_commits_after(
    log: &dyn ObjectLog,
    from_version: u64,
) -> Result<Vec<(u64, Commit)>, UcError> {
    let keys = log.list_after("_uc_log/", &commit_key(from_version)).await?;
    let mut out = Vec::new();
    for key in keys {
        let Some(version) = version_from_key(&key) else {
            continue; // checkpoint, _last_checkpoint, _keys.json
        };
        if version <= from_version {
            continue;
        }
        let Some(bytes) = log.get(&key).await? else {
            // Listed but unreadable. Do NOT skip: a hole in the log means the
            // replayed state is silently wrong, which is worse than refusing
            // to start.
            return Err(UcError::new(
                ErrorCode::Internal,
                format!("log hole: {key} was listed but could not be read"),
            ));
        };
        let commit: Commit = serde_json::from_slice(&bytes).map_err(|e| {
            UcError::new(ErrorCode::Internal, format!("corrupt commit {key}: {e}"))
        })?;
        if commit.format > super::action::FORMAT_VERSION {
            return Err(UcError::new(
                ErrorCode::Internal,
                format!(
                    "commit {key} has format {} but this build understands {}",
                    commit.format,
                    super::action::FORMAT_VERSION
                ),
            ));
        }
        out.push((version, commit));
    }
    Ok(out)
}

/// Resolve the version to start replay from, using `_last_checkpoint` as a
/// hint. Advisory only: a missing or unreadable pointer falls back to a full
/// scan from 0 rather than failing, because a checkpoint write that did not
/// land must not become data loss.
pub async fn resolve_checkpoint(log: &dyn ObjectLog) -> Result<Option<(u64, Vec<u8>)>, UcError> {
    let Some(ptr) = log.get(LAST_CHECKPOINT_KEY).await? else {
        return Ok(None);
    };
    let Ok(parsed) = serde_json::from_slice::<serde_json::Value>(&ptr) else {
        tracing::warn!("unparseable _last_checkpoint; falling back to full log scan");
        return Ok(None);
    };
    let Some(version) = parsed.get("version").and_then(|v| v.as_u64()) else {
        tracing::warn!("_last_checkpoint has no version; falling back to full log scan");
        return Ok(None);
    };
    match log.get(&checkpoint_key(version)).await? {
        Some(body) => Ok(Some((version, body))),
        None => {
            tracing::warn!(version, "checkpoint pointer dangles; falling back to full scan");
            Ok(None)
        }
    }
}
