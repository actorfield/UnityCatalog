use axum::{extract::{Path, Query, State}, Json};
use uc_credentials::context::{CredentialContext, CredentialOperation};
use uc_db::repos::{StagingTableRepo, TableRepo};
use uc_errors::UcError;
use uc_openapi::delta::{DeltaCredentialOperation, DeltaCredentialsResponse, DeltaStorageCredential};
use uc_types::UriScheme;
use uuid::Uuid;
use crate::state::AppState;

pub async fn get_table_credentials(
    State(state): State<AppState>,
    Path((catalog, schema, table)): Path<(String, String, String)>,
) -> Result<Json<DeltaCredentialsResponse>, UcError> {
    let schema_row = uc_db::repos::SchemaRepo::get_by_full_name(&state.pool, &catalog, &schema).await?;
    let row = TableRepo::get_by_schema_and_name(&state.pool, schema_row.id, &table).await?;
    let url = row.url.clone().unwrap_or_default();
    let scheme = UriScheme::from_url(&url);
    let ctx = CredentialContext { scheme, locations: vec![url.clone()], operation: CredentialOperation::ReadWrite, table_id: Some(row.id), credential_json: None, role_arn: None, external_id: None };
    let creds = state.credential_vendor.vend(&ctx).await?;
    let storage_creds = if let Some(aws) = creds.aws_temp_credentials {
        vec![DeltaStorageCredential {
            prefix: url, operation: DeltaCredentialOperation::ReadWrite,
            config: Some(serde_json::json!({
                "awsAccessKey": aws.access_key_id,
                "awsSecretKey": aws.secret_access_key,
                "awsSessionToken": aws.session_token,
            })),
            expiration_time_ms: None,
        }]
    } else { vec![] };
    Ok(Json(DeltaCredentialsResponse { storage_credentials: storage_creds }))
}

pub async fn get_staging_credentials(
    State(state): State<AppState>,
    Path(table_id): Path<String>,
) -> Result<Json<DeltaCredentialsResponse>, UcError> {
    let uid: Uuid = table_id.parse().map_err(|_| UcError::invalid_argument("invalid table_id"))?;
    let staging = StagingTableRepo::get_by_id(&state.pool, uid).await?;
    let scheme = UriScheme::from_url(&staging.staging_location);
    let ctx = CredentialContext { scheme, locations: vec![staging.staging_location.clone()], operation: CredentialOperation::ReadWrite, table_id: Some(uid), credential_json: None, role_arn: None, external_id: None };
    let creds = state.credential_vendor.vend(&ctx).await?;
    let storage_creds = if let Some(aws) = creds.aws_temp_credentials {
        vec![DeltaStorageCredential {
            prefix: staging.staging_location.clone(), operation: DeltaCredentialOperation::ReadWrite,
            config: Some(serde_json::json!({
                "awsAccessKey": aws.access_key_id,
                "awsSecretKey": aws.secret_access_key,
                "awsSessionToken": aws.session_token,
            })),
            expiration_time_ms: None,
        }]
    } else { vec![] };
    Ok(Json(DeltaCredentialsResponse { storage_credentials: storage_creds }))
}

#[derive(serde::Deserialize)]
pub struct PathCredParams { pub path: Option<String>, pub operation: Option<String> }

pub async fn get_path_credentials(
    State(state): State<AppState>,
    Query(params): Query<PathCredParams>,
) -> Result<Json<DeltaCredentialsResponse>, UcError> {
    let url = params.path.unwrap_or_default();
    let scheme = UriScheme::from_url(&url);
    let ctx = CredentialContext { scheme, locations: vec![url.clone()], operation: CredentialOperation::ReadWrite, table_id: None, credential_json: None, role_arn: None, external_id: None };
    let _creds = state.credential_vendor.vend(&ctx).await?;
    Ok(Json(DeltaCredentialsResponse { storage_credentials: vec![] }))
}
