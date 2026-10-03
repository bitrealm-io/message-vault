//! Pool construction for the SQLite database file.

use std::path::Path;

use anyhow::{Context, Result};
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

/// The statement that begins a write transaction. IMMEDIATE takes the write
/// lock at once, so overlapping writers wait on the busy timeout instead of
/// failing at their first write.
pub const BEGIN_IMMEDIATE_SQL: &str = "BEGIN IMMEDIATE TRANSACTION";

/// The server's historical pragma set, applied to each new connection:
/// busy timeout first (overlapping auth and UI writes wait), foreign keys on,
/// synchronous NORMAL, `temp_store` MEMORY, `cache_size` -200000.
fn with_pragmas(pool: SqlitePoolOptions) -> SqlitePoolOptions {
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

/// Connect options for a database file, created when missing.
fn connect_options(path: &Path) -> SqliteConnectOptions {
    SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
}

/// Pool options for SQLite: four connections plus the pragmas. Every
/// SQLite pool comes through here, so this is also where the server's SQL
/// functions are registered for the connections the pool will open
/// ([`crate::db::sqlite_functions`]).
fn sqlite_pool_options() -> SqlitePoolOptions {
    crate::db::sqlite_functions::register();
    with_pragmas(SqlitePoolOptions::new().max_connections(4))
}

/// Best-effort WAL enablement: a hot rollback journal or another process
/// holding the database can make it fail, and callers still get a usable
/// pool.
async fn try_enable_wal(pool: &SqlitePool) {
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
/// # Errors
///
/// Returns an error when the file cannot be opened or created.
pub async fn open_pool_for_path(path: &Path) -> Result<SqlitePool> {
    let pool = sqlite_pool_options()
        .connect_with(connect_options(path))
        .await
        .with_context(|| format!("failed to open database {}", path.display()))?;
    try_enable_wal(&pool).await;
    Ok(pool)
}

/// Shared test pool: file-backed SQLite in a fresh temp dir, returned with
/// the pool so the test's files live there too. It runs in WAL mode, as
/// [`open_pool_for_path`] does, because a write that lands between another
/// transaction's read and its write fails differently under WAL than under a
/// rollback journal, and a test must see what the server sees.
#[cfg(test)]
pub(crate) async fn test_pool() -> (SqlitePool, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("messagecrate.db");
    let pool = sqlite_pool_options()
        .connect_with(connect_options(&path))
        .await
        .unwrap();
    try_enable_wal(&pool).await;
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
        // The tests run in the journal mode the server runs in, so a test can
        // show what WAL does to a write that lands between a read and a write.
        let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(journal_mode, "wal", "journal_mode");
        // The pool is usable for real work.
        sqlx::query("CREATE TABLE t1 (id INTEGER PRIMARY KEY, v TEXT)")
            .execute(&pool)
            .await
            .unwrap();
    }
}
