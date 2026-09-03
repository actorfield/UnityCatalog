//! Log-structured body for repos::table. Signatures identical to table.rs.

use crate::models::table::{ColumnRow, TableRow};
use crate::store::action::{Action, EntityKind};
use crate::store::Store;
use uc_errors::{ErrorCode, UcError};
use uuid::Uuid;

fn table_of(v: &serde_json::Value) -> Result<TableRow, UcError> {
    serde_json::from_value(v.clone())
        .map_err(|e| UcError::new(ErrorCode::Internal, format!("corrupt table row: {e}")))
}

fn column_of(v: &serde_json::Value) -> Result<ColumnRow, UcError> {
    serde_json::from_value(v.clone())
        .map_err(|e| UcError::new(ErrorCode::Internal, format!("corrupt column row: {e}")))
}

/// UNIQUE(schema_id, name)
fn nk(schema_id: Uuid, name: &str) -> String {
    format!("{schema_id}\u{0}{name}")
}

fn schema_prefix(schema_id: Uuid) -> String {
    format!("{schema_id}\u{0}")
}

/// Columns are keyed (table_id, ordinal_position); this is the group prefix.
fn column_prefix(table_id: Uuid) -> String {
    format!("{table_id}\u{0}")
}

pub async fn create(store: &Store, row: &TableRow) -> Result<TableRow, UcError> {
    let row = row.clone();
    store
        .commit("CREATE TABLE", |snap| {
            if snap
                .get_by_natural_key(EntityKind::Table, &nk(row.schema_id, &row.name))
                .is_some()
            {
                return Err(UcError::new(
                    ErrorCode::TableAlreadyExists,
                    format!("Table '{}' already exists", row.name),
                ));
            }
            let body = serde_json::to_value(&row)
                .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
            Ok((
                vec![Action::Upsert {
                    kind: EntityKind::Table,
                    id: row.id,
                    body,
                }],
                row.clone(),
            ))
        })
        .await
}

pub async fn get_by_id(store: &Store, id: Uuid) -> Result<TableRow, UcError> {
    let snap = store.snapshot().await;
    snap.get(EntityKind::Table, id)
        .ok_or_else(|| {
            UcError::new(
                ErrorCode::TableNotFound,
                format!("Table '{}' not found", id),
            )
        })
        .and_then(table_of)
}

pub async fn get_by_schema_and_name(
    store: &Store,
    schema_id: Uuid,
    name: &str,
) -> Result<TableRow, UcError> {
    let snap = store.snapshot().await;
    snap.get_by_natural_key(EntityKind::Table, &nk(schema_id, name))
        .ok_or_else(|| {
            UcError::new(
                ErrorCode::TableNotFound,
                format!("Table '{}' not found", name),
            )
        })
        .and_then(table_of)
}

pub async fn list(
    store: &Store,
    schema_id: Uuid,
    page_token: Option<&str>,
    max_results: i64,
) -> Result<(Vec<TableRow>, Option<String>), UcError> {
    let snap = store.snapshot().await;
    let found = snap.scan_prefix(
        EntityKind::Table,
        &schema_prefix(schema_id),
        page_token,
        crate::pagination::over_fetch(max_results),
    );
    let rows: Vec<TableRow> = found.into_iter().map(table_of).collect::<Result<_, _>>()?;
    let (rows, next_token) = crate::pagination::page(rows, max_results, |r| r.name.clone());
    Ok((rows, next_token))
}

pub async fn delete(store: &Store, id: Uuid) -> Result<(), UcError> {
    store
        .commit("DROP TABLE", |snap| {
            if snap.get(EntityKind::Table, id).is_none() {
                return Err(UcError::new(
                    ErrorCode::TableNotFound,
                    format!("Table '{}' not found", id),
                ));
            }
            // Columns are left behind, matching the SQL: FK enforcement is off
            // and nothing cascades there either. Callers that want them gone
            // call delete_columns, as they do today.
            Ok((
                vec![Action::Remove {
                    kind: EntityKind::Table,
                    id,
                }],
                (),
            ))
        })
        .await
}

// ── Columns ───────────────────────────────────────────────────────────────

