//! The log record format, modelled on the Delta transaction log.
//!
//! A commit file is **JSONL**: one JSON object per line, no enclosing array.
//! Delta names these files `.json` even though the contents are
//! newline-delimited; we follow that convention rather than inventing `.jsonl`,
//! so anyone who knows `_delta_log` can read `_uc_log` without being told.
//!
//! JSONL rather than a single JSON document because:
//!   - a commit can be parsed and applied line by line, so replay never holds a
//!     whole file in memory;
//!   - a truncated trailing line is detectable, where a truncated JSON array is
//!     just a parse error with no indication of how much was recoverable;
//!   - line count is the action count, which is what `_last_checkpoint.size`
//!     reports.
//!
//! Actions are externally tagged single-key objects (`{"upsert": {...}}`),
//! matching Delta's `{"add": {...}}` / `{"remove": {...}}` shape. That is
//! serde's default enum representation, so it costs nothing and keeps the log
//! greppable by action type.

use serde::{Deserialize, Serialize};
use uc_errors::{ErrorCode, UcError};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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

/// Provenance for one commit. Delta writes this as the first line of every
/// commit file; so do we. It carries no state — replay ignores it entirely —
/// which is exactly why it is safe to extend without a format bump.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitInfo {
    /// Bump only for incompatible changes. Replay refuses a version it does not
    /// understand rather than guessing.
    pub format: u32,
    /// Wall-clock ms. Diagnostics only — ordering comes from the filename.
    pub timestamp: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
}

/// One line of a commit file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Line {
    CommitInfo(CommitInfo),
    /// Insert or replace the whole entity. Updates are full-row replacements,
    /// matching what `UPDATE ... RETURNING *` already produced.
    Upsert {
        kind: EntityKind,
        id: Uuid,
        body: serde_json::Value,
    },
    Remove {
        kind: EntityKind,
        id: Uuid,
    },
}

/// The state-bearing subset of `Line`. `CommitInfo` is deliberately not
/// representable here, so `Snapshot::apply` cannot be handed something it has
/// to ignore at runtime.
#[derive(Debug, Clone)]
pub enum Action {
    Upsert {
        kind: EntityKind,
        id: Uuid,
        body: serde_json::Value,
    },
    Remove {
        kind: EntityKind,
        id: Uuid,
    },
}

impl From<Action> for Line {
    fn from(a: Action) -> Line {
        match a {
            Action::Upsert { kind, id, body } => Line::Upsert { kind, id, body },
            Action::Remove { kind, id } => Line::Remove { kind, id },
        }
    }
}

pub const FORMAT_VERSION: u32 = 1;

/// Serialise a commit to JSONL: `commitInfo` first, then one action per line.
pub fn encode_commit(info: &CommitInfo, actions: &[Action]) -> Result<Vec<u8>, UcError> {
    let internal = |e: serde_json::Error| UcError::new(ErrorCode::Internal, e.to_string());
    let mut out = serde_json::to_vec(&Line::CommitInfo(info.clone())).map_err(internal)?;
    out.push(b'\n');
    for action in actions {
        let line: Line = action.clone().into();
        out.extend(serde_json::to_vec(&line).map_err(internal)?);
        out.push(b'\n');
    }
    Ok(out)
}

