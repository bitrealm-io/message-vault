//! Write [`ConversationDocument`] as JSON, JSON Lines, CSV, or mail.

use crate::util;
use crate::write_sbr;
use anyhow::{Context, Result, bail};
use mail::{MailAttachment, MailMessage, MailPackage, Participant, write_mail_package};
use message_csv::{AttachmentCell, ParticipantCell, format_local_ts, json_cell};
use message_ir::{ConversationDocument, ConversationHeader, IrImessage, IrMessage, IrMessageKind};
use message_vault_io_core::OutputFormat;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Unified CSV columns for every exporter (IR v3 projection).
///
/// Apple-only cells are empty for non-iMessage sources. Legacy names
/// (`date_ms`, `contact_name`, `xml_fields_json`) are gone — use
/// `timestamp_unix_ms`, `sender_display_name`, and `source_fields_json`.
pub const CSV_HEADERS: &[&str] = &[
    "chat_identifier",
    "conversation_type",
    "group_title",
    "participants_json",
    "guid",
    "timestamp",
    "timestamp_utc",
    "timestamp_display",
    "timestamp_unix_ms",
    "direction",
    "service",
    "sender_handle",
    "sender_display_name",
    "handle_type",
    "subject",
    "text",
    "attachments_json",
    "message_kind",
    "export_source",
    "export_tool",
    "export_tool_version",
    "owner_handle",
    "owner_display_name",
    "android_type",
    "source_fields_json",
    "read_receipt",
    "is_deleted",
    "send_effect",
    "shared_location",
    "is_announcement",
    "announcement",
    "is_reply",
    "thread_originator_guid",
    "thread_originator_part",
    "num_replies",
    "parts_json",
    "edits_json",
    "tapbacks_json",
    "app_json",
    "balloon_bundle_id",
    "balloon_kind",
    "associated_guid",
    "associated_part",
    "tapback_kind",
    "tapback_emoji",
    "tapback_action",
];

/// Write one conversation in a per-chat format.
///
/// For multi-chat exports (including XML `smses.xml`), use [`FormatSink`] instead.
/// [`OutputFormat::Xml`] returns an error here.
///
/// # Errors
///
/// Returns an error when the directory cannot be created, a file cannot be
/// written, or `format` is XML.
pub(crate) fn write_format(
    output_dir: &Path,
    format: OutputFormat,
    mut doc: ConversationDocument,
) -> Result<PathBuf> {
    doc.finalize_stats();
    match format {
        OutputFormat::Csv => write_conversation_csv(output_dir, &doc),
        OutputFormat::Json => write_conversation_json(output_dir, &doc),
        OutputFormat::Jsonl => write_conversation_jsonl(output_dir, &doc),
        OutputFormat::Eml => write_conversation_mail(output_dir, &doc, MailPackage::EmlFolders),
        OutputFormat::Mbox => write_conversation_mail(output_dir, &doc, MailPackage::Mbox),
        OutputFormat::Xml => write_sbr::write_format_xml_unsupported(),
    }
}

/// Per-conversation JSON artifact (`<stem>.json`).
fn write_conversation_json(output_dir: &Path, doc: &ConversationDocument) -> Result<PathBuf> {
    let path = output_dir.join(format!("{}.json", doc.filename_stem()));
    let json = serde_json::to_vec_pretty(doc).context("serialize ConversationDocument")?;
    util::write_atomic(&path, |out| {
        out.write_all(&json)?;
        out.write_all(b"\n")?;
        Ok(())
    })?;
    Ok(path)
}

/// Write `doc` as JSON Lines to exactly `path`, atomically.
///
/// Unlike the export writers this does not derive the file name from the
/// document: a caller patching a file it already read must write back to the
/// same path. The write goes through a `.tmp` sibling and a rename, so a
/// reader never sees a half-written conversation.
///
/// # Errors
///
/// Returns an error when the file cannot be created, serialized, or renamed.
pub fn write_conversation_jsonl_to(path: &Path, doc: &ConversationDocument) -> Result<()> {
    util::write_atomic(path, |out| {
        let header = ConversationHeader::from_document(doc);
        serde_json::to_writer(&mut *out, &header).context("serialize JSONL header")?;
        out.write_all(b"\n")?;
        for msg in &doc.messages {
            serde_json::to_writer(&mut *out, msg).context("serialize JSONL message")?;
            out.write_all(b"\n")?;
        }
        Ok(())
    })
}

