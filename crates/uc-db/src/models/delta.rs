use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg_attr(feature = "sql", derive(sqlx::FromRow))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaCommitRow {
    pub id: Uuid,
    pub table_id: Uuid,
    pub commit_version: i64,
    pub commit_filename: String,
    pub commit_filesize: i64,
    pub commit_file_modification_timestamp: i64,
    pub commit_timestamp: i64,
    pub is_backfilled_latest_commit: bool,
}
