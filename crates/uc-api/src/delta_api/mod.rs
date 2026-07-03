pub mod config;
pub mod credentials;
pub mod tables;
use crate::{error_ext::inject_delta_format, state::AppState};
use axum::{
    middleware,
    routing::{get, post},
    Router,
};
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/delta/v1/config", get(config::get_config))
        .route(
            "/delta/v1/catalogs/:catalog/schemas/:schema/staging-tables",
            post(tables::create_staging_table),
        )
        .route(
            "/delta/v1/catalogs/:catalog/schemas/:schema/tables",
            post(tables::create_table),
        )
        .route(
            "/delta/v1/catalogs/:catalog/schemas/:schema/tables/:table",
            get(tables::load_table)
                .head(tables::table_exists)
                .post(tables::update_table)
                .delete(tables::delete_table),
        )
        .route(
            "/delta/v1/catalogs/:catalog/schemas/:schema/tables/:table/rename",
            post(tables::rename_table),
        )
        .route(
            "/delta/v1/catalogs/:catalog/schemas/:schema/tables/:table/metrics",
            post(tables::report_metrics),
        )
        .route(
            "/delta/v1/catalogs/:catalog/schemas/:schema/tables/:table/credentials",
            get(credentials::get_table_credentials),
        )
        .route(
            "/delta/v1/staging-tables/:table_id/credentials",
            get(credentials::get_staging_credentials),
        )
        .route(
            "/delta/v1/temporary-path-credentials",
            get(credentials::get_path_credentials),
        )
        .layer(middleware::from_fn(inject_delta_format))
        .with_state(state)
}
