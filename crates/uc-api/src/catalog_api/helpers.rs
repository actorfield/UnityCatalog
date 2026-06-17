use uc_auth::UcClaims;
use uc_db::repos::UserRepo;
use uc_errors::UcError;
use uc_types::Privilege;
use uuid::Uuid;
use crate::state::AppState;

pub fn split2(s: &str) -> Result<(&str, &str), UcError> {
    let mut it = s.splitn(2, '.');
    let a = it.next().ok_or_else(|| UcError::invalid_argument("expected catalog.name"))?;
    let b = it.next().ok_or_else(|| UcError::invalid_argument("expected catalog.name"))?;
    Ok((a, b))
}

pub fn split3(s: &str) -> Result<(&str, &str, &str), UcError> {
    let v: Vec<&str> = s.splitn(3, '.').collect();
    if v.len() != 3 { return Err(UcError::invalid_argument("expected catalog.schema.name")); }
    Ok((v[0], v[1], v[2]))
}

pub async fn get_user(state: &AppState, email: &str) -> Result<uc_db::models::user::UserRow, UcError> {
    UserRepo::get_by_email(&state.pool, email).await?
        .ok_or_else(|| UcError::unauthenticated("User not found"))
}

pub async fn require_any(state: &AppState, principal: Uuid, resource: Uuid, privs: &[Privilege]) -> Result<(), UcError> {
    if !state.authorizer.authorize_any(principal, resource, privs).await? {
        return Err(UcError::permission_denied(format!("Insufficient privileges on {}", resource)));
    }
    Ok(())
}

pub fn auth_sub<'a>(state: &AppState, claims: &'a UcClaims) -> Option<&'a str> {
    if state.auth_enabled { Some(claims.sub.as_str()) } else { None }
}

pub fn now_ms() -> i64 { chrono::Utc::now().timestamp_millis() }

/// Filter a list of resources to only those the principal can see.
/// When auth is disabled, all resources are visible.
/// Fix for issue #1105: schema/table/volume/function/model owners couldn't see
/// their own resources in list responses because listing didn't check RBAC.
pub async fn filter_visible(
    state: &AppState,
    principal: Option<Uuid>,
    resource_ids: Vec<(Uuid, impl Send)>,
    view_privs: &[Privilege],
) -> Result<Vec<Uuid>, UcError> {
    if !state.auth_enabled {
        return Ok(resource_ids.into_iter().map(|(id, _)| id).collect());
    }
    let principal = match principal {
        Some(p) => p,
        None => return Ok(vec![]),
    };
    let mut visible = Vec::new();
    for (id, _) in resource_ids {
        if state.authorizer.authorize_any(principal, id, view_privs).await? {
            visible.push(id);
        }
    }
    Ok(visible)
}
