//! Convert iMazing Messages / WhatsApp CSV (plus vCard CSV / VCF contacts)
//! into the shared conversation structure ([`message_ir::ConversationDocument`])
//! every exporter writes.
//!
//! Library entry: [`run`] for the full pipeline.
//! The `imazing-exporter` binary is a thin CLI over [`run`].

mod attachments;
mod attachments_emit;
mod emit;
mod parse;
mod parse_emit;
mod run;

pub use message_vault_io_core::{RunResult, parse_date_range_tz as parse_date_range};
pub use run::run;

#[cfg(test)]
#[path = "../tests/convert_smoke.rs"]
mod convert_smoke;

#[cfg(test)]
#[path = "../tests/run_summary.rs"]
mod run_summary;
