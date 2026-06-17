use crate::IntoUcResult;
use crate::{models::model::{ModelVersionRow, RegisteredModelRow}, pool::AnyPool};
use uc_errors::{ErrorCode, UcError};
use uuid::Uuid;

pub struct ModelRepo;

impl ModelRepo {
    pub async fn create_model(pool: &AnyPool, row: &RegisteredModelRow) -> Result<RegisteredModelRow, UcError> {
        sqlx::query_as::<_, RegisteredModelRow>(
            "INSERT INTO uc_registered_models (id, schema_id, name, owner, created_at, created_by, comment, url)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8) RETURNING *",
        )
        .bind(row.id).bind(row.schema_id).bind(&row.name).bind(&row.owner)
        .bind(row.created_at).bind(&row.created_by).bind(&row.comment).bind(&row.url)
        .fetch_one(pool).await.map_err(crate::sqlx_err)
    }

    pub async fn get_model_by_schema_and_name(pool: &AnyPool, schema_id: Uuid, name: &str) -> Result<RegisteredModelRow, UcError> {
        sqlx::query_as::<_, RegisteredModelRow>(
            "SELECT * FROM uc_registered_models WHERE schema_id=$1 AND name=$2",
        )
        .bind(schema_id).bind(name).fetch_one(pool).await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => UcError::new(ErrorCode::NotFound, format!("Model '{}' not found", name)),
            other => crate::sqlx_err(other),
        })
    }

    pub async fn list_models(
        pool: &AnyPool, schema_id: Uuid,
        page_token: Option<&str>, max_results: i64,
    ) -> Result<(Vec<RegisteredModelRow>, Option<String>), UcError> {
        // not compile-time checked
        let rows: Vec<RegisteredModelRow> = if let Some(t) = page_token {
            sqlx::query_as::<_, RegisteredModelRow>(
                "SELECT * FROM uc_registered_models WHERE schema_id=$1 AND name>$2 ORDER BY name LIMIT $3",
            ).bind(schema_id).bind(t).bind(max_results+1).fetch_all(pool).await.map_err(crate::sqlx_err)?
        } else {
            sqlx::query_as::<_, RegisteredModelRow>(
                "SELECT * FROM uc_registered_models WHERE schema_id=$1 ORDER BY name LIMIT $2",
            ).bind(schema_id).bind(max_results+1).fetch_all(pool).await.map_err(crate::sqlx_err)?
        };
        let next = if rows.len() as i64 > max_results { rows.get(max_results as usize-1).map(|r| r.name.clone()) } else { None };
        Ok((rows.into_iter().take(max_results as usize).collect(), next))
    }

    pub async fn create_version(pool: &AnyPool, row: &ModelVersionRow) -> Result<ModelVersionRow, UcError> {
        sqlx::query_as::<_, ModelVersionRow>(
            "INSERT INTO uc_model_versions (id, registered_model_id, version, source, run_id, status, owner, created_at, created_by, comment, url)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) RETURNING *",
        )
        .bind(row.id).bind(row.registered_model_id).bind(row.version)
        .bind(&row.source).bind(&row.run_id).bind(&row.status).bind(&row.owner)
        .bind(row.created_at).bind(&row.created_by).bind(&row.comment).bind(&row.url)
        .fetch_one(pool).await.map_err(crate::sqlx_err)
    }

    pub async fn get_version(pool: &AnyPool, model_id: Uuid, version: i32) -> Result<ModelVersionRow, UcError> {
        sqlx::query_as::<_, ModelVersionRow>(
            "SELECT * FROM uc_model_versions WHERE registered_model_id=$1 AND version=$2",
        )
        .bind(model_id).bind(version).fetch_one(pool).await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => UcError::new(ErrorCode::NotFound, format!("Model version {} not found", version)),
            other => crate::sqlx_err(other),
        })
    }

    pub async fn delete_model(pool: &AnyPool, id: Uuid) -> Result<(), UcError> {
        sqlx::query("DELETE FROM uc_model_versions WHERE registered_model_id=$1").bind(id).execute(pool).await.map_err(crate::sqlx_err)?;
        sqlx::query("DELETE FROM uc_registered_models WHERE id=$1").bind(id).execute(pool).await.map_err(crate::sqlx_err)?;
        Ok(())
    }

    pub async fn delete_version(pool: &AnyPool, model_id: Uuid, version: i32) -> Result<(), UcError> {
        sqlx::query("DELETE FROM uc_model_versions WHERE registered_model_id=$1 AND version=$2")
            .bind(model_id).bind(version).execute(pool).await.map_err(crate::sqlx_err)?;
        Ok(())
    }
}
