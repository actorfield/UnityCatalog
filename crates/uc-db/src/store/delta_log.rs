//! Per-table Delta commit logs.
//!
//! Delta commits are partitioned out of the main log, one stream per table:
//!
//!     _uc_log/tables/{table_id}/00000000000000000007.json
//!
//! which is exactly how Delta itself lays out `_delta_log` — one log per table,
//! not one per metastore.
//!
//! Two independent reasons, both pointing the same way:
//!
//! 1. **Write concurrency.** A single org-wide log serialises every write
//!    through one version counter. Catalog CRUD does not care — humans create
//!    schemas — but every Delta commit in the org lands here. SQLite accepts
//!    concurrent commits to different tables today via UNIQUE(table_id,
//!    commit_version); a shared log would not, which is a regression.
//!
//! 2. **Boot cost.** `uc_delta_commits` is the only unbounded-growth entity:
//!    every commit to every table appends forever. Left in the main log it
//!    would dominate both replay and every checkpoint, making startup scale
//!    with total commit history rather than with schema size.
//!
//! The partition boundary is safe because no invariant spans it. UNIQUE(
//! table_id, commit_version) lives entirely inside one table's stream.
//!
//! The payoff is that `commit_version` *is* the log version, so the constraint
//! and the filename are the same thing. A commit is one conditional PUT: no
//! read, no snapshot, no retry loop, and a conflict is `AlreadyExists` rather
//! than something that has to be detected. Nothing about a table's commit
//! history needs to be in memory.

use super::log::{list_all_after, ObjectLog, PutResult};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uc_errors::{ErrorCode, UcError};
use uuid::Uuid;

pub fn partition_prefix(table_id: Uuid) -> String {
    format!("_uc_log/tables/{table_id}/")
}

pub fn commit_key(table_id: Uuid, version: i64) -> String {
    format!("{}{version:020}.json", partition_prefix(table_id))
}

pub fn version_from_key(key: &str) -> Option<i64> {
    key.rsplit('/').next()?.strip_suffix(".json")?.parse().ok()
}

pub struct DeltaLog {
    log: Arc<dyn ObjectLog>,
    /// Last version this replica knows about, per table.
    ///
    /// Purely a hint, and safe to be stale or absent: appends are conditional,
    /// so a wrong hint costs one rejected PUT and a re-list, never correctness.
    /// That is what lets it be a plain cache with no invalidation protocol
    /// across replicas.
    latest_hint: RwLock<HashMap<Uuid, i64>>,
}

impl DeltaLog {
    pub fn new(log: Arc<dyn ObjectLog>) -> Self {
        Self {
            log,
            latest_hint: RwLock::new(HashMap::new()),
        }
    }

    /// Append one commit. The version is the caller's, not ours: Delta clients
    /// decide which version they are committing, and racing them is the whole
    /// point of optimistic concurrency.
    pub async fn append(
        &self,
        table_id: Uuid,
        version: i64,
        body: Vec<u8>,
    ) -> Result<(), UcError> {
        match self
            .log
            .put_if_absent(&commit_key(table_id, version), body)
            .await?
        {
            PutResult::Created => {
                let mut hint = self.latest_hint.write().await;
                let entry = hint.entry(table_id).or_insert(version);
                *entry = (*entry).max(version);
                Ok(())
            }
            PutResult::AlreadyExists => Err(UcError::new(
                ErrorCode::CommitVersionConflict,
                format!("Commit version {version} already exists for this table"),
            )),
        }
    }

    /// Versions present for a table, ascending, optionally bounded.
    pub async fn versions(
        &self,
        table_id: Uuid,
        starting: Option<i64>,
        ending: Option<i64>,
    ) -> Result<Vec<i64>, UcError> {
        let prefix = partition_prefix(table_id);
        // `start_after` is exclusive, so step back one to make `starting`
        // inclusive as the SQL's `commit_version >= $2` was. At v == 0 this
        // formats as "-0000000000000000001", and '-' (0x2D) sorts below '0'
        // (0x30), so it lands before version 0 rather than after it. That is
        // correct but only by accident of ASCII, so pin it explicitly instead.
        let after = match starting {
            Some(v) if v <= 0 => prefix.clone(),
            Some(v) => commit_key(table_id, v - 1),
            None => prefix.clone(),
        };
        let mut versions: Vec<i64> = list_all_after(&*self.log, &prefix, &after)
            .await?
            .iter()
            .filter_map(|k| version_from_key(k))
            .filter(|v| ending.is_none_or(|e| *v <= e))
            .collect();
        versions.sort_unstable();
        Ok(versions)
    }

    pub async fn read(&self, table_id: Uuid, version: i64) -> Result<Option<Vec<u8>>, UcError> {
        self.log.get(&commit_key(table_id, version)).await
    }

    /// Highest committed version, or None for a table with no commits.
    ///
    /// Consults the hint first, then confirms by listing forward from it. That
    /// keeps the common case to a short listing instead of paging the entire
    /// history, without ever trusting the hint to be current.
    pub async fn latest_version(&self, table_id: Uuid) -> Result<Option<i64>, UcError> {
        let hint = self.latest_hint.read().await.get(&table_id).copied();
        let prefix = partition_prefix(table_id);
        let after = match hint {
            Some(v) => commit_key(table_id, v),
            None => prefix.clone(),
        };
        let beyond = list_all_after(&*self.log, &prefix, &after)
            .await?
            .iter()
            .filter_map(|k| version_from_key(k))
            .max();

        let latest = match (hint, beyond) {
            (_, Some(v)) => Some(v),
            (Some(v), None) => Some(v),
            (None, None) => None,
        };
        if let Some(v) = latest {
            // max, not insert: concurrent callers can compute different answers
            // and finish out of order. A hint that goes backwards is safe (it
            // only costs a longer listing) but there is no reason to allow it.
            let mut hint = self.latest_hint.write().await;
            let entry = hint.entry(table_id).or_insert(v);
            *entry = (*entry).max(v);
        }
        Ok(latest)
    }
}
