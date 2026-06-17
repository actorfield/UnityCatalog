use axum::{extract::State, Extension, Json};
use std::sync::Arc;
use uc_auth::UcClaims;
use uc_db::{
    managed_storage,
    models::staging::StagingTableRow,
    repos::{SchemaRepo, StagingTableRepo},
};
use uc_errors::UcError;
use uc_openapi::catalog::{CreateStagingTable, StagingTableInfo};
use uc_types::Privilege;
use uuid::Uuid;
use crate::{catalog_api::helpers::*, state::AppState};

pub async fn create(
    State(state): State<AppState>,
    Extension(claims): Extension<Arc<UcClaims>>,
    Json(req): Json<CreateStagingTable>,
) -> Result<Json<StagingTableInfo>, UcError> {
    let schema = SchemaRepo::get_by_full_name(&state.pool, &req.catalog_name, &req.schema_name).await?;

    if state.auth_enabled {
        let user = get_user(&state, &claims.sub).await?;
        require_any(&state, user.id, schema.id, &[Privilege::Owner, Privilege::CreateTable]).await?;
    }

    let id = Uuid::new_v4();
    let now = now_ms();

    // Derive staging location from storage_root hierarchy (schema → catalog → error)
    let staging_location = match managed_storage::resolve_storage_root(
        &state.pool, &req.catalog_name, &req.schema_name,
    ).await {
        Ok(root) => managed_storage::staging_table_location(&root, schema.id, id),
        // Fall back to local temp path if no storage root configured (dev mode)
        Err(_) => format!("file:///tmp/uc/staging/{}/{}/{}", req.catalog_name, req.schema_name, id),
    };

    let row = StagingTableRow {
        id, schema_id: schema.id, name: req.name.clone(),
        staging_location: staging_location.clone(),
        created_at: now, created_by: auth_sub(&state, &claims).map(String::from),
        accessed_at: now, stage_committed: false, stage_committed_at: None,
        purge_state: 0, num_cleanup_retries: 0, last_cleanup_at: None,
    };
    StagingTableRepo::create(&state.pool, &row).await?;

    // Grant owner on the staging table for auth validation during commit
    if state.auth_enabled {
        if let Some(user) = uc_db::repos::UserRepo::get_by_email(&state.pool, &claims.sub).await? {
            state.authorizer.grant(user.id, id, Privilege::Owner).await?;
        }
    }

    Ok(Json(StagingTableInfo {
        table_id: id,
        staging_location,
        schema_name: req.schema_name,
        catalog_name: req.catalog_name,
    }))
}
