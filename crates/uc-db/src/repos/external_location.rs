use crate::{models::external_location::ExternalLocationRow, pool::AnyPool};
use uc_errors::{ErrorCode, UcError};

pub async fn create(
    pool: &AnyPool,
    row: &ExternalLocationRow,
) -> Result<ExternalLocationRow, UcError> {
    sqlx::query_as::<_, ExternalLocationRow>(
            "INSERT INTO uc_external_locations (id, name, url, comment, owner, credential_id, created_at, created_by)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8) RETURNING *",
        )
        .bind(row.id).bind(&row.name).bind(&row.url).bind(&row.comment).bind(&row.owner)
        .bind(row.credential_id).bind(row.created_at).bind(&row.created_by)
        .fetch_one(pool).await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db) if db.is_unique_violation() =>
                UcError::new(ErrorCode::ExternalLocationAlreadyExists, format!("External location '{}' already exists", row.name)),
            other => crate::sqlx_err(other),
        })
}

pub async fn get_by_name(pool: &AnyPool, name: &str) -> Result<ExternalLocationRow, UcError> {
    sqlx::query_as::<_, ExternalLocationRow>("SELECT * FROM uc_external_locations WHERE name=$1")
        .bind(name)
        .fetch_one(pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => UcError::new(
                ErrorCode::NotFound,
                format!("External location '{}' not found", name),
            ),
            other => crate::sqlx_err(other),
        })
}

pub async fn list(
    pool: &AnyPool,
    page_token: Option<&str>,
    max_results: i64,
) -> Result<(Vec<ExternalLocationRow>, Option<String>), UcError> {
    // not compile-time checked
    let rows: Vec<ExternalLocationRow> = if let Some(t) = page_token {
        sqlx::query_as::<_, ExternalLocationRow>(
            "SELECT * FROM uc_external_locations WHERE name>$1 ORDER BY name LIMIT $2",
        )
        .bind(t)
        .bind(max_results + 1)
        .fetch_all(pool)
        .await
        .map_err(crate::sqlx_err)?
    } else {
        sqlx::query_as::<_, ExternalLocationRow>(
            "SELECT * FROM uc_external_locations ORDER BY name LIMIT $1",
        )
        .bind(max_results + 1)
        .fetch_all(pool)
        .await
        .map_err(crate::sqlx_err)?
    };
    let next = if rows.len() as i64 > max_results {
        rows.get(max_results as usize - 1).map(|r| r.name.clone())
    } else {
        None
    };
    Ok((rows.into_iter().take(max_results as usize).collect(), next))
}

pub async fn delete(pool: &AnyPool, name: &str) -> Result<(), UcError> {
    let r = sqlx::query("DELETE FROM uc_external_locations WHERE name=$1")
        .bind(name)
        .execute(pool)
        .await
        .map_err(crate::sqlx_err)?;
    if r.rows_affected() == 0 {
        return Err(UcError::new(
            ErrorCode::NotFound,
            format!("External location '{}' not found", name),
        ));
    }
    Ok(())
}

/// Find the external location whose url is a prefix of the given path.
pub async fn find_by_path_prefix(
    pool: &AnyPool,
    path: &str,
) -> Result<Option<ExternalLocationRow>, UcError> {
    // not compile-time checked — LIKE with runtime pattern
    sqlx::query_as::<_, ExternalLocationRow>(
            "SELECT * FROM uc_external_locations WHERE $1 LIKE (url || '%') ORDER BY LENGTH(url) DESC LIMIT 1",
        )
        .bind(path).fetch_optional(pool).await.map_err(crate::sqlx_err)
}
