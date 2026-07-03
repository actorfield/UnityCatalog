use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Extension, Json,
};
use std::sync::Arc;
use uc_auth::UcClaims;
use uc_db::repos::{catalog, property};
use uc_errors::UcError;
use uc_openapi::catalog::{CatalogInfo, CreateCatalog, ListCatalogsResponse, UpdateCatalog};
use uc_types::Privilege;
use uuid::Uuid;

use crate::{
    catalog_api::helpers::{get_user, validate_sql_name},
    state::AppState,
};

#[derive(serde::Deserialize)]
pub struct ListParams {
    pub max_results: Option<i64>,
    pub page_token: Option<String>,
}

pub async fn create(
    State(state): State<AppState>,
    Extension(claims): Extension<Arc<UcClaims>>,
    Json(req): Json<CreateCatalog>,
) -> Result<Json<CatalogInfo>, UcError> {
    // Auth: caller needs CREATE_CATALOG on the metastore
    if state.auth_enabled {
        let user = get_user(&state, &claims.sub).await?;
        let allowed = state
            .authorizer
            .authorize(user.id, state.metastore_id, Privilege::CreateCatalog)
            .await?;
        if !allowed {
            return Err(UcError::permission_denied(
                "CREATE CATALOG privilege required on metastore",
            ));
        }
    }

    validate_sql_name(&req.name)?;
    let id = Uuid::new_v4();
    let now = chrono::Utc::now().timestamp_millis();
    let creator = if state.auth_enabled {
        Some(claims.sub.as_str())
    } else {
        None
    };

    let row = catalog::create(
        &state.pool,
        id,
        &req.name,
        req.comment.as_deref(),
        None,
        creator,
        req.storage_root.as_deref(),
        now,
    )
    .await?;

    // Store properties if provided
    if let Some(ref props) = req.properties {
        property::replace(&state.pool, id, "catalog", props).await?;
    }

    // Grant OWNER to the creator
    if state.auth_enabled {
        if let Ok(user) = get_user(&state, &claims.sub).await {
            state
                .authorizer
                .grant(user.id, id, Privilege::Owner)
                .await?;
            // Catalog is a child of the metastore in the hierarchy
            state
                .authorizer
                .add_hierarchy_child(state.metastore_id, id)
                .await?;
        }
    }

    let props = property::get_for_entity(&state.pool, id, "catalog")
        .await
        .ok();

    Ok(Json(CatalogInfo {
        name: row.name,
        comment: row.comment,
        properties: props,
        owner: row.owner,
        created_at: Some(row.created_at),
        created_by: row.created_by,
        updated_at: row.updated_at,
        updated_by: row.updated_by,
        id: Some(row.id),
        storage_root: row.storage_root,
        storage_location: row.storage_location,
    }))
}

