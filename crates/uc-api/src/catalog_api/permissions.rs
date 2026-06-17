use axum::{extract::{Path, Query, State}, Extension, Json};
use std::sync::Arc;
use uc_auth::UcClaims;
use uc_db::repos::UserRepo;
use uc_errors::UcError;
use uc_openapi::catalog::{PermissionsList, PrivilegeAssignment, PrivilegeAssignmentChange, UpdatePermissions};
use uc_types::{Privilege, SecurableType};
use uuid::Uuid;
use crate::{catalog_api::helpers::get_user, state::AppState};

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
    use uc_db::repos::{CatalogRepo, SchemaRepo, TableRepo, VolumeRepo, FunctionRepo, ModelRepo};
    use crate::catalog_api::helpers::{split2, split3};

    match securable_type.to_uppercase().as_str() {
        "METASTORE" => Ok(state.metastore_id),
        "CATALOG" => Ok(CatalogRepo::get_by_name(&state.pool, full_name).await?.id),
        "SCHEMA" => {
            let (cat, sch) = split2(full_name)?;
            Ok(SchemaRepo::get_by_full_name(&state.pool, cat, sch).await?.id)
        }
        "TABLE" => {
            let (cat, sch, tbl) = split3(full_name)?;
            let schema = SchemaRepo::get_by_full_name(&state.pool, cat, sch).await?;
            Ok(TableRepo::get_by_schema_and_name(&state.pool, schema.id, tbl).await?.id)
        }
        "VOLUME" => {
            let (cat, sch, vol) = split3(full_name)?;
            let schema = SchemaRepo::get_by_full_name(&state.pool, cat, sch).await?;
            Ok(VolumeRepo::get_by_schema_and_name(&state.pool, schema.id, vol).await?.id)
        }
        "FUNCTION" => {
            let (cat, sch, func) = split3(full_name)?;
            let schema = SchemaRepo::get_by_full_name(&state.pool, cat, sch).await?;
            Ok(FunctionRepo::get_by_schema_and_name(&state.pool, schema.id, func).await?.id)
        }
        "REGISTERED_MODEL" | "MODEL" => {
            let (cat, sch, mdl) = split3(full_name)?;
            let schema = SchemaRepo::get_by_full_name(&state.pool, cat, sch).await?;
            Ok(ModelRepo::get_model_by_schema_and_name(&state.pool, schema.id, mdl).await?.id)
        }
        _ => Err(UcError::invalid_argument(format!("Unknown securable type: {}", securable_type))),
    }
}

pub async fn get(
    State(state): State<AppState>,
    Extension(claims): Extension<Arc<UcClaims>>,
    Path((securable_type, full_name)): Path<(String, String)>,
    Query(params): Query<GetParams>,
) -> Result<Json<PermissionsList>, UcError> {
    let resource_id = resolve_resource_id(&state, &securable_type, &full_name).await?;

    let grants = if let Some(ref principal_email) = params.principal {
        // Filter to a specific principal
        match UserRepo::get_by_email(&state.pool, principal_email).await? {
            Some(user) => {
                let privs = state.authorizer.list_privileges(user.id, resource_id).await?;
                if privs.is_empty() { vec![] } else {
                    vec![(user.id, privs, principal_email.clone())]
                }
            }
            None => vec![],
        }
    } else {
        // All principals on this resource
        let grants = state.authorizer.list_grants_on_resource(resource_id).await?;
        // Resolve UUIDs back to emails for the response
        let mut result = Vec::new();
        for (principal_id, privs) in grants {
            let email = match UserRepo::get_by_id(&state.pool, principal_id).await {
                Ok(u) => u.email.unwrap_or(u.name),
                Err(_) => principal_id.to_string(),
            };
            result.push((principal_id, privs, email));
        }
        result
    };

    let privilege_assignments = grants.into_iter().map(|(_, privs, email)| PrivilegeAssignment {
        principal: email,
        privileges: privs.iter().map(|p| p.as_casbin_str().to_string()).collect(),
    }).collect();

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
        if !state.authorizer.authorize_any(caller.id, resource_id, &[Privilege::Owner]).await? {
            return Err(UcError::permission_denied("OWNER privilege required to manage permissions"));
        }
    }

    for change in &req.changes {
        // Resolve principal email to UUID
        let user = UserRepo::get_by_email(&state.pool, &change.principal).await?
            .ok_or_else(|| UcError::not_found("User", &change.principal))?;

        for priv_str in &change.add {
            if let Some(p) = Privilege::from_casbin_str(priv_str) {
                state.authorizer.grant(user.id, resource_id, p).await?;
            }
        }
        for priv_str in &change.remove {
            if let Some(p) = Privilege::from_casbin_str(priv_str) {
                state.authorizer.revoke(user.id, resource_id, p).await?;
            }
        }
    }

    // Return the updated state
    let grants = state.authorizer.list_grants_on_resource(resource_id).await?;
    let privilege_assignments = {
        let mut result = Vec::new();
        for (principal_id, privs) in grants {
            let email = match UserRepo::get_by_id(&state.pool, principal_id).await {
                Ok(u) => u.email.unwrap_or(u.name),
                Err(_) => principal_id.to_string(),
            };
            result.push(PrivilegeAssignment {
                principal: email,
                privileges: privs.iter().map(|p| p.as_casbin_str().to_string()).collect(),
            });
        }
        result
    };

    Ok(Json(PermissionsList {
        securable_type: Some(securable_type.to_uppercase()),
        full_name: Some(full_name),
        privilege_assignments,
    }))
}
