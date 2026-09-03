//! Casbin policy storage, log-structured body. Signatures identical to casbin.rs.

use crate::models::casbin::CasbinRule;
use crate::store::action::{Action, EntityKind};
use crate::store::row::Row;
use crate::store::Store;
use uc_errors::{ErrorCode, UcError};
use uuid::Uuid;

fn rule_of(v: &Row) -> Result<CasbinRule, UcError> {
    crate::typed_row!(v, Row::CasbinRule, "casbin rule")
}

fn body_of(rule: &CasbinRule) -> Result<serde_json::Value, UcError> {
    serde_json::to_value(rule).map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))
}

/// The UNIQUE INDEX on (ptype, v0..v5), rendered as the store's natural key.
fn nk(rule: &CasbinRule) -> String {
    [
        rule.ptype.as_str(),
        rule.v0.as_str(),
        rule.v1.as_str(),
        rule.v2.as_str(),
        rule.v3.as_str(),
        rule.v4.as_str(),
        rule.v5.as_str(),
    ]
    .join("\u{0}")
}

/// All rules in insertion order.
///
/// The SQL orders by an AUTOINCREMENT id; here ids are UUIDv7, so ordering by
/// id is ordering by creation time. Same result, and only because the ids are
/// time-ordered — this would return arbitrary order under v4.
pub async fn load_all(store: &Store) -> Result<Vec<CasbinRule>, UcError> {
    let snap = store.snapshot().await;
    snap.iter_by_id(EntityKind::CasbinRule)
        .into_iter()
        .map(|(_, v)| rule_of(v))
        .collect()
}

/// Returns false when the rule was already present, matching INSERT OR IGNORE.
pub async fn insert(store: &Store, rule: &CasbinRule) -> Result<bool, UcError> {
    let rule = rule.clone();
    let key = nk(&rule);
    store
        .commit("ADD POLICY", |snap| {
            if snap
                .get_by_natural_key(EntityKind::CasbinRule, &key)
                .is_some()
            {
                // No actions: an empty commit is skipped, so a duplicate add
                // does not append to the log.
                return Ok((vec![], false));
            }
            Ok((
                vec![Action::Upsert {
                    kind: EntityKind::CasbinRule,
                    id: Uuid::now_v7(),
                    body: body_of(&rule)?,
                }],
                true,
            ))
        })
        .await
}

/// Returns false when no such rule existed.
pub async fn delete(store: &Store, rule: &CasbinRule) -> Result<bool, UcError> {
    let key = nk(rule);
    store
        .commit("REMOVE POLICY", |snap| {
            let Some(id) = snap.id_by_natural_key(EntityKind::CasbinRule, &key) else {
                return Ok((vec![], false));
            };
            Ok((
                vec![Action::Remove {
                    kind: EntityKind::CasbinRule,
                    id,
                }],
                true,
            ))
        })
        .await
}

/// True if any rule was removed. One commit for the whole batch, so a filtered
/// removal is all-or-nothing rather than a sequence of independent deletes.
pub async fn delete_many(store: &Store, rules: &[CasbinRule]) -> Result<bool, UcError> {
    let keys: Vec<String> = rules.iter().map(nk).collect();
    store
        .commit("REMOVE POLICIES", |snap| {
            let actions: Vec<Action> = keys
                .iter()
                .filter_map(|k| snap.id_by_natural_key(EntityKind::CasbinRule, k))
                .map(|id| Action::Remove {
                    kind: EntityKind::CasbinRule,
                    id,
                })
                .collect();
            let any = !actions.is_empty();
            Ok((actions, any))
        })
        .await
}

/// Replace the entire policy set.
///
/// The SQL wraps delete-then-insert in a transaction, with a comment about
/// minimising the window where the table is empty — an authorizer reading
/// mid-replace would see no policy and deny everything. As a single commit
/// that window is not merely short, it does not exist: no reader can observe a
/// state between the two.
pub async fn replace_all(store: &Store, rules: &[CasbinRule]) -> Result<(), UcError> {
    let rules = rules.to_vec();
    store
        .commit("SAVE POLICY", |snap| {
            let mut actions: Vec<Action> = snap
                .iter_by_id(EntityKind::CasbinRule)
                .into_iter()
                .map(|(id, _)| Action::Remove {
                    kind: EntityKind::CasbinRule,
                    id,
                })
                .collect();
            // Deduplicate on the way in, matching INSERT OR IGNORE: casbin can
            // hand back the same rule under more than one section.
            let mut seen = std::collections::HashSet::new();
            for rule in &rules {
                if !seen.insert(nk(rule)) {
                    continue;
                }
                actions.push(Action::Upsert {
                    kind: EntityKind::CasbinRule,
                    id: Uuid::now_v7(),
                    body: body_of(rule)?,
                });
            }
            Ok((actions, ()))
        })
        .await
}

pub async fn clear(store: &Store) -> Result<(), UcError> {
    store
        .commit("CLEAR POLICY", |snap| {
            let actions: Vec<Action> = snap
                .iter_by_id(EntityKind::CasbinRule)
                .into_iter()
                .map(|(id, _)| Action::Remove {
                    kind: EntityKind::CasbinRule,
                    id,
                })
                .collect();
            Ok((actions, ()))
        })
        .await
}
