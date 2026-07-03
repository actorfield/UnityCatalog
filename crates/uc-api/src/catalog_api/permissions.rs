use crate::{catalog_api::helpers::get_user, state::AppState};
use axum::{
    extract::{Path, Query, State},
    Extension, Json,
};
use std::sync::Arc;
use uc_auth::UcClaims;
use uc_db::repos::user;
use uc_errors::UcError;
use uc_openapi::catalog::{PermissionsList, PrivilegeAssignment, UpdatePermissions};
use uc_types::Privilege;
use uuid::Uuid;

/// Convert a list of (principal_uuid, privileges) pairs into PrivilegeAssignment responses
/// by resolving UUIDs back to email strings. Falls back to UUID string if user not found.
async fn grants_to_assignments(
    pool: &uc_db::AnyPool,
    grants: Vec<(uuid::Uuid, Vec<Privilege>)>,
) -> Result<Vec<PrivilegeAssignment>, UcError> {
    let mut result = Vec::new();
    for (principal_id, privs) in grants {
        let email = match user::get_by_id(pool, principal_id).await {
            Ok(u) => u.email.unwrap_or(u.name),
            Err(_) => principal_id.to_string(),
        };
        result.push(PrivilegeAssignment {
            principal: email,
            privileges: privs
                .iter()
                .map(|p| p.as_casbin_str().to_string())
                .collect(),
        });
    }
    Ok(result)
}

#[derive(serde::Deserialize)]
pub struct GetParams {
    pub principal: Option<String>,
}

/// Resolve a securable full_name to its UUID by looking up the resource by name.
async fn resolve_resource_id(
    state: &AppState,
    securable_type: &str,
    full_name: &str,
) -> Result<Uuid, UcError> {
    use crate::catalog_api::helpers::{split2, split3};
    use uc_db::repos::{catalog, external_location, function, model, schema, table, volume};

    match securable_type.to_uppercase().as_str() {
        "METASTORE" => Ok(state.metastore_id),
        "CATALOG" => Ok(catalog::get_by_name(&state.pool, full_name).await?.id),
        "EXTERNAL_LOCATION" => Ok(external_location::get_by_name(&state.pool, full_name)
            .await?
            .id),
        "SCHEMA" => {
            let (cat, sch) = split2(full_name)?;
            Ok(schema::get_by_full_name(&state.pool, cat, sch).await?.id)
        }
        "TABLE" => {
            let (cat, sch, tbl) = split3(full_name)?;
            let schema = schema::get_by_full_name(&state.pool, cat, sch).await?;
            Ok(table::get_by_schema_and_name(&state.pool, schema.id, tbl)
                .await?
                .id)
        }
        "VOLUME" => {
            let (cat, sch, vol) = split3(full_name)?;
            let schema = schema::get_by_full_name(&state.pool, cat, sch).await?;
            Ok(volume::get_by_schema_and_name(&state.pool, schema.id, vol)
                .await?
                .id)
        }
        "FUNCTION" => {
            let (cat, sch, func) = split3(full_name)?;
            let schema = schema::get_by_full_name(&state.pool, cat, sch).await?;
            Ok(
                function::get_by_schema_and_name(&state.pool, schema.id, func)
                    .await?
                    .id,
            )
        }
        "REGISTERED_MODEL" | "MODEL" => {
            let (cat, sch, mdl) = split3(full_name)?;
            let schema = schema::get_by_full_name(&state.pool, cat, sch).await?;
            Ok(
                model::get_model_by_schema_and_name(&state.pool, schema.id, mdl)
                    .await?
                    .id,
            )
        }
        _ => Err(UcError::invalid_argument(format!(
            "Unknown securable type: {}",
            securable_type
        ))),
    }
}