/// Write `doc` as `<stem>.jsonl` under `output_dir` and return that path.
///
/// # Errors
///
/// Returns an error when the file cannot be created or written.
pub fn write_conversation_jsonl(output_dir: &Path, doc: &ConversationDocument) -> Result<PathBuf> {
    let path = output_dir.join(format!("{}.jsonl", doc.filename_stem()));
    write_conversation_jsonl_to(&path, doc)?;
    Ok(path)
}

/// Compact JSON for nested bags; empty string when absent (never the literal `null`).
fn value_cell(v: Option<&Value>) -> String {
    v.filter(|v| !v.is_null())
        .map(|v| serde_json::to_string(v).unwrap_or_default())
        .filter(|s| !s.is_empty())
        .unwrap_or_default()
}

/// CSV `parts_json` cell: only from the iMessage bag, and omit a single plain
/// text/run part that merely duplicates [`IrMessage::text`].
fn parts_cell_for_csv(text: &str, parts: Option<&Value>) -> String {
    if parts_are_trivial_text_duplicate(text, parts) {
        return String::new();
    }
    value_cell(parts)
}

/// True when `parts` is a one-element array whose text equals `message_text`
/// and kind is absent, `run`, or `text`.
pub(crate) fn parts_are_trivial_text_duplicate(message_text: &str, parts: Option<&Value>) -> bool {
    let Some(Value::Array(items)) = parts else {
        return false;
    };
    if items.len() != 1 {
        return false;
    }
    let Some(obj) = items[0].as_object() else {
        return false;
    };
    let Some(part_text) = obj.get("text").and_then(|v| v.as_str()) else {
        return false;
    };
    if part_text != message_text {
        return false;
    }
    matches!(
        obj.get("kind").and_then(|v| v.as_str()),
        None | Some("run" | "text")
    )
}

/// CSV `handle_type` cell: the sender's handle type, inferred from the sender
/// handle with the same rules the EML/mbox reader uses on re-import. Empty
/// when the message has no sender handle.
fn sender_handle_type_cell(sender_handle: Option<&str>) -> &'static str {
    match sender_handle {
        Some(handle) => crate::util::infer_handle_type(handle).as_str(),
        None => "",
    }
}

/// Per-conversation CSV using the unified [`CSV_HEADERS`] contract.
///
/// # Errors
///
/// Returns an error when a message's timestamp cannot be formatted or the
/// file cannot be written.
pub(crate) fn write_conversation_csv(
    output_dir: &Path,
    doc: &ConversationDocument,
) -> Result<PathBuf> {
    let path = output_dir.join(format!("{}.csv", doc.filename_stem()));
    let participants_json = json_cell(
        &doc.conversation
            .participants
            .iter()
            .map(|p| ParticipantCell {
                handle: p.handle.clone().unwrap_or_default(),
                display_name: p.display_name.clone().unwrap_or_default(),
                handle_type: p.handle_type,
            })
            .collect::<Vec<_>>(),
    );

    util::write_atomic(&path, |out| {
        let mut wtr = csv::Writer::from_writer(out);
        wtr.write_record(CSV_HEADERS)
            .with_context(|| format!("write header {}", path.display()))?;
        for msg in &doc.messages {
            let cells = MessageCells::new(msg)?;
            wtr.write_record(csv_record(doc, &participants_json, msg, &cells))
                .with_context(|| format!("write row {}", path.display()))?;
        }
        wtr.flush()?;
        Ok(())
    })?;

    Ok(path)
}

/// The cells of one message that have to be built rather than borrowed:
/// formatted times, JSON columns, and the `imessage` bag's columns.
struct MessageCells {
    ts_local: String,
    ts_utc: String,
    ts_display: String,
    timestamp_unix_ms: String,
    attachments_json: String,
    android_type: String,
    source_fields_json: String,
    imessage: ImessageCells,
}

