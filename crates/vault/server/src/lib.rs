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
pub(crate) mod media_tools;
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
// modules, so the search-parity suite reaches the test pools and the schema
// and export entry points through these re-exports. Test-support surface,
// not product API.
#[doc(hidden)]
pub use db::engine::{pg_test_schema_pool, sqlite_test_pool};
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
