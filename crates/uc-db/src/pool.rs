/// Thin wrapper around the compile-time selected sqlx pool type.
/// Feature `sqlite` (default) uses SqlitePool; feature `postgres` uses PgPool.
#[cfg(feature = "sqlite")]
pub type AnyPool = sqlx::SqlitePool;

#[cfg(feature = "postgres")]
pub type AnyPool = sqlx::PgPool;

/// Run all migrations from the appropriate migrations directory.
#[cfg(feature = "sqlite")]
pub async fn run_migrations(pool: &AnyPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("../../migrations/sqlite").run(pool).await
}

#[cfg(feature = "postgres")]
pub async fn run_migrations(pool: &AnyPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("../../migrations/postgres").run(pool).await
}
