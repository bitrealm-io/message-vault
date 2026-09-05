//! Read a unified conversation CSV back into a [`ConversationDocument`].

use crate::CSV_HEADERS;
use crate::normalize::{imessage_from_parts, source_from_parts};
use anyhow::{Context, Result, bail};
use message_csv::{AttachmentCell, ParticipantCell};
use message_ir::{
    ConversationDocument, ConversationHeader, ConversationMeta, ConversationStats, ExportMeta,
    HandleType, IrAttachment, IrConversationType, IrDirection, IrImessage, IrMessage,
    IrMessageKind, IrParticipant, IrService, SCHEMA_VERSION, nonempty, parse_android_type,
};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

/// Read a conversation CSV written by `write_conversation_csv`.
///
/// Conversation and export header fields come from the first data row.
///
/// # Errors
///
/// Returns an error when the file cannot be opened, a required column is
/// missing, there are no data rows, or a row cannot be parsed.
pub fn read_conversation_csv(path: &Path) -> Result<ConversationDocument> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(BufReader::new(file));

    let headers = rdr
        .headers()
        .with_context(|| format!("read CSV headers {}", path.display()))?
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    let cols = validate_headers(&headers)?;

    let mut rows = Vec::new();
    for (i, result) in rdr.records().enumerate() {
        let record =
            result.with_context(|| format!("read CSV row {} in {}", i + 1, path.display()))?;
        rows.push(record);
    }
    if rows.is_empty() {
        bail!("CSV has no data rows: {}", path.display());
    }

    let header = header_from_row(&cols, &rows[0]);
    let packaging_stem_suffix = path
        .file_stem()
        .and_then(|n| n.to_str())
        .and_then(crate::util::packaging_suffix_from_stem);

    let mut messages = Vec::with_capacity(rows.len());
    for (i, record) in rows.iter().enumerate() {
        messages.push(
            message_from_record(&cols, record)
                .with_context(|| format!("parse CSV row {} in {}", i + 1, path.display()))?,
        );
    }

    Ok(header.into_document(messages, packaging_stem_suffix))
}

/// Rebuild the conversation header from the first CSV row's conversation columns.
fn header_from_row(cols: &HashMap<&str, usize>, row: &csv::StringRecord) -> ConversationHeader {
    let get = |name: &str| cell(cols, row, name).unwrap_or("");
    let mut participants = parse_participants(get("participants_json"));
    // Legacy files predate handle_type in the participants cell. For
    // single-participant conversations, fall back to the per-row
    // `handle_type` column (the sender's inferred type) so the peer keeps
    // a type. Group chats have no single type, so they are left untouched.
    if participants.len() == 1
        && participants[0].handle_type.is_none()
        && let Some(t) = parse_handle_type_cell(get("handle_type"))
    {
        participants[0].handle_type = Some(t);
    }
    let group_title = {
        let t = get("group_title");
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    };
    ConversationHeader {
        schema_version: SCHEMA_VERSION,
        export: ExportMeta {
            source: get("export_source").to_string(),
            tool: get("export_tool").to_string(),
            tool_version: get("export_tool_version").to_string(),
            owner_handle: nonempty(get("owner_handle")),
            owner_display_name: nonempty(get("owner_display_name")),
        },
        conversation: ConversationMeta {
            chat_identifier: get("chat_identifier").to_string(),
            conversation_type: IrConversationType::parse(get("conversation_type")),
            group_title,
            participants,
            stats: ConversationStats::default(),
        },
    }
}