/// One commit for the whole column set rather than the SQL's insert-per-column
/// loop, so a table never becomes visible with half its schema.
pub async fn insert_columns(store: &Store, columns: &[ColumnRow]) -> Result<(), UcError> {
    let columns = columns.to_vec();
    store
        .commit("ADD COLUMNS", |_| {
            let mut actions = Vec::with_capacity(columns.len());
            for col in &columns {
                let body = serde_json::to_value(col)
                    .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
                actions.push(Action::Upsert {
                    kind: EntityKind::Column,
                    id: col.id,
                    body,
                });
            }
            Ok((actions, ()))
        })
        .await
}

/// `ORDER BY ordinal_position` comes free: the natural key is
/// (table_id, pad_i64(ordinal_position)), so prefix order is ordinal order.
pub async fn get_columns(store: &Store, table_id: Uuid) -> Result<Vec<ColumnRow>, UcError> {
    let snap = store.snapshot().await;
    snap.ids_under_prefix(EntityKind::Column, &column_prefix(table_id))
        .into_iter()
        .filter_map(|id| snap.get(EntityKind::Column, id))
        .map(column_of)
        .collect()
}

pub async fn delete_columns(store: &Store, table_id: Uuid) -> Result<(), UcError> {
    let pfx = column_prefix(table_id);
    store
        .commit("DROP COLUMNS", |snap| {
            let actions: Vec<Action> = snap
                .ids_under_prefix(EntityKind::Column, &pfx)
                .into_iter()
                .map(|id| Action::Remove {
                    kind: EntityKind::Column,
                    id,
                })
                .collect();
            Ok((actions, ()))
        })
        .await
}

/// Patch the fields the Delta commit handler updates in place. `None` leaves a
/// field alone, matching COALESCE.
#[allow(clippy::too_many_arguments)]
pub async fn patch(
    store: &Store,
    id: Uuid,
    column_count: Option<i32>,
    comment: Option<&str>,
    iceberg_version: Option<i64>,
    iceberg_timestamp: Option<i64>,
    updated_at: i64,
) -> Result<(), UcError> {
    store
        .commit("ALTER TABLE", |snap| {
            // Zero rows matched is success in the SQL; preserved.
            let Some(current) = snap.get(EntityKind::Table, id) else {
                return Ok((vec![], ()));
            };
            let mut row = table_of(current)?;
            if let Some(c) = column_count {
                row.column_count = Some(c);
            }
            if let Some(c) = comment {
                row.comment = Some(c.to_string());
            }
            if let Some(v) = iceberg_version {
                row.uniform_iceberg_converted_delta_version = Some(v);
            }
            if let Some(t) = iceberg_timestamp {
                row.uniform_iceberg_converted_delta_timestamp = Some(t);
            }
            row.updated_at = Some(updated_at);
            let body = serde_json::to_value(&row)
                .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
            Ok((
                vec![Action::Upsert {
                    kind: EntityKind::Table,
                    id,
                    body,
                }],
                (),
            ))
        })
        .await
}

/// Rename a table.
///
/// The SQL issues a bare UPDATE, so renaming onto an occupied (schema_id, name)
/// trips the UNIQUE constraint and surfaces as a 500. Returning the domain
/// error instead is a deliberate change, consistent with the other renames.
pub async fn rename(
    store: &Store,
    id: Uuid,
    new_name: &str,
    updated_at: i64,
) -> Result<(), UcError> {
    store
        .commit("RENAME TABLE", |snap| {
            let Some(current) = snap.get(EntityKind::Table, id) else {
                return Ok((vec![], ()));
            };
            let mut row = table_of(current)?;
            if new_name != row.name
                && snap
                    .get_by_natural_key(EntityKind::Table, &nk(row.schema_id, new_name))
                    .is_some()
            {
                return Err(UcError::new(
                    ErrorCode::TableAlreadyExists,
                    format!("Table '{}' already exists", new_name),
                ));
            }
            row.name = new_name.to_string();
            row.updated_at = Some(updated_at);
            let body = serde_json::to_value(&row)
                .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
            Ok((
                vec![Action::Upsert {
                    kind: EntityKind::Table,
                    id,
                    body,
                }],
                (),
            ))
        })
        .await
}
