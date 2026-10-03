//! The database, opened from its config.
//!
//! Every command line entry point and the HTTP server open the database the
//! same way: the config (with the command line's `--db` already applied, see
//! [`Config::load_with_db`]) names the file, the pool opens it, and the schema is made sure of before anything reads.
//! Only a Message Crate database is opened ([`schema::APPLICATION_ID`]),
//! and only `serve`, `create-database` and `reset-demo` make a new one.
//! [`OpenDb`] is that opened database plus the config it came from, so a
//! caller holds one value and never re-derives the file.

use std::path::Path;

use anyhow::{Context, Result, bail};
use sqlx::pool::PoolConnection;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions, Connection, SqlitePool};

use crate::config::Config;
use crate::db::schema::DatabaseKind;
use crate::db::{account_profile, engine, schema};

/// An opened database and the config it was opened from.
#[derive(Debug, Clone)]
pub struct OpenDb {
    /// The config the database was opened from, with every override applied.
    pub cfg: Config,
    /// Connection pool for the database `cfg` names.
    pub db: SqlitePool,
}

/// What is at `path`: `None` when no file is there, or else what the file is.
///
/// The file is read through a read-only connection, so a file that is not a
/// Message Crate database is refused before any statement changes it: the
/// server's own pool would switch it to write-ahead logging on open.
///
/// # Errors
///
/// Returns an error, naming the file, when it cannot be read as SQLite.
pub(crate) async fn inspect(path: &Path) -> Result<Option<DatabaseKind>> {
    if !path.exists() {
        return Ok(None);
    }
    let mut conn = SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .connect()
        .await
        .with_context(|| format!("failed to open database {}", path.display()))?;
    let kind = schema::database_kind(&mut conn)
        .await
        .with_context(|| format!("failed to read database {}", path.display()));
    conn.close().await?;
    Ok(Some(kind?))
}

impl OpenDb {
    /// Open the Message Crate database `cfg` names and make sure its schema
    /// is current. This is how every command but `serve` and
    /// `create-database` opens it.
    ///
    /// # Errors
    ///
    /// Returns an error, naming the path, when no file is there or the file
    /// is not a Message Crate database; then nothing is created or changed,
    /// so a mistyped `--db` or another program's SQLite file is left alone.
    /// Also when the file cannot be opened or the schema cannot be applied.
    pub async fn open(cfg: Config) -> Result<Self> {
        match inspect(&cfg.paths.db).await? {
            Some(DatabaseKind::MessageCrate) => Self::open_pool(cfg).await,
            Some(DatabaseKind::Empty | DatabaseKind::Foreign) => {
                Err(schema::not_a_message_crate_database(&cfg.paths.db))
            }
            None => bail!(
                "there is no database at {}. Only `serve` and `create-database` make a new \
                 one; this command opens a database that exists",
                cfg.paths.db.display()
            ),
        }
    }

    /// Open the database `cfg` names, making it when no file is there or the
    /// file is empty. A new database's folder is made too, which is how a new
    /// Message Crate begins. `serve` and `create-database` open it this way,
    /// as does `reset-demo`, which builds the Demo Account into a new one.
    ///
    /// # Errors
    ///
    /// Returns an error, naming the path, when the file there is not a
    /// Message Crate database; the file is left as it is. Also when the file
    /// cannot be opened or created, or the schema cannot be applied.
    pub async fn create_or_open(cfg: Config) -> Result<Self> {
        if inspect(&cfg.paths.db).await? == Some(DatabaseKind::Foreign) {
            return Err(schema::not_a_message_crate_database(&cfg.paths.db));
        }
        if let Some(parent) = cfg.paths.db.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
        Self::open_pool(cfg).await
    }

    /// Open the pool on the file `cfg` names and make sure of the schema.
    async fn open_pool(cfg: Config) -> Result<Self> {
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
    pub async fn conn(&self) -> Result<PoolConnection<sqlx::Sqlite>> {
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
        let opened = OpenDb::create_or_open(cfg).await.unwrap();

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
        let opened = OpenDb::create_or_open(fresh_config(dir.path()))
            .await
            .unwrap();
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

    /// S7-12: a mistyped `--db` names a path where no file is. The command
    /// stops there, naming the path, and creates neither the file nor its
    /// folder.
    #[tokio::test]
    async fn opening_a_path_where_no_file_is_refuses_and_creates_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = fresh_config(dir.path());
        cfg.paths.db = dir.path().join("mistyped/messagecrate.db");

        let err = OpenDb::open(cfg.clone()).await.unwrap_err();

        assert!(
            err.to_string()
                .contains(&cfg.paths.db.display().to_string()),
            "{err}"
        );
        assert!(!dir.path().join("mistyped").exists());
    }

    /// S7-3: another program's SQLite file is refused by name, by the
    /// commands that open a database and by those that make one, and is
    /// left byte for byte as it was: not rebuilt, and not even switched to
    /// write-ahead logging.
    #[tokio::test]
    async fn another_programs_sqlite_file_is_refused_and_left_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = fresh_config(dir.path());
        cfg.paths.db = dir.path().join("chat.db");
        let mut conn = SqliteConnectOptions::new()
            .filename(&cfg.paths.db)
            .create_if_missing(true)
            .connect()
            .await
            .unwrap();
        sqlx::query("CREATE TABLE message (ROWID INTEGER PRIMARY KEY, text TEXT)")
            .execute(&mut conn)
            .await
            .unwrap();
        sqlx::query("INSERT INTO message (text) VALUES ('hello from another program')")
            .execute(&mut conn)
            .await
            .unwrap();
        conn.close().await.unwrap();
        let before = std::fs::read(&cfg.paths.db).unwrap();

        for refused in [
            OpenDb::open(cfg.clone()).await,
            OpenDb::create_or_open(cfg.clone()).await,
        ] {
            let err = refused.unwrap_err().to_string();
            assert!(err.contains("chat.db"), "{err}");
            assert!(err.contains("not a Message Crate database"), "{err}");
        }

        assert_eq!(std::fs::read(&cfg.paths.db).unwrap(), before);
    }

    /// A database this server made opens again, and an empty file is not
    /// one: only the commands that make a database build in it.
    #[tokio::test]
    async fn open_takes_a_message_crate_database_and_refuses_an_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = fresh_config(dir.path());
        OpenDb::create_or_open(cfg.clone())
            .await
            .unwrap()
            .close()
            .await;
        OpenDb::open(cfg.clone()).await.unwrap().close().await;

        let mut empty = fresh_config(dir.path());
        empty.paths.db = dir.path().join("empty.db");
        std::fs::write(&empty.paths.db, b"").unwrap();
        assert!(OpenDb::open(empty.clone()).await.is_err());
        OpenDb::create_or_open(empty).await.unwrap().close().await;
    }

    #[tokio::test]
    async fn location_names_the_sqlite_file() {
        let dir = tempfile::tempdir().unwrap();
        let opened = OpenDb::create_or_open(fresh_config(dir.path()))
            .await
            .unwrap();

        assert_eq!(opened.location(), dir.path().join("messagecrate.db"));
        opened.close().await;
    }
}
