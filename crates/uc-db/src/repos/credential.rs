//! Log-structured body for repos::credential. Signatures identical to credential.rs.

use crate::models::credential::CredentialRow;
use crate::store::action::{Action, EntityKind};
use crate::store::Store;
use uc_errors::{ErrorCode, UcError};
use uuid::Uuid;

fn row_of(v: &serde_json::Value) -> Result<CredentialRow, UcError> {
    serde_json::from_value(v.clone())
        .map_err(|e| UcError::new(ErrorCode::Internal, format!("corrupt credential row: {e}")))
}

pub async fn create(store: &Store, row: &CredentialRow) -> Result<CredentialRow, UcError> {
    let row = row.clone();
    store
        .commit("CREATE CREDENTIAL", |snap| {
            if snap
                .get_by_natural_key(EntityKind::Credential, &row.name)
                .is_some()
            {
                return Err(UcError::new(
                    ErrorCode::StorageCredentialAlreadyExists,
                    format!("Credential '{}' already exists", row.name),
                ));
            }
            let body = serde_json::to_value(&row)
                .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
            Ok((
                vec![Action::Upsert { kind: EntityKind::Credential, id: row.id, body }],
                row.clone(),
            ))
        })
        .await
}

pub async fn get_by_name(store: &Store, name: &str) -> Result<CredentialRow, UcError> {
    let snap = store.snapshot().await;
    snap.get_by_natural_key(EntityKind::Credential, name)
        .ok_or_else(|| {
            UcError::new(
                ErrorCode::NotFound,
                format!("Credential '{}' not found", name),
            )
        })
        .and_then(row_of)
}

pub async fn get_by_id(store: &Store, id: Uuid) -> Result<CredentialRow, UcError> {
    let snap = store.snapshot().await;
    snap.get(EntityKind::Credential, id)
        .ok_or_else(|| {
            UcError::new(
                ErrorCode::NotFound,
                format!("Credential '{}' not found", id),
            )
        })
        .and_then(row_of)
}

pub async fn list(
    store: &Store,
    page_token: Option<&str>,
    max_results: i64,
) -> Result<(Vec<CredentialRow>, Option<String>), UcError> {
    let snap = store.snapshot().await;
    let found = snap.scan(EntityKind::Credential, page_token, crate::pagination::over_fetch(max_results));
    let rows: Vec<CredentialRow> = found.into_iter().map(row_of).collect::<Result<_, _>>()?;
    let (rows, next) =
        crate::pagination::page(rows, max_results, |r| r.name.clone());
    Ok((rows, next))
}

pub async fn delete(store: &Store, name: &str) -> Result<(), UcError> {
    store
        .commit("DROP CREDENTIAL", |snap| {
            let current = snap
                .get_by_natural_key(EntityKind::Credential, name)
                .ok_or_else(|| {
                    UcError::new(
                        ErrorCode::NotFound,
                        format!("Credential '{}' not found", name),
                    )
                })?;
            let row = row_of(current)?;
            Ok((
                vec![Action::Remove { kind: EntityKind::Credential, id: row.id }],
                (),
            ))
        })
        .await
}

/// Patch a credential in place. `None` leaves a field alone, matching COALESCE.
#[allow(clippy::too_many_arguments)]
pub async fn update(
    store: &Store,
    id: Uuid,
    new_name: Option<&str>,
    comment: Option<&str>,
    owner: Option<&str>,
    credential: Option<&str>,
    updated_at: i64,
    updated_by: Option<&str>,
) -> Result<(), UcError> {
    store
        .commit("UPDATE CREDENTIAL", |snap| {
            // The SQL UPDATE matches zero rows and reports success for a
            // missing id; preserved.
            let Some(current) = snap.get(EntityKind::Credential, id) else {
                return Ok((vec![], ()));
            };
            let mut row = row_of(current)?;

            if let Some(target) = new_name {
                if target != row.name
                    && snap
                        .get_by_natural_key(EntityKind::Credential, target)
                        .is_some()
                {
                    return Err(UcError::new(
                        ErrorCode::StorageCredentialAlreadyExists,
                        format!("Credential '{}' already exists", target),
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
            if let Some(c) = credential {
                row.credential = c.to_string();
            }
            row.updated_at = Some(updated_at);
            row.updated_by = updated_by.map(str::to_owned);

            let body = serde_json::to_value(&row)
                .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
            Ok((
                vec![Action::Upsert { kind: EntityKind::Credential, id, body }],
                (),
            ))
        })
        .await
}
