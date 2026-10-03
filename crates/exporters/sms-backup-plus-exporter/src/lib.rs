//! Convert SMS Backup+ (jberkel) EML files into the shared conversation
//! structure ([`message_ir::ConversationDocument`]) every exporter writes.
//!
//! Library entry: [`run`] for the full pipeline; the desktop app calls it
//! in process.

mod assets;
mod attachments_emit;
mod emit;
mod flat_eml;
mod identity;
mod parse_emit;
mod run;
mod types;

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
