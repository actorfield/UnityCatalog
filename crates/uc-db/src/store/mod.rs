//! In-memory materialisation of the log, plus the commit loop.
//!
//! `Store` replaces `sqlx::SqlitePool` as the handle every repo function takes.
//! Keeping the same shape (`&Store` where `&AnyPool` used to be) is what lets
//! the ~262 call sites in uc-api compile untouched.

pub mod action;
pub mod log;

#[cfg(test)]
mod tests;

use action::{Action, CommitInfo, EntityKind, Line};
use log::{LastCheckpoint, ObjectLog, PutResult, MAX_COMMIT_ATTEMPTS};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tokio::sync::RwLock;
use uc_errors::{ErrorCode, UcError};
use uuid::Uuid;

/// The materialised state at a particular log version.
///
/// Rows are held as `serde_json::Value` rather than typed structs so one
/// snapshot serves all 18 entity kinds without a trait object per kind. Repo
/// functions deserialise into their own `*Row` on the way out, which is the
/// same boundary `FromRow` occupied before.
#[derive(Default)]
pub struct Snapshot {
    pub version: u64,
    entities: HashMap<EntityKind, HashMap<Uuid, serde_json::Value>>,
    /// Natural-key index: `(kind, composite)` where `composite` is the SQL
    /// UNIQUE tuple rendered in sort order.
    ///
    /// A BTreeMap rather than a HashMap because the SQL this replaces is
    /// `WHERE name > $token ORDER BY name LIMIT $n` — cursor pagination is a
    /// range scan, and a hash index cannot serve it.
    by_natural_key: BTreeMap<(EntityKind, String), Uuid>,
}

impl Snapshot {
    pub fn get(&self, kind: EntityKind, id: Uuid) -> Option<&serde_json::Value> {
        self.entities.get(&kind)?.get(&id)
    }

    pub fn get_by_natural_key(&self, kind: EntityKind, key: &str) -> Option<&serde_json::Value> {
        let id = self.by_natural_key.get(&(kind, key.to_string()))?;
        self.get(kind, *id)
    }

    /// Ordered scan for cursor pagination. `after` is exclusive, matching
    /// `WHERE name > $token`.
    pub fn scan(
        &self,
        kind: EntityKind,
        after: Option<&str>,
        limit: usize,
    ) -> Vec<&serde_json::Value> {
        let lower = after.map(str::to_owned).unwrap_or_default();
        self.by_natural_key
            .range((kind, lower)..)
            .take_while(|((k, _), _)| *k == kind)
            .filter(|((_, nk), _)| after.is_none_or(|a| nk.as_str() > a))
            .filter_map(|(_, id)| self.get(kind, *id))
            .take(limit)
            .collect()
    }

