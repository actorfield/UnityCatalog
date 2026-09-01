pub mod managed_storage;
pub mod models;
pub mod pagination;
pub mod pool;
pub mod store;
pub mod repos;

pub use pool::AnyPool;

// Everything below converts `sqlx::Error`, so it exists only when a SQL backend
// does. A log-store build links no driver, and constructs UcError directly.
#[cfg(feature = "sql")]
mod sql_errors {
    use uc_errors::UcError;

    /// Convert a sqlx error to UcError.
    pub fn sqlx_err(e: sqlx::Error) -> UcError {
        match e {
            sqlx::Error::RowNotFound => {
                UcError::new(uc_errors::ErrorCode::NotFound, "Resource not found")
            }
            other => UcError::new(uc_errors::ErrorCode::Internal, other.to_string()),
        }
    }

    /// Newtype wrapper so we can implement From<sqlx::Error> and use `?` in repo
    /// functions.
    #[doc(hidden)]
    pub struct SqlxResult<T>(pub Result<T, sqlx::Error>);

    impl<T> SqlxResult<T> {
        pub fn uc(self) -> Result<T, UcError> {
            self.0.map_err(sqlx_err)
        }
    }

    /// Extension trait to convert sqlx Results to UcError Results via `.uc_err()?`
    pub trait IntoUcResult<T> {
        fn uc_err(self) -> Result<T, UcError>;
    }

    impl<T> IntoUcResult<T> for Result<T, sqlx::Error> {
        fn uc_err(self) -> Result<T, UcError> {
            self.map_err(sqlx_err)
        }
    }
}

#[cfg(feature = "sql")]
pub use sql_errors::*;
