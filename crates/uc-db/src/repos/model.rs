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
        crate::pagination::over_fetch(max_results),
    );
    let rows: Vec<RegisteredModelRow> =
        found.into_iter().map(model_of).collect::<Result<_, _>>()?;
    let (rows, next) = crate::pagination::page(rows, max_results, |r| r.name.clone());
    Ok((rows, next))
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

/// Patch a registered model in place. `None` leaves a field alone.
pub async fn update_model(
    store: &Store,
    id: Uuid,
    new_name: Option<&str>,
    comment: Option<&str>,
    owner: Option<&str>,
    updated_at: i64,
    updated_by: Option<&str>,
) -> Result<(), UcError> {
    store
        .commit("UPDATE MODEL", |snap| {
            // Zero rows matched is success in the SQL; preserved.
            let Some(current) = snap.get(EntityKind::RegisteredModel, id) else {
                return Ok((vec![], ()));
            };
            let mut row = model_of(current)?;

            if let Some(target) = new_name {
                if target != row.name
                    && snap
                        .get_by_natural_key(EntityKind::RegisteredModel, &nk(row.schema_id, target))
                        .is_some()
                {
                    return Err(UcError::new(
                        ErrorCode::ResourceAlreadyExists,
                        format!("Model '{}' already exists", target),
                    ));
                }
                row.name = target.to_string();
            }
            if let Some(c) = comment {
                row.comment = Some(c.to_string());
            }
            if let Some(o) = owner {
                row.owner = Some(o.to_string());
            }
            row.updated_at = Some(updated_at);
            row.updated_by = updated_by.map(str::to_owned);

            let body = serde_json::to_value(&row)
                .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
            Ok((
                vec![Action::Upsert {
                    kind: EntityKind::RegisteredModel,
                    id,
                    body,
                }],
                (),
            ))
        })
        .await
}

pub async fn set_max_version(store: &Store, id: Uuid, next: i32) -> Result<(), UcError> {
    store
        .commit("SET MAX VERSION", |snap| {
            let Some(current) = snap.get(EntityKind::RegisteredModel, id) else {
                return Ok((vec![], ()));
            };
            let mut row = model_of(current)?;
            row.max_version_number = Some(next);
            let body = serde_json::to_value(&row)
                .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
            Ok((
                vec![Action::Upsert {
                    kind: EntityKind::RegisteredModel,
                    id,
                    body,
                }],
                (),
            ))
        })
        .await
}

/// All versions of a model, ordered by version.
pub async fn list_versions(store: &Store, model_id: Uuid) -> Result<Vec<ModelVersionRow>, UcError> {
    let snap = store.snapshot().await;
    versions_of(&snap, model_id)
}

pub async fn update_version(
    store: &Store,
    id: Uuid,
    comment: Option<&str>,
    updated_at: i64,
    updated_by: Option<&str>,
) -> Result<(), UcError> {
    store
        .commit("UPDATE MODEL VERSION", |snap| {
            let Some(current) = snap.get(EntityKind::ModelVersion, id) else {
                return Ok((vec![], ()));
            };
            let mut row = version_of(current)?;
            if let Some(c) = comment {
                row.comment = Some(c.to_string());
            }
            row.updated_at = Some(updated_at);
            row.updated_by = updated_by.map(str::to_owned);
            let body = serde_json::to_value(&row)
                .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
            Ok((
                vec![Action::Upsert {
                    kind: EntityKind::ModelVersion,
                    id,
                    body,
                }],
                (),
            ))
        })
        .await
}

pub async fn set_version_status(store: &Store, id: Uuid, status: &str) -> Result<(), UcError> {
    store
        .commit("SET MODEL VERSION STATUS", |snap| {
            let Some(current) = snap.get(EntityKind::ModelVersion, id) else {
                return Ok((vec![], ()));
            };
            let mut row = version_of(current)?;
            row.status = Some(status.to_string());
            let body = serde_json::to_value(&row)
                .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
            Ok((
                vec![Action::Upsert {
                    kind: EntityKind::ModelVersion,
                    id,
                    body,
                }],
                (),
            ))
        })
        .await
}