    /// Ordered scan within a natural-key prefix — the `WHERE parent_id = $1 AND
    /// name > $2 ORDER BY name` shape that every child entity paginates with.
    ///
    /// `after` compares against the portion *after* the prefix, matching the SQL
    /// where the page token is a bare name, not a composite key.
    pub fn scan_prefix(
        &self,
        kind: EntityKind,
        prefix: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Vec<&serde_json::Value> {
        let start = format!("{prefix}{}", after.unwrap_or(""));
        self.by_natural_key
            .range((kind, start)..)
            .take_while(|((k, nk), _)| *k == kind && nk.starts_with(prefix))
            .filter(|((_, nk), _)| match after {
                Some(a) => &nk[prefix.len()..] > a,
                None => true,
            })
            .filter_map(|(_, id)| self.get(kind, *id))
            .take(limit)
            .collect()
    }

    /// Apply one action. Must be deterministic and total — replay correctness
    /// depends on identical input producing identical state.
    fn apply(&mut self, act: &Action) {
        match act {
            Action::Upsert { kind, id, body } => {
                // Rename: drop the stale natural-key entry before inserting the
                // new one, or the old name stays reachable forever.
                if let Some(prev) = self.get(*kind, *id) {
                    if let Some(old) = natural_key_for(*kind, prev) {
                        self.by_natural_key.remove(&(*kind, old));
                    }
                }
                if let Some(nk) = natural_key_for(*kind, body) {
                    self.by_natural_key.insert((*kind, nk), *id);
                }
                self.entities
                    .entry(*kind)
                    .or_default()
                    .insert(*id, body.clone());
            }
            Action::Remove { kind, id } => {
                if let Some(prev) = self.entities.entry(*kind).or_default().remove(id) {
                    if let Some(nk) = natural_key_for(*kind, &prev) {
                        self.by_natural_key.remove(&(*kind, nk));
                    }
                }
            }
        }
    }

    /// Fold the whole snapshot into JSONL upserts — the checkpoint body.
    ///
    /// Emits only `upsert` lines: a checkpoint is the *state*, not the history,
    /// so a removal is represented by absence rather than a `remove` line.
    fn encode_checkpoint(&self) -> Result<(Vec<u8>, u64), UcError> {
        let mut out = Vec::new();
        let mut count = 0u64;
        // Sorted so a checkpoint is byte-reproducible: two replicas
        // checkpointing the same version must produce the same object, or the
        // `size` in `_last_checkpoint` cannot be trusted as a truncation guard.
        let mut kinds: Vec<_> = self.entities.keys().copied().collect();
        kinds.sort();
        for kind in kinds {
            let table = &self.entities[&kind];
            let mut ids: Vec<_> = table.keys().copied().collect();
            ids.sort();
            for id in ids {
                let line = Line::Upsert {
                    kind,
                    id,
                    body: table[&id].clone(),
                };
                out.extend(
                    serde_json::to_vec(&line)
                        .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?,
                );
                out.push(b'\n');
                count += 1;
            }
        }
        Ok((out, count))
    }

    /// Rebuild a snapshot from a checkpoint body.
    ///
    /// The natural-key index is recomputed from the rows rather than stored, so
    /// an index bug cannot be baked into an object and outlive the fix.
    fn decode_checkpoint(bytes: &[u8]) -> Result<Snapshot, UcError> {
        let text = std::str::from_utf8(bytes)
            .map_err(|e| UcError::new(ErrorCode::Internal, format!("checkpoint not utf-8: {e}")))?;
        let mut snap = Snapshot::default();
        for (n, raw) in text.lines().enumerate() {
            if raw.trim().is_empty() {
                continue;
            }
            let line: Line = serde_json::from_str(raw).map_err(|e| {
                UcError::new(
                    ErrorCode::Internal,
                    format!("checkpoint line {}: {e}", n + 1),
                )
            })?;
            match line {
                Line::Upsert { kind, id, body } => {
                    snap.apply(&Action::Upsert { kind, id, body });
                }
                // A checkpoint is a state dump. Anything else means the writer
                // and reader disagree about the format.
                other => {
                    return Err(UcError::new(
                        ErrorCode::Internal,
                        format!("checkpoint line {} is not an upsert: {other:?}", n + 1),
                    ))
                }
            }
        }
        Ok(snap)
    }
}

pub struct Store {
    log: Arc<dyn ObjectLog>,
    state: RwLock<Snapshot>,
}

impl Store {
    /// Replay the log into memory. Called once at startup, before serving.
    pub async fn open(log: Arc<dyn ObjectLog>) -> Result<Self, UcError> {
        let store = Self {
            log,
            state: RwLock::new(Snapshot::default()),
        };
        store.catch_up().await?;
        Ok(store)
    }

    pub async fn snapshot(&self) -> tokio::sync::RwLockReadGuard<'_, Snapshot> {
        self.state.read().await
    }

    /// Pull in every commit written since our current version.
    async fn catch_up(&self) -> Result<(), UcError> {
        let mut state = self.state.write().await;
        if state.version == 0 {
            if let Some((version, body)) = log::resolve_checkpoint(&*self.log).await? {
                *state = Snapshot::decode_checkpoint(&body)?;
                state.version = version;
            }
        }
        for (version, _info, actions) in log::read_commits_after(&*self.log, state.version).await? {
            for act in &actions {
                state.apply(act);
            }
            state.version = version;
        }
        Ok(())
    }

