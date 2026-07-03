use crate::{models::staging::StagingTableRow, pool::AnyPool};
use uc_errors::UcError;
use uuid::Uuid;

pub async fn create(pool: &AnyPool, row: &StagingTableRow) -> Result<StagingTableRow, UcError> {
    sqlx::query_as::<_, StagingTableRow>(
            "INSERT INTO uc_staging_tables (id, schema_id, name, staging_location, created_at, created_by, accessed_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7) RETURNING *",
        )
        .bind(row.id).bind(row.schema_id).bind(&row.name).bind(&row.staging_location)
        .bind(row.created_at).bind(&row.created_by).bind(row.accessed_at)
        .fetch_one(pool).await.map_err(crate::sqlx_err)
}

pub async fn get_by_id(pool: &AnyPool, id: Uuid) -> Result<StagingTableRow, UcError> {
    sqlx::query_as::<_, StagingTableRow>("SELECT * FROM uc_staging_tables WHERE id=$1")
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(crate::sqlx_err)
}

/// Find a staging table by its storage location (used during MANAGED table commit).
pub async fn get_by_location(pool: &AnyPool, location: &str) -> Result<StagingTableRow, UcError> {
    sqlx::query_as::<_, StagingTableRow>(
        "SELECT * FROM uc_staging_tables WHERE staging_location = $1",
    )
    .bind(location)
    .fetch_one(pool)
    .await
    .map_err(crate::sqlx_err)
}

pub async fn mark_committed(pool: &AnyPool, id: Uuid, committed_at: i64) -> Result<(), UcError> {
    sqlx::query(
        "UPDATE uc_staging_tables SET stage_committed=1, stage_committed_at=$1 WHERE id=$2",
    )
    .bind(committed_at)
    .bind(id)
    .execute(pool)
    .await
    .map_err(crate::sqlx_err)?;
    Ok(())
}
