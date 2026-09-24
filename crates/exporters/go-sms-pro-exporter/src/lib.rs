//! Convert GO SMS Pro backups into the shared conversation structure
//! ([`message_ir::ConversationDocument`]) every exporter writes.
//!
//! Library entry: [`run`] for the full pipeline.
//! The `go-sms-pro-exporter` binary is a thin CLI over [`run`].

mod attachments_emit;
mod chat_id;
mod emit;
mod phone;
mod run;
mod xml;

pub use message_vault_io_core::{RunResult, parse_date_range};
pub use run::run;

#[cfg(test)]
#[path = "../tests/convert_smoke.rs"]
mod convert_smoke;

#[cfg(test)]
#[path = "../tests/run_summary.rs"]
mod run_summary;

#[cfg(test)]
#[path = "../tests/mms_export.rs"]
mod mms_export;