/// Rebuild one message from a CSV row.
fn message_from_record(cols: &HashMap<&str, usize>, row: &csv::StringRecord) -> Result<IrMessage> {
    let get = |name: &str| cell(cols, row, name).unwrap_or("");
    let timestamp_unix_ms = get("timestamp_unix_ms")
        .parse::<i64>()
        .with_context(|| format!("bad timestamp_unix_ms {:?}", get("timestamp_unix_ms")))?;
    let direction = match get("direction").to_ascii_lowercase().as_str() {
        "outgoing" => IrDirection::Outgoing,
        _ => IrDirection::Incoming,
    };
    let attachments = parse_attachments(get("attachments_json"))?;
    let source = source_from_parts(
        parse_android_type(get("android_type")),
        get("source_fields_json"),
    );

    let is_reply = message_csv::parse_bool(get("is_reply"));
    let is_deleted = message_csv::parse_bool(get("is_deleted"));
    let thread_originator_part = {
        let s = get("thread_originator_part");
        if s.is_empty() { None } else { s.parse().ok() }
    };
    let num_replies = {
        let s = get("num_replies");
        if s.is_empty() { None } else { s.parse().ok() }
    };
    let associated_part = {
        let s = get("associated_part");
        if s.is_empty() { None } else { s.parse().ok() }
    };
    let imessage = imessage_from_parts(IrImessage {
        is_reply,
        in_reply_to_guid: nonempty(get("thread_originator_guid")),
        thread_originator_part,
        num_replies,
        is_deleted,
        send_effect: nonempty(get("send_effect")),
        shared_location: nonempty(get("shared_location")),
        announcement: nonempty(get("announcement")),
        read_receipt_rfc3339: nonempty(get("read_receipt")),
        parts: parse_json_cell(get("parts_json")),
        edits: parse_json_cell(get("edits_json")),
        tapbacks: parse_json_cell(get("tapbacks_json")),
        app: parse_json_cell(get("app_json")),
        balloon_bundle_id: nonempty(get("balloon_bundle_id")),
        balloon_kind: nonempty(get("balloon_kind")),
        associated_guid: nonempty(get("associated_guid")),
        associated_part,
        tapback_kind: nonempty(get("tapback_kind")),
        tapback_emoji: nonempty(get("tapback_emoji")),
        tapback_action: nonempty(get("tapback_action")),
    });

    Ok(IrMessage {
        guid: get("guid").to_string(),
        timestamp_unix_ms,
        direction,
        service: IrService::parse(get("service")),
        message_kind: IrMessageKind::parse(get("message_kind")),
        sender_handle: nonempty(get("sender_handle")),
        sender_display_name: nonempty(get("sender_display_name")),
        subject: nonempty(get("subject")),
        text: get("text").to_string(),
        attachments,
        imessage,
        source,
    })
}

/// Check every required column is present and return the name → index map
/// used for per-row lookups.
fn validate_headers(headers: &[String]) -> Result<HashMap<&str, usize>> {
    let mut cols: HashMap<&str, usize> = HashMap::with_capacity(headers.len());
    for (i, h) in headers.iter().enumerate() {
        // First occurrence wins, matching the old linear `position` lookup.
        cols.entry(h.as_str()).or_insert(i);
    }
    for required in CSV_HEADERS {
        if !cols.contains_key(required) {
            bail!("CSV missing required column `{required}`");
        }
    }
    Ok(cols)
}

/// The value of the named column in this row, if the column exists.
fn cell<'a>(
    cols: &HashMap<&str, usize>,
    row: &'a csv::StringRecord,
    name: &str,
) -> Option<&'a str> {
    row.get(*cols.get(name)?)
}

/// Parse a JSON cell, treating blank and `null` as absent.
fn parse_json_cell(s: &str) -> Option<Value> {
    let t = s.trim();
    if t.is_empty() || t == "null" {
        return None;
    }
    serde_json::from_str(t).ok()
}

/// Participants from the `participants_json` cell; malformed JSON yields none.
fn parse_participants(raw: &str) -> Vec<IrParticipant> {
    if raw.trim().is_empty() {
        return Vec::new();
    }
    let cells: Vec<ParticipantCell> = serde_json::from_str(raw).unwrap_or_default();
    cells
        .into_iter()
        .map(|p| IrParticipant {
            handle: if p.handle.is_empty() {
                None
            } else {
                Some(p.handle)
            },
            display_name: if p.display_name.is_empty() {
                None
            } else {
                Some(p.display_name)
            },
            handle_type: p.handle_type,
        })
        .collect()
}

/// Parse the dedicated `handle_type` column cell (empty → `None`).
fn parse_handle_type_cell(raw: &str) -> Option<HandleType> {
    let t = raw.trim();
    if t.is_empty() {
        None
    } else {
        Some(HandleType::parse(t))
    }
}

/// Attachments from the `attachments_json` cell.
fn parse_attachments(raw: &str) -> Result<Vec<IrAttachment>> {
    if raw.trim().is_empty() || raw.trim() == "null" {
        return Ok(Vec::new());
    }
    let cells: Vec<AttachmentCell> =
        serde_json::from_str(raw).with_context(|| format!("parse attachments_json: {raw}"))?;
    Ok(cells.into_iter().map(Into::into).collect())
}
