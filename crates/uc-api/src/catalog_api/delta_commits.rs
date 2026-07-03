use crate::state::AppState;
use axum::{
    extract::{Query, State},
    Json,
};
use uc_db::{
    models::delta::DeltaCommitRow,
    repos::{delta, schema, table},
};
use uc_errors::UcError;
use uc_openapi::catalog::{CommitInfo, GetCommitsResponse};
use uuid::Uuid;

#[derive(serde::Deserialize)]
pub struct GetParams {
    pub table_full_name: String,
    pub starting_version: Option<i64>,
    pub ending_version: Option<i64>,
}

#[derive(serde::Deserialize)]
pub struct CommitRequest {
    pub table_full_name: String,
    pub version: i64,
    pub timestamp: i64,
    pub file_name: Option<String>,
    pub file_size: Option<i64>,
    pub file_modification_timestamp: Option<i64>,
}

pub async fn get_commits(
    State(state): State<AppState>,
    Query(params): Query<GetParams>,
) -> Result<Json<GetCommitsResponse>, UcError> {
    let parts: Vec<&str> = params.table_full_name.splitn(3, '.').collect();
    if parts.len() != 3 {
        return Err(UcError::invalid_argument(
            "table_full_name must be catalog.schema.table",
        ));
    }
    let schema = schema::get_by_full_name(&state.pool, parts[0], parts[1]).await?;
    let table = table::get_by_schema_and_name(&state.pool, schema.id, parts[2]).await?;
    let commits = delta::list_for_table(
        &state.pool,
        table.id,
        params.starting_version,
        params.ending_version,
    )
    .await?;
    let latest = delta::latest_version(&state.pool, table.id)
        .await?
        .unwrap_or(-1);
    let commits_info = commits
        .into_iter()
        .map(|c| CommitInfo {
            version: c.commit_version,
            timestamp: c.commit_timestamp,
            operation: None,
            file_name: Some(c.commit_filename),
            file_size: Some(c.commit_filesize),
            file_modification_timestamp: Some(c.commit_file_modification_timestamp),
        })
        .collect();
    Ok(Json(GetCommitsResponse {
        commits_info,
        latest_table_version: latest,
    }))
}

pub async fn commit(
    State(state): State<AppState>,
    Json(req): Json<CommitRequest>,
) -> Result<Json<GetCommitsResponse>, UcError> {
    let parts: Vec<&str> = req.table_full_name.splitn(3, '.').collect();
    if parts.len() != 3 {
        return Err(UcError::invalid_argument(
            "table_full_name must be catalog.schema.table",
        ));
    }
    let schema = schema::get_by_full_name(&state.pool, parts[0], parts[1]).await?;
    let table = table::get_by_schema_and_name(&state.pool, schema.id, parts[2]).await?;

    let row = DeltaCommitRow {
        id: Uuid::new_v4(),
        table_id: table.id,
        commit_version: req.version,
        commit_filename: req
            .file_name
            .unwrap_or_else(|| format!("{:020}.json", req.version)),
        commit_filesize: req.file_size.unwrap_or(0),
        commit_file_modification_timestamp: req
            .file_modification_timestamp
            .unwrap_or(req.timestamp),
        commit_timestamp: req.timestamp,
        is_backfilled_latest_commit: false,
    };
    delta::insert(&state.pool, &row).await?;

    let latest = delta::latest_version(&state.pool, table.id)
        .await?
        .unwrap_or(-1);
    Ok(Json(GetCommitsResponse {
        commits_info: vec![CommitInfo {
            version: req.version,
            timestamp: req.timestamp,
            operation: None,
            file_name: row.commit_filename.clone().into(),
            file_size: Some(row.commit_filesize),
            file_modification_timestamp: Some(row.commit_file_modification_timestamp),
        }],
        latest_table_version: latest,
    }))
}
