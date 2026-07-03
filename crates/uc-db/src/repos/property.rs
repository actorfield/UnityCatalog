use crate::IntoUcResult;
use crate::{models::property::PropertyRow, pool::AnyPool};
use std::collections::HashMap;
use uc_errors::UcError;
use uuid::Uuid;

/// Fetch all properties for one entity.
pub async fn get_for_entity(
    pool: &AnyPool,
    entity_id: Uuid,
    entity_type: &str,
) -> Result<HashMap<String, String>, UcError> {
    let rows: Vec<PropertyRow> = sqlx::query_as::<_, PropertyRow>(
        "SELECT * FROM uc_properties WHERE entity_id = $1 AND entity_type = $2",
    )
    .bind(entity_id)
    .bind(entity_type)
    .fetch_all(pool)
    .await
    .uc_err()?;

    Ok(rows
        .into_iter()
        .map(|r| (r.property_key, r.property_value))
        .collect())
}

/// Replace-on-update: delete all existing properties then insert new ones.
/// Must be called inside a transaction.
pub async fn replace(
    pool: &AnyPool,
    entity_id: Uuid,
    entity_type: &str,
    properties: &HashMap<String, String>,
) -> Result<(), UcError> {
    sqlx::query("DELETE FROM uc_properties WHERE entity_id = $1 AND entity_type = $2")
        .bind(entity_id)
        .bind(entity_type)
        .execute(pool)
        .await
        .uc_err()?;

    for (key, value) in properties {
        sqlx::query(
            "INSERT INTO uc_properties (id, entity_id, entity_type, property_key, property_value)
                 VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(Uuid::new_v4())
        .bind(entity_id)
        .bind(entity_type)
        .bind(key)
        .bind(value)
        .execute(pool)
        .await
        .uc_err()?;
    }
    Ok(())
}

/// Delete all properties for an entity (e.g. when the entity is deleted).
pub async fn delete_for_entity(
    pool: &AnyPool,
    entity_id: Uuid,
    entity_type: &str,
) -> Result<(), UcError> {
    sqlx::query("DELETE FROM uc_properties WHERE entity_id = $1 AND entity_type = $2")
        .bind(entity_id)
        .bind(entity_type)
        .execute(pool)
        .await
        .uc_err()?;
    Ok(())
}
