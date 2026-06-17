use axum::{extract::{Path, Query, State}, http::StatusCode, Extension, Json};
use std::sync::Arc;
use uc_auth::UcClaims;
use uc_db::{managed_storage, models::model::{ModelVersionRow, RegisteredModelRow}, repos::{ModelRepo, SchemaRepo, UserRepo}};
use uc_errors::UcError;
use uc_openapi::catalog::{CreateModelVersion, CreateRegisteredModel, FinalizeModelVersion, ListModelVersionsResponse, ListRegisteredModelsResponse, ModelVersionInfo, ModelVersionStatus, RegisteredModelInfo, UpdateModelVersion, UpdateRegisteredModel};
use uc_types::Privilege;
use uuid::Uuid;
use crate::{catalog_api::helpers::*, state::AppState};

#[derive(serde::Deserialize)]
pub struct ListModelsParams { pub catalog_name: Option<String>, pub schema_name: Option<String>, pub max_results: Option<i64>, pub page_token: Option<String> }

pub async fn create_model(State(state): State<AppState>, Extension(claims): Extension<Arc<UcClaims>>, Json(req): Json<CreateRegisteredModel>) -> Result<Json<RegisteredModelInfo>, UcError> {
    let schema = SchemaRepo::get_by_full_name(&state.pool, &req.catalog_name, &req.schema_name).await?;
    if state.auth_enabled {
        let user = get_user(&state, &claims.sub).await?;
        require_any(&state, user.id, schema.id, &[Privilege::Owner, Privilege::CreateModel]).await?;
    }
    validate_sql_name(&req.name)?;
    let id = Uuid::new_v4(); let now = now_ms();
    // #1143: auto-derive model storage location from storage_root if not provided
    let model_storage = match req.storage_location {
        Some(ref loc) => Some(loc.clone()),
        None => match managed_storage::resolve_storage_root(&state.pool, &req.catalog_name, &req.schema_name).await {
            Ok(root) => Some(managed_storage::managed_model_location(&root, schema.id, id)),
            Err(_) => None,
        },
    };
    let row = RegisteredModelRow { id, schema_id: schema.id, name: req.name.clone(), owner: None,
        created_at: Some(now), created_by: auth_sub(&state, &claims).map(String::from),
        updated_at: None, updated_by: None, comment: req.comment.clone(), url: model_storage,
        max_version_number: Some(0) };
    let created = ModelRepo::create_model(&state.pool, &row).await?;
    if state.auth_enabled {
        if let Some(user) = UserRepo::get_by_email(&state.pool, &claims.sub).await? {
            state.authorizer.grant(user.id, id, Privilege::Owner).await?;
            state.authorizer.add_hierarchy_child(schema.id, id).await?;
        }
    }
    Ok(Json(to_model_info(created, &req.catalog_name, &req.schema_name)))
}

pub async fn list_models(State(state): State<AppState>, Extension(claims): Extension<Arc<UcClaims>>, Query(params): Query<ListModelsParams>) -> Result<Json<ListRegisteredModelsResponse>, UcError> {
    let cat = params.catalog_name.as_deref().unwrap_or("");
    let sch = params.schema_name.as_deref().unwrap_or("");
    let schema = SchemaRepo::get_by_full_name(&state.pool, cat, sch).await?;
    let max = params.max_results.unwrap_or(50).min(1000);
    let (rows, next_token) = ModelRepo::list_models(&state.pool, schema.id, params.page_token.as_deref(), max).await?;
    // #1105: filter to only models the caller can see when auth is enabled
    let principal = if state.auth_enabled {
        uc_db::repos::UserRepo::get_by_email(&state.pool, &claims.sub).await?.map(|u| u.id)
    } else { None };
    let visible_ids: std::collections::HashSet<uuid::Uuid> = if state.auth_enabled {
        crate::catalog_api::helpers::filter_visible(&state, principal,
            rows.iter().map(|r| (r.id, ())).collect(),
            &[uc_types::Privilege::Owner, uc_types::Privilege::Select]).await?.into_iter().collect()
    } else {
        rows.iter().map(|r| r.id).collect()
    };
    let registered_models = rows.into_iter().filter(|r| visible_ids.contains(&r.id)).map(|r| to_model_info(r, cat, sch)).collect();
    Ok(Json(ListRegisteredModelsResponse { registered_models, next_page_token: next_token }))
}

