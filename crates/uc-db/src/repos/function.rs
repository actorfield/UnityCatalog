//! Log-structured body for repos::function. Signatures identical to function.rs.

use crate::models::function::{FunctionParamRow, FunctionRow};
use crate::store::action::{Action, EntityKind};
use crate::store::row::Row;
use crate::store::Store;
use uc_errors::{ErrorCode, UcError};
use uuid::Uuid;

fn fn_of(v: &Row) -> Result<FunctionRow, UcError> {
    crate::typed_row!(v, Row::Function, "function")
}

fn param_of(v: &Row) -> Result<FunctionParamRow, UcError> {
    crate::typed_row!(v, Row::FunctionParameter, "function parameter")
}

/// UNIQUE(schema_id, name)
fn nk(schema_id: Uuid, name: &str) -> String {
    format!("{schema_id}\u{0}{name}")
}

fn prefix(schema_id: Uuid) -> String {
    format!("{schema_id}\u{0}")
}

pub async fn create(store: &Store, row: &FunctionRow) -> Result<FunctionRow, UcError> {
    let row = row.clone();
    store
        .commit("CREATE FUNCTION", |snap| {
            // The SQL maps no unique violation here, so a duplicate function
            // name surfaces as a 500 today. Returning the domain error is a
            // deliberate change: preserving the old behaviour would mean
            // preserving a 500 for a client-side conflict.
            if snap
                .get_by_natural_key(EntityKind::Function, &nk(row.schema_id, &row.name))
                .is_some()
            {
                return Err(UcError::new(
                    ErrorCode::ResourceAlreadyExists,
                    format!("Function '{}' already exists", row.name),
                ));
            }
            let body = serde_json::to_value(&row)
                .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
            Ok((
                vec![Action::Upsert {
                    kind: EntityKind::Function,
                    id: row.id,
                    body,
                }],
                row.clone(),
            ))
        })
        .await
}

/// One commit for the whole parameter list rather than the SQL's
/// insert-per-parameter loop.
pub async fn insert_params(store: &Store, params: &[FunctionParamRow]) -> Result<(), UcError> {
    let params = params.to_vec();
    store
        .commit("ADD FUNCTION PARAMS", |_| {
            let mut actions = Vec::with_capacity(params.len());
            for p in &params {
                let body = serde_json::to_value(p)
                    .map_err(|e| UcError::new(ErrorCode::Internal, e.to_string()))?;
                actions.push(Action::Upsert {
                    kind: EntityKind::FunctionParameter,
                    id: p.id,
                    body,
                });
            }
            Ok((actions, ()))
        })
        .await
}

pub async fn get_by_schema_and_name(
    store: &Store,
    schema_id: Uuid,
    name: &str,
) -> Result<FunctionRow, UcError> {
    let snap = store.snapshot().await;
    snap.get_by_natural_key(EntityKind::Function, &nk(schema_id, name))
        .ok_or_else(|| {
            UcError::new(
                ErrorCode::NotFound,
                format!("Function '{}' not found", name),
            )
        })
        .and_then(fn_of)
}

/// uc_function_parameters has no UNIQUE constraint, so there is no natural-key
/// index to ride on and this scans. Parameter counts are small and bounded by
/// the function signature; a secondary index would cost more than it saves.
pub async fn get_params(
    store: &Store,
    function_id: Uuid,
) -> Result<(Vec<FunctionParamRow>, Vec<FunctionParamRow>), UcError> {
    let snap = store.snapshot().await;
    let mut all: Vec<FunctionParamRow> = snap
        .iter(EntityKind::FunctionParameter)
        .map(param_of)
        .collect::<Result<Vec<_>, _>>()?;
    all.retain(|p| p.function_id == function_id);
    // ORDER BY ordinal_position. Hash iteration order is arbitrary, so this
    // sort is what makes the result deterministic, not a nicety.
    all.sort_by_key(|p| p.ordinal_position);
    let (input, ret): (Vec<_>, Vec<_>) = all.into_iter().partition(|p| p.input_or_return == 0);
    Ok((input, ret))
}

pub async fn list(
    store: &Store,
    schema_id: Uuid,
    page_token: Option<&str>,
    max_results: i64,
) -> Result<(Vec<FunctionRow>, Option<String>), UcError> {
    let snap = store.snapshot().await;
    let found = snap.scan_prefix(
        EntityKind::Function,
        &prefix(schema_id),
        page_token,
        crate::pagination::over_fetch(max_results),
    );
    let rows: Vec<FunctionRow> = found.into_iter().map(fn_of).collect::<Result<_, _>>()?;
    let (rows, next) = crate::pagination::page(rows, max_results, |r| r.name.clone());
    Ok((rows, next))
}

/// Drops the function and its parameters in one commit.
///
/// The SQL deletes parameters first, then the function, then reports NotFound
/// if the function was absent — so a call for a missing function still deletes
/// any orphaned parameters and *then* errors, leaving a partial effect behind.
/// Here the error abandons the whole commit, so nothing is written. That is a
/// behaviour change, and the better one.
pub async fn delete(store: &Store, id: Uuid) -> Result<(), UcError> {
    store
        .commit("DROP FUNCTION", |snap| {
            if snap.get(EntityKind::Function, id).is_none() {
                return Err(UcError::new(
                    ErrorCode::NotFound,
                    format!("Function '{}' not found", id),
                ));
            }
            let mut actions = vec![Action::Remove {
                kind: EntityKind::Function,
                id,
            }];
            for p in snap.iter(EntityKind::FunctionParameter) {
                let param = param_of(p)?;
                if param.function_id == id {
                    actions.push(Action::Remove {
                        kind: EntityKind::FunctionParameter,
                        id: param.id,
                    });
                }
            }
            Ok((actions, ()))
        })
        .await
}
