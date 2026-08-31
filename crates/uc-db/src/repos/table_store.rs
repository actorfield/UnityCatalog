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
                vec![Action::Upsert { kind: EntityKind::Table, id: row.id, body }],
                row.clone(),
            ))
        })
        .await
}

pub async fn get_by_id(store: &Store, id: Uuid) -> Result<TableRow, UcError> {
    let snap = store.snapshot().await;
    snap.get(EntityKind::Table, id)
        .ok_or_else(|| {
            UcError::new(ErrorCode::TableNotFound, format!("Table '{}' not found", id))
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
        max_results as usize + 1,
    );
    let rows: Vec<TableRow> = found.into_iter().map(table_of).collect::<Result<_, _>>()?;
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
            Ok((vec![Action::Remove { kind: EntityKind::Table, id }], ()))
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
                .map(|id| Action::Remove { kind: EntityKind::Column, id })
                .collect();
            Ok((actions, ()))
        })
        .await
}
