//! Convert SMS Backup+ (jberkel) EML files into the shared conversation
//! structure ([`message_ir::ConversationDocument`]) every exporter writes.
//!
//! Library entry: [`run`] for the full pipeline.
//! The `sms-backup-plus-exporter` binary is a thin CLI over [`run`].

mod assets;
mod attachments_emit;
mod emit;
mod flat_eml;
mod identity;
mod parse_emit;
mod run;
mod types;

pub use message_vault_io_core::RunResult;
pub use run::run;

#[cfg(test)]
#[path = "../tests/convert_smoke.rs"]
mod convert_smoke;

#[cfg(test)]
#[path = "../tests/run_summary.rs"]
mod run_summary;

#[cfg(test)]
#[path = "../tests/roster_and_inputs.rs"]
mod roster_and_inputs;
