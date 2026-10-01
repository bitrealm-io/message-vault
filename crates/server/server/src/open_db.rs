//! The database, opened from its config.
//!
//! Every command line entry point and the HTTP server open the database the
//! same way: the config (with the command line's `--db` already applied, see
//! [`Config::with_db_override`]) names the file, the pool opens it, and the schema is made sure of before anything reads.
//! [`OpenDb`] is that opened database plus the config it came from, so a
//! caller holds one value and never re-derives the file.

use std::path::Path;

use anyhow::{Context, Result};
use sqlx::AnyPool;
use sqlx::pool::PoolConnection;

use crate::config::Config;
use crate::db::{account_profile, engine, schema};

/// An opened database and the config it was opened from.
#[derive(Debug, Clone)]
pub struct OpenDb {
    /// The config the database was opened from, with every override applied.
    pub cfg: Config,
    /// Connection pool for the database `cfg` names.
    pub db: AnyPool,
}

impl OpenDb {
    /// Open the database `cfg` names and make sure the schema exists.
    ///
    /// A database file that does not exist yet is created, folder and all,
    /// which is how a new Message Crate begins.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be opened or created, or the
    /// schema cannot be applied.
    pub async fn open(cfg: Config) -> Result<Self> {
        if let Some(parent) = cfg.paths.db.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
        let db = engine::open_pool_for_path(&cfg.paths.db).await?;
        {
            let mut conn = db.acquire().await?;
            schema::ensure_schema(&mut conn).await?;
        }
        Ok(Self { cfg, db })
    }

    /// The database file, for status lines and errors.
    pub fn location(&self) -> &Path {
        &self.cfg.paths.db
    }

    /// A connection from the pool.
    ///
    /// # Errors
    ///
    /// Returns an error when the pool cannot hand one out.
    pub async fn conn(&self) -> Result<PoolConnection<sqlx::Any>> {
        Ok(self.db.acquire().await?)
    }

    /// The account id behind a `--account` value: a username, or an id
    /// given as is.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty value or a username the database does not
    /// have.
    pub async fn account_id(&self, account_ref: &str) -> Result<i64> {
        let mut conn = self.conn().await?;
        account_profile::resolve_account_ref(&mut conn, account_ref).await
    }

    /// Close the pool, so a command line run ends with the file released.
    pub async fn close(self) {
        self.db.close().await;
    }
}

/// A config for a fresh database under `dir`, for tests that open one through
/// [`OpenDb`].
#[cfg(test)]
pub(crate) fn fresh_config(dir: &Path) -> Config {
    use crate::config::PathsConfig;

    Config {
        paths: PathsConfig {
            db: dir.join("messagecrate.db"),
            data_dir: dir.join("data"),
            assets_dir: "assets".into(),
            assets_converted_dir: "assets_converted".into(),
        },
        server: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn opening_a_new_database_creates_it_with_its_schema() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = fresh_config(dir.path());
        cfg.paths.db = dir.path().join("new/folder/messagecrate.db");
        let opened = OpenDb::open(cfg).await.unwrap();

        let mut conn = opened.conn().await.unwrap();
        let accounts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
        assert_eq!(accounts, 0);
    }

    #[tokio::test]
    async fn account_id_resolves_a_username_and_rejects_an_unknown_one() {
        let dir = tempfile::tempdir().unwrap();
        let opened = OpenDb::open(fresh_config(dir.path())).await.unwrap();
        let mut conn = opened.conn().await.unwrap();
        let alice = account_profile::insert_account(&mut conn, "alice", None, None)
            .await
            .unwrap();
        drop(conn);

        assert_eq!(opened.account_id("Alice").await.unwrap(), alice);
        let err = opened.account_id("nobody").await.unwrap_err();
        assert_eq!(
            err.to_string(),
            "account not found: nobody (use an existing username or account id)"
        );
    }

    #[tokio::test]
    async fn location_names_the_sqlite_file() {
        let dir = tempfile::tempdir().unwrap();
        let opened = OpenDb::open(fresh_config(dir.path())).await.unwrap();

        assert_eq!(opened.location(), dir.path().join("messagecrate.db"));
        opened.close().await;
    }
}
