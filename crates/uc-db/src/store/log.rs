//! Object-store log: the commit protocol, replay, and checkpointing.
//!
//! Structured after `_delta_log`: numbered JSONL commits, periodic checkpoints
//! that fold the whole state into one file, and a `_last_checkpoint` pointer
//! that is a hint rather than a source of truth.
//!
//! The only thing this design needs from the object store is atomic
//! create-if-absent. `ObjectLog` is the seam so that can be S3, MinIO, or an
//! in-memory fake in tests without the store knowing which.

use super::action::{checkpoint_key, commit_key, decode_commit, version_from_key, Action, CommitInfo, LAST_CHECKPOINT_KEY};
use serde::{Deserialize, Serialize};
use uc_errors::{ErrorCode, UcError};

/// Outcome of a conditional create. Distinguishing "someone beat me" from a
/// transport error is the whole point — collapsing them into one error type is
/// how you end up retrying a 500 forever, or reporting a lost race as an outage.
#[derive(Debug, PartialEq, Eq)]
pub enum PutResult {
    Created,
    AlreadyExists,
}

#[async_trait::async_trait]
pub trait ObjectLog: Send + Sync {
    /// PUT with `If-None-Match: *`. MUST NOT overwrite an existing object and
    /// MUST report `AlreadyExists` rather than succeeding. A backend that
    /// cannot guarantee this is not usable here — a silent overwrite is
    /// undetectable data loss, not a degraded mode.
    async fn put_if_absent(&self, key: &str, body: Vec<u8>) -> Result<PutResult, UcError>;

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, UcError>;

    /// Keys under `prefix` sorting strictly after `start_after`, lexicographic.
    /// Order is load-bearing (see `action::commit_key`).
    ///
    /// MAY return a partial page. Real backends paginate — S3 ListObjectsV2
    /// caps at 1000 keys — so callers must never treat one call's result as
    /// the complete set. Use `list_all_after`, which drives the listing to
    /// exhaustion; a truncating backend then costs extra round trips instead
    /// of correctness.
    async fn list_after(&self, prefix: &str, start_after: &str) -> Result<Vec<String>, UcError>;

    /// Unconditional overwrite. Only for `_last_checkpoint`, which is advisory.
    async fn put(&self, key: &str, body: Vec<u8>) -> Result<(), UcError>;
}

/// Bounded so a pathological write-storm surfaces as an error instead of
/// spinning. Hitting this is worth alerting on, not a normal path.
pub const MAX_COMMIT_ATTEMPTS: usize = 8;

pub const CHECKPOINT_INTERVAL: u64 = 100;

/// The `_last_checkpoint` pointer, same role as Delta's.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastCheckpoint {
    pub version: u64,
    /// Number of action lines in the checkpoint file. Catches truncation.
    pub size: u64,
    /// Content hash of the checkpoint body.
    ///
    /// `size` alone only catches a *short* file; a flipped byte inside a line
    /// leaves the line count intact, and a corrupted row that still parses as
    /// JSON would be materialised as real state. This closes that gap.
    ///
    /// Optional so a pointer written by an older build still loads — it is
    /// verified when present and skipped when absent, rather than treating a
    /// missing hash as a mismatch and refusing every pre-existing checkpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
}

