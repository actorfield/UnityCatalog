use axum::{extract::{Path, Query, State}, http::StatusCode, Extension, Json};
use std::sync::Arc;
use uc_auth::UcClaims;
use uc_db::{models::function::{FunctionParamRow, FunctionRow}, repos::{FunctionRepo, SchemaRepo}};
use uc_errors::UcError;
use uc_openapi::catalog::{ColumnTypeName, CreateFunctionRequest, FunctionInfo, FunctionParameterInfo, FunctionParameterInfos, ListFunctionsResponse};
use uc_types::Privilege;
use uuid::Uuid;
use crate::{catalog_api::helpers::*, state::AppState};

#[derive(serde::Deserialize)]
pub struct ListParams { pub catalog_name: String, pub schema_name: String, pub max_results: Option<i64>, pub page_token: Option<String> }

pub async fn create(State(state): State<AppState>, Extension(claims): Extension<Arc<UcClaims>>, Json(req): Json<CreateFunctionRequest>) -> Result<Json<FunctionInfo>, UcError> {
    let fi = req.function_info;
    let schema = SchemaRepo::get_by_full_name(&state.pool, &fi.catalog_name, &fi.schema_name).await?;
    if state.auth_enabled {
        let user = get_user(&state, &claims.sub).await?;
        require_any(&state, user.id, schema.id, &[Privilege::Owner, Privilege::CreateFunction]).await?;
    }
    validate_sql_name(&fi.name)?;
    let id = Uuid::new_v4(); let now = now_ms();
    let row = FunctionRow { id, schema_id: schema.id, name: fi.name.clone(), comment: fi.comment.clone(),
        owner: None, created_at: Some(now), created_by: auth_sub(&state, &claims).map(String::from),
        updated_at: None, updated_by: None, data_type: fi.data_type.as_ref().map(|t| format!("{:?}", t).to_uppercase()),
        full_data_type: fi.full_data_type.clone(), external_language: fi.external_language.clone(),
        is_deterministic: fi.is_deterministic, is_null_call: fi.is_null_call,
        parameter_style: fi.parameter_style.clone(), routine_body: fi.routine_body.clone(),
        routine_definition: fi.routine_definition.clone(), sql_data_access: fi.sql_data_access.clone(),
        security_type: fi.security_type.clone(), specific_name: fi.specific_name.clone() };
    let created = FunctionRepo::create(&state.pool, &row).await?;
    // Insert input params
    if let Some(ref ip) = fi.input_params {
        if let Some(ref params) = ip.parameters {
            let rows: Vec<FunctionParamRow> = params.iter().enumerate().map(|(i, p)| FunctionParamRow {
                id: Uuid::new_v4(), function_id: id, name: p.name.clone(), input_or_return: 0,
                ordinal_position: i as i32, type_text: p.type_text.clone(), type_json: p.type_json.clone(),
                type_name: p.type_name.as_ref().map(|t| format!("{:?}", t).to_uppercase()),
                type_precision: p.type_precision, type_scale: p.type_scale,
                type_interval_type: p.type_interval_type.clone(), comment: p.comment.clone(),
                parameter_mode: p.parameter_mode.clone(), parameter_default: p.parameter_default.clone(),
            }).collect();
            FunctionRepo::insert_params(&state.pool, &rows).await?;
        }
    }
    if state.auth_enabled {
        if let Some(user) = uc_db::repos::UserRepo::get_by_email(&state.pool, &claims.sub).await? {
            state.authorizer.grant(user.id, id, Privilege::Owner).await?;
            state.authorizer.add_hierarchy_child(schema.id, id).await?;
        }
    }
    let (input, _ret) = FunctionRepo::get_params(&state.pool, id).await?;
    Ok(Json(to_function_info(created, &fi.catalog_name, &fi.schema_name, input)))
}

pub async fn list(State(state): State<AppState>, Extension(claims): Extension<Arc<UcClaims>>, Query(params): Query<ListParams>) -> Result<Json<ListFunctionsResponse>, UcError> {
    let schema = SchemaRepo::get_by_full_name(&state.pool, &params.catalog_name, &params.schema_name).await?;
    let max = params.max_results.unwrap_or(50).min(1000);
    let (rows, next_token) = FunctionRepo::list(&state.pool, schema.id, params.page_token.as_deref(), max).await?;
    // #1105: filter to only functions the caller can see when auth is enabled
    let principal = if state.auth_enabled {
        uc_db::repos::UserRepo::get_by_email(&state.pool, &claims.sub).await?.map(|u| u.id)
    } else { None };
    let visible_ids: std::collections::HashSet<uuid::Uuid> = if state.auth_enabled {
        crate::catalog_api::helpers::filter_visible(&state, principal,
            rows.iter().map(|r| (r.id, ())).collect(),
            &[uc_types::Privilege::Owner, uc_types::Privilege::Execute]).await?.into_iter().collect()
    } else {
        rows.iter().map(|r| r.id).collect()
    };
    let functions = rows.into_iter().filter(|r| visible_ids.contains(&r.id)).map(|r| to_function_info(r, &params.catalog_name, &params.schema_name, vec![])).collect();
    Ok(Json(ListFunctionsResponse { functions, next_page_token: next_token }))
}

