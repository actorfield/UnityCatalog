use crate::IntoUcResult;
use crate::{models::metastore::MetastoreRow, pool::AnyPool};
use uc_errors::UcError;
use uuid::Uuid;

/// Get the singleton metastore row, creating it if it doesn't exist.
pub async fn get_or_init(pool: &AnyPool, name: &str) -> Result<MetastoreRow, UcError> {
    if let Some(row) = sqlx::query_as::<_, MetastoreRow>("SELECT * FROM uc_metastore LIMIT 1")
        .fetch_optional(pool)
        .await
        .uc_err()?
    {
        return Ok(row);
    }

    let id = Uuid::new_v4();
    sqlx::query_as::<_, MetastoreRow>(
        "INSERT INTO uc_metastore (id, name) VALUES ($1, $2) RETURNING *",
    )
    .bind(id)
    .bind(name)
    .fetch_one(pool)
    .await
    .map_err(crate::sqlx_err)
}

pub async fn get(pool: &AnyPool) -> Result<MetastoreRow, UcError> {
    sqlx::query_as::<_, MetastoreRow>("SELECT * FROM uc_metastore LIMIT 1")
        .fetch_one(pool)
        .await
        .map_err(crate::sqlx_err)
}
