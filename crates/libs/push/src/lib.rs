//! Upload a folder of conversation files into the Message Crate HTTP server.
//!
//! Each conversation is a JSON Lines file (one JSON object per line). The
//! desktop app's Import screen calls this crate.
//!
//! Module map:
//! - [`run`] — configuration, login, and the main loop that ties the rest together.
//! - `prepare` — read one conversation, upload its media, cut it into chunks (worker threads).
//! - `pipeline` — batch chunks into import requests and settle each conversation's result.
//! - `progress` — the log file and live progress callback.
//! - `journal` — the on-disk record of what already succeeded.
//! - `folder` — what counts as a conversation file, and where attachments live.
//! - `report` — the summary written at the end.

mod folder;
mod http;
mod journal;
mod pipeline;
mod prepare;
mod progress;
mod project;
mod report;
mod run;

pub use folder::detect_source;
pub use journal::{JOURNAL_NAME, LOG_NAME, REPORT_NAME};
pub use message_crate_api_types::ImportMode;
pub use message_crate_http::AuthError;
pub use message_crate_http::AuthInfo;
pub use progress::{ProgressEvent, ProgressFn};
pub use report::{FileResult, PushReport, UploadProfile, format_duration_ms, format_push_summary};
pub use run::{
    DEFAULT_ASSET_MAX_BYTES, DEFAULT_ASSET_UPLOAD_WORKERS, DEFAULT_BATCH_SIZE,
    DEFAULT_PREPARE_AHEAD, DEFAULT_PREPARE_WORKERS, MAX_IMPORT_BODY_BYTES, MAX_PROXY_BODY_BYTES,
    NO_MESSAGE_COUNT_LIMIT, PushConfig, authenticate, run,
};