    /// The commit loop.
    ///
    /// `build` is handed a fresh snapshot and returns the actions to write plus
    /// whatever the caller wants back. It is re-run on every attempt — that is
    /// the point, not an implementation detail.
    ///
    /// The conditional PUT alone only decides *who owns version N+1*. It does
    /// not enforce uniqueness. Two replicas can both observe that a catalog
    /// name is free; one wins N+1, and the other must re-evaluate its
    /// precondition against the commit that just beat it. A retry that reused
    /// the previously built actions would cheerfully write a duplicate name at
    /// N+2. So `build` re-runs, and it is where `AlreadyExists` is raised.
    pub async fn commit<T, F>(&self, operation: &str, mut build: F) -> Result<T, UcError>
    where
        F: FnMut(&Snapshot) -> Result<(Vec<Action>, T), UcError>,
    {
        for _ in 0..MAX_COMMIT_ATTEMPTS {
            let (actions, out, target) = {
                let state = self.state.read().await;
                let (actions, out) = build(&state)?;
                (actions, out, state.version + 1)
            };

            let info = CommitInfo {
                format: action::FORMAT_VERSION,
                timestamp: chrono::Utc::now().timestamp_millis(),
                operation: Some(operation.to_string()),
                actor: None,
            };
            let body = action::encode_commit(&info, &actions)?;

            match self
                .log
                .put_if_absent(&action::commit_key(target), body)
                .await?
            {
                PutResult::Created => {
                    let mut state = self.state.write().await;
                    // Another task may have advanced us past `target` while we
                    // were writing; catch_up would re-apply our own commit, so
                    // only apply when we are still the next in line.
                    if state.version + 1 == target {
                        for act in &actions {
                            state.apply(act);
                        }
                        state.version = target;
                    }
                    drop(state);
                    self.maybe_checkpoint(target).await;
                    return Ok(out);
                }
                PutResult::AlreadyExists => {
                    // Lost the race. Replay, then let `build` re-decide.
                    self.catch_up().await?;
                    continue;
                }
            }
        }
        Err(UcError::new(
            ErrorCode::Internal,
            "commit contention: exceeded max attempts",
        ))
    }

    async fn maybe_checkpoint(&self, version: u64) {
        if version % log::CHECKPOINT_INTERVAL != 0 {
            return;
        }
        // Best-effort: a failed checkpoint costs replay time, never data,
        // because `_last_checkpoint` is only ever a hint.
        if let Err(e) = self.write_checkpoint(version).await {
            tracing::warn!(version, error = %e, "checkpoint failed; replay will be slower");
        }
    }

    #[cfg(test)]
    async fn put_catalog_for_test(&self, name: &str) -> Result<Uuid, UcError> {
        tests::put_catalog_helper(self, name).await
    }

    async fn write_checkpoint(&self, version: u64) -> Result<(), UcError> {
        let (body, size) = {
            let state = self.state.read().await;
            // Only checkpoint the version we were asked for. If we have already
            // moved on, the snapshot no longer matches `version` and writing it
            // under that name would misrepresent history.
            if state.version != version {
                return Ok(());
            }
            state.encode_checkpoint()?
        };

        // Idempotent by construction: encode_checkpoint is deterministic, so two
        // replicas checkpointing the same version write identical bytes and the
        // loser's AlreadyExists is not an error.
        self.log
            .put_if_absent(&action::checkpoint_key(version), body)
            .await?;

        // Pointer last, always. If this write fails the checkpoint is simply not
        // adopted; if it were written first, a crash between the two would leave
        // a pointer to an object that does not exist.
        let ptr = serde_json::to_vec(&LastCheckpoint { version, size })
            .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
        self.log.put(action::LAST_CHECKPOINT_KEY, ptr).await
    }
}

/// The natural key per entity kind — the SQL UNIQUE tuple, rendered so that
/// lexicographic order matches the `ORDER BY` the API promises.
///
/// NUL as separator because it cannot appear in a UC identifier, so
/// ("a\u{0}b", "c") cannot collide with ("a", "b\u{0}c").
fn natural_key_for(kind: EntityKind, body: &serde_json::Value) -> Option<String> {
    let s = |f: &str| body.get(f).and_then(|v| v.as_str()).map(str::to_owned);
    let u = |f: &str| body.get(f).and_then(|v| v.as_u64());
    match kind {
        EntityKind::Catalog => s("name"),
        EntityKind::Schema => Some(format!("{}\u{0}{}", s("catalog_id")?, s("name")?)),
        EntityKind::Table => Some(format!("{}\u{0}{}", s("schema_id")?, s("name")?)),
        EntityKind::Volume => Some(format!("{}\u{0}{}", s("schema_id")?, s("name")?)),
        EntityKind::Function => Some(format!("{}\u{0}{}", s("schema_id")?, s("name")?)),
        EntityKind::RegisteredModel => Some(format!("{}\u{0}{}", s("schema_id")?, s("name")?)),
        // UNIQUE(table_id, commit_version) — the Delta OCC constraint, and the
        // reason this design needs conditional PUT at all. Zero-padded so range
        // scans over versions stay ordered.
        EntityKind::DeltaCommit => {
            Some(format!("{}\u{0}{:020}", s("table_id")?, u("commit_version")?))
        }
        EntityKind::ExternalLocation => s("name"),
        EntityKind::Credential => s("name"),
        EntityKind::User => s("email"),
        // No UNIQUE constraint in the schema — id-addressed only.
        _ => None,
    }
}
