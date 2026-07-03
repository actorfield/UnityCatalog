use crate::{models::delta::DeltaCommitRow, pool::AnyPool};
use uc_errors::UcError;
use uuid::Uuid;

pub async fn insert(pool: &AnyPool, row: &DeltaCommitRow) -> Result<DeltaCommitRow, UcError> {
    sqlx::query_as::<_, DeltaCommitRow>(
            "INSERT INTO uc_delta_commits (id, table_id, commit_version, commit_filename, commit_filesize,
              commit_file_modification_timestamp, commit_timestamp, is_backfilled_latest_commit)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8) RETURNING *",
        )
        .bind(row.id).bind(row.table_id).bind(row.commit_version).bind(&row.commit_filename)
        .bind(row.commit_filesize).bind(row.commit_file_modification_timestamp)
        .bind(row.commit_timestamp).bind(row.is_backfilled_latest_commit)
        .fetch_one(pool)
        .await
        .map_err(|e| match e {
            // Unique constraint on (table_id, commit_version) → 409 CommitVersionConflict, not 500
            sqlx::Error::Database(ref db) if db.is_unique_violation() => {
                uc_errors::UcError::new(
                    uc_errors::ErrorCode::CommitVersionConflict,
                    format!("Commit version {} already exists for this table", row.commit_version),
                )
            }
            other => crate::sqlx_err(other),
        })
}

pub async fn list_for_table(
    pool: &AnyPool,
    table_id: Uuid,
    starting_version: Option<i64>,
    ending_version: Option<i64>,
) -> Result<Vec<DeltaCommitRow>, UcError> {
    // not compile-time checked — dynamic range filter
    let rows: Vec<DeltaCommitRow> = sqlx::query_as::<_, DeltaCommitRow>(
        "SELECT * FROM uc_delta_commits
             WHERE table_id=$1
               AND ($2 IS NULL OR commit_version >= $2)
               AND ($3 IS NULL OR commit_version <= $3)
             ORDER BY commit_version",
    )
    .bind(table_id)
    .bind(starting_version)
    .bind(ending_version)
    .fetch_all(pool)
    .await
    .map_err(crate::sqlx_err)?;
    Ok(rows)
}

pub async fn latest_version(pool: &AnyPool, table_id: Uuid) -> Result<Option<i64>, UcError> {
    let row: Option<(i64,)> =
        sqlx::query_as("SELECT MAX(commit_version) FROM uc_delta_commits WHERE table_id=$1")
            .bind(table_id)
            .fetch_optional(pool)
            .await
            .map_err(crate::sqlx_err)?;
    Ok(row.map(|(v,)| v))
}
