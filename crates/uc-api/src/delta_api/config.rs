use crate::state::AppState;
use axum::{extract::State, Json};
use uc_errors::UcError;
use uc_openapi::delta::DeltaCatalogConfig;

pub async fn get_config(
    State(_state): State<AppState>,
) -> Result<Json<DeltaCatalogConfig>, UcError> {
    Ok(Json(DeltaCatalogConfig {
        endpoints: vec![],
        protocol_version: "1".to_string(),
    }))
}
