use crate::IntoUcResult;
use crate::{models::user::UserRow, pool::AnyPool};
use uc_errors::{ErrorCode, UcError};
use uuid::Uuid;

pub struct UserRepo;

impl UserRepo {
    pub async fn create(
        pool: &AnyPool,
        id: Uuid,
        name: &str,
        email: Option<&str>,
        external_id: Option<&str>,
        state: &str,
        created_at: i64,
    ) -> Result<UserRow, UcError> {
        sqlx::query_as::<_, UserRow>(
            "INSERT INTO uc_users (id, name, email, external_id, state, created_at)
             VALUES ($1, $2, $3, $4, $5, $6)
             RETURNING *",
        )
        .bind(id).bind(name).bind(email).bind(external_id)
        .bind(state).bind(created_at)
        .fetch_one(pool)
        .await
        .map_err(crate::sqlx_err)
    }

    pub async fn get_by_id(pool: &AnyPool, id: Uuid) -> Result<UserRow, UcError> {
        sqlx::query_as::<_, UserRow>("SELECT * FROM uc_users WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .map_err(|e| match e {
                sqlx::Error::RowNotFound => UcError::new(ErrorCode::NotFound, format!("User '{}' not found", id)),
                other => crate::sqlx_err(other),
            })
    }

    pub async fn get_by_name(pool: &AnyPool, name: &str) -> Result<UserRow, UcError> {
        sqlx::query_as::<_, UserRow>("SELECT * FROM uc_users WHERE name = $1")
            .bind(name)
            .fetch_one(pool)
            .await
            .map_err(|e| match e {
                sqlx::Error::RowNotFound => UcError::new(ErrorCode::NotFound, format!("User '{}' not found", name)),
                other => crate::sqlx_err(other),
            })
    }

    pub async fn get_by_email(pool: &AnyPool, email: &str) -> Result<Option<UserRow>, UcError> {
        sqlx::query_as::<_, UserRow>("SELECT * FROM uc_users WHERE email = $1")
            .bind(email)
            .fetch_optional(pool)
            .await
            .map_err(crate::sqlx_err)
    }

    pub async fn list(
        pool: &AnyPool,
        page_token: Option<&str>,
        max_results: i64,
    ) -> Result<(Vec<UserRow>, Option<String>), UcError> {
        // not compile-time checked
        let rows: Vec<UserRow> = if let Some(token) = page_token {
            sqlx::query_as::<_, UserRow>(
                "SELECT * FROM uc_users WHERE name > $1 ORDER BY name LIMIT $2",
            )
            .bind(token).bind(max_results + 1)
            .fetch_all(pool).await.map_err(crate::sqlx_err)?
        } else {
            sqlx::query_as::<_, UserRow>(
                "SELECT * FROM uc_users ORDER BY name LIMIT $1",
            )
            .bind(max_results + 1)
            .fetch_all(pool).await.map_err(crate::sqlx_err)?
        };

        let next_token = if rows.len() as i64 > max_results {
            rows.get(max_results as usize - 1).map(|r| r.name.clone())
        } else {
            None
        };
        Ok((rows.into_iter().take(max_results as usize).collect(), next_token))
    }

    pub async fn update(
        pool: &AnyPool,
        id: Uuid,
        name: Option<&str>,
        email: Option<&str>,
        state: Option<&str>,
        updated_at: i64,
    ) -> Result<UserRow, UcError> {
        sqlx::query_as::<_, UserRow>(
            "UPDATE uc_users
             SET name = COALESCE($1, name),
                 email = COALESCE($2, email),
                 state = COALESCE($3, state),
                 updated_at = $4
             WHERE id = $5
             RETURNING *",
        )
        .bind(name).bind(email).bind(state).bind(updated_at).bind(id)
        .fetch_one(pool)
        .await
        .map_err(crate::sqlx_err)
    }

    pub async fn delete(pool: &AnyPool, id: Uuid) -> Result<(), UcError> {
        let result = sqlx::query("DELETE FROM uc_users WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await.uc_err()?;
        if result.rows_affected() == 0 {
            return Err(UcError::new(ErrorCode::NotFound, format!("User '{}' not found", id)));
        }
        Ok(())
    }
}
