use crate::state::AppState;
use axum::{extract::State, Json};
use uc_db::repos::metastore;
use uc_errors::UcError;
use uc_openapi::catalog::MetastoreSummary;

pub async fn get_summary(State(state): State<AppState>) -> Result<Json<MetastoreSummary>, UcError> {
    let row = metastore::get(&state.pool).await?;
    Ok(Json(MetastoreSummary {
        metastore_id: row.id,
        name: row.name,
        owner: None,
        created_at: None,
        created_by: None,
        updated_at: None,
        updated_by: None,
    }))
}
