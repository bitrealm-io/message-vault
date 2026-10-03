//! Read an iPhone backup before, or beside, an import.
//!
//! An iPhone backup is a container that several sources read: Apple Messages
//! keeps its database in it, and WhatsApp keeps its files in an app-group
//! domain of it. This crate answers the questions asked of the backup
//! itself: whether it is encrypted ([`ios_backup_encrypted_flag`]), which
//! phone number it names ([`ios_backup_phone_number`]), which addresses its
//! device sent from ([`backup_identities`], which also reads a Mac
//! `chat.db`), and the files of one domain decrypted to a folder
//! ([`decrypt_ios_backup_domain`]).
//!
//! Opening a backup's databases and decrypting its files needs
//! `imessage-database` and `crabapple`, which are GPL-3.0-or-later, and this
//! crate is under the Fair Core License, so that work happens in a separate
//! program: `imessage-reader` (`crates/helpers/imessage-reader`). [`Helper`]
//! finds that program, starts it, and reads its answer over the wire types
//! of `imessage-reader-protocol`; [`ScratchDir`] is the folder one request
//! decrypts into. `imessage-ir-exporter` starts the program through the same
//! two types for an export. Why: `docs/adr/0014-gpl-code-only-behind-a-process-boundary.md`.

mod backup;
mod backup_domain;
mod helper;
mod identity;
#[cfg(any(test, feature = "testutil"))]
pub mod reader_build;
mod scratch;
#[cfg(all(unix, any(test, feature = "testutil")))]
pub mod testutil;

pub use backup::ios_backup_encrypted_flag;
pub use backup_domain::{DecryptedDomain, decrypt_ios_backup_domain};
pub use helper::Helper;
pub use identity::{backup_identities, ios_backup_phone_number};
pub use scratch::ScratchDir;
