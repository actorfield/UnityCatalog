use axum::{extract::{Path, Query, State}, http::StatusCode, Extension, Json};
use std::sync::Arc;
use uc_auth::UcClaims;
use uc_db::{models::credential::CredentialRow, repos::CredentialRepo};
use uc_errors::UcError;
use uc_openapi::catalog::{AwsIamRoleRequest, CreateCredentialRequest, CredentialInfo, CredentialPurpose, ListCredentialsResponse, UpdateCredentialRequest};
use uc_types::Privilege;
use uuid::Uuid;
use crate::{catalog_api::helpers::*, state::AppState};

#[derive(serde::Deserialize)]
pub struct ListParams { pub max_results: Option<i64>, pub page_token: Option<String> }

pub async fn create(State(state): State<AppState>, Extension(claims): Extension<Arc<UcClaims>>, Json(req): Json<CreateCredentialRequest>) -> Result<Json<CredentialInfo>, UcError> {
    if state.auth_enabled {
        let user = get_user(&state, &claims.sub).await?;
        require_any(&state, user.id, state.metastore_id, &[Privilege::Owner, Privilege::CreateStorageCredential]).await?;
    }
    let id = Uuid::new_v4(); let now = now_ms();
    let credential_json = serde_json::to_string(&req.aws_iam_role).unwrap_or_default();
    let row = CredentialRow { id, name: req.name.clone(),
        credential_type: format!("{:?}", req.purpose).to_uppercase(),
        credential: credential_json, purpose: format!("{:?}", req.purpose).to_uppercase(),
        comment: req.comment.clone(), owner: None, created_at: now,
        created_by: auth_sub(&state, &claims).map(String::from), updated_at: None, updated_by: None };
    let created = CredentialRepo::create(&state.pool, &row).await?;
    if state.auth_enabled {
        if let Ok(user) = get_user(&state, &claims.sub).await {
            state.authorizer.grant(user.id, id, Privilege::Owner).await?;
        }
    }
    Ok(Json(to_cred_info(created)))
}

pub async fn list(State(state): State<AppState>, Extension(claims): Extension<Arc<UcClaims>>, Query(params): Query<ListParams>) -> Result<Json<ListCredentialsResponse>, UcError> {
    if state.auth_enabled {
        let user = get_user(&state, &claims.sub).await?;
        require_any(&state, user.id, state.metastore_id, &[Privilege::Owner, Privilege::CreateStorageCredential]).await?;
    }
    let max = params.max_results.unwrap_or(50).min(1000);
    let (rows, next_token) = CredentialRepo::list(&state.pool, params.page_token.as_deref(), max).await?;
    let credentials = rows.into_iter().map(to_cred_info).collect();
    Ok(Json(ListCredentialsResponse { credentials, next_page_token: next_token }))
}

pub async fn get(State(state): State<AppState>, Extension(claims): Extension<Arc<UcClaims>>, Path(name): Path<String>) -> Result<Json<CredentialInfo>, UcError> {
    let row = CredentialRepo::get_by_name(&state.pool, &name).await?;
    if state.auth_enabled {
        let user = get_user(&state, &claims.sub).await?;
        require_any(&state, user.id, state.metastore_id, &[Privilege::Owner, Privilege::CreateStorageCredential]).await?;
    }
    Ok(Json(to_cred_info(row)))
}

pub async fn update(State(state): State<AppState>, Extension(claims): Extension<Arc<UcClaims>>, Path(name): Path<String>, Json(req): Json<UpdateCredentialRequest>) -> Result<Json<CredentialInfo>, UcError> {
    let existing = CredentialRepo::get_by_name(&state.pool, &name).await?;
    if state.auth_enabled {
        let user = get_user(&state, &claims.sub).await?;
        require_any(&state, user.id, existing.id, &[uc_types::Privilege::Owner]).await?;
    }
    let effective_name = req.new_name.as_deref().unwrap_or(&name);
    let now = now_ms();
    let new_credential_json = req.aws_iam_role.as_ref().map(|r| serde_json::to_string(r).unwrap_or_default());
    sqlx::query(
        "UPDATE uc_credentials SET name=COALESCE($1,name), comment=COALESCE($2,comment), owner=COALESCE($3,owner), credential=COALESCE($4,credential), updated_at=$5, updated_by=$6 WHERE id=$7"
    )
    .bind(req.new_name.as_deref())
    .bind(req.comment.as_deref())
    .bind(req.owner.as_deref())
    .bind(new_credential_json.as_deref())
    .bind(now)
    .bind(auth_sub(&state, &claims))
    .bind(existing.id)
    .execute(state.pool.as_ref()).await.map_err(crate::db_err)?;
    let updated = CredentialRepo::get_by_name(&state.pool, effective_name).await?;
    Ok(Json(to_cred_info(updated)))
}

pub async fn delete(State(state): State<AppState>, Extension(claims): Extension<Arc<UcClaims>>, Path(name): Path<String>) -> Result<StatusCode, UcError> {
    if state.auth_enabled {
        let existing = CredentialRepo::get_by_name(&state.pool, &name).await?;
        let user = get_user(&state, &claims.sub).await?;
        require_any(&state, user.id, existing.id, &[Privilege::Owner]).await?;
    }
    CredentialRepo::delete(&state.pool, &name).await?;
    Ok(StatusCode::OK)
}

fn to_cred_info(r: CredentialRow) -> CredentialInfo {
    let aws: Option<AwsIamRoleRequest> = serde_json::from_str(&r.credential).ok();
    let full_name = Some(r.name.clone());
    CredentialInfo { id: r.id, name: r.name, purpose: CredentialPurpose::AwsIamRole,
        full_name, comment: r.comment, owner: r.owner, created_at: Some(r.created_at), created_by: r.created_by,
        updated_at: r.updated_at, updated_by: r.updated_by, aws_iam_role: aws }
}
