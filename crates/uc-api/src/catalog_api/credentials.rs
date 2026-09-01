use crate::{catalog_api::helpers::*, state::AppState};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Extension, Json,
};
use std::sync::Arc;
use uc_auth::UcClaims;
use uc_db::{models::credential::CredentialRow, repos::credential};
use uc_errors::UcError;
use uc_openapi::catalog::{
    AwsIamRoleRequest, CreateCredentialRequest, CredentialInfo, CredentialPurpose,
    ListCredentialsResponse, UpdateCredentialRequest,
};
use uc_types::Privilege;
use uuid::Uuid;

#[derive(serde::Deserialize)]
pub struct ListParams {
    pub max_results: Option<i64>,
    pub page_token: Option<String>,
}

pub async fn create(
    State(state): State<AppState>,
    Extension(claims): Extension<Arc<UcClaims>>,
    Json(req): Json<CreateCredentialRequest>,
) -> Result<Json<CredentialInfo>, UcError> {
    if state.auth_enabled {
        let user = get_user(&state, &claims.sub).await?;
        require(
            &state,
            user.id,
            state.metastore_id,
            Privilege::CreateStorageCredential,
        )
        .await?;
    }
    let id = Uuid::now_v7();
    let now = now_ms();
    let credential_json = serde_json::to_string(&req.aws_iam_role).unwrap_or_default();
    let row = CredentialRow {
        id,
        name: req.name.clone(),
        credential_type: format!("{:?}", req.purpose).to_uppercase(),
        credential: credential_json,
        purpose: format!("{:?}", req.purpose).to_uppercase(),
        comment: req.comment.clone(),
        owner: None,
        created_at: now,
        created_by: auth_sub(&state, &claims).map(String::from),
        updated_at: None,
        updated_by: None,
    };
    let created = credential::create(&state.pool, &row).await?;
    if state.auth_enabled {
        if let Ok(user) = get_user(&state, &claims.sub).await {
            state
                .authorizer
                .grant(user.id, id, Privilege::Owner)
                .await?;
        }
    }
    Ok(Json(to_cred_info(created)))
}

pub async fn list(
    State(state): State<AppState>,
    Extension(claims): Extension<Arc<UcClaims>>,
    Query(params): Query<ListParams>,
) -> Result<Json<ListCredentialsResponse>, UcError> {
    if state.auth_enabled {
        let user = get_user(&state, &claims.sub).await?;
        require(
            &state,
            user.id,
            state.metastore_id,
            Privilege::CreateStorageCredential,
        )
        .await?;
    }
    // A non-positive max_results means "unspecified", not "an empty page". It
    // used to reach the repo layer and underflow there.
    let max = params.max_results.filter(|n| *n > 0).unwrap_or(50).min(1000);
    let (rows, next_token) =
        credential::list(&state.pool, params.page_token.as_deref(), max).await?;
    let credentials = rows.into_iter().map(to_cred_info).collect();
    Ok(Json(ListCredentialsResponse {
        credentials,
        next_page_token: next_token,
    }))
}

pub async fn get(
    State(state): State<AppState>,
    Extension(claims): Extension<Arc<UcClaims>>,
    Path(name): Path<String>,
) -> Result<Json<CredentialInfo>, UcError> {
    let row = credential::get_by_name(&state.pool, &name).await?;
    if state.auth_enabled {
        let user = get_user(&state, &claims.sub).await?;
        require(
            &state,
            user.id,
            state.metastore_id,
            Privilege::CreateStorageCredential,
        )
        .await?;
    }
    Ok(Json(to_cred_info(row)))
}

pub async fn update(
    State(state): State<AppState>,
    Extension(claims): Extension<Arc<UcClaims>>,
    Path(name): Path<String>,
    Json(req): Json<UpdateCredentialRequest>,
) -> Result<Json<CredentialInfo>, UcError> {
    let existing = credential::get_by_name(&state.pool, &name).await?;
    if state.auth_enabled {
        let user = get_user(&state, &claims.sub).await?;
        require(&state, user.id, existing.id, Privilege::Owner).await?;
    }
    let effective_name = req.new_name.as_deref().unwrap_or(&name);
    let now = now_ms();
    let new_credential_json = req
        .aws_iam_role
        .as_ref()
        .map(|r| serde_json::to_string(r).unwrap_or_default());
    credential::update(
        &state.pool,
        existing.id,
        req.new_name.as_deref(),
        req.comment.as_deref(),
        req.owner.as_deref(),
        new_credential_json.as_deref(),
        now,
        auth_sub(&state, &claims),
    )
    .await?;
    let updated = credential::get_by_name(&state.pool, effective_name).await?;
    Ok(Json(to_cred_info(updated)))
}

pub async fn delete(
    State(state): State<AppState>,
    Extension(claims): Extension<Arc<UcClaims>>,
    Path(name): Path<String>,
) -> Result<StatusCode, UcError> {
    if state.auth_enabled {
        let existing = credential::get_by_name(&state.pool, &name).await?;
        let user = get_user(&state, &claims.sub).await?;
        require(&state, user.id, existing.id, Privilege::Owner).await?;
    }
    credential::delete(&state.pool, &name).await?;
    Ok(StatusCode::OK)
}

fn to_cred_info(r: CredentialRow) -> CredentialInfo {
    let aws: Option<AwsIamRoleRequest> = serde_json::from_str(&r.credential).ok();
    let full_name = Some(r.name.clone());
    CredentialInfo {
        id: r.id,
        name: r.name,
        purpose: CredentialPurpose::AwsIamRole,
        full_name,
        comment: r.comment,
        owner: r.owner,
        created_at: Some(r.created_at),
        created_by: r.created_by,
        updated_at: r.updated_at,
        updated_by: r.updated_by,
        aws_iam_role: aws,
    }
}