pub async fn get_model(State(state): State<AppState>, Path(full_name): Path<String>) -> Result<Json<RegisteredModelInfo>, UcError> {
    let (cat, sch, mdl) = split3(&full_name)?;
    let schema = SchemaRepo::get_by_full_name(&state.pool, cat, sch).await?;
    let row = ModelRepo::get_model_by_schema_and_name(&state.pool, schema.id, mdl).await?;
    Ok(Json(to_model_info(row, cat, sch)))
}

pub async fn update_model(State(state): State<AppState>, Extension(claims): Extension<Arc<UcClaims>>, Path(full_name): Path<String>, Json(req): Json<UpdateRegisteredModel>) -> Result<Json<RegisteredModelInfo>, UcError> {
    let (cat, sch, mdl) = split3(&full_name)?;
    let schema = SchemaRepo::get_by_full_name(&state.pool, cat, sch).await?;
    let existing = ModelRepo::get_model_by_schema_and_name(&state.pool, schema.id, mdl).await?;
    if state.auth_enabled {
        let user = get_user(&state, &claims.sub).await?;
        require_any(&state, user.id, existing.id, &[Privilege::Owner]).await?;
    }
    let now = now_ms();
    if let Some(ref new_name) = req.new_name { validate_sql_name(new_name)?; }
    sqlx::query(
        "UPDATE uc_registered_models SET name=COALESCE($1,name), comment=COALESCE($2,comment), owner=COALESCE($3,owner), updated_at=$4, updated_by=$5 WHERE id=$6"
    )
    .bind(req.new_name.as_deref())
    .bind(req.comment.as_deref())
    .bind(req.owner.as_deref())
    .bind(now)
    .bind(auth_sub(&state, &claims))
    .bind(existing.id)
    .execute(state.pool.as_ref())
    .await.map_err(crate::db_err)?;
    let effective_name = req.new_name.as_deref().unwrap_or(mdl);
    let updated = ModelRepo::get_model_by_schema_and_name(&state.pool, schema.id, effective_name).await
        .unwrap_or_else(|_| existing.clone());
    Ok(Json(to_model_info(updated, cat, sch)))
}

pub async fn delete_model(State(state): State<AppState>, Extension(claims): Extension<Arc<UcClaims>>, Path(full_name): Path<String>) -> Result<StatusCode, UcError> {
    let (cat, sch, mdl) = split3(&full_name)?;
    let schema = SchemaRepo::get_by_full_name(&state.pool, cat, sch).await?;
    let existing = ModelRepo::get_model_by_schema_and_name(&state.pool, schema.id, mdl).await?;
    if state.auth_enabled {
        let user = get_user(&state, &claims.sub).await?;
        require_any(&state, user.id, existing.id, &[Privilege::Owner]).await?;
    }
    ModelRepo::delete_model(&state.pool, existing.id).await?;
    Ok(StatusCode::OK)
}

