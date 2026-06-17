use axum::{extract::State, Json};
use uc_db::repos::{TableRepo, VolumeRepo};
use uc_errors::UcError;
use uc_openapi::catalog::{GenerateTemporaryModelVersionCredential, GenerateTemporaryPathCredential, GenerateTemporaryTableCredential, GenerateTemporaryVolumeCredential, TemporaryCredentials};
use uc_credentials::context::{CredentialContext, CredentialOperation};
use uc_types::UriScheme;
use crate::state::AppState;

pub async fn table_credentials(State(state): State<AppState>, Json(req): Json<GenerateTemporaryTableCredential>) -> Result<Json<TemporaryCredentials>, UcError> {
    let table = TableRepo::get_by_id(&state.pool, req.table_id).await?;
    let url = table.url.unwrap_or_default();
    let scheme = UriScheme::from_url(&url);
    let ctx = CredentialContext { scheme, locations: vec![url], operation: CredentialOperation::ReadWrite, table_id: Some(req.table_id), credential_json: None, role_arn: None, external_id: None };
    let creds = state.credential_vendor.vend(&ctx).await?;
    Ok(Json(creds))
}

pub async fn volume_credentials(State(state): State<AppState>, Json(req): Json<GenerateTemporaryVolumeCredential>) -> Result<Json<TemporaryCredentials>, UcError> {
    let volume = VolumeRepo::get_by_id(&state.pool, req.volume_id).await?;
    let url = volume.storage_location.unwrap_or_default();
    let scheme = UriScheme::from_url(&url);
    let ctx = CredentialContext { scheme, locations: vec![url], operation: CredentialOperation::ReadWrite, table_id: None, credential_json: None, role_arn: None, external_id: None };
    let creds = state.credential_vendor.vend(&ctx).await?;
    Ok(Json(creds))
}

pub async fn model_version_credentials(State(state): State<AppState>, Json(_req): Json<GenerateTemporaryModelVersionCredential>) -> Result<Json<TemporaryCredentials>, UcError> {
    Ok(Json(TemporaryCredentials::default()))
}

pub async fn path_credentials(State(state): State<AppState>, Json(req): Json<GenerateTemporaryPathCredential>) -> Result<Json<TemporaryCredentials>, UcError> {
    let scheme = UriScheme::from_url(&req.url);
    let ctx = CredentialContext { scheme, locations: vec![req.url], operation: CredentialOperation::ReadWrite, table_id: None, credential_json: None, role_arn: None, external_id: None };
    let creds = state.credential_vendor.vend(&ctx).await?;
    Ok(Json(creds))
}