pub async fn get(
    State(state): State<AppState>,
    Extension(_claims): Extension<Arc<UcClaims>>,
    Path((securable_type, full_name)): Path<(String, String)>,
    Query(params): Query<GetParams>,
) -> Result<Json<PermissionsList>, UcError> {
    let resource_id = resolve_resource_id(&state, &securable_type, &full_name).await?;

    let privilege_assignments: Vec<PrivilegeAssignment> =
        if let Some(ref principal_email) = params.principal {
            match user::get_by_email(&state.pool, principal_email).await? {
                Some(user) => {
                    let privs = state
                        .authorizer
                        .list_privileges(user.id, resource_id)
                        .await?;
                    if privs.is_empty() {
                        vec![]
                    } else {
                        vec![PrivilegeAssignment {
                            principal: principal_email.clone(),
                            privileges: privs
                                .iter()
                                .map(|p| p.as_casbin_str().to_string())
                                .collect(),
                        }]
                    }
                }
                None => vec![],
            }
        } else {
            let grants = state
                .authorizer
                .list_grants_on_resource(resource_id)
                .await?;
            grants_to_assignments(&state.pool, grants).await?
        };

    Ok(Json(PermissionsList {
        securable_type: Some(securable_type.to_uppercase()),
        full_name: Some(full_name),
        privilege_assignments,
    }))
}

pub async fn update(
    State(state): State<AppState>,
    Extension(claims): Extension<Arc<UcClaims>>,
    Path((securable_type, full_name)): Path<(String, String)>,
    Json(req): Json<UpdatePermissions>,
) -> Result<Json<PermissionsList>, UcError> {
    let resource_id = resolve_resource_id(&state, &securable_type, &full_name).await?;

    if state.auth_enabled {
        let caller = get_user(&state, &claims.sub).await?;
        // Only an OWNER may manage a securable's grants (Databricks delegation:
        // "only the owner can grant"). OWNER cascades through the object
        // hierarchy, so a catalog/schema owner can grant on children. Metastore
        // admins (OWNER on the metastore) may manage grants on ANY securable --
        // an explicit override, since top-level securables like external
        // locations / storage credentials are not linked to the metastore in g2.
        let is_owner = state
            .authorizer
            .authorize(caller.id, resource_id, Privilege::Owner)
            .await?;
        let is_metastore_admin = state
            .authorizer
            .authorize(caller.id, state.metastore_id, Privilege::Owner)
            .await?;
        if !is_owner && !is_metastore_admin {
            return Err(UcError::permission_denied(
                "OWNER privilege required to manage permissions",
            ));
        }
    }

    for change in &req.changes {
        // Resolve principal to a user UUID. Human principals are addressed by
        // email; OIDC principals (e.g. k8s service accounts granted via
        // --operator-external-id) have `email: None` and are keyed by their
        // external_id sub, so email lookup can never match them (see
        // find_or_create_by_external_id's doc). Fall back to external_id, and
        // auto-provision it when absent so grants can target a principal that
        // hasn't authenticated yet (mirrors get_user's email-then-external_id
        // resolution).
        let user = match user::get_by_email(&state.pool, &change.principal).await? {
            Some(u) => u,
            None => match user::get_by_external_id(&state.pool, &change.principal).await? {
                Some(u) => u,
                None => user::find_or_create_by_external_id(&state.pool, &change.principal).await?,
            },
        };

        for priv_str in &change.add {
            let p = Privilege::from_casbin_str(priv_str).ok_or_else(|| {
                UcError::invalid_argument(format!("Unknown privilege: '{}'", priv_str))
            })?;
            state.authorizer.grant(user.id, resource_id, p).await?;
        }
        for priv_str in &change.remove {
            let p = Privilege::from_casbin_str(priv_str).ok_or_else(|| {
                UcError::invalid_argument(format!("Unknown privilege: '{}'", priv_str))
            })?;
            state.authorizer.revoke(user.id, resource_id, p).await?;
        }
    }

    // Return the updated state
    let grants = state
        .authorizer
        .list_grants_on_resource(resource_id)
        .await?;
    let privilege_assignments = grants_to_assignments(&state.pool, grants).await?;

    Ok(Json(PermissionsList {
        securable_type: Some(securable_type.to_uppercase()),
        full_name: Some(full_name),
        privilege_assignments,
    }))
}
