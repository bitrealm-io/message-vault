//! Load KnugiHK WhatsApp-Chat-Exporter single-file JSON (`ChatCollection.to_dict`).

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// Top-level JSON: map of JID → chat.
pub(crate) type ChatStoreFile = BTreeMap<String, ChatJson>;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ChatJson {
    pub name: Option<String>,
    /// Prefix for relative media `data` paths (iOS often `AppDomainGroup-…/`).
    #[serde(default)]
    pub media_base: Option<String>,
    #[serde(default)]
    pub messages: BTreeMap<String, MessageJson>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct MessageJson {
    #[serde(default)]
    pub from_me: bool,
    /// Unix seconds (or milliseconds — converted when writing the conversation).
    pub timestamp: Option<f64>,
    pub data: Option<Value>,
    pub sender: Option<String>,
    /// `false` or a media path string.
    #[serde(default)]
    pub media: Value,
    pub mime: Option<String>,
    pub caption: Option<String>,
    #[serde(default)]
    pub sticker: bool,
    pub key_id: Option<Value>,
    pub reply: Option<Value>,
    #[serde(default)]
    pub reactions: Value,
}

/// Load a wtsexporter `result.json` (one JSON object: JID → chat).
///
/// # Errors
///
/// Returns an error when the file cannot be read or parsed.
pub(crate) fn load_chat_store(path: &Path) -> Result<ChatStoreFile> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))
}

/// True when the message's `media` field is the boolean `true`.
fn has_media_flag(msg: &MessageJson) -> bool {
    matches!(&msg.media, Value::Bool(true))
}

/// True for the text wtsexporter writes when a media file was not in the backup.
fn is_missing_media_placeholder(s: &str) -> bool {
    s.eq_ignore_ascii_case("The media is missing")
}

/// Body text from `data` (string) or caption.
///
/// When `media` is true, wtsexporter stores the file path in `data`, so only
/// `caption` (if any) is treated as message text.
pub(crate) fn message_text(msg: &MessageJson) -> String {
    if has_media_flag(msg) {
        return msg.caption.clone().unwrap_or_default();
    }
    let body = match &msg.data {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    };
    if body.is_empty() {
        msg.caption.clone().unwrap_or_default()
    } else if let Some(cap) = msg.caption.as_deref().filter(|c| !c.is_empty()) {
        if body.contains(cap) {
            body
        } else {
            format!("{body}\n{cap}")
        }
    } else {
        body
    }
}

/// Path hint for an attachment.
///
/// Upstream sets `media: true` and puts the path in `data` (Android/iOS). Older
/// or alternate dumps may put a path string directly in `media`.
pub(crate) fn media_path(msg: &MessageJson) -> Option<&str> {
    match &msg.media {
        Value::String(s) if !s.is_empty() && !is_missing_media_placeholder(s) => Some(s.as_str()),
        Value::Bool(true) => match &msg.data {
            Some(Value::String(s)) if !s.is_empty() && !is_missing_media_placeholder(s) => {
                Some(s.as_str())
            }
            _ => None,
        },
        _ => None,
    }
}

/// Normalize wtsexporter timestamp to Unix milliseconds.
pub(crate) fn timestamp_ms(ts: f64) -> i64 {
    if ts > 9_999_999_999.0 {
        ts as i64
    } else {
        (ts * 1000.0) as i64
    }
}

/// Normalize wtsexporter timestamp to Unix seconds.
pub(crate) fn timestamp_secs(ts: f64) -> i64 {
    timestamp_ms(ts) / 1000
}
