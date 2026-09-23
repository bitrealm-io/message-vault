//! Read JSON or JSON Lines back into a [`ConversationDocument`].
//!
//! JSON Lines is one JSON object per line. Line 1 is a conversation header.
//! Each following line is one message.

use anyhow::{Context, Result, bail};
use message_ir::{
    ConversationDocument, ConversationHeader, IrMessage, check_schema_version_in_json,
};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::Path;

/// Read a conversation JSON file written by `write_conversation_json`.
///
/// # Errors
///
/// Returns an error when the file cannot be read, `schema_version` is not
/// the current schema (checked before the rest is parsed, so an old file is
/// refused by its version), or the JSON is invalid.
pub fn read_conversation_json(path: &Path) -> Result<ConversationDocument> {
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    check_schema_version_in_json(&raw).with_context(|| format!("read {}", path.display()))?;
    let mut doc: ConversationDocument = serde_json::from_str(&raw)
        .with_context(|| format!("parse ConversationDocument {}", path.display()))?;
    if doc.packaging_stem_suffix.is_none() {
        doc.packaging_stem_suffix = path
            .file_stem()
            .and_then(|n| n.to_str())
            .and_then(crate::util::packaging_suffix_from_stem);
    }
    doc.finalize_stats();
    Ok(doc)
}

/// Read a conversation JSON Lines file written by `write_conversation_jsonl`.
///
/// Line 1 is a [`ConversationHeader`]. Each following line is one [`IrMessage`].
///
/// # Errors
///
/// Returns an error when the file is empty, `schema_version` is not the
/// current schema (checked before the header is parsed, so an old file is
/// refused by its version), or a line cannot be parsed.
pub fn read_conversation_jsonl(path: &Path) -> Result<ConversationDocument> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut lines = BufReader::new(file).lines();
    let header_line = lines
        .next()
        .ok_or_else(|| anyhow::anyhow!("empty JSONL: {}", path.display()))?
        .with_context(|| format!("read JSONL header {}", path.display()))?;
    check_schema_version_in_json(&header_line)
        .with_context(|| format!("read {}", path.display()))?;
    let header: ConversationHeader = serde_json::from_str(&header_line)
        .with_context(|| format!("parse JSONL header {}", path.display()))?;

    let mut messages = Vec::new();
    for (i, line) in lines.enumerate() {
        let line =
            line.with_context(|| format!("read JSONL line {} in {}", i + 2, path.display()))?;
        if line.trim().is_empty() {
            continue;
        }
        let msg: IrMessage = serde_json::from_str(&line)
            .with_context(|| format!("parse JSONL message line {} in {}", i + 2, path.display()))?;
        messages.push(msg);
    }
    if messages.is_empty() {
        bail!("JSONL has no message lines: {}", path.display());
    }

    let packaging_stem_suffix = path
        .file_stem()
        .and_then(|n| n.to_str())
        .and_then(crate::util::packaging_suffix_from_stem);

    Ok(header.into_document(messages, packaging_stem_suffix))
}
