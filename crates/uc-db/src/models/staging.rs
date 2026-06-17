use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct StagingTableRow {
    pub id: Uuid,
    pub schema_id: Uuid,
    pub name: String,
    pub staging_location: String,
    pub created_at: i64,
    pub created_by: Option<String>,
    pub accessed_at: i64,
    pub stage_committed: bool,
    pub stage_committed_at: Option<i64>,
    pub purge_state: i32,
    pub num_cleanup_retries: i32,
    pub last_cleanup_at: Option<i64>,
}
