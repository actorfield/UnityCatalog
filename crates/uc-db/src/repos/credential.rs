use crate::{models::credential::CredentialRow, pool::AnyPool};
use uc_errors::{ErrorCode, UcError};
use uuid::Uuid;

pub async fn create(pool: &AnyPool, row: &CredentialRow) -> Result<CredentialRow, UcError> {
    sqlx::query_as::<_, CredentialRow>(
            "INSERT INTO uc_credentials (id, name, credential_type, credential, purpose, comment, owner, created_at, created_by)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING *",
        )
        .bind(row.id).bind(&row.name).bind(&row.credential_type).bind(&row.credential)
        .bind(&row.purpose).bind(&row.comment).bind(&row.owner)
        .bind(row.created_at).bind(&row.created_by)
        .fetch_one(pool).await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db) if db.is_unique_violation() =>
                UcError::new(ErrorCode::StorageCredentialAlreadyExists, format!("Credential '{}' already exists", row.name)),
            other => crate::sqlx_err(other),
        })
}

pub async fn get_by_name(pool: &AnyPool, name: &str) -> Result<CredentialRow, UcError> {
    sqlx::query_as::<_, CredentialRow>("SELECT * FROM uc_credentials WHERE name=$1")
        .bind(name)
        .fetch_one(pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => UcError::new(
                ErrorCode::NotFound,
                format!("Credential '{}' not found", name),
            ),
            other => crate::sqlx_err(other),
        })
}

pub async fn get_by_id(pool: &AnyPool, id: Uuid) -> Result<CredentialRow, UcError> {
    sqlx::query_as::<_, CredentialRow>("SELECT * FROM uc_credentials WHERE id=$1")
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(crate::sqlx_err)
}

pub async fn list(
    pool: &AnyPool,
    page_token: Option<&str>,
    max_results: i64,
) -> Result<(Vec<CredentialRow>, Option<String>), UcError> {
    // not compile-time checked
    let rows: Vec<CredentialRow> = if let Some(t) = page_token {
        sqlx::query_as::<_, CredentialRow>(
            "SELECT * FROM uc_credentials WHERE name>$1 ORDER BY name LIMIT $2",
        )
        .bind(t)
        .bind(max_results + 1)
        .fetch_all(pool)
        .await
        .map_err(crate::sqlx_err)?
    } else {
        sqlx::query_as::<_, CredentialRow>("SELECT * FROM uc_credentials ORDER BY name LIMIT $1")
            .bind(max_results + 1)
            .fetch_all(pool)
            .await
            .map_err(crate::sqlx_err)?
    };
    let (rows, next) =
        crate::pagination::page(rows, max_results, |r| r.name.clone());
    Ok((rows, next))
}

pub async fn delete(pool: &AnyPool, name: &str) -> Result<(), UcError> {
    let r = sqlx::query("DELETE FROM uc_credentials WHERE name=$1")
        .bind(name)
        .execute(pool)
        .await
        .map_err(crate::sqlx_err)?;
    if r.rows_affected() == 0 {
        return Err(UcError::new(
            ErrorCode::NotFound,
            format!("Credential '{}' not found", name),
        ));
    }
    Ok(())
}

/// Patch a credential in place.
///
/// Lifted out of the update handler in uc-api, which issued this UPDATE
/// inline. `None` leaves a field alone, matching COALESCE.
#[allow(clippy::too_many_arguments)]
pub async fn update(
    pool: &AnyPool,
    id: Uuid,
    new_name: Option<&str>,
    comment: Option<&str>,
    owner: Option<&str>,
    credential: Option<&str>,
    updated_at: i64,
    updated_by: Option<&str>,
) -> Result<(), UcError> {
    sqlx::query(
        "UPDATE uc_credentials SET name=COALESCE($1,name), comment=COALESCE($2,comment), \
         owner=COALESCE($3,owner), credential=COALESCE($4,credential), updated_at=$5, \
         updated_by=$6 WHERE id=$7",
    )
    .bind(new_name)
    .bind(comment)
    .bind(owner)
    .bind(credential)
    .bind(updated_at)
    .bind(updated_by)
    .bind(id)
    .execute(pool)
    .await
    .map_err(crate::sqlx_err)?;
    Ok(())
}
