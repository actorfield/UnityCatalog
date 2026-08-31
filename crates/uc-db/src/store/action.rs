//! The log record format.
//!
//! One commit file is a `Commit` — a small header plus an ordered list of
//! `Action`s applied atomically. Actions are deliberately generic
//! (`Upsert`/`Delete` over an `EntityKind` + JSON body) rather than one variant
//! per entity: the repo layer is a keyed document store, so 20 typed variant
//! families would be 20 ways to say the same thing, and every new field would
//! touch the log format.
//!
//! The tradeoff is that the log is not self-validating — a bad body is only
//! caught at replay. That is acceptable because the only writer is this crate.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Metastore,
    Catalog,
    Schema,
    Table,
    Column,
    Volume,
    Function,
    FunctionParameter,
    RegisteredModel,
    ModelVersion,
    StagingTable,
    DeltaCommit,
    User,
    Credential,
    ExternalLocation,
    Property,
    Dependency,
    CasbinRule,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Action {
    /// Insert or replace the whole entity. Updates are full-row replacements,
    /// matching what the SQL `UPDATE ... RETURNING *` path already produced.
    Upsert {
        kind: EntityKind,
        id: Uuid,
        body: serde_json::Value,
    },
    Delete {
        kind: EntityKind,
        id: Uuid,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Commit {
    /// Log format version. Bump only for incompatible changes; replay must
    /// refuse a version it does not understand rather than guess.
    pub format: u32,
    /// Wall-clock ms. Diagnostics only — ordering comes from the filename.
    pub timestamp: i64,
    /// Best-effort provenance for debugging a surprising log.
    pub actor: Option<String>,
    pub actions: Vec<Action>,
}

pub const FORMAT_VERSION: u32 = 1;

/// Zero-padded to 20 digits so lexicographic LIST order == numeric order.
/// This is load-bearing: replay reads the listing in returned order.
pub fn commit_key(version: u64) -> String {
    format!("_uc_log/{version:020}.json")
}

pub fn checkpoint_key(version: u64) -> String {
    format!("_uc_log/{version:020}.checkpoint.json")
}

pub const LAST_CHECKPOINT_KEY: &str = "_uc_log/_last_checkpoint";
pub const KEYS_KEY: &str = "_uc_log/_keys.json";

/// Parse a version back out of a commit key. Returns None for checkpoints and
/// for anything else that happens to live under the prefix.
pub fn version_from_key(key: &str) -> Option<u64> {
    let name = key.rsplit('/').next()?;
    let stem = name.strip_suffix(".json")?;
    if stem.ends_with(".checkpoint") {
        return None;
    }
    stem.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_sort_lexicographically_in_numeric_order() {
        let a = commit_key(9);
        let b = commit_key(10);
        assert!(a < b, "{a} should sort before {b}");
    }

    #[test]
    fn checkpoints_are_not_mistaken_for_commits() {
        assert_eq!(version_from_key(&commit_key(7)), Some(7));
        assert_eq!(version_from_key(&checkpoint_key(7)), None);
    }
}
