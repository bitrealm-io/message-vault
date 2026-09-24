//! Import-side records mapped from message-ir JSONL.

use anyhow::{Context, Result};
use chrono::{TimeZone, Utc};
use message_ir::{
    ConversationHeader, HandleService, HandleType, IrAttachment, IrDirection, IrImessage,
    IrMessage, IrMessageKind, IrService, check_schema_version_in_json,
};
use phone::sanitize_number;
use serde::Deserialize;
use serde_json::Value;

/// One JSONL conversation after IR → vault-row mapping.
#[derive(Debug, Clone)]
pub enum ExportRecord {
    /// A conversation header record.
    Conversation(ConversationRecord),
    /// One message record.
    Message(MessageRecord),
}

/// The conversation header of one JSONL conversation.
#[derive(Debug, Clone)]
pub struct ConversationRecord {
    /// Original chat id from the export.
    pub chat_identifier: String,
    /// Platform service, e.g. `imessage`.
    pub service: Option<String>,
    /// `individual` or `group`.
    pub conversation_type: String,
    /// Group label, when set.
    pub group_title: Option<String>,
    /// Participants of the conversation.
    pub participants: Vec<ParticipantRecord>,
    /// UTC time the export was produced.
    pub exported_at: Option<String>,
    /// IR `export.source` — used as `messages.source` for directory import.
    pub export_source: Option<String>,
}

/// One participant of an imported conversation.
#[derive(Debug, Clone)]
pub struct ParticipantRecord {
    /// Raw identity value. `None` when the source named this person and
    /// recorded no address for them; `name_alias` then carries who they are.
    pub handle: Option<String>,
    /// Display-name alias, when the export supplied one.
    pub name_alias: Option<String>,
    /// Handle type (phone, email, or username).
    pub handle_type: Option<HandleType>,
}

/// One message of an imported conversation.
#[derive(Debug, Clone)]
pub struct MessageRecord {
    /// Export GUID for replies and grouping.
    pub guid: Option<String>,
    /// The instant the message was sent: RFC 3339 in UTC with a `Z` suffix.
    pub timestamp: String,
    /// True for messages sent by the account owner.
    pub is_from_me: bool,
    /// Sender handle for incoming messages.
    pub sender: Option<String>,
    /// Sender handle type (phone, email, or username).
    pub sender_handle_type: Option<HandleType>,
    /// The account holder's own address on this message, sent from or
    /// received at: the message's owner handle, else the header's.
    pub owner: Option<String>,
    /// Per-message transport (`sms` / `imessage` / `rcs` / `whatsapp` / …).
    pub service: Option<String>,
    /// Subject line, when set.
    pub subject: Option<String>,
    /// Body text, when present.
    pub text: Option<String>,
    /// True for group announcements.
    pub is_announcement: bool,
    /// Announcement text when `is_announcement`.
    pub announcement: Option<String>,
    /// Attachments on this message.
    pub attachments: Vec<AttachmentRecord>,
    /// Reactions on this message.
    pub tapbacks: Vec<TapbackRecord>,
    /// True when part of a reply thread.
    pub is_reply: bool,
    /// GUID of the message this replies to.
    pub thread_originator_guid: Option<String>,
    /// Part index of the originator (for tapbacks).
    pub thread_originator_part: Option<i64>,
    /// Replies in this thread.
    pub num_replies: i64,
}

/// One attachment of an imported message.
#[derive(Debug, Clone)]
pub struct AttachmentRecord {
    /// Path inside the export.
    pub path: Option<String>,
    /// File name from the export.
    pub original_name: Option<String>,
    /// MIME type, when known.
    pub mime_type: Option<String>,
    /// Content fingerprint, when the exporter computed one.
    pub sha256: Option<String>,
    /// True for sticker files.
    pub is_sticker: bool,
    /// OCR/ASR transcription, when the exporter produced one.
    pub transcription: Option<String>,
    /// File size in bytes, when known.
    pub size_bytes: Option<u64>,
    /// Why the file is missing, when it is.
    pub missing_reason: Option<String>,
}

