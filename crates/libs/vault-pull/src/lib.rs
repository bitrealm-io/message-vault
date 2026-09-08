//! Pulls messages out of a running vault as one Export Run: `POST /v1/exports`
//! records what is asked for, `GET /v1/exports/{id}/messages` pages the rows,
//! and `complete` or `cancel` closes the run. The messages are written as
//! chat files.
//!
//! The `vault-pull` command and the desktop app Vault Export screen both call
//! this crate.

mod http;
pub mod journal;
mod project;
mod run;

pub use journal::{PULL_JOURNAL_NAME, PullJournalEvent, PullJournalState, journal_path};
pub use run::{
    DEFAULT_ASSET_DOWNLOAD_WORKERS, DEFAULT_PAGE_LIMIT, MAX_PAGE_LIMIT, ProgressEvent, ProgressFn,
    PullReport, VaultPullConfig, run,
};
pub use vault_api_types::{ExportRun, ExportScope, Message};
pub use vault_http::{AuthError, AuthInfo, auth_check as authenticate};
