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

/// Resolve a caller's `sub` claim to a `uc_users` row. Tries `email` first
/// (covers UC-issued password/admin tokens), then falls back to `external_id`
/// (covers OIDC-mapped synthetic principals, which have no email).
pub async fn get_user(state: &AppState, sub: &str) -> Result<uc_db::models::user::UserRow, UcError> {
    if let Some(row) = UserRepo::get_by_email(&state.pool, sub).await? {
        return Ok(row);
    }
    UserRepo::get_by_external_id(&state.pool, sub).await?
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

/// Validate a SQL object name matches Unity Catalog naming rules.
/// Mirrors Java's ValidationUtils.validateSqlObjectName():
///   - non-empty
///   - max 255 characters
///   - no dots, slashes, whitespace, or control characters
pub fn validate_sql_name(name: &str) -> Result<(), UcError> {
    if name.is_empty() {
        return Err(UcError::invalid_argument("Name must not be empty"));
    }
    if name.len() > 255 {
        return Err(UcError::invalid_argument(
            format!("Name '{}' exceeds maximum length of 255 characters", name)
        ));
    }
    for ch in name.chars() {
        if ch == '.' || ch == '/' || ch.is_whitespace() || ch.is_control() {
            return Err(UcError::invalid_argument(
                format!("Name '{}' contains invalid character '{}'", name, ch)
            ));
        }
    }
    Ok(())
}

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
