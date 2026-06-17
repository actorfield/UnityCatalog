use axum::{extract::State, Extension, Json};
use std::sync::Arc;
use uc_auth::UcClaims;
use uc_credentials::context::{CredentialContext, CredentialOperation};
use uc_db::repos::{CredentialRepo, ExternalLocationRepo, TableRepo, UserRepo, VolumeRepo};
use uc_errors::{ErrorCode, UcError};
use uc_openapi::catalog::{
    GenerateTemporaryModelVersionCredential, GenerateTemporaryPathCredential,
    GenerateTemporaryTableCredential, GenerateTemporaryVolumeCredential, TemporaryCredentials,
};
use uc_types::{Privilege, UriScheme};
use crate::state::AppState;

pub async fn table_credentials(
    State(state): State<AppState>,
    Extension(claims): Extension<Arc<UcClaims>>,
    Json(req): Json<GenerateTemporaryTableCredential>,
) -> Result<Json<TemporaryCredentials>, UcError> {
    let table = TableRepo::get_by_id(&state.pool, req.table_id).await?;

    if state.auth_enabled {
        let user = UserRepo::get_by_email(&state.pool, &claims.sub).await?
            .ok_or_else(|| UcError::unauthenticated("User not found"))?;
        let priv_needed = match req.operation {
            uc_openapi::catalog::CredentialOperation::Read => Privilege::Select,
            _ => Privilege::Modify,
        };
        if !state.authorizer.authorize_any(user.id, table.id, &[Privilege::Owner, priv_needed]).await? {
            return Err(UcError::permission_denied("Insufficient privileges on table"));
        }
    }

    let url = table.url.unwrap_or_default();
    let ctx = build_ctx(&url, CredentialOperation::ReadWrite, Some(req.table_id), &state).await?;
    Ok(Json(state.credential_vendor.vend(&ctx).await?))
}

pub async fn volume_credentials(
    State(state): State<AppState>,
    Extension(claims): Extension<Arc<UcClaims>>,
    Json(req): Json<GenerateTemporaryVolumeCredential>,
) -> Result<Json<TemporaryCredentials>, UcError> {
    let volume = VolumeRepo::get_by_id(&state.pool, req.volume_id).await?;

    if state.auth_enabled {
        let user = UserRepo::get_by_email(&state.pool, &claims.sub).await?
            .ok_or_else(|| UcError::unauthenticated("User not found"))?;
        let priv_needed = match req.operation {
            uc_openapi::catalog::CredentialOperation::ReadVolume => Privilege::ReadVolume,
            _ => Privilege::Owner,
        };
        if !state.authorizer.authorize_any(user.id, volume.id, &[Privilege::Owner, priv_needed]).await? {
            return Err(UcError::permission_denied("Insufficient privileges on volume"));
        }
    }

    let url = volume.storage_location.unwrap_or_default();
    let ctx = build_ctx(&url, CredentialOperation::ReadWrite, None, &state).await?;
    Ok(Json(state.credential_vendor.vend(&ctx).await?))
}

pub async fn model_version_credentials(
    State(state): State<AppState>,
    Extension(claims): Extension<Arc<UcClaims>>,
    Json(req): Json<GenerateTemporaryModelVersionCredential>,
) -> Result<Json<TemporaryCredentials>, UcError> {
    use uc_db::repos::{ModelRepo, SchemaRepo};
    let schema = SchemaRepo::get_by_full_name(&state.pool, &req.catalog_name, &req.schema_name).await?;
    let model = ModelRepo::get_model_by_schema_and_name(&state.pool, schema.id, &req.model_name).await?;
    let version = ModelRepo::get_version(&state.pool, model.id, req.version as i32).await?;
    let url = version.url.or(model.url).unwrap_or_default();
    let ctx = build_ctx(&url, CredentialOperation::ReadWrite, None, &state).await?;
    Ok(Json(state.credential_vendor.vend(&ctx).await?))
}

/// Fix for issue #1160: authorize against the matched external location's privilege
/// (READ_FILES / WRITE_FILES), NOT OWNER on metastore (which was too restrictive in Java).
pub async fn path_credentials(
    State(state): State<AppState>,
    Extension(claims): Extension<Arc<UcClaims>>,
    Json(req): Json<GenerateTemporaryPathCredential>,
) -> Result<Json<TemporaryCredentials>, UcError> {
    if state.auth_enabled {
        let user = UserRepo::get_by_email(&state.pool, &claims.sub).await?
            .ok_or_else(|| UcError::unauthenticated("User not found"))?;

        // Find the external location whose URL is a prefix of the requested path
        let ext_loc = ExternalLocationRepo::find_by_path_prefix(&state.pool, &req.url).await?
            .ok_or_else(|| UcError::new(
                ErrorCode::NotFound,
                format!("No external location found covering path '{}'", req.url),
            ))?;

        // Require READ_FILES or WRITE_FILES on the external location (not OWNER on metastore)
        let required = match req.operation {
            uc_openapi::catalog::CredentialOperation::Read => Privilege::ReadFiles,
            _ => Privilege::WriteFiles,
        };
        if !state.authorizer.authorize_any(user.id, ext_loc.id, &[Privilege::Owner, required]).await? {
            return Err(UcError::permission_denied(
                format!("Insufficient privileges on external location '{}'", ext_loc.name),
            ));
        }
    }

    let ctx = build_ctx(&req.url, CredentialOperation::ReadWrite, None, &state).await?;
    Ok(Json(state.credential_vendor.vend(&ctx).await?))
}

/// Build a CredentialContext, enriching it with the credential payload from the
/// matched external location so the vendor has role_arn / external_id to call STS.
async fn build_ctx(
    url: &str,
    operation: CredentialOperation,
    table_id: Option<uuid::Uuid>,
    state: &AppState,
) -> Result<CredentialContext, UcError> {
    let scheme = UriScheme::from_url(url);

    let (role_arn, external_id, credential_json) =
        if matches!(scheme, UriScheme::File | UriScheme::Null) {
            (None, None, None)
        } else {
            // Look up the external location covering this URL to get the credential
            match ExternalLocationRepo::find_by_path_prefix(&state.pool, url).await? {
                Some(ext_loc) => {
                    let cred = CredentialRepo::get_by_id(&state.pool, ext_loc.credential_id).await?;
                    // credential column is a JSON blob — parse role_arn from it
                    let role_arn: Option<String> = serde_json::from_str::<serde_json::Value>(&cred.credential)
                        .ok()
                        .and_then(|v| v.get("role_arn").and_then(|r| r.as_str()).map(String::from));
                    (role_arn, None, Some(cred.credential))
                }
                None => (None, None, None),
            }
        };

    Ok(CredentialContext {
        scheme,
        locations: vec![url.to_string()],
        operation,
        table_id,
        credential_json,
        role_arn,
        external_id,
    })
}
