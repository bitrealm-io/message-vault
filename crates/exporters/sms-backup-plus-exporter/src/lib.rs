//! SMS Backup+ (jberkel) mail, in both directions: [`run`] turns its `.eml`
//! files into the shared conversation structure
//! ([`message_ir::ConversationDocument`]) every exporter writes, and
//! [`SmsBackupPlusArchive`] writes the SMS and MMS of conversations back out
//! as that mail.
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
mod write;

pub use message_crate_core::RunResult;
pub use run::run;
pub use write::{SmsBackupPlusArchive, left_out_line};

#[cfg(test)]
#[path = "../tests/convert_smoke.rs"]
mod convert_smoke;

#[cfg(test)]
#[path = "../tests/run_summary.rs"]
mod run_summary;

#[cfg(test)]
#[path = "../tests/roster_and_inputs.rs"]
mod roster_and_inputs;

#[cfg(test)]
#[path = "../tests/round_trip.rs"]
mod round_trip;
