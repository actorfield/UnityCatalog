use crate::{models::volume::VolumeRow, pool::AnyPool};
use uc_errors::{ErrorCode, UcError};
use uuid::Uuid;

pub struct VolumeRepo;

impl VolumeRepo {
    pub async fn create(pool: &AnyPool, row: &VolumeRow) -> Result<VolumeRow, UcError> {
        sqlx::query_as::<_, VolumeRow>(
            "INSERT INTO uc_volumes (id, schema_id, name, comment, storage_location, owner,
              created_at, created_by, volume_type)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
             RETURNING *",
        )
        .bind(row.id).bind(row.schema_id).bind(&row.name).bind(&row.comment)
        .bind(&row.storage_location).bind(&row.owner).bind(row.created_at)
        .bind(&row.created_by).bind(&row.volume_type)
        .fetch_one(pool).await.map_err(|e| match e {
            sqlx::Error::Database(ref db) if db.is_unique_violation() => {
                UcError::new(ErrorCode::ResourceAlreadyExists, format!("Volume '{}' already exists", row.name))
            }
            other => crate::sqlx_err(other),
        })
    }

    pub async fn get_by_id(pool: &AnyPool, id: Uuid) -> Result<VolumeRow, UcError> {
        sqlx::query_as::<_, VolumeRow>("SELECT * FROM uc_volumes WHERE id = $1")
            .bind(id).fetch_one(pool).await
            .map_err(|e| match e {
                sqlx::Error::RowNotFound => UcError::new(ErrorCode::NotFound, format!("Volume '{}' not found", id)),
                other => crate::sqlx_err(other),
            })
    }

    pub async fn get_by_schema_and_name(pool: &AnyPool, schema_id: Uuid, name: &str) -> Result<VolumeRow, UcError> {
        sqlx::query_as::<_, VolumeRow>(
            "SELECT * FROM uc_volumes WHERE schema_id = $1 AND name = $2",
        )
        .bind(schema_id).bind(name).fetch_one(pool).await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => UcError::new(ErrorCode::NotFound, format!("Volume '{}' not found", name)),
            other => crate::sqlx_err(other),
        })
    }

    pub async fn list(
        pool: &AnyPool, schema_id: Uuid,
        page_token: Option<&str>, max_results: i64,
    ) -> Result<(Vec<VolumeRow>, Option<String>), UcError> {
        // not compile-time checked
        let rows: Vec<VolumeRow> = if let Some(t) = page_token {
            sqlx::query_as::<_, VolumeRow>(
                "SELECT * FROM uc_volumes WHERE schema_id=$1 AND name>$2 ORDER BY name LIMIT $3",
            ).bind(schema_id).bind(t).bind(max_results+1).fetch_all(pool).await.map_err(crate::sqlx_err)?
        } else {
            sqlx::query_as::<_, VolumeRow>(
                "SELECT * FROM uc_volumes WHERE schema_id=$1 ORDER BY name LIMIT $2",
            ).bind(schema_id).bind(max_results+1).fetch_all(pool).await.map_err(crate::sqlx_err)?
        };
        let next = if rows.len() as i64 > max_results { rows.get(max_results as usize-1).map(|r| r.name.clone()) } else { None };
        Ok((rows.into_iter().take(max_results as usize).collect(), next))
    }

    pub async fn update(
        pool: &AnyPool, id: Uuid, new_name: Option<&str>,
        comment: Option<&str>, owner: Option<&str>, updated_at: i64, updated_by: Option<&str>,
    ) -> Result<VolumeRow, UcError> {
        sqlx::query_as::<_, VolumeRow>(
            "UPDATE uc_volumes SET name=COALESCE($1,name), comment=COALESCE($2,comment),
              owner=COALESCE($3,owner), updated_at=$4, updated_by=$5
             WHERE id=$6 RETURNING *",
        )
        .bind(new_name).bind(comment).bind(owner).bind(updated_at).bind(updated_by).bind(id)
        .fetch_one(pool).await.map_err(crate::sqlx_err)
    }

    pub async fn delete(pool: &AnyPool, id: Uuid) -> Result<(), UcError> {
        let r = sqlx::query("DELETE FROM uc_volumes WHERE id=$1").bind(id).execute(pool).await.map_err(crate::sqlx_err)?;
        if r.rows_affected()==0 { return Err(UcError::new(ErrorCode::NotFound, format!("Volume '{}' not found", id))); }
        Ok(())
    }
}
