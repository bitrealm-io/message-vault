//! Write and read [`message_ir::ConversationDocument`] in each format Message
//! Vault emits itself: JSON, JSON Lines (one JSON object per line), CSV, EML
//! and MBOX. [`FormatSink`] buffers documents, applies obfuscation and the
//! media mode, and writes them; a merged archive owned by another crate
//! plugs in through [`MergedArchive`].
//!
//! The resumable write path lives in `message-staging`, directory convert in
//! `message-reexport`, the run model in `message-vault-io-core`, and the
//! schema types in `message-ir`.

mod clean;
mod export_transforms;
mod format_sink;
mod normalize;
mod read_csv;
mod read_json;
mod read_mail;
mod util;
mod write;

pub use clean::{EXPORT_SENTINEL, clean_previous_ir_output, write_export_sentinel};
pub use export_transforms::clear_attachments_when_disabled;
pub use format_sink::{FormatSink, MergedArchive, write_documents_through_sink};
pub use read_csv::read_conversation_csv;
pub use read_json::{read_conversation_json, read_conversation_jsonl};
pub use read_mail::{read_conversation_eml_dir, read_conversation_mbox};
pub use util::{is_complete_file, load_attachment_bytes};
pub use write::{
    CSV_HEADERS, document_to_mail_messages, write_conversation_jsonl, write_conversation_jsonl_to,
    write_format,
};

#[cfg(test)]
use normalize::normalize_document_for_compare;
#[cfg(test)]
use write::write_conversation_csv;

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
