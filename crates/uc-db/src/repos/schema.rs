use crate::IntoUcResult;
use crate::{models::schema::SchemaRow, pool::AnyPool};
use uc_errors::{ErrorCode, UcError};
use uuid::Uuid;

// One arg per column of the INSERT below; a params struct would just move the noise.
#[allow(clippy::too_many_arguments)]
pub async fn create(
    pool: &AnyPool,
    id: Uuid,
    catalog_id: Uuid,
    name: &str,
    comment: Option<&str>,
    owner: Option<&str>,
    created_by: Option<&str>,
    storage_root: Option<&str>,
    created_at: i64,
) -> Result<SchemaRow, UcError> {
    sqlx::query_as::<_, SchemaRow>(
            "INSERT INTO uc_schemas (id, catalog_id, name, comment, owner, created_at, created_by, storage_root)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             RETURNING *",
        )
        .bind(id).bind(catalog_id).bind(name).bind(comment)
        .bind(owner).bind(created_at).bind(created_by).bind(storage_root)
        .fetch_one(pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db) if db.is_unique_violation() => UcError::new(
                ErrorCode::SchemaAlreadyExists,
                format!("Schema '{}' already exists", name),
            ),
            other => crate::sqlx_err(other),
        })
}

pub async fn get_by_full_name(
    pool: &AnyPool,
    catalog_name: &str,
    schema_name: &str,
) -> Result<SchemaRow, UcError> {
    // not compile-time checked — joins on runtime names
    sqlx::query_as::<_, SchemaRow>(
        "SELECT s.* FROM uc_schemas s
             JOIN uc_catalogs c ON c.id = s.catalog_id
             WHERE c.name = $1 AND s.name = $2",
    )
    .bind(catalog_name)
    .bind(schema_name)
    .fetch_one(pool)
    .await
    .map_err(|e| match e {
        sqlx::Error::RowNotFound => UcError::new(
            ErrorCode::SchemaNotFound,
            format!("Schema '{}.{}' not found", catalog_name, schema_name),
        ),
        other => crate::sqlx_err(other),
    })
}

pub async fn get_by_id(pool: &AnyPool, id: Uuid) -> Result<SchemaRow, UcError> {
    sqlx::query_as::<_, SchemaRow>("SELECT * FROM uc_schemas WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => UcError::new(
                ErrorCode::SchemaNotFound,
                format!("Schema '{}' not found", id),
            ),
            other => crate::sqlx_err(other),
        })
}

pub async fn list(
    pool: &AnyPool,
    catalog_id: Uuid,
    page_token: Option<&str>,
    max_results: i64,
) -> Result<(Vec<SchemaRow>, Option<String>), UcError> {
    // not compile-time checked — dynamic WHERE clause
    let rows: Vec<SchemaRow> = if let Some(token) = page_token {
        sqlx::query_as::<_, SchemaRow>(
            "SELECT * FROM uc_schemas WHERE catalog_id = $1 AND name > $2 ORDER BY name LIMIT $3",
        )
        .bind(catalog_id)
        .bind(token)
        .bind(max_results + 1)
        .fetch_all(pool)
        .await
        .map_err(crate::sqlx_err)?
    } else {
        sqlx::query_as::<_, SchemaRow>(
            "SELECT * FROM uc_schemas WHERE catalog_id = $1 ORDER BY name LIMIT $2",
        )
        .bind(catalog_id)
        .bind(max_results + 1)
        .fetch_all(pool)
        .await
        .map_err(crate::sqlx_err)?
    };

    let next_token = if rows.len() as i64 > max_results {
        rows.get(max_results as usize - 1).map(|r| r.name.clone())
    } else {
        None
    };
    Ok((
        rows.into_iter().take(max_results as usize).collect(),
        next_token,
    ))
}

pub async fn update(
    pool: &AnyPool,
    id: Uuid,
    new_name: Option<&str>,
    comment: Option<&str>,
    owner: Option<&str>,
    updated_by: Option<&str>,
    updated_at: i64,
) -> Result<SchemaRow, UcError> {
    sqlx::query_as::<_, SchemaRow>(
        "UPDATE uc_schemas
             SET name = COALESCE($1, name),
                 comment = COALESCE($2, comment),
                 owner = COALESCE($3, owner),
                 updated_at = $4, updated_by = $5
             WHERE id = $6
             RETURNING *",
    )
    .bind(new_name)
    .bind(comment)
    .bind(owner)
    .bind(updated_at)
    .bind(updated_by)
    .bind(id)
    .fetch_one(pool)
    .await
    .map_err(crate::sqlx_err)
}

pub async fn delete(pool: &AnyPool, id: Uuid) -> Result<(), UcError> {
    let result = sqlx::query("DELETE FROM uc_schemas WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .uc_err()?;
    if result.rows_affected() == 0 {
        return Err(UcError::new(
            ErrorCode::SchemaNotFound,
            format!("Schema '{}' not found", id),
        ));
    }
    Ok(())
}
