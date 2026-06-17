use axum::{extract::{Query, State}, Json};
use uc_db::repos::{DeltaCommitRepo, SchemaRepo, TableRepo};
use uc_errors::UcError;
use uc_openapi::catalog::{CommitInfo, GetCommitsResponse};
use crate::state::AppState;

#[derive(serde::Deserialize)]
pub struct GetParams { pub table_full_name: String, pub starting_version: Option<i64>, pub ending_version: Option<i64> }

pub async fn get_commits(State(state): State<AppState>, Query(params): Query<GetParams>) -> Result<Json<GetCommitsResponse>, UcError> {
    let parts: Vec<&str> = params.table_full_name.splitn(3, '.').collect();
    if parts.len() != 3 { return Err(UcError::invalid_argument("table_full_name must be catalog.schema.table")); }
    let schema = SchemaRepo::get_by_full_name(&state.pool, parts[0], parts[1]).await?;
    let table = TableRepo::get_by_schema_and_name(&state.pool, schema.id, parts[2]).await?;
    let commits = DeltaCommitRepo::list_for_table(&state.pool, table.id, params.starting_version, params.ending_version).await?;
    let latest = DeltaCommitRepo::latest_version(&state.pool, table.id).await?.unwrap_or(-1);
    let commits_info = commits.into_iter().map(|c| CommitInfo { version: c.commit_version, timestamp: c.commit_timestamp, operation: None }).collect();
    Ok(Json(GetCommitsResponse { commits_info, latest_table_version: latest }))
}

pub async fn commit(State(_state): State<AppState>) -> Result<Json<()>, UcError> {
    Ok(Json(()))
}
