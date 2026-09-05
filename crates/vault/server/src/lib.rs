//! HTTP API and SQLite storage for browsing imported messages.
#![warn(missing_docs)]

pub mod cli;
pub mod cli_docs;
pub mod config;

pub(crate) mod admin_api;
pub(crate) mod api_tokens_api;
pub(crate) mod asset_uploads;
pub(crate) mod assets;
pub(crate) mod auth;
pub(crate) mod contacts_api;
pub(crate) mod conversations_api;
pub(crate) mod db;
pub(crate) mod dedupe;
pub(crate) mod export_api;
pub(crate) mod extract;
pub(crate) mod import;
pub(crate) mod import_cli;
pub(crate) mod import_media;
pub(crate) mod jsonl;
pub(crate) mod messages_api;
pub(crate) mod models;
pub(crate) mod named_membership;
pub(crate) mod named_set_api;
pub(crate) mod openapi;
pub(crate) mod operation_lock;
pub(crate) mod paging;
pub(crate) mod process_assets;
pub(crate) mod profile;
pub(crate) mod reset_demo;
pub(crate) mod saved_searches_api;
pub(crate) mod search;
pub(crate) mod search_api;
pub(crate) mod server;
#[cfg(test)]
pub mod test_support;
pub(crate) mod trash_api;

pub use server::{ApiError, AppState, AuthCapability, AuthIdentity, ErrorBody, resolve_auth, run};

// Integration tests (crates/vault/server/tests) cannot see `pub(crate)`
// modules, so the search-parity suite reaches the schema and export entry
// points through these re-exports. Test-support surface, not product API.
#[doc(hidden)]
pub use db::schema::ensure_vault_schema;
#[doc(hidden)]
pub use export_api::{ExportPageOpts, export_messages};

use clap::Command;

/// Postgres test URL when the gated suite should run (CI sets this).
pub fn pg_test_url() -> Option<String> {
    std::env::var("MV_TEST_POSTGRES_URL")
        .ok()
        .filter(|u| !u.is_empty())
}

/// Serializes the Postgres-gated tests against their shared test database.
/// Concurrent `ensure_vault_schema` calls race on Postgres's composite-type
/// creation (`CREATE TABLE IF NOT EXISTS` is not race-safe there), and the
/// gated unit tests (`messages_fts_stays_in_sync_pg`,
/// `promote_fts_cycle_pg`) and the search-parity integration test clear and
/// reuse the same message-id range. Cargo runs the lib and integration test
/// binaries as separate processes against the same database, so threads
/// within one binary take [`PG_TEST_LOCK`] and the binaries exclude each
/// other with an fs2 file lock — both via [`acquire_pg_test_lock`].
/// Test-support surface, not product API.
#[doc(hidden)]
pub static PG_TEST_LOCK: std::sync::LazyLock<tokio::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

/// Take the in-process Postgres test mutex and the cross-process test
/// database lock together (blocking until both are held). The returned file
/// must be kept alive for as long as the database is in use — dropping it
/// releases the cross-process lock.
#[doc(hidden)]
pub async fn acquire_pg_test_lock() -> (tokio::sync::MutexGuard<'static, ()>, std::fs::File) {
    let guard = PG_TEST_LOCK.lock().await;
    let lock_path = std::env::temp_dir().join("message-vault-pg-tests.lock");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .expect("open the Postgres test lock file");
    let to_lock = file.try_clone().expect("clone the Postgres test lock file");
    tokio::task::spawn_blocking(move || {
        fs2::FileExt::lock_exclusive(&to_lock).expect("lock the Postgres test lock file");
    })
    .await
    .expect("Postgres test lock task");
    (guard, file)
}

/// Clap command definition for the `message-vault-server` CLI; delegates to
/// [`cli::clap_command`].
pub fn clap_command() -> Command {
    cli::clap_command()
}

#[cfg(test)]
mod clap_command_tests {
    #[test]
    fn clap_command_is_message_vault_server() {
        let cmd = crate::clap_command();
        assert_eq!(cmd.get_name(), "message-vault-server");
        let subs: Vec<&str> = cmd.get_subcommands().map(|s| s.get_name()).collect();
        assert!(subs.contains(&"serve"));
        assert!(subs.contains(&"dump-openapi"));
        assert!(subs.contains(&"import"));
    }
}
