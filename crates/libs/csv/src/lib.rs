//! Shared CSV helpers for writing conversation files.

#![warn(missing_docs)]

mod date_range;
mod utc_offset;

pub use date_range::DateRange;
pub use utc_offset::parse_utc_offset;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::path::Path;

/// One attachment object written into `attachments_json`.
#[derive(Debug, Serialize, Deserialize)]
pub struct AttachmentCell {
    /// Shared attachment metadata (serialized inline — same JSON shape as before).
    #[serde(flatten)]
    pub meta: message_ir::AttachmentMeta,
    /// Sticker flag.
    #[serde(default)]
    pub is_sticker: bool,
    /// Transcribed text of the attachment (e.g., OCR of an image or a
    /// voice-note transcript).
    pub transcription: Option<String>,
    /// iMessage sticker effect name.
    pub sticker_effect: Option<String>,
}

impl From<AttachmentCell> for message_ir::IrAttachment {
    fn from(cell: AttachmentCell) -> Self {
        let AttachmentCell {
            meta,
            is_sticker,
            transcription,
            sticker_effect,
        } = cell;
        Self {
            path: meta.path,
            original_name: meta.original_name,
            mime_type: meta.mime_type,
            digest_sha256: meta.digest_sha256,
            is_sticker,
            transcription,
            sticker_effect,
            size_bytes: None,
            missing_reason: None,
            bytes: None,
        }
    }
}

/// One participant object written into (and read back from) the CSV
/// `participants_json` cell.
#[derive(Debug, Serialize, Deserialize)]
pub struct ParticipantCell {
    /// Raw handle (phone, email, or other identifier).
    pub handle: String,
    /// Display name; empty string when unknown.
    #[serde(default)]
    pub display_name: String,
    /// Absent (legacy cells) → `Some(HandleType::Other)`; explicit `null` →
    /// `None`; any other string is parsed leniently via
    /// [`message_ir::HandleType::parse`].
    #[serde(
        default = "default_participant_handle_type",
        deserialize_with = "deserialize_handle_type"
    )]
    pub handle_type: Option<message_ir::HandleType>,
}

/// serde default for a participant cell's handle type: `Other`.
fn default_participant_handle_type() -> Option<message_ir::HandleType> {
    Some(message_ir::HandleType::Other)
}

/// Parse a handle type cell, accepting a missing cell.
fn deserialize_handle_type<'de, D>(de: D) -> Result<Option<message_ir::HandleType>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = Option::<String>::deserialize(de)?;
    Ok(s.map(|s| message_ir::HandleType::parse(&s)))
}

/// Timestamp formatting and stable GUID derivation (defined in `message-ir`,
/// where the shared projection uses them; re-exported here for existing callers).
pub use message_ir::{format_local_ts, stable_guid};

/// Serialize a value for a CSV JSON cell (`null` on failure).
pub fn json_cell(value: &impl Serialize) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

/// A CSV reader over a file already read into memory.
pub type CsvBytesReader = csv::Reader<Cursor<Vec<u8>>>;

/// Open a CSV export for reading: the whole file in memory with a UTF-8
/// byte-order mark stripped, a flexible reader over it, and the headers
/// trimmed and lower-cased so [`col`] lookups ignore case.
///
/// # Errors
///
/// Returns an error when the file cannot be read or has no header row.
pub fn open_csv_lowercase(path: &Path) -> anyhow::Result<(CsvBytesReader, Vec<String>)> {
    let mut bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        bytes.drain(..3);
    }
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(Cursor::new(bytes));
    let headers = rdr
        .headers()
        .with_context(|| format!("headers {}", path.display()))?
        .iter()
        .map(|h| h.trim().to_ascii_lowercase())
        .collect();
    Ok((rdr, headers))
}

/// Index of a required CSV header column.
///
/// # Errors
///
/// Returns an error naming the missing column and the headers found.
pub fn col(headers: &[String], name: &str) -> anyhow::Result<usize> {
    headers
        .iter()
        .position(|h| h == name)
        .with_context(|| format!("missing column {name:?} (have {headers:?})"))
}

/// A CSV boolean cell: `1`, `true`, `yes`, or `y` in any case, after trimming.
pub fn parse_bool(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "y"
    )
}

/// Trimmed value of one CSV cell (empty string when missing).
pub fn field(rec: &csv::StringRecord, idx: usize) -> String {
    rec.get(idx).unwrap_or("").trim().to_string()
}
