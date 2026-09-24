//! Convert WhatsApp chats (via KnugiHK wtsexporter JSON) into the shared
//! conversation structure ([`message_ir::ConversationDocument`]) every exporter writes.
//!
//! Library entry: [`run`] for the full pipeline.
//! The `whatsapp-exporter` binary is a thin CLI over [`run`].

mod emit;
mod jid;
mod owner;
mod parse;
mod run;
mod wtsexporter;

pub use message_vault_io_core::RunResult;
pub use run::run;

#[cfg(test)]
#[path = "../tests/convert_smoke.rs"]
mod convert_smoke;

#[cfg(test)]
#[path = "../tests/run_summary.rs"]
mod run_summary;
