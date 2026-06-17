use axum::{extract::State, Json};
use uc_db::{models::staging::StagingTableRow, repos::{SchemaRepo, StagingTableRepo}};
use uc_errors::UcError;
use uc_openapi::catalog::{CreateStagingTable, StagingTableInfo};
use uuid::Uuid;
use crate::{catalog_api::helpers::now_ms, state::AppState};

pub async fn create(State(state): State<AppState>, Json(req): Json<CreateStagingTable>) -> Result<Json<StagingTableInfo>, UcError> {
    let schema = SchemaRepo::get_by_full_name(&state.pool, &req.catalog_name, &req.schema_name).await?;
    let id = Uuid::new_v4(); let now = now_ms();
    let staging_location = format!("file:///tmp/uc/staging/{}", id);
    let row = StagingTableRow { id, schema_id: schema.id, name: req.name.clone(),
        staging_location: staging_location.clone(), created_at: now, created_by: None,
        accessed_at: now, stage_committed: false, stage_committed_at: None,
        purge_state: 0, num_cleanup_retries: 0, last_cleanup_at: None };
    StagingTableRepo::create(&state.pool, &row).await?;
    Ok(Json(StagingTableInfo { table_id: id, staging_location, schema_name: req.schema_name, catalog_name: req.catalog_name }))
}
