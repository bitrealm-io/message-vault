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
//! `imessage-reader-protocol`; the program is found and started by
//! `ios_backup::Helper`, which the checks run on an iPhone backup before an
//! import use too.
//!
//! [`run`] is this crate's whole public surface
//! (`docs/adr/0001-no-command-line-except-the-server.md`).

mod convert;
mod run;

pub use run::run;