pub async fn create_version(State(state): State<AppState>, Extension(claims): Extension<Arc<UcClaims>>, Json(req): Json<CreateModelVersion>) -> Result<Json<ModelVersionInfo>, UcError> {
    let schema = SchemaRepo::get_by_full_name(&state.pool, &req.catalog_name, &req.schema_name).await?;
    let model = ModelRepo::get_model_by_schema_and_name(&state.pool, schema.id, &req.model_name).await?;
    let next_ver = model.max_version_number.unwrap_or(0) + 1;
    let id = Uuid::new_v4(); let now = now_ms();
    // #1143: derive version storage location from model storage location
    let version_url = model.url.as_deref().map(|model_loc|
        managed_storage::managed_model_version_location(model_loc, next_ver)
    );
    let row = ModelVersionRow { id, registered_model_id: model.id, version: Some(next_ver),
        source: req.source.clone(), run_id: req.run_id.clone(), status: Some("PENDING_REGISTRATION".into()),
        owner: None, created_at: Some(now), created_by: auth_sub(&state, &claims).map(String::from),
        updated_at: None, updated_by: None, comment: req.comment.clone(), url: version_url };
    let created = ModelRepo::create_version(&state.pool, &row).await?;
    // Update max_version_number on the parent model
    sqlx::query("UPDATE uc_registered_models SET max_version_number=$1 WHERE id=$2")
        .bind(next_ver).bind(model.id)
        .execute(state.pool.as_ref()).await.map_err(crate::db_err)?;
    Ok(Json(to_version_info(created, &req.catalog_name, &req.schema_name, &req.model_name)))
}

pub async fn list_versions(State(state): State<AppState>, Path(full_name): Path<String>) -> Result<Json<ListModelVersionsResponse>, UcError> {
    let (cat, sch, mdl) = split3(&full_name)?;
    let schema = SchemaRepo::get_by_full_name(&state.pool, cat, sch).await?;
    let model = ModelRepo::get_model_by_schema_and_name(&state.pool, schema.id, mdl).await?;
    let rows: Vec<ModelVersionRow> = sqlx::query_as::<_, ModelVersionRow>(
        "SELECT * FROM uc_model_versions WHERE registered_model_id=$1 ORDER BY version"
    )
    .bind(model.id)
    .fetch_all(state.pool.as_ref())
    .await.map_err(crate::db_err)?;
    let model_versions = rows.into_iter().map(|r| to_version_info(r, cat, sch, mdl)).collect();
    Ok(Json(ListModelVersionsResponse { model_versions, next_page_token: None }))
}

pub async fn get_version(State(state): State<AppState>, Path((full_name, version)): Path<(String, String)>) -> Result<Json<ModelVersionInfo>, UcError> {
    let (cat, sch, mdl) = split3(&full_name)?;
    let schema = SchemaRepo::get_by_full_name(&state.pool, cat, sch).await?;
    let model = ModelRepo::get_model_by_schema_and_name(&state.pool, schema.id, mdl).await?;
    let ver: i32 = version.parse().map_err(|_| UcError::invalid_argument("version must be an integer"))?;
    let row = ModelRepo::get_version(&state.pool, model.id, ver).await?;
    Ok(Json(to_version_info(row, cat, sch, mdl)))
}

pub async fn update_version(State(state): State<AppState>, Extension(claims): Extension<Arc<UcClaims>>, Path((full_name, version)): Path<(String, String)>, Json(req): Json<UpdateModelVersion>) -> Result<Json<ModelVersionInfo>, UcError> {
    let (cat, sch, mdl) = split3(&full_name)?;
    let schema = SchemaRepo::get_by_full_name(&state.pool, cat, sch).await?;
    let model = ModelRepo::get_model_by_schema_and_name(&state.pool, schema.id, mdl).await?;
    let ver: i32 = version.parse().map_err(|_| UcError::invalid_argument("version must be an integer"))?;
    let row = ModelRepo::get_version(&state.pool, model.id, ver).await?;
    if state.auth_enabled {
        let user = get_user(&state, &claims.sub).await?;
        require_any(&state, user.id, model.id, &[Privilege::Owner]).await?;
    }
    let now = now_ms();
    sqlx::query("UPDATE uc_model_versions SET comment=COALESCE($1,comment), updated_at=$2, updated_by=$3 WHERE id=$4")
        .bind(req.comment.as_deref())
        .bind(now)
        .bind(auth_sub(&state, &claims))
        .bind(row.id)
        .execute(state.pool.as_ref()).await.map_err(crate::db_err)?;
    let updated = ModelRepo::get_version(&state.pool, model.id, ver).await?;
    Ok(Json(to_version_info(updated, cat, sch, mdl)))
}

