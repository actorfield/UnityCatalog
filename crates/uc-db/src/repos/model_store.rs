//! Log-structured body for repos::model. Signatures identical to model.rs.

use crate::models::model::{ModelVersionRow, RegisteredModelRow};
use crate::store::action::{Action, EntityKind};
use crate::store::Store;
use uc_errors::{ErrorCode, UcError};
use uuid::Uuid;

fn model_of(v: &serde_json::Value) -> Result<RegisteredModelRow, UcError> {
    serde_json::from_value(v.clone())
        .map_err(|e| UcError::new(ErrorCode::Internal, format!("corrupt model row: {e}")))
}

fn version_of(v: &serde_json::Value) -> Result<ModelVersionRow, UcError> {
    serde_json::from_value(v.clone())
        .map_err(|e| UcError::new(ErrorCode::Internal, format!("corrupt model version: {e}")))
}

/// UNIQUE(schema_id, name)
fn nk(schema_id: Uuid, name: &str) -> String {
    format!("{schema_id}\u{0}{name}")
}

fn prefix(schema_id: Uuid) -> String {
    format!("{schema_id}\u{0}")
}

/// Versions of one model, ordered.
///
/// uc_model_versions has only an INDEX on (registered_model_id, version), not a
/// UNIQUE, so duplicates are representable and there is no natural-key index to
/// use. Scanning and sorting is also what makes the result deterministic: the
/// SQL's `fetch_one` returned an arbitrary row among duplicates.
fn versions_of(
    snap: &crate::store::Snapshot,
    model_id: Uuid,
) -> Result<Vec<ModelVersionRow>, UcError> {
    let mut rows: Vec<ModelVersionRow> = snap
        .iter(EntityKind::ModelVersion)
        .map(version_of)
        .collect::<Result<Vec<_>, _>>()?;
    rows.retain(|r| r.registered_model_id == model_id);
    rows.sort_by_key(|r| (r.version, r.id));
    Ok(rows)
}

pub async fn create_model(
    store: &Store,
    row: &RegisteredModelRow,
) -> Result<RegisteredModelRow, UcError> {
    let row = row.clone();
    store
        .commit("CREATE MODEL", |snap| {
            // As with functions, the SQL maps no unique violation, so a
            // duplicate name is a 500 today. Deliberate change to the domain
            // error.
            if snap
                .get_by_natural_key(EntityKind::RegisteredModel, &nk(row.schema_id, &row.name))
                .is_some()
            {
                return Err(UcError::new(
                    ErrorCode::ResourceAlreadyExists,
                    format!("Model '{}' already exists", row.name),
                ));
            }
            let body = serde_json::to_value(&row)
                .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
            Ok((
                vec![Action::Upsert {
                    kind: EntityKind::RegisteredModel,
                    id: row.id,
                    body,
                }],
                row.clone(),
            ))
        })
        .await
}

pub async fn get_model_by_schema_and_name(
    store: &Store,
    schema_id: Uuid,
    name: &str,
) -> Result<RegisteredModelRow, UcError> {
    let snap = store.snapshot().await;
    snap.get_by_natural_key(EntityKind::RegisteredModel, &nk(schema_id, name))
        .ok_or_else(|| UcError::new(ErrorCode::NotFound, format!("Model '{}' not found", name)))
        .and_then(model_of)
}

pub async fn list_models(
    store: &Store,
    schema_id: Uuid,
    page_token: Option<&str>,
    max_results: i64,
) -> Result<(Vec<RegisteredModelRow>, Option<String>), UcError> {
    let snap = store.snapshot().await;
    let found = snap.scan_prefix(
        EntityKind::RegisteredModel,
        &prefix(schema_id),
        page_token,
        max_results as usize + 1,
    );
    let rows: Vec<RegisteredModelRow> =
        found.into_iter().map(model_of).collect::<Result<_, _>>()?;
    let next = if rows.len() as i64 > max_results {
        rows.get(max_results as usize - 1).map(|r| r.name.clone())
    } else {
        None
    };
    Ok((rows.into_iter().take(max_results as usize).collect(), next))
}

pub async fn create_version(
    store: &Store,
    row: &ModelVersionRow,
) -> Result<ModelVersionRow, UcError> {
    let row = row.clone();
    store
        .commit("CREATE MODEL VERSION", |_| {
            let body = serde_json::to_value(&row)
                .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
            Ok((
                vec![Action::Upsert {
                    kind: EntityKind::ModelVersion,
                    id: row.id,
                    body,
                }],
                row.clone(),
            ))
        })
        .await
}

pub async fn get_version(
    store: &Store,
    model_id: Uuid,
    version: i32,
) -> Result<ModelVersionRow, UcError> {
    let snap = store.snapshot().await;
    versions_of(&snap, model_id)?
        .into_iter()
        .find(|r| r.version == Some(version))
        .ok_or_else(|| {
            UcError::new(
                ErrorCode::NotFound,
                format!("Model version {} not found", version),
            )
        })
}

/// Drops the model and every version, in one commit.
///
/// The SQL issues two DELETEs with nothing joining them and never checks
/// rows_affected, so a missing model is not an error. Both behaviours preserved.
pub async fn delete_model(store: &Store, id: Uuid) -> Result<(), UcError> {
    store
        .commit("DROP MODEL", |snap| {
            let mut actions: Vec<Action> = versions_of(snap, id)?
                .into_iter()
                .map(|r| Action::Remove {
                    kind: EntityKind::ModelVersion,
                    id: r.id,
                })
                .collect();
            if snap.get(EntityKind::RegisteredModel, id).is_some() {
                actions.push(Action::Remove {
                    kind: EntityKind::RegisteredModel,
                    id,
                });
            }
            Ok((actions, ()))
        })
        .await
}

/// Deleting an absent version is not an error, matching the SQL's unchecked
/// rows_affected.
pub async fn delete_version(store: &Store, model_id: Uuid, version: i32) -> Result<(), UcError> {
    store
        .commit("DROP MODEL VERSION", |snap| {
            let actions: Vec<Action> = versions_of(snap, model_id)?
                .into_iter()
                .filter(|r| r.version == Some(version))
                .map(|r| Action::Remove {
                    kind: EntityKind::ModelVersion,
                    id: r.id,
                })
                .collect();
            Ok((actions, ()))
        })
        .await
}
