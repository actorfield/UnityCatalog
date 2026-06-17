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