pub async fn list(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> Result<Json<ListCatalogsResponse>, UcError> {
    let max = params.max_results.unwrap_or(50).min(1000);
    let (rows, next_token) = catalog::list(&state.pool, params.page_token.as_deref(), max).await?;

    let catalogs = rows
        .into_iter()
        .map(|r| CatalogInfo {
            name: r.name,
            comment: r.comment,
            properties: None, // Properties loaded on demand per-catalog for list
            owner: r.owner,
            created_at: Some(r.created_at),
            created_by: r.created_by,
            updated_at: r.updated_at,
            updated_by: r.updated_by,
            id: Some(r.id),
            storage_root: r.storage_root,
            storage_location: r.storage_location,
        })
        .collect();

    Ok(Json(ListCatalogsResponse {
        catalogs,
        next_page_token: next_token,
    }))
}

pub async fn get(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<CatalogInfo>, UcError> {
    let row = catalog::get_by_name(&state.pool, &name).await?;
    let props = property::get_for_entity(&state.pool, row.id, "catalog")
        .await
        .ok();

    Ok(Json(CatalogInfo {
        name: row.name,
        comment: row.comment,
        properties: props,
        owner: row.owner,
        created_at: Some(row.created_at),
        created_by: row.created_by,
        updated_at: row.updated_at,
        updated_by: row.updated_by,
        id: Some(row.id),
        storage_root: row.storage_root,
        storage_location: row.storage_location,
    }))
}

pub async fn update(
    State(state): State<AppState>,
    Extension(claims): Extension<Arc<UcClaims>>,
    Path(name): Path<String>,
    Json(req): Json<UpdateCatalog>,
) -> Result<Json<CatalogInfo>, UcError> {
    let existing = catalog::get_by_name(&state.pool, &name).await?;

    // Auth: OWNER on this catalog
    if state.auth_enabled {
        let user = get_user(&state, &claims.sub).await?;
        let allowed = state
            .authorizer
            .authorize(user.id, existing.id, Privilege::Owner)
            .await?;
        if !allowed {
            return Err(UcError::permission_denied(
                "OWNER privilege required on catalog",
            ));
        }
    }

    let now = chrono::Utc::now().timestamp_millis();
    let updater = if state.auth_enabled {
        Some(claims.sub.as_str())
    } else {
        None
    };

    let row = catalog::update(
        &state.pool,
        &name,
        req.new_name.as_deref(),
        req.comment.as_deref(),
        req.owner.as_deref(),
        updater,
        now,
    )
    .await?;

    if let Some(ref props) = req.properties {
        if !props.is_empty() {
            property::replace(&state.pool, row.id, "catalog", props).await?;
        }
    }

    let props = property::get_for_entity(&state.pool, row.id, "catalog")
        .await
        .ok();

    Ok(Json(CatalogInfo {
        name: row.name,
        comment: row.comment,
        properties: props,
        owner: row.owner,
        created_at: Some(row.created_at),
        created_by: row.created_by,
        updated_at: row.updated_at,
        updated_by: row.updated_by,
        id: Some(row.id),
        storage_root: row.storage_root,
        storage_location: row.storage_location,
    }))
}

#[derive(serde::Deserialize)]
pub struct DeleteParams {
    pub force: Option<bool>,
}

pub async fn delete(
    State(state): State<AppState>,
    Extension(claims): Extension<Arc<UcClaims>>,
    Path(name): Path<String>,
    Query(params): Query<DeleteParams>,
) -> Result<StatusCode, UcError> {
    let existing = catalog::get_by_name(&state.pool, &name).await?;

    if state.auth_enabled {
        let user = get_user(&state, &claims.sub).await?;
        let allowed = state
            .authorizer
            .authorize(user.id, existing.id, Privilege::Owner)
            .await?;
        if !allowed {
            return Err(UcError::permission_denied(
                "OWNER privilege required on catalog",
            ));
        }
    }

    let force = params.force.unwrap_or(false);

    // Check for child schemas before deletion
    let (schemas, _) = uc_db::repos::schema::list(&state.pool, existing.id, None, 1).await?;
    if !schemas.is_empty() {
        if !force {
            return Err(UcError::new(
                uc_errors::ErrorCode::FailedPrecondition,
                format!(
                    "Catalog '{}' is not empty. Use force=true to force deletion.",
                    name
                ),
            ));
        }
        // force=true: delete all child schemas (with their children)
        let (all_schemas, _) =
            uc_db::repos::schema::list(&state.pool, existing.id, None, 10000).await?;
        for schema in all_schemas {
            delete_schema_children(&state.pool, schema.id).await?;
            property::delete_for_entity(&state.pool, schema.id, "schema").await?;
            state
                .authorizer
                .remove_hierarchy_children(schema.id)
                .await?;
            uc_db::repos::schema::delete(&state.pool, schema.id).await?;
        }
    }

    // Remove properties first
    property::delete_for_entity(&state.pool, existing.id, "catalog").await?;
    // Remove hierarchy
    state
        .authorizer
        .remove_hierarchy_children(existing.id)
        .await?;
    // Delete the catalog row
    catalog::delete(&state.pool, &name).await?;

    Ok(StatusCode::OK)
}

/// Delete all children of a schema (tables, volumes, functions, models) without deleting the schema itself.
async fn delete_schema_children(
    pool: &uc_db::AnyPool,
    schema_id: uuid::Uuid,
) -> Result<(), UcError> {
    use uc_db::repos::{function, model, table, volume};

    // Delete tables (with columns and properties)
    let (tables, _) = table::list(pool, schema_id, None, 10000).await?;
    for t in tables {
        table::delete_columns(pool, t.id).await?;
        uc_db::repos::property::delete_for_entity(pool, t.id, "table").await?;
        table::delete(pool, t.id).await?;
    }
    // Delete volumes
    let (volumes, _) = volume::list(pool, schema_id, None, 10000).await?;
    for v in volumes {
        volume::delete(pool, v.id).await?;
    }
    // Delete functions
    let (funcs, _) = function::list(pool, schema_id, None, 10000).await?;
    for f in funcs {
        function::delete(pool, f.id).await?;
    }
    // Delete registered models (with versions)
    let (models, _) = model::list_models(pool, schema_id, None, 10000).await?;
    for m in models {
        model::delete_model(pool, m.id).await?;
    }
    Ok(())
}