impl MessageCells {
    /// # Errors
    ///
    /// Returns an error when the timestamp is outside the representable range.
    fn new(msg: &IrMessage) -> Result<Self> {
        let secs = msg.timestamp_unix_ms.div_euclid(1000);
        let (ts_local, ts_utc, ts_display) = format_local_ts(secs).ok_or_else(|| {
            anyhow::anyhow!("invalid timestamp_unix_ms {}", msg.timestamp_unix_ms)
        })?;
        let attachment_cells: Vec<AttachmentCell> = msg
            .attachments
            .iter()
            .map(|a| AttachmentCell {
                meta: a.into(),
                is_sticker: a.is_sticker,
                transcription: a.transcription.clone(),
                sticker_effect: a.sticker_effect.clone(),
            })
            .collect();
        Ok(Self {
            ts_local,
            ts_utc,
            ts_display,
            timestamp_unix_ms: msg.timestamp_unix_ms.to_string(),
            attachments_json: json_cell(&attachment_cells),
            android_type: msg
                .source
                .as_ref()
                .and_then(|s| s.android_type)
                .map(|n| n.to_string())
                .unwrap_or_default(),
            source_fields_json: msg
                .source
                .as_ref()
                .filter(|s| !s.fields.is_empty())
                .map(|s| serde_json::to_string(&s.fields).unwrap_or_default())
                .unwrap_or_default(),
            imessage: ImessageCells::new(&msg.text, msg.imessage.as_ref()),
        })
    }
}

/// The `imessage` bag's columns as cell text; every cell is blank when the
/// message has no bag.
#[derive(Default)]
struct ImessageCells {
    read_receipt: String,
    is_deleted: bool,
    send_effect: String,
    shared_location: String,
    announcement: String,
    is_reply: bool,
    thread_originator_guid: String,
    thread_originator_part: String,
    num_replies: String,
    parts_json: String,
    edits_json: String,
    tapbacks_json: String,
    app_json: String,
    balloon_bundle_id: String,
    balloon_kind: String,
    associated_guid: String,
    associated_part: String,
    tapback_kind: String,
    tapback_emoji: String,
    tapback_action: String,
}

impl ImessageCells {
    /// `text` is the message text, which decides whether the parts column
    /// repeats it and is therefore left blank.
    fn new(text: &str, im: Option<&IrImessage>) -> Self {
        let Some(im) = im else {
            return Self {
                parts_json: parts_cell_for_csv(text, None),
                ..Self::default()
            };
        };
        Self {
            read_receipt: text_cell(im.read_receipt_rfc3339.as_deref()),
            is_deleted: im.is_deleted,
            send_effect: text_cell(im.send_effect.as_deref()),
            shared_location: text_cell(im.shared_location.as_deref()),
            announcement: text_cell(im.announcement.as_deref()),
            is_reply: im.is_reply,
            thread_originator_guid: text_cell(im.in_reply_to_guid.as_deref()),
            thread_originator_part: number_cell(im.thread_originator_part),
            num_replies: number_cell(im.num_replies),
            parts_json: parts_cell_for_csv(text, im.parts.as_ref()),
            edits_json: value_cell(im.edits.as_ref()),
            tapbacks_json: value_cell(im.tapbacks.as_ref()),
            app_json: value_cell(im.app.as_ref()),
            balloon_bundle_id: text_cell(im.balloon_bundle_id.as_deref()),
            balloon_kind: text_cell(im.balloon_kind.as_deref()),
            associated_guid: text_cell(im.associated_guid.as_deref()),
            associated_part: number_cell(im.associated_part),
            tapback_kind: text_cell(im.tapback_kind.as_deref()),
            tapback_emoji: text_cell(im.tapback_emoji.as_deref()),
            tapback_action: text_cell(im.tapback_action.as_deref()),
        }
    }
}

/// An optional string as a cell: the string, or blank.
fn text_cell(value: Option<&str>) -> String {
    value.unwrap_or_default().to_string()
}

/// An optional number as a cell: the number, or blank.
fn number_cell(value: Option<u32>) -> String {
    value.map(|n| n.to_string()).unwrap_or_default()
}