pub async fn delete_version(State(state): State<AppState>, Path((full_name, version)): Path<(String, String)>) -> Result<StatusCode, UcError> {
    let (cat, sch, mdl) = split3(&full_name)?;
    let schema = SchemaRepo::get_by_full_name(&state.pool, cat, sch).await?;
    let model = ModelRepo::get_model_by_schema_and_name(&state.pool, schema.id, mdl).await?;
    let ver: i32 = version.parse().map_err(|_| UcError::invalid_argument("version must be an integer"))?;
    ModelRepo::delete_version(&state.pool, model.id, ver).await?;
    Ok(StatusCode::OK)
}

pub async fn finalize_version(State(state): State<AppState>, Path((full_name, version)): Path<(String, String)>, Json(req): Json<FinalizeModelVersion>) -> Result<Json<ModelVersionInfo>, UcError> {
    let (cat, sch, mdl) = split3(&full_name)?;
    let schema = SchemaRepo::get_by_full_name(&state.pool, cat, sch).await?;
    let model = ModelRepo::get_model_by_schema_and_name(&state.pool, schema.id, mdl).await?;
    let ver: i32 = version.parse().map_err(|_| UcError::invalid_argument("version must be an integer"))?;
    let row = ModelRepo::get_version(&state.pool, model.id, ver).await?;
    // Update status via raw query
    let status_str = match req.status {
        ModelVersionStatus::Ready => "READY",
        ModelVersionStatus::PendingRegistration => "PENDING_REGISTRATION",
        ModelVersionStatus::FailedRegistration => "FAILED_REGISTRATION",
        ModelVersionStatus::ModelVersionStatusUnknown => "MODEL_VERSION_STATUS_UNKNOWN",
    };
    sqlx::query("UPDATE uc_model_versions SET status=$1 WHERE id=$2")
        .bind(status_str)
        .bind(row.id)
        .execute(state.pool.as_ref())
        .await.map_err(crate::db_err)?;
    let updated = ModelRepo::get_version(&state.pool, model.id, ver).await?;
    Ok(Json(to_version_info(updated, cat, sch, mdl)))
}

fn to_model_info(r: RegisteredModelRow, cat: &str, sch: &str) -> RegisteredModelInfo {
    RegisteredModelInfo { name: r.name.clone(), catalog_name: cat.to_string(), schema_name: sch.to_string(),
        storage_location: r.url, full_name: Some(format!("{}.{}.{}", cat, sch, r.name)), comment: r.comment,
        owner: r.owner, created_at: r.created_at, created_by: r.created_by, updated_at: r.updated_at,
        updated_by: r.updated_by, id: Some(r.id) }
}

fn parse_model_status(s: Option<&str>) -> Option<ModelVersionStatus> {
    match s {
        Some("READY") => Some(ModelVersionStatus::Ready),
        Some("PENDING_REGISTRATION") => Some(ModelVersionStatus::PendingRegistration),
        Some("FAILED_REGISTRATION") => Some(ModelVersionStatus::FailedRegistration),
        _ => Some(ModelVersionStatus::ModelVersionStatusUnknown),
    }
}

fn to_version_info(r: ModelVersionRow, cat: &str, sch: &str, mdl: &str) -> ModelVersionInfo {
    ModelVersionInfo { model_name: Some(mdl.to_string()), catalog_name: Some(cat.to_string()),
        schema_name: Some(sch.to_string()), version: r.version.map(|v| v as i64), source: r.source,
        run_id: r.run_id, status: parse_model_status(r.status.as_deref()), storage_location: r.url,
        comment: r.comment, created_at: r.created_at, created_by: r.created_by,
        updated_at: r.updated_at, updated_by: r.updated_by, id: Some(r.id) }
}