/// Parse a JSONL commit file into its header and actions.
///
/// A malformed line is fatal, never skipped. Skipping one silently produces a
/// materialised state that differs from what was committed, and nothing
/// downstream would ever detect it.
pub fn decode_commit(key: &str, bytes: &[u8]) -> Result<(CommitInfo, Vec<Action>), UcError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|e| UcError::new(ErrorCode::Internal, format!("{key} is not utf-8: {e}")))?;

    let mut info: Option<CommitInfo> = None;
    let mut actions = Vec::new();

    for (n, raw) in text.lines().enumerate() {
        // Tolerate blank lines only; anything else must parse.
        if raw.trim().is_empty() {
            continue;
        }
        let line: Line = serde_json::from_str(raw).map_err(|e| {
            UcError::new(
                ErrorCode::Internal,
                format!("{key} line {}: malformed action: {e}", n + 1),
            )
        })?;
        match line {
            Line::CommitInfo(ci) => {
                if ci.format > FORMAT_VERSION {
                    return Err(UcError::new(
                        ErrorCode::Internal,
                        format!(
                            "{key} has format {} but this build understands {FORMAT_VERSION}",
                            ci.format
                        ),
                    ));
                }
                info = Some(ci);
            }
            Line::Upsert { kind, id, body } => actions.push(Action::Upsert { kind, id, body }),
            Line::Remove { kind, id } => actions.push(Action::Remove { kind, id }),
        }
    }

    // A commit with no commitInfo is a commit we cannot version-check. Refuse
    // it rather than assume format 1.
    let info = info.ok_or_else(|| {
        UcError::new(ErrorCode::Internal, format!("{key} has no commitInfo line"))
    })?;
    Ok((info, actions))
}

/// Zero-padded to 20 digits so lexicographic LIST order == numeric order, as in
/// `_delta_log`. This is load-bearing: replay applies keys in listing order.
pub fn commit_key(version: u64) -> String {
    format!("_uc_log/{version:020}.json")
}

pub fn checkpoint_key(version: u64) -> String {
    format!("_uc_log/{version:020}.checkpoint.json")
}

pub const LAST_CHECKPOINT_KEY: &str = "_uc_log/_last_checkpoint";
pub const KEYS_KEY: &str = "_uc_log/_keys.json";

/// Parse a version back out of a commit key. Returns None for checkpoints and
/// anything else living under the prefix.
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

    fn info() -> CommitInfo {
        CommitInfo {
            format: FORMAT_VERSION,
            timestamp: 1,
            operation: Some("CREATE CATALOG".into()),
            actor: None,
        }
    }

    #[test]
    fn keys_sort_lexicographically_in_numeric_order() {
        assert!(commit_key(9) < commit_key(10));
    }

    #[test]
    fn checkpoints_are_not_mistaken_for_commits() {
        assert_eq!(version_from_key(&commit_key(7)), Some(7));
        assert_eq!(version_from_key(&checkpoint_key(7)), None);
    }

    #[test]
    fn commit_is_jsonl_with_commitinfo_first() {
        let acts = vec![
            Action::Upsert {
                kind: EntityKind::Catalog,
                id: Uuid::nil(),
                body: serde_json::json!({"name": "a"}),
            },
            Action::Remove {
                kind: EntityKind::Catalog,
                id: Uuid::nil(),
            },
        ];
        let bytes = encode_commit(&info(), &acts).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();

        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3, "one header + one line per action");
        assert!(lines[0].starts_with(r#"{"commitInfo":"#));
        assert!(lines[1].starts_with(r#"{"upsert":"#));
        assert!(lines[2].starts_with(r#"{"remove":"#));
        // Every line must stand alone as a JSON document.
        for l in &lines {
            serde_json::from_str::<serde_json::Value>(l).unwrap();
        }

        let (back, decoded) = decode_commit("k", &bytes).unwrap();
        assert_eq!(back.format, FORMAT_VERSION);
        assert_eq!(decoded.len(), 2);
    }

    #[test]
    fn a_malformed_line_is_fatal_not_skipped() {
        let mut bytes = encode_commit(&info(), &[]).unwrap();
        bytes.extend(b"{not json}\n");
        let err = decode_commit("k", &bytes).unwrap_err();
        assert!(format!("{err:?}").contains("line 2"), "error should name the line");
    }

    #[test]
    fn a_future_format_is_refused() {
        let mut i = info();
        i.format = FORMAT_VERSION + 1;
        let bytes = encode_commit(&i, &[]).unwrap();
        assert!(decode_commit("k", &bytes).is_err());
    }

    #[test]
    fn a_commit_without_commitinfo_is_refused() {
        let line = Line::Remove { kind: EntityKind::Catalog, id: Uuid::nil() };
        let mut bytes = serde_json::to_vec(&line).unwrap();
        bytes.push(b'\n');
        assert!(decode_commit("k", &bytes).is_err());
    }
}
