//! In-memory materialisation of the log, plus the commit loop.
//!
//! `Store` replaces `sqlx::SqlitePool` as the handle every repo function takes.
//! Keeping the same shape (`&Store` where `&AnyPool` used to be) is what lets
//! the ~262 call sites in uc-api compile untouched.

pub mod action;
pub mod log;

use action::{Action, EntityKind};
use log::{ObjectLog, PutResult, MAX_COMMIT_ATTEMPTS};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tokio::sync::RwLock;
use uc_errors::UcError;
use uuid::Uuid;

/// The materialised state at a particular log version.
///
/// Rows are held as `serde_json::Value` rather than typed structs so one
/// snapshot can serve all 18 entity kinds without a trait object per kind.
/// Repo functions deserialise into their own `*Row` on the way out, which is
/// the same boundary `FromRow` occupied before.
#[derive(Default)]
pub struct Snapshot {
    pub version: u64,
    entities: HashMap<EntityKind, HashMap<Uuid, serde_json::Value>>,
    /// Natural-key index. The key is `(kind, composite)` where `composite` is
    /// the SQL UNIQUE tuple rendered in sort order — `"name"` for catalogs,
    /// `"{catalog_id}\u{0}{name}"` for schemas, and so on.
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
        let lower = after.map(|a| a.to_string()).unwrap_or_default();
        self.by_natural_key
            .range((kind, lower.clone())..)
            .take_while(|((k, _), _)| *k == kind)
            .filter(|((_, nk), _)| after.is_none_or(|a| nk.as_str() > a))
            .filter_map(|(_, id)| self.get(kind, *id))
            .take(limit)
            .collect()
    }

    /// Apply one action. Must be deterministic and total — replay correctness
    /// depends on this producing identical state from identical input.
    fn apply(&mut self, act: &Action, natural_key: impl Fn(EntityKind, &serde_json::Value) -> Option<String>) {
        match act {
            Action::Upsert { kind, id, body } => {
                // Rename: drop the stale natural-key entry before inserting the
                // new one, or the old name stays reachable forever.
                if let Some(prev) = self.get(*kind, *id) {
                    if let Some(old) = natural_key(*kind, prev) {
                        self.by_natural_key.remove(&(*kind, old));
                    }
                }
                if let Some(nk) = natural_key(*kind, body) {
                    self.by_natural_key.insert((*kind, nk), *id);
                }
                self.entities.entry(*kind).or_default().insert(*id, body.clone());
            }
            Action::Delete { kind, id } => {
                if let Some(prev) = self.entities.entry(*kind).or_default().remove(id) {
                    if let Some(nk) = natural_key(*kind, &prev) {
                        self.by_natural_key.remove(&(*kind, nk));
                    }
                }
            }
        }
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
                *state = deserialise_checkpoint(&body)?;
                state.version = version;
            }
        }
        for (version, commit) in log::read_commits_after(&*self.log, state.version).await? {
            for act in &commit.actions {
                state.apply(act, natural_key_for);
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
    /// name is free; one wins N+1 and the other must re-evaluate its
    /// precondition against the commit that just beat it. A retry that reuses
    /// the previously built actions would cheerfully write a duplicate name at
    /// N+2. So `build` re-runs, and it is where `AlreadyExists` is raised.
    pub async fn commit<T, F>(&self, mut build: F) -> Result<T, UcError>
    where
        F: FnMut(&Snapshot) -> Result<(Vec<Action>, T), UcError>,
    {
        for _ in 0..MAX_COMMIT_ATTEMPTS {
            let (actions, out, target) = {
                let state = self.state.read().await;
                let (actions, out) = build(&state)?;
                (actions, out, state.version + 1)
            };

            let commit = action::Commit {
                format: action::FORMAT_VERSION,
                timestamp: chrono::Utc::now().timestamp_millis(),
                actor: None,
                actions,
            };
            let body = serde_json::to_vec(&commit)
                .map_err(|e| UcError::new(uc_errors::ErrorCode::Internal, e.to_string()))?;

            match self.log.put_if_absent(&action::commit_key(target), body).await? {
                PutResult::Created => {
                    let mut state = self.state.write().await;
                    // Another task may have advanced us past `target` while we
                    // were writing; re-applying our own commit is handled by
                    // catch_up, so only apply when we are still the next in line.
                    if state.version + 1 == target {
                        for act in &commit.actions {
                            state.apply(act, natural_key_for);
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
            uc_errors::ErrorCode::Internal,
            "commit contention: exceeded max attempts",
        ))
    }

    async fn maybe_checkpoint(&self, version: u64) {
        if version % log::CHECKPOINT_INTERVAL != 0 {
            return;
        }
        // Best-effort: a failed checkpoint costs replay time, never data.
        // `_last_checkpoint` is only ever a hint (see log::resolve_checkpoint).
        if let Err(e) = self.write_checkpoint(version).await {
            tracing::warn!(version, error = %e, "checkpoint failed; replay will be slower");
        }
    }

    async fn write_checkpoint(&self, _version: u64) -> Result<(), UcError> {
        todo!("serialise Snapshot -> checkpoint_key(version), then put _last_checkpoint")
    }
}

fn deserialise_checkpoint(_body: &[u8]) -> Result<Snapshot, UcError> {
    todo!("inverse of write_checkpoint; must rebuild by_natural_key, not trust a stored index")
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
        // UNIQUE(table_id, commit_version) — this is the Delta OCC constraint,
        // and the reason the whole design needs conditional PUT. Zero-pad so
        // range scans over versions stay ordered.
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
