pub mod catalogs;
pub mod credentials;
pub mod delta_commits;
pub mod external_locations;
pub mod functions;
pub mod helpers;
pub mod metastore;
pub mod models;
pub mod permissions;
pub mod schemas;
pub mod staging_tables;
pub mod tables;
pub mod temp_credentials;
pub mod volumes;

use crate::{error_ext::inject_catalog_format, state::AppState};
use axum::{
    middleware,
    routing::{get, patch, post},
    Router,
};

pub fn router(state: AppState) -> Router {
    Router::new()
        // Catalogs
        .route(
            "/api/2.1/unity-catalog/catalogs",
            post(catalogs::create).get(catalogs::list),
        )
        .route(
            "/api/2.1/unity-catalog/catalogs/:name",
            get(catalogs::get)
                .patch(catalogs::update)
                .delete(catalogs::delete),
        )
        // Schemas
        .route(
            "/api/2.1/unity-catalog/schemas",
            post(schemas::create).get(schemas::list),
        )
        .route(
            "/api/2.1/unity-catalog/schemas/:full_name",
            get(schemas::get)
                .patch(schemas::update)
                .delete(schemas::delete),
        )
        // Tables
        .route(
            "/api/2.1/unity-catalog/tables",
            post(tables::create).get(tables::list),
        )
        .route(
            "/api/2.1/unity-catalog/tables/:full_name",
            get(tables::get).delete(tables::delete),
        )
        // Volumes
        .route(
            "/api/2.1/unity-catalog/volumes",
            post(volumes::create).get(volumes::list),
        )
        .route(
            "/api/2.1/unity-catalog/volumes/:name",
            get(volumes::get)
                .patch(volumes::update)
                .delete(volumes::delete),
        )
        // Functions
        .route(
            "/api/2.1/unity-catalog/functions",
            post(functions::create).get(functions::list),
        )
        .route(
            "/api/2.1/unity-catalog/functions/:name",
            get(functions::get).delete(functions::delete),
        )
        // Registered Models
        .route(
            "/api/2.1/unity-catalog/models",
            post(models::create_model).get(models::list_models),
        )
        .route(
            "/api/2.1/unity-catalog/models/:full_name",
            get(models::get_model)
                .patch(models::update_model)
                .delete(models::delete_model),
        )
        .route(
            "/api/2.1/unity-catalog/models/versions",
            post(models::create_version),
        )
        .route(
            "/api/2.1/unity-catalog/models/:full_name/versions",
            get(models::list_versions),
        )
        .route(
            "/api/2.1/unity-catalog/models/:full_name/versions/:version",
            get(models::get_version)
                .patch(models::update_version)
                .delete(models::delete_version),
        )
        .route(
            "/api/2.1/unity-catalog/models/:full_name/versions/:version/finalize",
            patch(models::finalize_version),
        )
        // Credentials
        .route(
            "/api/2.1/unity-catalog/credentials",
            post(credentials::create).get(credentials::list),
        )
        .route(
            "/api/2.1/unity-catalog/credentials/:name",
            get(credentials::get)
                .patch(credentials::update)
                .delete(credentials::delete),
        )
        // External Locations
        .route(
            "/api/2.1/unity-catalog/external-locations",
            post(external_locations::create).get(external_locations::list),
        )
        .route(
            "/api/2.1/unity-catalog/external-locations/:name",
            get(external_locations::get)
                .patch(external_locations::update)
                .delete(external_locations::delete),
        )
        // Permissions
        .route(
            "/api/2.1/unity-catalog/permissions/:securable_type/:full_name",
            get(permissions::get).patch(permissions::update),
        )
        // Metastore
        .route(
            "/api/2.1/unity-catalog/metastore_summary",
            get(metastore::get_summary),
        )
        // Temp Credentials
        .route(
            "/api/2.1/unity-catalog/temporary-table-credentials",
            post(temp_credentials::table_credentials),
        )
        .route(
            "/api/2.1/unity-catalog/temporary-volume-credentials",
            post(temp_credentials::volume_credentials),
        )
        .route(
            "/api/2.1/unity-catalog/temporary-model-version-credentials",
            post(temp_credentials::model_version_credentials),
        )
        .route(
            "/api/2.1/unity-catalog/temporary-path-credentials",
            post(temp_credentials::path_credentials),
        )
        // Staging tables
        .route(
            "/api/2.1/unity-catalog/staging-tables",
            post(staging_tables::create),
        )
        // Delta commits
        .route(
            "/api/2.1/unity-catalog/delta/preview/commits",
            get(delta_commits::get_commits).post(delta_commits::commit),
        )
        // Inject error format for this router
        .layer(middleware::from_fn(inject_catalog_format))
        .with_state(state)
}
