/// The log-structured store, when `logstore` is on.
///
/// The name stays `AnyPool`: it is whichever backend handle this build selected,
/// and both remain supported. Renaming it to `Store` was planned back when the
/// log store was going to replace SQL outright — it would now be a lie on the
/// SQLite path, where this is a connection pool and nothing store-shaped.
#[cfg(feature = "logstore")]
pub type AnyPool = crate::store::Store;

/// Thin wrapper around the compile-time selected sqlx pool type.
/// Feature `sqlite` (default) uses SqlitePool; feature `postgres` uses PgPool.
#[cfg(all(feature = "sqlite", not(feature = "logstore")))]
pub type AnyPool = sqlx::SqlitePool;

#[cfg(all(feature = "postgres", not(feature = "logstore")))]
pub type AnyPool = sqlx::PgPool;

/// Connect to the database, applying backend-specific tuning.
///
/// For SQLite this enables WAL journal mode (concurrent readers + one writer,
/// which eliminates the "database is locked" errors seen during schema sync)
/// and a busy_timeout so a writer waits for the lock instead of failing
/// immediately. Both are applied via connect options so every pooled
/// connection inherits them.
#[cfg(all(feature = "sqlite", not(feature = "logstore")))]
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
#[cfg(all(feature = "postgres", not(feature = "logstore")))]
pub async fn connect(url: &str) -> Result<AnyPool, sqlx::Error> {
    AnyPool::connect(url).await
}

/// Run all migrations from the appropriate migrations directory.
#[cfg(all(feature = "sqlite", not(feature = "logstore")))]
pub async fn run_migrations(pool: &AnyPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("../../migrations/sqlite").run(pool).await
}

#[cfg(all(feature = "postgres", not(feature = "logstore")))]
pub async fn run_migrations(pool: &AnyPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("../../migrations/postgres").run(pool).await
}
