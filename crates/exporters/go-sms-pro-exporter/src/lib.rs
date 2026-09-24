//! Convert GO SMS Pro backups into the shared conversation structure
//! ([`message_ir::ConversationDocument`]) every exporter writes.
//!
//! Library entry: [`run`] for the full pipeline; the desktop app calls it
//! in process.

mod attachments_emit;
mod chat_id;
mod emit;
mod run;
mod xml;

pub use message_vault_io_core::RunResult;
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

#[cfg(test)]
#[path = "../tests/real_backup.rs"]
mod real_backup;
