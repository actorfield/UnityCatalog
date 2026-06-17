use crate::{models::function::{FunctionParamRow, FunctionRow}, pool::AnyPool};
use uc_errors::{ErrorCode, UcError};
use uuid::Uuid;

pub struct FunctionRepo;

impl FunctionRepo {
    pub async fn create(pool: &AnyPool, row: &FunctionRow) -> Result<FunctionRow, UcError> {
        sqlx::query_as::<_, FunctionRow>(
            "INSERT INTO uc_functions (id, schema_id, name, comment, owner, created_at, created_by,
              data_type, full_data_type, external_language, is_deterministic, is_null_call,
              parameter_style, routine_body, routine_definition, sql_data_access, security_type, specific_name)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18)
             RETURNING *",
        )
        .bind(row.id).bind(row.schema_id).bind(&row.name).bind(&row.comment).bind(&row.owner)
        .bind(row.created_at).bind(&row.created_by).bind(&row.data_type).bind(&row.full_data_type)
        .bind(&row.external_language).bind(row.is_deterministic).bind(row.is_null_call)
        .bind(&row.parameter_style).bind(&row.routine_body).bind(&row.routine_definition)
        .bind(&row.sql_data_access).bind(&row.security_type).bind(&row.specific_name)
        .fetch_one(pool).await.map_err(crate::sqlx_err)
    }

    pub async fn insert_params(pool: &AnyPool, params: &[FunctionParamRow]) -> Result<(), UcError> {
        for p in params {
            sqlx::query(
                "INSERT INTO uc_function_parameters
                  (id, function_id, name, input_or_return, ordinal_position, type_text, type_json,
                   type_name, type_precision, type_scale, type_interval_type, comment, parameter_mode, parameter_default)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)",
            )
            .bind(p.id).bind(p.function_id).bind(&p.name).bind(p.input_or_return)
            .bind(p.ordinal_position).bind(&p.type_text).bind(&p.type_json).bind(&p.type_name)
            .bind(p.type_precision).bind(p.type_scale).bind(&p.type_interval_type)
            .bind(&p.comment).bind(&p.parameter_mode).bind(&p.parameter_default)
            .execute(pool).await.map_err(crate::sqlx_err)?;
        }
        Ok(())
    }

    pub async fn get_by_schema_and_name(pool: &AnyPool, schema_id: Uuid, name: &str) -> Result<FunctionRow, UcError> {
        sqlx::query_as::<_, FunctionRow>(
            "SELECT * FROM uc_functions WHERE schema_id=$1 AND name=$2",
        )
        .bind(schema_id).bind(name).fetch_one(pool).await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => UcError::new(ErrorCode::NotFound, format!("Function '{}' not found", name)),
            other => crate::sqlx_err(other),
        })
    }

    pub async fn get_params(pool: &AnyPool, function_id: Uuid) -> Result<(Vec<FunctionParamRow>, Vec<FunctionParamRow>), UcError> {
        let all: Vec<FunctionParamRow> = sqlx::query_as::<_, FunctionParamRow>(
            "SELECT * FROM uc_function_parameters WHERE function_id=$1 ORDER BY ordinal_position",
        )
        .bind(function_id).fetch_all(pool).await.map_err(crate::sqlx_err)?;
        let (input, ret): (Vec<_>, Vec<_>) = all.into_iter().partition(|p| p.input_or_return == 0);
        Ok((input, ret))
    }

    pub async fn list(
        pool: &AnyPool, schema_id: Uuid,
        page_token: Option<&str>, max_results: i64,
    ) -> Result<(Vec<FunctionRow>, Option<String>), UcError> {
        // not compile-time checked
        let rows: Vec<FunctionRow> = if let Some(t) = page_token {
            sqlx::query_as::<_, FunctionRow>(
                "SELECT * FROM uc_functions WHERE schema_id=$1 AND name>$2 ORDER BY name LIMIT $3",
            ).bind(schema_id).bind(t).bind(max_results+1).fetch_all(pool).await.map_err(crate::sqlx_err)?
        } else {
            sqlx::query_as::<_, FunctionRow>(
                "SELECT * FROM uc_functions WHERE schema_id=$1 ORDER BY name LIMIT $2",
            ).bind(schema_id).bind(max_results+1).fetch_all(pool).await.map_err(crate::sqlx_err)?
        };
        let next = if rows.len() as i64 > max_results { rows.get(max_results as usize-1).map(|r| r.name.clone()) } else { None };
        Ok((rows.into_iter().take(max_results as usize).collect(), next))
    }

    pub async fn delete(pool: &AnyPool, id: Uuid) -> Result<(), UcError> {
        sqlx::query("DELETE FROM uc_function_parameters WHERE function_id=$1").bind(id).execute(pool).await.map_err(crate::sqlx_err)?;
        let r = sqlx::query("DELETE FROM uc_functions WHERE id=$1").bind(id).execute(pool).await.map_err(crate::sqlx_err)?;
        if r.rows_affected()==0 { return Err(UcError::new(ErrorCode::NotFound, format!("Function '{}' not found", id))); }
        Ok(())
    }
}
