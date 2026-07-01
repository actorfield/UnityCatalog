/// Thin wrapper around the compile-time selected sqlx pool type.
/// Feature `sqlite` (default) uses SqlitePool; feature `postgres` uses PgPool.
#[cfg(feature = "sqlite")]
pub type AnyPool = sqlx::SqlitePool;

#[cfg(feature = "postgres")]
pub type AnyPool = sqlx::PgPool;

/// Connect to the database, applying backend-specific tuning.
///
/// For SQLite this enables WAL journal mode (concurrent readers + one writer,
/// which eliminates the "database is locked" errors seen during schema sync)
/// and a busy_timeout so a writer waits for the lock instead of failing
/// immediately. Both are applied via connect options so every pooled
/// connection inherits them.
#[cfg(feature = "sqlite")]
pub async fn connect(url: &str) -> Result<AnyPool, sqlx::Error> {
    use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
    use std::str::FromStr;
    use std::time::Duration;

    let opts = SqliteConnectOptions::from_str(url)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));

    SqlitePoolOptions::new().connect_with(opts).await
}

/// Connect to the database. Postgres needs no SQLite-specific tuning.
#[cfg(feature = "postgres")]
pub async fn connect(url: &str) -> Result<AnyPool, sqlx::Error> {
    AnyPool::connect(url).await
}

/// Run all migrations from the appropriate migrations directory.
#[cfg(feature = "sqlite")]
pub async fn run_migrations(pool: &AnyPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("../../migrations/sqlite").run(pool).await
}

#[cfg(feature = "postgres")]
pub async fn run_migrations(pool: &AnyPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("../../migrations/postgres").run(pool).await
}
