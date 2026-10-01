//! Pool construction for the SQLite database file.

use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result};
use sqlx::any::{AnyConnectOptions, AnyPoolOptions};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{AnyPool, ConnectOptions};

/// The server's historical pragma set, applied to each new connection:
/// busy timeout first (overlapping auth and UI writes wait), foreign keys on,
/// synchronous NORMAL, `temp_store` MEMORY, `cache_size` -200000.
fn with_pragmas(pool: AnyPoolOptions) -> AnyPoolOptions {
    pool.after_connect(|conn, _meta| {
        Box::pin(async move {
            sqlx::query("PRAGMA busy_timeout = 15000")
                .execute(&mut *conn)
                .await?;
            sqlx::query("PRAGMA foreign_keys = ON")
                .execute(&mut *conn)
                .await?;
            sqlx::query("PRAGMA synchronous = NORMAL")
                .execute(&mut *conn)
                .await?;
            sqlx::query("PRAGMA temp_store = MEMORY")
                .execute(&mut *conn)
                .await?;
            sqlx::query("PRAGMA cache_size = -200000")
                .execute(&mut *conn)
                .await?;
            Ok(())
        })
    })
}

/// `sqlite://` URL for a file path, with create-if-missing set.
fn sqlite_url_from_path(path: &Path) -> String {
    SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .to_url_lossy()
        .to_string()
}

/// Pool options for SQLite: four connections plus the pragmas. Every
/// SQLite pool comes through here, so this is also where the server's SQL
/// functions are registered for the connections the pool will open
/// ([`crate::db::sqlite_functions`]).
fn sqlite_pool_options() -> AnyPoolOptions {
    crate::db::sqlite_functions::register();
    with_pragmas(AnyPoolOptions::new().max_connections(4))
}

/// Best-effort WAL enablement: a hot rollback journal or another process
/// holding the database can make it fail, and callers still get a usable
/// pool.
async fn try_enable_wal(pool: &AnyPool) {
    match sqlx::query("PRAGMA journal_mode = WAL").execute(pool).await {
        Ok(_) => {}
        Err(err) => {
            tracing::warn!(error = %err, "could not enable write-ahead logging; continuing without it");
        }
    }
}

/// Open the configured pool for a SQLite file, naming the file in the error
/// context. The file is created when missing.
///
/// The sqlx Any drivers are installed here, the one place a pool is opened,
/// so no entry point has to remember to.
///
/// # Errors
///
/// Returns an error when the file cannot be opened or created.
pub async fn open_pool_for_path(path: &Path) -> Result<AnyPool> {
    sqlx::any::install_default_drivers();
    let pool = sqlite_pool_options()
        .connect_with(AnyConnectOptions::from_str(&sqlite_url_from_path(path))?)
        .await
        .with_context(|| format!("failed to open database {}", path.display()))?;
    try_enable_wal(&pool).await;
    Ok(pool)
}

/// Shared test pool: file-backed SQLite in a fresh temp dir, returned with
/// the pool so the test's files live there too.
#[cfg(test)]
pub(crate) async fn test_pool() -> (AnyPool, tempfile::TempDir) {
    sqlx::any::install_default_drivers();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("messagecrate.db");
    let pool = sqlite_pool_options()
        .connect_with(AnyConnectOptions::from_str(&sqlite_url_from_path(&path)).unwrap())
        .await
        .unwrap();
    (pool, dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn opens_sqlite_pool_and_applies_pragmas() {
        let (pool, _dir) = test_pool().await;
        // All five pragmas, read back through their pragma table
        // functions (values must match with_pragmas).
        let busy_timeout: i64 = sqlx::query_scalar("SELECT timeout FROM pragma_busy_timeout")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(busy_timeout, 15000, "busy_timeout");
        let on: i64 = sqlx::query_scalar("SELECT foreign_keys FROM pragma_foreign_keys")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(on, 1, "foreign_keys");
        let synchronous: i64 = sqlx::query_scalar("SELECT synchronous FROM pragma_synchronous")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(synchronous, 1, "synchronous must be NORMAL");
        let temp_store: i64 = sqlx::query_scalar("SELECT temp_store FROM pragma_temp_store")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(temp_store, 2, "temp_store must be MEMORY");
        let cache_size: i64 = sqlx::query_scalar("SELECT cache_size FROM pragma_cache_size")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(cache_size, -200000, "cache_size");
        // The pool is usable for real work.
        sqlx::query("CREATE TABLE t1 (id INTEGER PRIMARY KEY, v TEXT)")
            .execute(&pool)
            .await
            .unwrap();
    }
}
