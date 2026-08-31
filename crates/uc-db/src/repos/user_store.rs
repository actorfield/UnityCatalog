//! Log-structured body for repos::user. Signatures identical to user.rs.

use crate::models::user::UserRow;
use crate::store::action::{Action, EntityKind};
use crate::store::{Snapshot, Store};
use uc_errors::{ErrorCode, UcError};
use uuid::Uuid;

fn row_of(v: &serde_json::Value) -> Result<UserRow, UcError> {
    serde_json::from_value(v.clone())
        .map_err(|e| UcError::new(ErrorCode::Internal, format!("corrupt user row: {e}")))
}

/// uc_users.name is the UNIQUE column. `email` and `external_id` carry no
/// constraint, so lookups on those scan rather than index.
fn first_where(
    snap: &Snapshot,
    pred: impl Fn(&UserRow) -> bool,
) -> Result<Option<UserRow>, UcError> {
    let mut hits: Vec<UserRow> = snap
        .iter(EntityKind::User)
        .map(row_of)
        .collect::<Result<Vec<_>, _>>()?;
    hits.retain(|u| pred(u));
    // Nothing stops duplicates on these columns, and the SQL's fetch_optional
    // returned an arbitrary one. Sorting by id makes the choice stable.
    hits.sort_by_key(|u| u.id);
    Ok(hits.into_iter().next())
}

#[allow(clippy::too_many_arguments)]
pub async fn create(
    store: &Store,
    id: Uuid,
    name: &str,
    email: Option<&str>,
    external_id: Option<&str>,
    state: &str,
    created_at: i64,
) -> Result<UserRow, UcError> {
    let row = UserRow {
        id,
        name: name.to_string(),
        email: email.map(str::to_owned),
        external_id: external_id.map(str::to_owned),
        state: Some(state.to_string()),
        created_at: Some(created_at),
        updated_at: None,
        picture_url: None,
    };
    store
        .commit("CREATE USER", |snap| {
            // The SQL maps no unique violation, so a duplicate name is a 500
            // today. Deliberate change to the domain error.
            if snap.get_by_natural_key(EntityKind::User, name).is_some() {
                return Err(UcError::new(
                    ErrorCode::ResourceAlreadyExists,
                    format!("User '{}' already exists", name),
                ));
            }
            let body = serde_json::to_value(&row)
                .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
            Ok((
                vec![Action::Upsert { kind: EntityKind::User, id, body }],
                row.clone(),
            ))
        })
        .await
}

pub async fn get_by_id(store: &Store, id: Uuid) -> Result<UserRow, UcError> {
    let snap = store.snapshot().await;
    snap.get(EntityKind::User, id)
        .ok_or_else(|| UcError::new(ErrorCode::NotFound, format!("User '{}' not found", id)))
        .and_then(row_of)
}

pub async fn get_by_name(store: &Store, name: &str) -> Result<UserRow, UcError> {
    let snap = store.snapshot().await;
    snap.get_by_natural_key(EntityKind::User, name)
        .ok_or_else(|| UcError::new(ErrorCode::NotFound, format!("User '{}' not found", name)))
        .and_then(row_of)
}

pub async fn get_by_email(store: &Store, email: &str) -> Result<Option<UserRow>, UcError> {
    let snap = store.snapshot().await;
    first_where(&snap, |u| u.email.as_deref() == Some(email))
}

pub async fn get_by_external_id(
    store: &Store,
    external_id: &str,
) -> Result<Option<UserRow>, UcError> {
    let snap = store.snapshot().await;
    first_where(&snap, |u| u.external_id.as_deref() == Some(external_id))
}

/// The SQL version reads then creates with nothing between, so two concurrent
/// logins for a new external_id can both miss and both insert — and since
/// external_id has no UNIQUE, neither fails. Doing the lookup inside the commit
/// closure means the loser re-runs it and adopts the winner's row.
pub async fn find_or_create_by_external_id(
    store: &Store,
    external_id: &str,
) -> Result<UserRow, UcError> {
    let now = chrono::Utc::now().timestamp_millis();
    store
        .commit("FIND OR CREATE USER", |snap| {
            if let Some(existing) = first_where(snap, |u| {
                u.external_id.as_deref() == Some(external_id)
            })? {
                return Ok((vec![], existing));
            }
            // Matches the SQL's construction: name and external_id both take
            // the external id, state ENABLED, no email.
            let row = UserRow {
                id: Uuid::now_v7(),
                name: external_id.to_string(),
                email: None,
                external_id: Some(external_id.to_string()),
                state: Some("ENABLED".to_string()),
                created_at: Some(now),
                updated_at: None,
                picture_url: None,
            };
            let body = serde_json::to_value(&row)
                .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
            Ok((
                vec![Action::Upsert { kind: EntityKind::User, id: row.id, body }],
                row,
            ))
        })
        .await
}

pub async fn list(
    store: &Store,
    page_token: Option<&str>,
    max_results: i64,
) -> Result<(Vec<UserRow>, Option<String>), UcError> {
    let snap = store.snapshot().await;
    let found = snap.scan(EntityKind::User, page_token, crate::pagination::over_fetch(max_results));
    let rows: Vec<UserRow> = found.into_iter().map(row_of).collect::<Result<_, _>>()?;
    let (rows, next) =
        crate::pagination::page(rows, max_results, |r| r.name.clone());
    Ok((rows, next))
}

pub async fn update(
    store: &Store,
    id: Uuid,
    name: Option<&str>,
    email: Option<&str>,
    state: Option<&str>,
    updated_at: i64,
) -> Result<UserRow, UcError> {
    store
        .commit("UPDATE USER", |snap| {
            let current = snap.get(EntityKind::User, id).ok_or_else(|| {
                UcError::new(ErrorCode::NotFound, format!("User '{}' not found", id))
            })?;
            let mut row = row_of(current)?;

            if let Some(target) = name {
                if target != row.name
                    && snap.get_by_natural_key(EntityKind::User, target).is_some()
                {
                    return Err(UcError::new(
                        ErrorCode::ResourceAlreadyExists,
                        format!("User '{}' already exists", target),
                    ));
                }
                row.name = target.to_string();
            }
            // COALESCE: None leaves the existing value alone.
            if let Some(e) = email {
                row.email = Some(e.to_string());
            }
            if let Some(s) = state {
                row.state = Some(s.to_string());
            }
            row.updated_at = Some(updated_at);

            let body = serde_json::to_value(&row)
                .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
            Ok((vec![Action::Upsert { kind: EntityKind::User, id, body }], row))
        })
        .await
}

pub async fn delete(store: &Store, id: Uuid) -> Result<(), UcError> {
    store
        .commit("DROP USER", |snap| {
            if snap.get(EntityKind::User, id).is_none() {
                return Err(UcError::new(
                    ErrorCode::NotFound,
                    format!("User '{}' not found", id),
                ));
            }
            Ok((vec![Action::Remove { kind: EntityKind::User, id }], ()))
        })
        .await
}
