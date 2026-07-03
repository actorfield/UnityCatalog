use crate::{
    models::table::{ColumnRow, TableRow},
    pool::AnyPool,
};
use uc_errors::{ErrorCode, UcError};
use uuid::Uuid;

pub async fn create(pool: &AnyPool, row: &TableRow) -> Result<TableRow, UcError> {
    sqlx::query_as::<_, TableRow>(
        "INSERT INTO uc_tables (id, schema_id, name, type, owner, created_at, created_by,
              data_source_format, comment, url, column_count, view_definition)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
             RETURNING *",
    )
    .bind(row.id)
    .bind(row.schema_id)
    .bind(&row.name)
    .bind(&row.r#type)
    .bind(&row.owner)
    .bind(row.created_at)
    .bind(&row.created_by)
    .bind(&row.data_source_format)
    .bind(&row.comment)
    .bind(&row.url)
    .bind(row.column_count)
    .bind(&row.view_definition)
    .fetch_one(pool)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref db) if db.is_unique_violation() => UcError::new(
            ErrorCode::TableAlreadyExists,
            format!("Table '{}' already exists", row.name),
        ),
        other => crate::sqlx_err(other),
    })
}

pub async fn get_by_id(pool: &AnyPool, id: Uuid) -> Result<TableRow, UcError> {
    sqlx::query_as::<_, TableRow>("SELECT * FROM uc_tables WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => UcError::new(
                ErrorCode::TableNotFound,
                format!("Table '{}' not found", id),
            ),
            other => crate::sqlx_err(other),
        })
}

pub async fn get_by_schema_and_name(
    pool: &AnyPool,
    schema_id: Uuid,
    name: &str,
) -> Result<TableRow, UcError> {
    sqlx::query_as::<_, TableRow>("SELECT * FROM uc_tables WHERE schema_id = $1 AND name = $2")
        .bind(schema_id)
        .bind(name)
        .fetch_one(pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => UcError::new(
                ErrorCode::TableNotFound,
                format!("Table '{}' not found", name),
            ),
            other => crate::sqlx_err(other),
        })
}

pub async fn list(
    pool: &AnyPool,
    schema_id: Uuid,
    page_token: Option<&str>,
    max_results: i64,
) -> Result<(Vec<TableRow>, Option<String>), UcError> {
    // not compile-time checked
    let rows: Vec<TableRow> = if let Some(token) = page_token {
        sqlx::query_as::<_, TableRow>(
            "SELECT * FROM uc_tables WHERE schema_id = $1 AND name > $2 ORDER BY name LIMIT $3",
        )
        .bind(schema_id)
        .bind(token)
        .bind(max_results + 1)
        .fetch_all(pool)
        .await
        .map_err(crate::sqlx_err)?
    } else {
        sqlx::query_as::<_, TableRow>(
            "SELECT * FROM uc_tables WHERE schema_id = $1 ORDER BY name LIMIT $2",
        )
        .bind(schema_id)
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

pub async fn delete(pool: &AnyPool, id: Uuid) -> Result<(), UcError> {
    let r = sqlx::query("DELETE FROM uc_tables WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(crate::sqlx_err)?;
    if r.rows_affected() == 0 {
        return Err(UcError::new(
            ErrorCode::TableNotFound,
            format!("Table '{}' not found", id),
        ));
    }
    Ok(())
}

// ── Columns ───────────────────────────────────────────────────────────────

pub async fn insert_columns(pool: &AnyPool, columns: &[ColumnRow]) -> Result<(), UcError> {
    for col in columns {
        sqlx::query(
            "INSERT INTO uc_columns (id, table_id, name, ordinal_position, type_text,
                  type_json, type_name, type_precision, type_scale, type_interval_type,
                  nullable, comment, partition_index)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)",
        )
        .bind(col.id)
        .bind(col.table_id)
        .bind(&col.name)
        .bind(col.ordinal_position)
        .bind(&col.type_text)
        .bind(&col.type_json)
        .bind(&col.type_name)
        .bind(col.type_precision)
        .bind(col.type_scale)
        .bind(&col.type_interval_type)
        .bind(col.nullable)
        .bind(&col.comment)
        .bind(col.partition_index)
        .execute(pool)
        .await
        .map_err(crate::sqlx_err)?;
    }
    Ok(())
}

pub async fn get_columns(pool: &AnyPool, table_id: Uuid) -> Result<Vec<ColumnRow>, UcError> {
    sqlx::query_as::<_, ColumnRow>(
        "SELECT * FROM uc_columns WHERE table_id = $1 ORDER BY ordinal_position",
    )
    .bind(table_id)
    .fetch_all(pool)
    .await
    .map_err(crate::sqlx_err)
}

pub async fn delete_columns(pool: &AnyPool, table_id: Uuid) -> Result<(), UcError> {
    sqlx::query("DELETE FROM uc_columns WHERE table_id = $1")
        .bind(table_id)
        .execute(pool)
        .await
        .map_err(crate::sqlx_err)?;
    Ok(())
}
