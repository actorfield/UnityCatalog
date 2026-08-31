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
    let found = snap.scan(EntityKind::Credential, page_token, max_results as usize + 1);
    let rows: Vec<CredentialRow> = found.into_iter().map(row_of).collect::<Result<_, _>>()?;
    let next = if rows.len() as i64 > max_results {
        rows.get(max_results as usize - 1).map(|r| r.name.clone())
    } else {
        None
    };
    Ok((rows.into_iter().take(max_results as usize).collect(), next))
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
