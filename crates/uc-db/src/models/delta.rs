use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
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
