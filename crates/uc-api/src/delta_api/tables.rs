use axum::{extract::{Path, State}, http::StatusCode};
use crate::state::AppState;
use uc_errors::UcError;
pub async fn create_staging_table(State(_s): State<AppState>, Path(_p): Path<(String,String)>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn create_table(State(_s): State<AppState>, Path(_p): Path<(String,String)>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn load_table(State(_s): State<AppState>, Path(_p): Path<(String,String,String)>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn table_exists(State(_s): State<AppState>, Path(_p): Path<(String,String,String)>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn update_table(State(_s): State<AppState>, Path(_p): Path<(String,String,String)>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn delete_table(State(_s): State<AppState>, Path(_p): Path<(String,String,String)>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn rename_table(State(_s): State<AppState>, Path(_p): Path<(String,String,String)>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
pub async fn report_metrics(State(_s): State<AppState>, Path(_p): Path<(String,String,String)>) -> Result<StatusCode, UcError> { Ok(StatusCode::NOT_IMPLEMENTED) }
