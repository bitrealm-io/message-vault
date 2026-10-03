//! HTTP API and SQLite storage for browsing imported messages.

/// This server's Build: the Product Version plus the commit it was built from,
/// such as `0.9.0+343fe0d8`. `build.rs` works it out.
pub const BUILD: &str = env!("MESSAGE_CRATE_BUILD");

pub mod cli;
pub mod cli_docs;
pub mod config;
pub mod error_docs;

pub(crate) mod accounts_api;
pub(crate) mod asset_store;
pub(crate) mod asset_uploads;
pub(crate) mod assets_api;
pub(crate) mod audit_trail_api;
pub(crate) mod contacts_api;
pub(crate) mod conversations_api;
pub(crate) mod credentials;
pub(crate) mod db;
pub(crate) mod declared_query;
pub(crate) mod dedupe;
pub(crate) mod exports_api;
pub(crate) mod extract;
pub(crate) mod import_cli;
pub(crate) mod import_media;
pub(crate) mod imports_api;
pub(crate) mod jsonl;
pub(crate) mod keyed_locks;
pub mod logging;
pub(crate) mod messages_api;
pub(crate) mod models;
pub(crate) mod named_set_api;
pub(crate) mod open_db;
pub(crate) mod openapi;
pub(crate) mod operation_lock;
pub(crate) mod owner_cli;
pub(crate) mod paging;
pub mod problem;
pub(crate) mod process_assets;
pub mod request_id;
pub(crate) mod reset_demo;
pub(crate) mod saved_searches_api;
pub(crate) mod search;
pub(crate) mod search_fields_api;
pub(crate) mod server;
pub(crate) mod server_api;
pub(crate) mod session_api;
#[cfg(test)]
pub mod test_support;
pub(crate) mod trash_api;

pub use db::conversation_messages::{DEFAULT_MESSAGE_SORT, MESSAGE_SORT_KEYS, MessageSort};
pub use server::{ApiError, AppState, AuthCapability, AuthIdentity, resolve_auth, run};

pub use message_crate_api_types::{ExportQueryList, ExportScope};

use clap::Command;

/// Clap command definition for the `message-crate-server` CLI; delegates to
/// [`cli::clap_command`].
pub fn clap_command() -> Command {
    cli::clap_command()
}

#[cfg(test)]
mod clap_command_tests {
    #[test]
    fn clap_command_is_message_crate_server() {
        let cmd = crate::clap_command();
        assert_eq!(cmd.get_name(), "message-crate-server");
        let subs: Vec<&str> = cmd.get_subcommands().map(|s| s.get_name()).collect();
        assert!(subs.contains(&"serve"));
        assert!(subs.contains(&"dump-openapi"));
        assert!(subs.contains(&"import"));
    }
}
