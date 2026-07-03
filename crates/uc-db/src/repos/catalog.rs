use crate::IntoUcResult;
use crate::{models::catalog::CatalogRow, pool::AnyPool};
use uc_errors::{ErrorCode, UcError};
use uuid::Uuid;

pub async fn create(
    pool: &AnyPool,
    id: Uuid,
    name: &str,
    comment: Option<&str>,
    owner: Option<&str>,
    created_by: Option<&str>,
    storage_root: Option<&str>,
    created_at: i64,
) -> Result<CatalogRow, UcError> {
    // not compile-time checked — name is a runtime param not a table identifier
    sqlx::query_as::<_, CatalogRow>(
        "INSERT INTO uc_catalogs (id, name, comment, owner, created_at, created_by, storage_root)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             RETURNING *",
    )
    .bind(id)
    .bind(name)
    .bind(comment)
    .bind(owner)
    .bind(created_at)
    .bind(created_by)
    .bind(storage_root)
    .fetch_one(pool)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref db) if db.is_unique_violation() => UcError::new(
            ErrorCode::CatalogAlreadyExists,
            format!("Catalog '{}' already exists", name),
        ),
        other => crate::sqlx_err(other),
    })
}

pub async fn get_by_name(pool: &AnyPool, name: &str) -> Result<CatalogRow, UcError> {
    sqlx::query_as::<_, CatalogRow>("SELECT * FROM uc_catalogs WHERE name = $1")
        .bind(name)
        .fetch_one(pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => UcError::new(
                ErrorCode::CatalogNotFound,
                format!("Catalog '{}' not found", name),
            ),
            other => crate::sqlx_err(other),
        })
}

pub async fn get_by_id(pool: &AnyPool, id: Uuid) -> Result<CatalogRow, UcError> {
    sqlx::query_as::<_, CatalogRow>("SELECT * FROM uc_catalogs WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => UcError::new(
                ErrorCode::CatalogNotFound,
                format!("Catalog '{}' not found", id),
            ),
            other => crate::sqlx_err(other),
        })
}

/// List catalogs with cursor-based pagination (page_token = last seen name).
/// Returns (rows, next_page_token).
pub async fn list(
    pool: &AnyPool,
    page_token: Option<&str>,
    max_results: i64,
) -> Result<(Vec<CatalogRow>, Option<String>), UcError> {
    // not compile-time checked — dynamic WHERE clause
    let rows: Vec<CatalogRow> = if let Some(token) = page_token {
        sqlx::query_as::<_, CatalogRow>(
            "SELECT * FROM uc_catalogs WHERE name > $1 ORDER BY name LIMIT $2",
        )
        .bind(token)
        .bind(max_results + 1)
        .fetch_all(pool)
        .await
        .uc_err()?
    } else {
        sqlx::query_as::<_, CatalogRow>("SELECT * FROM uc_catalogs ORDER BY name LIMIT $1")
            .bind(max_results + 1)
            .fetch_all(pool)
            .await
            .uc_err()?
    };

    let next_token = if rows.len() as i64 > max_results {
        rows.get(max_results as usize - 1).map(|r| r.name.clone())
    } else {
        None
    };
    let rows = rows.into_iter().take(max_results as usize).collect();
    Ok((rows, next_token))
}

pub async fn update(
    pool: &AnyPool,
    name: &str,
    new_name: Option<&str>,
    comment: Option<&str>,
    owner: Option<&str>,
    updated_by: Option<&str>,
    updated_at: i64,
) -> Result<CatalogRow, UcError> {
    let effective_name = new_name.unwrap_or(name);
    sqlx::query_as::<_, CatalogRow>(
        "UPDATE uc_catalogs
             SET name = $1, comment = COALESCE($2, comment), owner = COALESCE($3, owner),
                 updated_at = $4, updated_by = $5
             WHERE name = $6
             RETURNING *",
    )
    .bind(effective_name)
    .bind(comment)
    .bind(owner)
    .bind(updated_at)
    .bind(updated_by)
    .bind(name)
    .fetch_one(pool)
    .await
    .map_err(|e| match e {
        sqlx::Error::RowNotFound => UcError::new(
            ErrorCode::CatalogNotFound,
            format!("Catalog '{}' not found", name),
        ),
        other => crate::sqlx_err(other),
    })
}

pub async fn delete(pool: &AnyPool, name: &str) -> Result<(), UcError> {
    let result = sqlx::query("DELETE FROM uc_catalogs WHERE name = $1")
        .bind(name)
        .execute(pool)
        .await
        .uc_err()?;
    if result.rows_affected() == 0 {
        return Err(UcError::new(
            ErrorCode::CatalogNotFound,
            format!("Catalog '{}' not found", name),
        ));
    }
    Ok(())
}
