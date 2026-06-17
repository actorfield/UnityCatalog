pub mod models;
pub mod repos;
pub mod pool;
pub mod managed_storage;

pub use pool::AnyPool;

/// Convert a sqlx error to UcError.
pub fn sqlx_err(e: sqlx::Error) -> uc_errors::UcError {
    match e {
        sqlx::Error::RowNotFound => uc_errors::UcError::new(uc_errors::ErrorCode::NotFound, "Resource not found"),
        other => uc_errors::UcError::new(uc_errors::ErrorCode::Internal, other.to_string()),
    }
}

/// Newtype wrapper so we can implement From<sqlx::Error> and use `?` in repo functions.
#[doc(hidden)]
pub struct SqlxResult<T>(pub Result<T, sqlx::Error>);

impl<T> SqlxResult<T> {
    pub fn uc(self) -> Result<T, uc_errors::UcError> {
        self.0.map_err(sqlx_err)
    }
}

/// Extension trait to convert sqlx Results to UcError Results via `.uc_err()?`
pub trait IntoUcResult<T> {
    fn uc_err(self) -> Result<T, uc_errors::UcError>;
}

impl<T> IntoUcResult<T> for Result<T, sqlx::Error> {
    fn uc_err(self) -> Result<T, uc_errors::UcError> {
        self.map_err(sqlx_err)
    }
}