/// One tapback reaction on an imported message.
#[derive(Debug, Clone)]
pub struct TapbackRecord {
    /// Attachment part the reaction applies to.
    pub part_index: i64,
    /// Reaction type, e.g. `love`.
    pub kind: String,
    /// Emoji form of the reaction, when one exists.
    pub emoji: Option<String>,
    /// True when the account owner reacted.
    pub is_from_me: bool,
    /// Reactor handle for incoming reactions.
    pub sender: Option<String>,
}

/// Strip Apple's attachment object-replacement character (U+FFFC) from body text.
pub fn clean_body(text: Option<&str>) -> Option<String> {
    text.map(|s| s.replace('\u{FFFC}', "").trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Parse message-ir JSONL lines into import records.
///
/// Accepts one or more concatenated conversations (each: header line, then
/// message lines). Remote push clients batch multiple conversations this way.
pub fn parse_ir_lines(
    lines: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<Vec<ExportRecord>> {
    use crate::imports_api::ImportFailure;

    let mut out = Vec::new();
    let mut saw_header = false;
    // The owner the current conversation's header names, for messages that
    // do not name their own.
    let mut header_owner: Option<String> = None;
    for (i, line) in lines.into_iter().enumerate() {
        let line = line.as_ref().trim();
        if line.is_empty() {
            continue;
        }
        let line_no = i + 1;
        let value: Value = serde_json::from_str(line).map_err(|e| ImportFailure::Parse {
            line: line_no,
            detail: e.to_string(),
        })?;
        if is_ir_header(&value) {
            // The version is checked before the header is deserialized, so a
            // file from another schema version is refused by its version and
            // not by whichever current field it lacks.
            check_schema_version_in_json(line).map_err(|refusal| ImportFailure::SchemaVersion {
                refusal,
                line: line_no,
            })?;
            let header: ConversationHeader =
                serde_json::from_value(value).map_err(|e| ImportFailure::Parse {
                    line: line_no,
                    detail: format!("the conversation header is not valid: {e}"),
                })?;
            out.push(ExportRecord::Conversation(conversation_from_ir(&header)));
            header_owner = header
                .export
                .owner_handle
                .as_deref()
                .and_then(message_ir::nonempty);
            saw_header = true;
        } else {
            if !saw_header {
                return Err(ImportFailure::Parse {
                    line: line_no,
                    detail: "a message appears before the conversation header".into(),
                }
                .into());
            }
            let msg: IrMessage =
                serde_json::from_value(value).map_err(|e| ImportFailure::Parse {
                    line: line_no,
                    detail: format!("the message is not valid: {e}"),
                })?;
            let record = message_from_ir(&msg, header_owner.as_deref()).map_err(|e| {
                ImportFailure::Parse {
                    line: line_no,
                    detail: format!("{e:#}"),
                }
            })?;
            out.push(ExportRecord::Message(record));
        }
    }
    if out.is_empty() {
        return Err(ImportFailure::Parse {
            line: 1,
            detail: "the file has no conversation header".into(),
        }
        .into());
    }
    Ok(out)
}

/// True when the JSON object is a conversation header rather than a message.
fn is_ir_header(value: &Value) -> bool {
    value.get("schema_version").is_some() && value.get("conversation").is_some()
}

/// Map a JSON Lines header onto the server's conversation record.
fn conversation_from_ir(header: &ConversationHeader) -> ConversationRecord {
    let export_source = {
        let s = header.export.source.trim();
        if s.is_empty() {
            None
        } else {
            Some(s.to_string())
        }
    };
    ConversationRecord {
        chat_identifier: header.conversation.chat_identifier.clone(),
        // Platform identity for handles (phone | whatsapp), not SMS/iMessage/RCS.
        service: Some(
            if export_source
                .as_deref()
                .is_some_and(|s| s.eq_ignore_ascii_case("whatsapp"))
            {
                HandleService::Whatsapp.as_str().to_string()
            } else {
                HandleService::Phone.as_str().to_string()
            },
        ),
        conversation_type: header.conversation.conversation_type.as_str().to_string(),
        group_title: header.conversation.group_title.clone(),
        participants: header
            .conversation
            .participants
            .iter()
            .map(|p| ParticipantRecord {
                handle: p.handle.clone(),
                name_alias: p.display_name.clone(),
                handle_type: p.handle_type,
            })
            .collect(),
        exported_at: None,
        export_source,
    }
}

/// Map one IR message onto the server's message record. `header_owner` is the
/// owner the conversation header names, used when the message names none.
fn message_from_ir(msg: &IrMessage, header_owner: Option<&str>) -> Result<MessageRecord> {
    let secs = msg.timestamp_unix_ms.div_euclid(1000);
    let timestamp = format_utc_timestamp(secs).with_context(|| {
        format!(
            "unrepresentable timestamp_unix_ms {}",
            msg.timestamp_unix_ms
        )
    })?;
    let is_from_me = msg.direction == IrDirection::Outgoing;
    let im = msg.imessage.as_ref();
    let text = {
        let t = msg.text.trim();
        if t.is_empty() {
            None
        } else {
            Some(msg.text.clone())
        }
    };
    let mut tapbacks = tapbacks_from_im(im, is_from_me, msg.sender_handle.as_deref());
    if tapbacks.is_empty()
        && let Some(kind) = im
            .and_then(|i| i.tapback_kind.as_ref())
            .filter(|s| !s.is_empty())
    {
        tapbacks.push(TapbackRecord {
            part_index: i64::from(im.and_then(|i| i.associated_part).unwrap_or(0)),
            kind: kind.clone(),
            emoji: im.and_then(|i| i.tapback_emoji.clone()),
            is_from_me,
            sender: if is_from_me {
                None
            } else {
                msg.sender_handle.clone()
            },
        });
    }

    Ok(MessageRecord {
        guid: if msg.guid.trim().is_empty() {
            None
        } else {
            Some(msg.guid.clone())
        },
        timestamp,
        is_from_me,
        sender: if is_from_me {
            None
        } else {
            msg.sender_handle.clone()
        },
        sender_handle_type: if is_from_me {
            None
        } else {
            infer_sender_handle_type(msg.sender_handle.as_deref(), msg.service)
        },
        owner: msg
            .owner_handle
            .as_deref()
            .and_then(message_ir::nonempty)
            .or_else(|| header_owner.map(str::to_string)),
        service: Some(msg.service.as_str().to_string()),
        subject: msg.subject.clone().filter(|s| !s.is_empty()),
        text,
        is_announcement: im.is_some_and(|i| i.announcement.is_some())
            || matches!(msg.message_kind, IrMessageKind::Announcement),
        announcement: im.and_then(|i| i.announcement.clone()),
        attachments: msg.attachments.iter().map(attachment_from_ir).collect(),
        tapbacks,
        is_reply: im.is_some_and(|i| i.is_reply),
        thread_originator_guid: im.and_then(|i| i.in_reply_to_guid.clone()),
        thread_originator_part: im.and_then(|i| i.thread_originator_part.map(i64::from)),
        num_replies: im.and_then(|i| i.num_replies.map(i64::from)).unwrap_or(0),
    })
}

/// Infer the sender's handle type for import records.
///
/// IR participants carry an explicit `handle_type` when the source knows it;
/// message rows only carry a raw sender handle, so the type is inferred here
/// from the handle shape plus the service. Handles containing `@` are emails;
/// SMS/iMessage/WhatsApp/RCS handles that sanitize as phone numbers are
/// phones; anything else is `Other`.
fn infer_sender_handle_type(sender_handle: Option<&str>, service: IrService) -> Option<HandleType> {
    let handle = sender_handle?.trim();
    if handle.is_empty() {
        return None;
    }
    if handle.contains('@') {
        return Some(HandleType::Email);
    }
    if matches!(
        service,
        IrService::Sms | IrService::IMessage | IrService::Whatsapp | IrService::Rcs
    ) && sanitize_number(handle).is_some()
    {
        return Some(HandleType::Phone);
    }
    Some(HandleType::Other)
}

/// Map one IR attachment onto the server's attachment record.
fn attachment_from_ir(a: &IrAttachment) -> AttachmentRecord {
    AttachmentRecord {
        path: a.path.clone(),
        original_name: a.original_name.clone(),
        mime_type: a.mime_type.clone(),
        sha256: a.digest_sha256.clone(),
        is_sticker: a.is_sticker,
        transcription: a.transcription.clone(),
        size_bytes: a.size_bytes,
        missing_reason: a.missing_reason.clone(),
    }
}

/// Tapback rows from the iMessage extension, falling back to the message's own sender and direction.
fn tapbacks_from_im(
    im: Option<&IrImessage>,
    fallback_from_me: bool,
    fallback_sender: Option<&str>,
) -> Vec<TapbackRecord> {
    let Some(im) = im else {
        return Vec::new();
    };
    let Some(raw) = im.tapbacks.as_ref() else {
        return Vec::new();
    };
    let items = match raw {
        Value::Array(items) => items.clone(),
        other if !other.is_null() => vec![other.clone()],
        _ => return Vec::new(),
    };
    items
        .into_iter()
        .filter_map(|v| {
            let t: WireTapback = serde_json::from_value(v).ok()?;
            Some(TapbackRecord {
                part_index: t.part_index,
                kind: t.kind,
                emoji: t.emoji,
                is_from_me: t.is_from_me.unwrap_or(fallback_from_me),
                sender: t.sender.or_else(|| fallback_sender.map(|s| s.to_string())),
            })
        })
        .collect()
}

#[derive(Debug, Deserialize)]
struct WireTapback {
    #[serde(default)]
    part_index: i64,
    kind: String,
    #[serde(default)]
    emoji: Option<String>,
    #[serde(default)]
    is_from_me: Option<bool>,
    #[serde(default)]
    sender: Option<String>,
}

/// The UTC RFC 3339 string (`Z` suffix) for a Unix timestamp, or `None` when
/// it cannot be represented. The vault stores the instant and nothing about
/// where the phone was; the account's time zone turns it into a clock reading.
fn format_utc_timestamp(secs: i64) -> Option<String> {
    Some(
        Utc.timestamp_opt(secs, 0)
            .single()?
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ir_sms_without_imessage_bag() {
        let lines = [
            r#"{"schema_version":4,"export":{"source":"sms-backup-restore","tool":"t","tool_version":"1","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+15555550101","conversation_type":"individual","group_title":null,"participants":[{"handle":"+15555550101","display_name":"Sam"}],"stats":{"message_count":1,"attachment_count":0,"first_timestamp_unix_ms":1400773261000,"last_timestamp_unix_ms":1400773261000}}}"#.to_string(),
            r#"{"guid":"g1","timestamp_unix_ms":1400773261000,"direction":"incoming","service":"sms","message_kind":"sms","sender_handle":"+15555550101","sender_display_name":"Sam","subject":null,"text":"hello","attachments":[],"imessage":null,"source":null}"#.to_string(),
        ];
        let records = parse_ir_lines(lines).unwrap();
        assert_eq!(records.len(), 2);
        match &records[1] {
            ExportRecord::Message(m) => {
                assert_eq!(m.guid.as_deref(), Some("g1"));
                assert!(!m.is_from_me);
                assert_eq!(m.text.as_deref(), Some("hello"));
                assert_eq!(m.service.as_deref(), Some("sms"));
                assert_eq!(m.sender_handle_type, Some(HandleType::Phone));
                assert!(m.tapbacks.is_empty());
                assert!(!m.is_reply);
            }
            _ => panic!("expected message"),
        }
    }

    #[test]
    fn parses_concatenated_ir_conversations() {
        let header = |chat: &str| {
            format!(
                r#"{{"schema_version":4,"export":{{"source":"sms-backup-restore","tool":"t","tool_version":"1","owner_handle":null,"owner_display_name":null}},"conversation":{{"chat_identifier":"{chat}","conversation_type":"individual","group_title":null,"participants":[{{"handle":"{chat}","display_name":null}}],"stats":{{"message_count":1,"attachment_count":0,"first_timestamp_unix_ms":1400773261000,"last_timestamp_unix_ms":1400773261000}}}}}}"#
            )
        };
        let msg = |guid: &str, handle: &str| {
            format!(
                r#"{{"guid":"{guid}","timestamp_unix_ms":1400773261000,"direction":"incoming","service":"sms","message_kind":"sms","sender_handle":"{handle}","sender_display_name":null,"subject":null,"text":"hi","attachments":[],"imessage":null,"source":null}}"#
            )
        };
        let records = parse_ir_lines([
            header("+15555550101"),
            msg("g1", "+15555550101"),
            header("+15555550102"),
            msg("g2", "+15555550102"),
        ])
        .unwrap();
        assert_eq!(records.len(), 4);
        match &records[0] {
            ExportRecord::Conversation(c) => assert_eq!(c.chat_identifier, "+15555550101"),
            _ => panic!("expected conversation"),
        }
        match &records[2] {
            ExportRecord::Conversation(c) => assert_eq!(c.chat_identifier, "+15555550102"),
            _ => panic!("expected conversation"),
        }
    }

    #[test]
    fn infers_sender_handle_type_from_handle_and_service() {
        assert_eq!(
            infer_sender_handle_type(Some("alice@example.com"), IrService::Unknown),
            Some(HandleType::Email)
        );
        assert_eq!(
            infer_sender_handle_type(Some("+15555550101"), IrService::Sms),
            Some(HandleType::Phone)
        );
        assert_eq!(
            infer_sender_handle_type(Some("+15555550101"), IrService::Signal),
            Some(HandleType::Other)
        );
        assert_eq!(
            infer_sender_handle_type(Some("alice_discord"), IrService::Discord),
            Some(HandleType::Other)
        );
        assert_eq!(infer_sender_handle_type(None, IrService::Sms), None);
    }

    #[test]
    fn parse_ir_lines_refuses_schema_3_as_a_failure() {
        let header = r#"{"schema_version":3,"export":{"source":"whatsapp","tool":"t","owner_handle":"+1","owner_display_name":"Me"},"conversation":{"chat_identifier":"+2","conversation_type":"individual","participants":[]}}"#;
        let err = parse_ir_lines([header]).unwrap_err();
        let failure = crate::imports_api::ImportFailure::in_error(&err).expect("typed failure");
        assert_eq!(
            *failure,
            crate::imports_api::ImportFailure::SchemaVersion {
                refusal: message_ir::UnsupportedSchemaVersion { found: 3 },
                line: 1
            }
        );
    }

    #[test]
    fn parse_ir_lines_reports_a_non_json_line_as_a_failure() {
        let err = parse_ir_lines(["this is not json"]).unwrap_err();
        let failure = crate::imports_api::ImportFailure::in_error(&err).expect("typed failure");
        match failure {
            crate::imports_api::ImportFailure::Parse { line, .. } => assert_eq!(*line, 1),
            other => panic!("expected Parse, got {other:?}"),
        }
    }

    #[test]
    fn parse_ir_lines_reports_a_message_before_any_header_as_a_failure() {
        let err = parse_ir_lines([r#"{"guid":"m1"}"#]).unwrap_err();
        let failure = crate::imports_api::ImportFailure::in_error(&err).expect("typed failure");
        match failure {
            crate::imports_api::ImportFailure::Parse { line, detail } => {
                assert_eq!(*line, 1);
                assert!(
                    detail.contains("before the conversation header"),
                    "{detail}"
                );
            }
            other => panic!("expected Parse, got {other:?}"),
        }
    }

    #[test]
    fn parse_ir_lines_reports_a_message_with_an_impossible_timestamp_as_a_failure() {
        let header = r#"{"schema_version":4,"export":{"source":"sms-backup-restore","tool":"t","tool_version":"1","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+15555550101","conversation_type":"individual","group_title":null,"participants":[{"handle":"+15555550101","display_name":"Sam"}],"stats":{"message_count":1,"attachment_count":0,"first_timestamp_unix_ms":1400773261000,"last_timestamp_unix_ms":1400773261000}}}"#;
        let msg = r#"{"guid":"g1","timestamp_unix_ms":9223372036854775807,"direction":"incoming","service":"sms","message_kind":"sms","sender_handle":"+15555550101","sender_display_name":"Sam","subject":null,"text":"hello","attachments":[],"imessage":null,"source":null}"#;
        let err = parse_ir_lines([header, msg]).unwrap_err();
        let failure = crate::imports_api::ImportFailure::in_error(&err).expect("typed failure");
        match failure {
            crate::imports_api::ImportFailure::Parse { line, .. } => assert_eq!(*line, 2),
            other => panic!("expected Parse, got {other:?}"),
        }
    }
}
