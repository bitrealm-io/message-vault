//! Convert Apple Messages (`chat.db` or an iOS backup) into the shared
//! conversation structure ([`message_ir::ConversationDocument`]) every exporter writes.
//!
//! This crate does not open `chat.db` itself. Reading it needs
//! `imessage-database` and `crabapple`, which are GPL-3.0-or-later, and this
//! crate is under the Fair Core License, so the reading happens in a separate
//! program: `imessage-reader` (`crates/helpers/imessage-reader`). [`run`]
//! starts it, writes one request on its stdin, and turns the records it
//! streams back into [`message_ir::IrMessage`]s, then
//! [`message_ir_format::FormatSink`] writes the chosen output format (JSON
//! Lines, JSON, CSV, EML, MBOX, or XML). The protocol is
//! `imessage-reader-protocol`; how the program is found is in [`helper`].

mod backup;
mod backup_domain;
mod convert;
mod helper;
mod identity;
mod run;

pub use backup::ios_backup_encrypted_flag;
pub use backup_domain::{DecryptedDomain, decrypt_ios_backup_domain};
pub use identity::{backup_identities, ios_backup_phone_number};
pub use run::run;
