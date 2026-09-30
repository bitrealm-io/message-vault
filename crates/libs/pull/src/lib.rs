//! Pulls messages out of a running vault as one Export Run: `POST /v1/exports`
//! records what is asked for, `GET /v1/exports/{id}/messages` pages the rows,
//! and `complete` or `cancel` closes the run. The messages are written as
//! chat files.
//!
//! The `message-crate-pull` command and the desktop app Vault Export screen both call
//! this crate.

mod http;
pub mod journal;
mod project;
mod run;

pub use journal::{PULL_JOURNAL_NAME, PullJournalEvent, PullJournalState, journal_path};
pub use message_crate_api_types::{ExportRun, ExportScope, Message};
pub use message_crate_http::{AuthError, AuthInfo, auth_check as authenticate};
pub use run::{
    DEFAULT_ASSET_DOWNLOAD_WORKERS, DEFAULT_PAGE_LIMIT, MAX_PAGE_LIMIT, ProgressEvent, ProgressFn,
    PullConfig, PullReport, run,
};
