pub mod state;
pub mod middleware;
pub mod catalog_api;
pub mod control_api;
pub mod delta_api;
pub mod error_ext;

pub use state::AppState;

/// Convert a sqlx::Error to UcError for use in uc-api handlers.
pub fn db_err(e: sqlx::Error) -> uc_errors::UcError {
    uc_db::sqlx_err(e)
}
