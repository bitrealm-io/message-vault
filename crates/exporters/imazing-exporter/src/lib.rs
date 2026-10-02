//! Convert iMazing Messages / WhatsApp CSV
//! into the shared conversation structure ([`message_ir::ConversationDocument`])
//! every exporter writes.
//!
//! Library entry: [`run`] for the full pipeline; the desktop app calls it
//! in process.

mod attachments;
mod attachments_emit;
mod emit;
mod parse;
mod parse_emit;
mod run;
mod unnamed_files;

pub use message_crate_core::RunResult;
pub use run::run;

#[cfg(test)]
#[path = "../tests/convert_smoke.rs"]
mod convert_smoke;

#[cfg(test)]
#[path = "../tests/run_summary.rs"]
mod run_summary;
