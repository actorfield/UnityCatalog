//! Log-structured body for repos::volume. Signatures identical to volume.rs.

use crate::models::volume::VolumeRow;
use crate::store::action::{Action, EntityKind};
use crate::store::Store;
use uc_errors::{ErrorCode, UcError};
use uuid::Uuid;

fn row_of(v: &serde_json::Value) -> Result<VolumeRow, UcError> {
    serde_json::from_value(v.clone())
        .map_err(|e| UcError::new(ErrorCode::Internal, format!("corrupt volume row: {e}")))
}

/// UNIQUE(schema_id, name)
fn nk(schema_id: Uuid, name: &str) -> String {
    format!("{schema_id}\u{0}{name}")
}

fn prefix(schema_id: Uuid) -> String {
    format!("{schema_id}\u{0}")
}

pub async fn create(store: &Store, row: &VolumeRow) -> Result<VolumeRow, UcError> {
    let row = row.clone();
    store
        .commit("CREATE VOLUME", |snap| {
            if snap
                .get_by_natural_key(EntityKind::Volume, &nk(row.schema_id, &row.name))
                .is_some()
            {
                return Err(UcError::new(
                    ErrorCode::ResourceAlreadyExists,
                    format!("Volume '{}' already exists", row.name),
                ));
            }
            let body = serde_json::to_value(&row)
                .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
            Ok((
                vec![Action::Upsert { kind: EntityKind::Volume, id: row.id, body }],
                row.clone(),
            ))
        })
        .await
}

pub async fn get_by_id(store: &Store, id: Uuid) -> Result<VolumeRow, UcError> {
    let snap = store.snapshot().await;
    snap.get(EntityKind::Volume, id)
        .ok_or_else(|| UcError::new(ErrorCode::NotFound, format!("Volume '{}' not found", id)))
        .and_then(row_of)
}

pub async fn get_by_schema_and_name(
    store: &Store,
    schema_id: Uuid,
    name: &str,
) -> Result<VolumeRow, UcError> {
    let snap = store.snapshot().await;
    snap.get_by_natural_key(EntityKind::Volume, &nk(schema_id, name))
        .ok_or_else(|| UcError::new(ErrorCode::NotFound, format!("Volume '{}' not found", name)))
        .and_then(row_of)
}

pub async fn list(
    store: &Store,
    schema_id: Uuid,
    page_token: Option<&str>,
    max_results: i64,
) -> Result<(Vec<VolumeRow>, Option<String>), UcError> {
    let snap = store.snapshot().await;
    let found = snap.scan_prefix(
        EntityKind::Volume,
        &prefix(schema_id),
        page_token,
        crate::pagination::over_fetch(max_results),
    );
    let rows: Vec<VolumeRow> = found.into_iter().map(row_of).collect::<Result<_, _>>()?;
    let (rows, next_token) =
        crate::pagination::page(rows, max_results, |r| r.name.clone());
    Ok((rows, next_token))
}

pub async fn update(
    store: &Store,
    id: Uuid,
    new_name: Option<&str>,
    comment: Option<&str>,
    owner: Option<&str>,
    updated_at: i64,
    updated_by: Option<&str>,
) -> Result<VolumeRow, UcError> {
    store
        .commit("UPDATE VOLUME", |snap| {
            let current = snap.get(EntityKind::Volume, id).ok_or_else(|| {
                UcError::new(ErrorCode::NotFound, format!("Volume '{}' not found", id))
            })?;
            let mut row = row_of(current)?;

            if let Some(target) = new_name {
                if target != row.name
                    && snap
                        .get_by_natural_key(EntityKind::Volume, &nk(row.schema_id, target))
                        .is_some()
                {
                    return Err(UcError::new(
                        ErrorCode::ResourceAlreadyExists,
                        format!("Volume '{}' already exists", target),
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
                vec![Action::Upsert { kind: EntityKind::Volume, id, body }],
                row,
            ))
        })
        .await
}

pub async fn delete(store: &Store, id: Uuid) -> Result<(), UcError> {
    store
        .commit("DROP VOLUME", |snap| {
            if snap.get(EntityKind::Volume, id).is_none() {
                return Err(UcError::new(
                    ErrorCode::NotFound,
                    format!("Volume '{}' not found", id),
                ));
            }
            Ok((vec![Action::Remove { kind: EntityKind::Volume, id }], ()))
        })
        .await
}