/// `true` or `false`.
fn bool_cell(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

/// One CSV row in [`CSV_HEADERS`] order.
fn csv_record<'a>(
    doc: &'a ConversationDocument,
    participants_json: &'a str,
    msg: &'a IrMessage,
    cells: &'a MessageCells,
) -> [&'a str; 46] {
    let im = &cells.imessage;
    [
        doc.conversation.chat_identifier.as_str(),
        doc.conversation.conversation_type.as_str(),
        doc.conversation.group_title.as_deref().unwrap_or(""),
        participants_json,
        msg.guid.as_str(),
        cells.ts_local.as_str(),
        cells.ts_utc.as_str(),
        cells.ts_display.as_str(),
        cells.timestamp_unix_ms.as_str(),
        msg.direction.as_str(),
        msg.service.as_str(),
        msg.sender_handle.as_deref().unwrap_or(""),
        msg.sender_display_name.as_deref().unwrap_or(""),
        sender_handle_type_cell(msg.sender_handle.as_deref()),
        msg.subject.as_deref().unwrap_or(""),
        msg.text.as_str(),
        cells.attachments_json.as_str(),
        msg.message_kind.as_str(),
        doc.export.source.as_str(),
        doc.export.tool.as_str(),
        doc.export.tool_version.as_str(),
        doc.export.owner_handle.as_deref().unwrap_or(""),
        doc.export.owner_display_name.as_deref().unwrap_or(""),
        cells.android_type.as_str(),
        cells.source_fields_json.as_str(),
        im.read_receipt.as_str(),
        bool_cell(im.is_deleted),
        im.send_effect.as_str(),
        im.shared_location.as_str(),
        bool_cell(msg.message_kind == IrMessageKind::Announcement),
        im.announcement.as_str(),
        bool_cell(im.is_reply),
        im.thread_originator_guid.as_str(),
        im.thread_originator_part.as_str(),
        im.num_replies.as_str(),
        im.parts_json.as_str(),
        im.edits_json.as_str(),
        im.tapbacks_json.as_str(),
        im.app_json.as_str(),
        im.balloon_bundle_id.as_str(),
        im.balloon_kind.as_str(),
        im.associated_guid.as_str(),
        im.associated_part.as_str(),
        im.tapback_kind.as_str(),
        im.tapback_emoji.as_str(),
        im.tapback_action.as_str(),
    ]
}

/// Write a conversation as EML files or one mbox, by `package`.
fn write_conversation_mail(
    output_dir: &Path,
    doc: &ConversationDocument,
    package: MailPackage,
) -> Result<PathBuf> {
    let messages = document_to_mail_messages(doc, output_dir)?;
    if messages.is_empty() {
        bail!("conversation has no messages");
    }
    write_mail_package(output_dir, package, &messages)
}

/// Build [`MailMessage`] list from IR (reads attachment bytes from disk when missing).
///
/// # Errors
///
/// Returns an error when an attachment file cannot be read from disk.
pub fn document_to_mail_messages(
    doc: &ConversationDocument,
    output_dir: &Path,
) -> Result<Vec<MailMessage>> {
    let participants: Vec<Participant> = doc
        .conversation
        .participants
        .iter()
        .map(|p| Participant {
            handle: p.handle.clone().unwrap_or_default(),
            display_name: p.display_name.clone(),
        })
        .collect();

    let mut out = Vec::with_capacity(doc.messages.len());
    for msg in &doc.messages {
        let mut attachments = Vec::with_capacity(msg.attachments.len());
        for a in &msg.attachments {
            let bytes = util::load_attachment_bytes_strict(a, output_dir)?;
            attachments.push(MailAttachment {
                bytes,
                meta: a.into(),
                is_sticker: a.is_sticker,
                transcription: a.transcription.clone(),
                sticker_effect: a.sticker_effect.clone(),
            });
        }

        out.push(MailMessage {
            chat_identifier: doc.conversation.chat_identifier.clone(),
            conversation_type: doc.conversation.conversation_type.as_str().to_string(),
            group_title: doc.conversation.group_title.clone(),
            participants: participants.clone(),
            owner_handle: doc.export.owner_handle.clone().unwrap_or_default(),
            owner_display_name: doc.export.owner_display_name.clone(),
            export_source: doc.export.source.clone(),
            export_tool: doc.export.tool.clone(),
            export_tool_version: doc.export.tool_version.clone(),
            filename_suffix: doc.packaging_stem_suffix.clone(),
            message: msg.clone(),
            attachments,
        });
    }
    Ok(out)
}