pub async fn get(State(state): State<AppState>, Path(full_name): Path<String>) -> Result<Json<FunctionInfo>, UcError> {
    let (cat, sch, func) = split3(&full_name)?;
    let schema = SchemaRepo::get_by_full_name(&state.pool, cat, sch).await?;
    let row = FunctionRepo::get_by_schema_and_name(&state.pool, schema.id, func).await?;
    let (input, _ret) = FunctionRepo::get_params(&state.pool, row.id).await?;
    Ok(Json(to_function_info(row, cat, sch, input)))
}

pub async fn delete(State(state): State<AppState>, Extension(claims): Extension<Arc<UcClaims>>, Path(full_name): Path<String>) -> Result<StatusCode, UcError> {
    let (cat, sch, func) = split3(&full_name)?;
    let schema = SchemaRepo::get_by_full_name(&state.pool, cat, sch).await?;
    let existing = FunctionRepo::get_by_schema_and_name(&state.pool, schema.id, func).await?;
    if state.auth_enabled {
        let user = get_user(&state, &claims.sub).await?;
        require_any(&state, user.id, existing.id, &[Privilege::Owner]).await?;
    }
    FunctionRepo::delete(&state.pool, existing.id).await?;
    Ok(StatusCode::OK)
}

fn parse_col_type_name(s: &str) -> Option<ColumnTypeName> {
    match s {
        "BOOLEAN" => Some(ColumnTypeName::Boolean), "BYTE" => Some(ColumnTypeName::Byte),
        "SHORT" => Some(ColumnTypeName::Short), "INT" => Some(ColumnTypeName::Int),
        "LONG" => Some(ColumnTypeName::Long), "FLOAT" => Some(ColumnTypeName::Float),
        "DOUBLE" => Some(ColumnTypeName::Double), "DATE" => Some(ColumnTypeName::Date),
        "TIMESTAMP" => Some(ColumnTypeName::Timestamp), "TIMESTAMP_NTZ" => Some(ColumnTypeName::TimestampNtz),
        "STRING" => Some(ColumnTypeName::String), "BINARY" => Some(ColumnTypeName::Binary),
        "DECIMAL" => Some(ColumnTypeName::Decimal), "ARRAY" => Some(ColumnTypeName::Array),
        "STRUCT" => Some(ColumnTypeName::Struct), "MAP" => Some(ColumnTypeName::Map),
        "NULL" => Some(ColumnTypeName::Null),
        _ => None,
    }
}

fn to_function_info(r: FunctionRow, cat: &str, sch: &str, input: Vec<FunctionParamRow>) -> FunctionInfo {
    let params: Vec<FunctionParameterInfo> = input.into_iter().map(|p| {
        let type_name = p.type_name.as_deref().and_then(parse_col_type_name);
        FunctionParameterInfo {
            name: p.name, type_text: p.type_text, type_json: p.type_json, type_name,
            type_precision: p.type_precision, type_scale: p.type_scale,
            type_interval_type: p.type_interval_type, position: Some(p.ordinal_position),
            parameter_type: p.parameter_mode.as_ref().map(|_| "PARAM".to_string()), parameter_mode: p.parameter_mode,
            parameter_default: p.parameter_default, comment: p.comment,
        }
    }).collect();
    FunctionInfo { name: r.name.clone(), catalog_name: cat.to_string(), schema_name: sch.to_string(),
        input_params: Some(FunctionParameterInfos { parameters: Some(params) }), return_params: None,
        data_type: None, full_data_type: r.full_data_type, routine_body: r.routine_body,
        routine_definition: r.routine_definition, parameter_style: r.parameter_style,
        is_deterministic: r.is_deterministic, sql_data_access: r.sql_data_access,
        is_null_call: r.is_null_call, security_type: r.security_type, specific_name: r.specific_name,
        comment: r.comment, properties: None,
        full_name: Some(format!("{}.{}.{}", cat, sch, r.name)),
        owner: r.owner, created_at: r.created_at, created_by: r.created_by,
        updated_at: r.updated_at, updated_by: r.updated_by, function_id: Some(r.id),
        external_language: r.external_language }
}
