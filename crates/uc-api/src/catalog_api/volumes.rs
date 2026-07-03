use crate::{catalog_api::helpers::*, state::AppState};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Extension, Json,
};
use std::sync::Arc;
use uc_auth::UcClaims;
use uc_db::{
    managed_storage,
    models::volume::VolumeRow,
    repos::{property, schema, volume},
};
use uc_errors::UcError;
use uc_openapi::catalog::{
    CreateVolumeRequestContent, ListVolumesResponseContent, UpdateVolumeRequestContent, VolumeInfo,
    VolumeType,
};
use uc_types::Privilege;
use uuid::Uuid;

#[derive(serde::Deserialize)]
pub struct ListParams {
    pub catalog_name: String,
    pub schema_name: String,
    pub max_results: Option<i64>,
    pub page_token: Option<String>,
}

pub async fn create(
    State(state): State<AppState>,
    Extension(claims): Extension<Arc<UcClaims>>,
    Json(req): Json<CreateVolumeRequestContent>,
) -> Result<Json<VolumeInfo>, UcError> {
    let schema = schema::get_by_full_name(&state.pool, &req.catalog_name, &req.schema_name).await?;
    if state.auth_enabled {
        let user = get_user(&state, &claims.sub).await?;
        require(&state, user.id, schema.id, Privilege::CreateVolume).await?;
    }
    validate_sql_name(&req.name)?;
    let id = Uuid::new_v4();
    let now = now_ms();
    // #1143: MANAGED volumes auto-derive storage_location from storage_root hierarchy
    let storage_location = match req.volume_type {
        VolumeType::Managed if req.storage_location.is_none() => {
            match managed_storage::resolve_storage_root(
                &state.pool,
                &req.catalog_name,
                &req.schema_name,
            )
            .await
            {
                Ok(root) => Some(managed_storage::managed_volume_location(
                    &root, schema.id, id,
                )),
                Err(_) => None,
            }
        }
        _ => req.storage_location.clone(),
    };
    let row = VolumeRow {
        id,
        schema_id: schema.id,
        name: req.name.clone(),
        comment: req.comment.clone(),
        storage_location,
        owner: None,
        created_at: now,
        created_by: auth_sub(&state, &claims).map(String::from),
        updated_at: None,
        updated_by: None,
        volume_type: format!("{:?}", req.volume_type).to_uppercase(),
    };
    let created = volume::create(&state.pool, &row).await?;
    if let Some(ref props) = req.properties {
        property::replace(&state.pool, id, "volume", props).await?;
    }
    if state.auth_enabled {
        if let Ok(user) = get_user(&state, &claims.sub).await {
            state
                .authorizer
                .grant(user.id, id, Privilege::Owner)
                .await?;
            state.authorizer.add_hierarchy_child(schema.id, id).await?;
        }
    }
    let props = property::get_for_entity(&state.pool, id, "volume")
        .await
        .ok();
    Ok(Json(to_volume_info(
        created,
        &req.catalog_name,
        &req.schema_name,
        props,
    )))
}

pub async fn list(
    State(state): State<AppState>,
    Extension(claims): Extension<Arc<UcClaims>>,
    Query(params): Query<ListParams>,
) -> Result<Json<ListVolumesResponseContent>, UcError> {
    let schema =
        schema::get_by_full_name(&state.pool, &params.catalog_name, &params.schema_name).await?;
    let max = params.max_results.unwrap_or(50).min(1000);
    let (rows, next_token) =
        volume::list(&state.pool, schema.id, params.page_token.as_deref(), max).await?;
    // #1105: filter to only volumes the caller can see when auth is enabled
    let principal = if state.auth_enabled {
        get_user(&state, &claims.sub).await.ok().map(|u| u.id)
    } else {
        None
    };
    let visible_ids: std::collections::HashSet<uuid::Uuid> = if state.auth_enabled {
        crate::catalog_api::helpers::filter_visible(
            &state,
            principal,
            rows.iter().map(|r| (r.id, ())).collect(),
            uc_types::Privilege::ReadVolume,
        )
        .await?
        .into_iter()
        .collect()
    } else {
        rows.iter().map(|r| r.id).collect()
    };
    let volumes = rows
        .into_iter()
        .filter(|r| visible_ids.contains(&r.id))
        .map(|r| to_volume_info(r, &params.catalog_name, &params.schema_name, None))
        .collect();
    Ok(Json(ListVolumesResponseContent {
        volumes,
        next_page_token: next_token,
    }))
}