/// FNV-1a, 64-bit. Detects accidental corruption -- a flipped bit, a partial
/// overwrite, a bad edit -- which is the actual threat here, since the object
/// store already authenticates and integrity-checks transfers. It is NOT
/// cryptographic: it does not detect deliberate tampering by someone who can
/// write to the bucket, and nothing here should be read as claiming otherwise.
///
/// Chosen over a real digest to avoid a dependency on the boot-critical path
/// for a check whose only job is catching corruption.
pub fn content_hash(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Every key under `prefix` after `start_after`, paging until exhausted.
///
/// This exists because a truncated listing is otherwise undetectable and
/// silently wrong. Tail truncation does not even trip the gap check below: keys
/// 1..N of a longer log are perfectly contiguous, so replay stops early and
/// materialises a stale snapshot with no error. Draining the listing here is
/// the only place that can be fixed once for every caller.
pub async fn list_all_after(
    log: &dyn ObjectLog,
    prefix: &str,
    start_after: &str,
) -> Result<Vec<String>, UcError> {
    list_all_in_range(log, prefix, start_after, None).await
}

/// As `list_all_after`, but stops paging once keys pass `stop_at` (inclusive).
///
/// Without a bound, reading commits 5..10 of a table with 100k of them would
/// page the entire partition before filtering. That is the hot Delta read path
/// -- clients ask for recent ranges -- so the bound is not a micro-optimisation.
pub async fn list_all_in_range(
    log: &dyn ObjectLog,
    prefix: &str,
    start_after: &str,
    stop_at: Option<&str>,
) -> Result<Vec<String>, UcError> {
    let mut all: Vec<String> = Vec::new();
    let mut cursor = start_after.to_string();
    loop {
        let page = log.list_after(prefix, &cursor).await?;
        let Some(last) = page.last().cloned() else {
            return Ok(all);
        };
        if let Some(stop) = stop_at {
            if last.as_str() > stop {
                all.extend(page.into_iter().take_while(|k| k.as_str() <= stop));
                return Ok(all);
            }
        }
        // A backend that ignores start_after would loop forever; refuse rather
        // than spin.
        if last <= cursor {
            return Err(UcError::new(
                ErrorCode::Internal,
                format!("object log ignored start_after: {last:?} <= {cursor:?}"),
            ));
        }
        cursor = last;
        all.extend(page);
    }
}

/// Read commits strictly after `from_version`, in order.
pub async fn read_commits_after(
    log: &dyn ObjectLog,
    from_version: u64,
) -> Result<Vec<(u64, CommitInfo, Vec<Action>)>, UcError> {
    let keys = list_all_after(log, "_uc_log/", &commit_key(from_version)).await?;
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
        let (info, actions) = decode_commit(&key, &bytes)?;
        out.push((version, info, actions));
    }

    // Versions must be gapless. A gap means a commit was deleted or a listing
    // was truncated, and applying the remainder would produce state that never
    // existed.
    for (i, (version, _, _)) in out.iter().enumerate() {
        let expected = from_version + 1 + i as u64;
        if *version != expected {
            return Err(UcError::new(
                ErrorCode::Internal,
                format!("log gap: expected version {expected}, found {version}"),
            ));
        }
    }
    Ok(out)
}

/// Resolve the checkpoint to start replay from, using `_last_checkpoint` as a
/// hint.
///
/// Advisory only: a missing, unparseable or dangling pointer falls back to a
/// full scan from version 0 rather than failing. A checkpoint write that did
/// not land must cost replay time, never data. The log itself is the source of
/// truth; the pointer is an optimisation.
pub async fn resolve_checkpoint(
    log: &dyn ObjectLog,
) -> Result<Option<(u64, Vec<u8>)>, UcError> {
    let Some(ptr) = log.get(LAST_CHECKPOINT_KEY).await? else {
        return Ok(None);
    };
    let Ok(parsed) = serde_json::from_slice::<LastCheckpoint>(&ptr) else {
        tracing::warn!("unparseable _last_checkpoint; falling back to full log scan");
        return Ok(None);
    };
    let Some(body) = log.get(&checkpoint_key(parsed.version)).await? else {
        tracing::warn!(
            version = parsed.version,
            "checkpoint pointer dangles; falling back to full log scan"
        );
        return Ok(None);
    };
    // Truncation guard: the pointer records the line count the writer intended.
    let lines = body.iter().filter(|b| **b == b'\n').count() as u64;
    if lines != parsed.size {
        tracing::warn!(
            version = parsed.version,
            expected = parsed.size,
            found = lines,
            "checkpoint is truncated; falling back to full log scan"
        );
        return Ok(None);
    }
    // Corruption guard. Falls back rather than failing: the log is the source
    // of truth and can always rebuild the state, so a bad checkpoint costs
    // replay time. Failing here would turn a recoverable situation into an
    // unstartable server.
    if let Some(ref expected) = parsed.checksum {
        let actual = content_hash(&body);
        if &actual != expected {
            tracing::warn!(
                version = parsed.version,
                %expected,
                %actual,
                "checkpoint checksum mismatch; falling back to full log scan"
            );
            return Ok(None);
        }
    }
    Ok(Some((parsed.version, body)))
}
