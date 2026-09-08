//! The vault's database, opened from its config.
//!
//! Every command line entry point and the HTTP server open the database the
//! same way: the config (with the command line's `--db` and `--db-url`
//! already applied, see [`Config::with_db_overrides`]) names the target, the
//! pool opens it, and the vault schema is made sure of before anything reads.
//! [`OpenVault`] is that opened database plus the config it came from, so a
//! caller holds one value and never re-derives the target.

use anyhow::{Context, Result};
use sqlx::AnyPool;
use sqlx::pool::PoolConnection;

use crate::config::Config;
use crate::db::engine::DbTarget;
use crate::db::{account_profile, schema};

/// An opened vault database and the config it was opened from.
#[derive(Debug, Clone)]
pub struct OpenVault {
    /// The config the database was opened from, with every override applied.
    pub cfg: Config,
    /// Connection pool for the database `cfg` names.
    pub db: AnyPool,
}

impl OpenVault {
    /// Open the database `cfg` names and make sure the vault schema exists.
    ///
    /// A SQLite file that does not exist yet is created, folder and all,
    /// which is how a new vault begins. The sqlx Any drivers are installed
    /// here, once, so no entry point has to remember to.
    ///
    /// # Errors
    ///
    /// Returns an error when the URL scheme is unknown, the connection fails,
    /// or the schema cannot be applied.
    pub async fn open(cfg: Config) -> Result<Self> {
        sqlx::any::install_default_drivers();
        if let DbTarget::Path(path) = cfg.db_target()
            && let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
        let db = cfg.db_target().open().await?;
        {
            let mut conn = db.acquire().await?;
            schema::ensure_vault_schema(&mut conn).await?;
        }
        Ok(Self { cfg, db })
    }

    /// Where the database is, for status lines and errors. Never includes
    /// credentials.
    pub fn location(&self) -> DbTarget<'_> {
        self.cfg.db_target()
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
    /// Returns an error for an empty value or a username the vault does not
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DatabaseConfig, PathsConfig};

    /// A config for a fresh vault under `dir`. On a Postgres run the database
    /// is a schema of its own on that server, as every other test's is.
    async fn fresh_config(dir: &std::path::Path) -> Config {
        let url = match crate::pg_test_url() {
            Some(url) => Some(crate::db::engine::pg_test_schema_url(&url).await),
            None => None,
        };
        Config {
            paths: PathsConfig {
                db: dir.join("vault.db"),
                data_dir: dir.join("data"),
                assets_dir: "assets".into(),
                assets_converted_dir: "assets_converted".into(),
            },
            server: None,
            database: DatabaseConfig { url },
        }
    }

    #[tokio::test]
    async fn opening_a_new_vault_creates_it_with_its_schema() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = fresh_config(dir.path()).await;
        cfg.paths.db = dir.path().join("new/folder/vault.db");
        let vault = OpenVault::open(cfg).await.unwrap();

        let mut conn = vault.conn().await.unwrap();
        let accounts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
        assert_eq!(accounts, 0);
    }

    #[tokio::test]
    async fn account_id_resolves_a_username_and_rejects_an_unknown_one() {
        let dir = tempfile::tempdir().unwrap();
        let vault = OpenVault::open(fresh_config(dir.path()).await)
            .await
            .unwrap();
        let mut conn = vault.conn().await.unwrap();
        let alice = account_profile::insert_account(&mut conn, "alice", None, None)
            .await
            .unwrap();
        drop(conn);

        assert_eq!(vault.account_id("Alice").await.unwrap(), alice);
        let err = vault.account_id("nobody").await.unwrap_err();
        assert_eq!(
            err.to_string(),
            "account not found: nobody (use an existing username or account id)"
        );
    }

    #[tokio::test]
    async fn location_names_the_sqlite_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = fresh_config(dir.path()).await;
        cfg.database.url = None;
        let vault = OpenVault::open(cfg).await.unwrap();

        assert_eq!(
            vault.location().to_string(),
            dir.path().join("vault.db").display().to_string()
        );
        vault.close().await;
    }
}
