//! Log-structured body for repos::property. Signatures identical to property.rs.

use crate::models::property::PropertyRow;
use crate::store::action::{Action, EntityKind};
use crate::store::row::Row;
use crate::store::Store;
use std::collections::HashMap;
use uc_errors::{ErrorCode, UcError};
use uuid::Uuid;

fn row_of(v: &Row) -> Result<PropertyRow, UcError> {
    crate::typed_row!(v, Row::Property, "property")
}

/// UNIQUE(entity_id, entity_type, property_key); this is the group prefix.
fn prefix(entity_id: Uuid, entity_type: &str) -> String {
    format!("{entity_id}\u{0}{entity_type}\u{0}")
}

pub async fn get_for_entity(
    store: &Store,
    entity_id: Uuid,
    entity_type: &str,
) -> Result<HashMap<String, String>, UcError> {
    let snap = store.snapshot().await;
    let mut out = HashMap::new();
    for id in snap.ids_under_prefix(EntityKind::Property, &prefix(entity_id, entity_type)) {
        if let Some(v) = snap.get(EntityKind::Property, id) {
            let row = row_of(v)?;
            out.insert(row.property_key, row.property_value);
        }
    }
    Ok(out)
}

/// Replace an entity's whole property set.
///
/// The SQL version documents itself as "must be called inside a transaction" —
/// it issues a DELETE then one INSERT per property, so a crash or a concurrent
/// reader lands between them and sees the entity with no properties at all.
/// Whether that transaction exists is up to each caller.
///
/// Here the deletes and inserts are actions in a single commit, so atomicity is
/// structural rather than a precondition on the caller. Same signature; the
/// warning in the doc comment no longer applies.
pub async fn replace(
    store: &Store,
    entity_id: Uuid,
    entity_type: &str,
    properties: &HashMap<String, String>,
) -> Result<(), UcError> {
    let pfx = prefix(entity_id, entity_type);
    store
        .commit("SET PROPERTIES", |snap| {
            let mut actions: Vec<Action> = snap
                .ids_under_prefix(EntityKind::Property, &pfx)
                .into_iter()
                .map(|id| Action::Remove {
                    kind: EntityKind::Property,
                    id,
                })
                .collect();

            // Sorted so the commit is reproducible: HashMap iteration order
            // varies per process, and an arbitrary action order would make two
            // replicas write different bytes for the same logical change.
            let mut entries: Vec<(&String, &String)> = properties.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));

            for (key, value) in entries {
                let row = PropertyRow {
                    id: Uuid::now_v7(),
                    entity_id,
                    entity_type: entity_type.to_string(),
                    property_key: key.clone(),
                    property_value: value.clone(),
                };
                let body = serde_json::to_value(&row)
                    .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
                actions.push(Action::Upsert {
                    kind: EntityKind::Property,
                    id: row.id,
                    body,
                });
            }
            Ok((actions, ()))
        })
        .await
}

pub async fn delete_for_entity(
    store: &Store,
    entity_id: Uuid,
    entity_type: &str,
) -> Result<(), UcError> {
    let pfx = prefix(entity_id, entity_type);
    store
        .commit("UNSET PROPERTIES", |snap| {
            let actions: Vec<Action> = snap
                .ids_under_prefix(EntityKind::Property, &pfx)
                .into_iter()
                .map(|id| Action::Remove {
                    kind: EntityKind::Property,
                    id,
                })
                .collect();
            Ok((actions, ()))
        })
        .await
}

/// Upsert a single property, replacing any existing value for that key.
///
/// The SQL `INSERT OR REPLACE` allocates a fresh id on conflict; this keeps the
/// existing row's id and changes only the value. Property ids are never
/// surfaced by the API, and a stable id makes the log diff show a value change
/// rather than a delete-plus-insert.
pub async fn set(
    store: &Store,
    entity_id: Uuid,
    entity_type: &str,
    key: &str,
    value: &str,
) -> Result<(), UcError> {
    let natural = format!("{}{}", prefix(entity_id, entity_type), key);
    store
        .commit("SET PROPERTY", |snap| {
            let id = snap
                .get_by_natural_key(EntityKind::Property, &natural)
                .map(row_of)
                .transpose()?
                .map(|r| r.id)
                .unwrap_or_else(Uuid::now_v7);
            let row = PropertyRow {
                id,
                entity_id,
                entity_type: entity_type.to_string(),
                property_key: key.to_string(),
                property_value: value.to_string(),
            };
            let body = serde_json::to_value(&row)
                .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
            Ok((
                vec![Action::Upsert {
                    kind: EntityKind::Property,
                    id,
                    body,
                }],
                (),
            ))
        })
        .await
}

/// Delete one property by key. Deleting an absent key is not an error.
pub async fn delete_key(
    store: &Store,
    entity_id: Uuid,
    entity_type: &str,
    key: &str,
) -> Result<(), UcError> {
    let natural = format!("{}{}", prefix(entity_id, entity_type), key);
    store
        .commit("UNSET PROPERTY", |snap| {
            let Some(existing) = snap.get_by_natural_key(EntityKind::Property, &natural) else {
                return Ok((vec![], ()));
            };
            let row = row_of(existing)?;
            Ok((
                vec![Action::Remove {
                    kind: EntityKind::Property,
                    id: row.id,
                }],
                (),
            ))
        })
        .await
}