pub async fn get(
    State(state): State<AppState>,
    Path(full_name): Path<String>,
) -> Result<Json<VolumeInfo>, UcError> {
    let (cat, sch, vol) = split3(&full_name)?;
    let schema = schema::get_by_full_name(&state.pool, cat, sch).await?;
    let row = volume::get_by_schema_and_name(&state.pool, schema.id, vol).await?;
    let props = property::get_for_entity(&state.pool, row.id, "volume")
        .await
        .ok();
    Ok(Json(to_volume_info(row, cat, sch, props)))
}

pub async fn update(
    State(state): State<AppState>,
    Extension(claims): Extension<Arc<UcClaims>>,
    Path(full_name): Path<String>,
    Json(req): Json<UpdateVolumeRequestContent>,
) -> Result<Json<VolumeInfo>, UcError> {
    let (cat, sch, vol) = split3(&full_name)?;
    let schema = schema::get_by_full_name(&state.pool, cat, sch).await?;
    let existing = volume::get_by_schema_and_name(&state.pool, schema.id, vol).await?;
    if state.auth_enabled {
        let user = get_user(&state, &claims.sub).await?;
        require(&state, user.id, existing.id, Privilege::Owner).await?;
    }
    let row = volume::update(
        &state.pool,
        existing.id,
        req.new_name.as_deref(),
        req.comment.as_deref(),
        req.owner.as_deref(),
        now_ms(),
        auth_sub(&state, &claims),
    )
    .await?;
    if let Some(ref props) = req.properties {
        property::replace(&state.pool, existing.id, "volume", props).await?;
    }
    let props = property::get_for_entity(&state.pool, existing.id, "volume")
        .await
        .ok();
    Ok(Json(to_volume_info(row, cat, sch, props)))
}

pub async fn delete(
    State(state): State<AppState>,
    Extension(claims): Extension<Arc<UcClaims>>,
    Path(full_name): Path<String>,
) -> Result<StatusCode, UcError> {
    let (cat, sch, vol) = split3(&full_name)?;
    let schema = schema::get_by_full_name(&state.pool, cat, sch).await?;
    let existing = volume::get_by_schema_and_name(&state.pool, schema.id, vol).await?;
    if state.auth_enabled {
        let user = get_user(&state, &claims.sub).await?;
        require(&state, user.id, existing.id, Privilege::Owner).await?;
    }
    state
        .authorizer
        .remove_hierarchy_children(existing.id)
        .await?;
    property::delete_for_entity(&state.pool, existing.id, "volume").await?;
    volume::delete(&state.pool, existing.id).await?;
    Ok(StatusCode::OK)
}

fn normalize_loc(url: Option<String>) -> Option<String> {
    url.map(|u| {
        if u.starts_with('/') {
            format!("file://{}", u)
        } else {
            u
        }
    })
}

fn to_volume_info(
    r: VolumeRow,
    cat: &str,
    sch: &str,
    props: Option<std::collections::HashMap<String, String>>,
) -> VolumeInfo {
    let vt = if r.volume_type == "MANAGED" {
        VolumeType::Managed
    } else {
        VolumeType::External
    };
    VolumeInfo {
        catalog_name: cat.to_string(),
        schema_name: sch.to_string(),
        name: r.name.clone(),
        comment: r.comment,
        owner: r.owner,
        created_at: Some(r.created_at),
        created_by: r.created_by,
        updated_at: r.updated_at,
        updated_by: r.updated_by,
        volume_id: Some(r.id),
        volume_type: vt,
        storage_location: normalize_loc(r.storage_location),
        properties: props,
        full_name: Some(format!("{}.{}.{}", cat, sch, r.name)),
    }
}
