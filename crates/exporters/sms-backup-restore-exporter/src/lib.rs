//! SMS Backup & Restore's backup format, in both directions: [`read_backup`]
//! turns `smses.xml` into the shared conversation structure
//! ([`message_ir::ConversationDocument`]) every exporter writes, and
//! [`SbrArchive`] writes conversations back out as one `smses.xml`.
//!
//! Library entry: [`run`] for the full export pipeline.

mod emit;
mod read;
mod run;
mod write;

pub use message_vault_io_core::RunResult;
pub use read::{ReadOptions, ReadReport, read_backup};
pub use run::run;
pub use write::SbrArchive;

#[cfg(test)]
#[path = "../tests/convert_smoke.rs"]
mod convert_smoke;

#[cfg(test)]
#[path = "../tests/run_summary.rs"]
mod run_summary;
