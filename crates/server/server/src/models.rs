//! Import-side records mapped from message-ir JSONL.

use anyhow::{Context, Result};
use chrono::{TimeZone, Utc};
use message_ir::{
    ConversationHeader, HandleService, HandleType, IrAttachment, IrDirection, IrImessage,
    IrMessage, IrMessageKind, check_schema_version_in_json,
};
use phone::Handle;
use serde::Deserialize;
use serde_json::Value;

/// One JSONL conversation after IR → database-row mapping.
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
    /// Raw identity value. For a person the source named with no address it
    /// is the name, and `handle_type` is `Other`.
    pub handle: String,
    /// Display-name alias, when the export supplied one.
    pub name_alias: Option<String>,
    /// Handle type (phone, email, username, or other).
    pub handle_type: Option<HandleType>,
}

/// One message of an imported conversation.
#[derive(Debug, Clone)]
pub struct MessageRecord {
    /// The message's id from the export, never empty: Apple's own for Apple
    /// Messages, otherwise the exporter's `MessageGuid`. Production skips a
    /// guid it already holds, which is what lets a batch be sent again.
    pub guid: String,
    /// The line the message is on in its file or batch, counted from 1 with
    /// blank lines included, so a refusal of one of its attachments names it.
    pub line: usize,
    /// The instant the message was sent: RFC 3339 in UTC with a `Z` suffix.
    pub timestamp: String,
    /// True for messages sent by the account owner.
    pub is_from_me: bool,
    /// Sender handle for incoming messages: the address, or the name when the
    /// source named the sender with no address (`sender_handle_type` is then
    /// `Other`).
    pub sender: Option<String>,
    /// The sender's identity type read from the address alone (phone, email
    /// or other). Staging prefers the type the header gives a participant
    /// with the same address.
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
    use crate::imports_api::{ImportFailure, MISSING_GUID_LINES_NAMED};

    let mut out = Vec::new();
    let mut saw_header = false;
    // The owner the current conversation's header names, for messages that
    // do not name their own.
    let mut header_owner: Option<String> = None;
    // Messages without a guid, counted and named up to a limit, so one
    // refusal names every line to fix rather than the first.
    let mut missing_guid_lines = Vec::new();
    let mut missing_guid_total = 0usize;
    for (i, line) in lines.into_iter().enumerate() {
        let line = line.as_ref().trim();
        if line.is_empty() {
            continue;
        }
        let line_no = i + 1;
        let value: Value = serde_json::from_str(line).map_err(|e| ImportFailure::NotJson {
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
                serde_json::from_value(value).map_err(|e| ImportFailure::Invalid {
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
                return Err(ImportFailure::Invalid {
                    line: line_no,
                    detail: "a message appears before the conversation header".into(),
                }
                .into());
            }
            let msg: IrMessage =
                serde_json::from_value(value).map_err(|e| ImportFailure::Invalid {
                    line: line_no,
                    detail: format!("the message is not valid: {e}"),
                })?;
            // A reaction row is not a message. The reaction reaches the
            // message it reacts to through that message's `tapbacks` list,
            // which already leaves removed reactions out.
            if matches!(
                msg.message_kind,
                IrMessageKind::Tapback | IrMessageKind::StickerTapback
            ) {
                continue;
            }
            if msg.guid.trim().is_empty() {
                missing_guid_total += 1;
                if missing_guid_lines.len() < MISSING_GUID_LINES_NAMED {
                    missing_guid_lines.push(line_no);
                }
                continue;
            }
            let record = message_from_ir(&msg, header_owner.as_deref(), line_no).map_err(|e| {
                ImportFailure::Invalid {
                    line: line_no,
                    detail: format!("{e:#}"),
                }
            })?;
            out.push(ExportRecord::Message(record));
        }
    }
    if missing_guid_total > 0 {
        return Err(ImportFailure::MissingGuid {
            lines: missing_guid_lines,
            total: missing_guid_total,
        }
        .into());
    }
    if out.is_empty() {
        return Err(ImportFailure::Invalid {
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
            .filter_map(participant_from_ir)
            .collect(),
        exported_at: None,
        export_source,
    }
}

/// Map one IR message onto the server's message record. `header_owner` is the
/// owner the conversation header names, used when the message names none.
fn message_from_ir(
    msg: &IrMessage,
    header_owner: Option<&str>,
    line: usize,
) -> Result<MessageRecord> {
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
    let tapbacks = tapbacks_from_im(im);
    let sender = if is_from_me {
        None
    } else {
        sender_identity(
            msg.sender_handle.as_deref(),
            msg.sender_display_name.as_deref(),
        )
    };

    Ok(MessageRecord {
        guid: msg.guid.clone(),
        line,
        timestamp,
        is_from_me,
        sender: sender.as_ref().map(|(value, _)| value.clone()),
        sender_handle_type: sender.and_then(|(_, kind)| kind),
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

/// One header participant as a record, or `None` for one that gives neither
/// an address nor a name and so says nothing.
///
/// Every participant is an identity. A person the source names with no
/// address gets an identity of type `other` whose value is the name, so the
/// same name on one service is one identity and one contact on every import
/// (`docs/architecture/contacts-identities-and-messages.md`).
fn participant_from_ir(p: &message_ir::IrParticipant) -> Option<ParticipantRecord> {
    let name_alias = p.display_name.clone();
    if let Some(handle) = p.handle.as_deref().and_then(message_ir::nonempty) {
        return Some(ParticipantRecord {
            handle,
            name_alias,
            handle_type: p.handle_type,
        });
    }
    let name = p.display_name.as_deref().and_then(message_ir::nonempty)?;
    Some(ParticipantRecord {
        handle: name,
        name_alias,
        handle_type: Some(HandleType::Other),
    })
}

/// An incoming message's sender as an identity value and, when known, its
/// type: the address, else the name the source gave with no address as an
/// identity of type `other`, the rule [`participant_from_ir`] applies.
/// `None` when the message names neither.
fn sender_identity(
    address: Option<&str>,
    name: Option<&str>,
) -> Option<(String, Option<HandleType>)> {
    if let Some(address) = address.and_then(message_ir::nonempty) {
        let kind = sender_handle_type(Some(address.as_str()));
        return Some((address, kind));
    }
    let name = name.and_then(message_ir::nonempty)?;
    Some((name, Some(HandleType::Other)))
}

/// The type of a sender's identity, read from the address alone.
///
/// A message carries only the sender's address, never its type. Staging uses
/// the type the header gives the participant with the same address, and this
/// one only when the header lists no such participant. It is
/// [`Handle::parse`], the one rule for what an address is, and it does not
/// read the message's service: a contact's number is a phone number on a
/// service the model does not know too, such as a message Apple Messages sent
/// by satellite (#1144).
fn sender_handle_type(sender_handle: Option<&str>) -> Option<HandleType> {
    sender_handle.and_then(Handle::parse).map(|h| h.kind())
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

/// Tapback rows from the iMessage extension. Each entry names its own
/// reactor: `is_from_me` when the owner reacted, `reactor_handle` otherwise.
/// An entry is never given the author or the direction of the message it
/// reacts to, because the reactor is rarely the author.
fn tapbacks_from_im(im: Option<&IrImessage>) -> Vec<TapbackRecord> {
    let Some(raw) = im.and_then(|im| im.tapbacks.as_ref()) else {
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
                is_from_me: t.is_from_me,
                sender: if t.is_from_me { None } else { t.reactor_handle },
            })
        })
        .collect()
}

/// One entry of an `imessage.tapbacks` list, as the Apple Messages reader
/// writes it.
#[derive(Debug, Deserialize)]
struct WireTapback {
    #[serde(default)]
    part_index: i64,
    kind: String,
    #[serde(default)]
    emoji: Option<String>,
    #[serde(default)]
    is_from_me: bool,
    #[serde(default)]
    reactor_handle: Option<String>,
}

/// The UTC RFC 3339 string (`Z` suffix) for a Unix timestamp, or `None` when
/// it cannot be represented. The server stores the instant and nothing about
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
                assert_eq!(m.guid, "g1");
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

    /// The header and one incoming iMessage whose `subject` and `imessage`
    /// fields are the JSON given.
    fn message_with(subject: &str, imessage: &str) -> MessageRecord {
        let header = r#"{"schema_version":4,"export":{"source":"imessage","tool":"t","tool_version":"1","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+15555550101","conversation_type":"individual","group_title":null,"participants":[{"handle":"+15555550101","display_name":"Sam"}],"stats":{"message_count":1,"attachment_count":0,"first_timestamp_unix_ms":1400773261000,"last_timestamp_unix_ms":1400773261000}}}"#.to_string();
        let message = format!(
            r#"{{"guid":"g1","timestamp_unix_ms":1400773261000,"direction":"incoming","service":"imessage","message_kind":"imessage","sender_handle":"+15555550101","sender_display_name":"Sam","subject":{subject},"text":"hello","attachments":[],"imessage":{imessage},"source":null}}"#
        );
        let mut records = parse_ir_lines([header, message]).unwrap();
        match records.pop() {
            Some(ExportRecord::Message(m)) => m,
            _ => panic!("expected message"),
        }
    }

    #[test]
    fn a_subject_is_kept_and_an_empty_one_is_none() {
        assert_eq!(
            message_with(r#""Dinner on Friday""#, "null")
                .subject
                .as_deref(),
            Some("Dinner on Friday")
        );
        assert_eq!(message_with(r#""""#, "null").subject, None);
        assert_eq!(message_with("null", "null").subject, None);
    }

    /// A reaction the Apple Messages reader writes names its reactor in
    /// `reactor_handle` and says in `is_from_me` whether the owner reacted.
    /// Neither is taken from the message reacted to (#1213).
    #[test]
    fn a_reaction_keeps_the_reactor_the_reader_wrote() {
        let im = message_ir::IrImessage {
            tapbacks: Some(serde_json::json!([
                {"part_index": 0, "kind": "loved", "is_from_me": false,
                 "reactor_handle": "+15555550110", "reactor_display_name": "Sam"},
                {"part_index": 0, "kind": "liked", "is_from_me": true,
                 "reactor_display_name": "Me"}
            ])),
            ..Default::default()
        };
        // The owner's own message, reacted to by Sam, then by the owner.
        let rows = tapbacks_from_im(Some(&im));
        assert_eq!(
            rows[0].sender.as_deref(),
            Some("+15555550110"),
            "Sam's reaction"
        );
        assert!(!rows[0].is_from_me, "Sam's reaction read as the owner's");
        assert_eq!(rows[1].sender, None, "the owner's reaction");
        assert!(rows[1].is_from_me, "the owner's reaction read as Sam's");

        // Sam's message, with the same two reactions.
        let on_sams = message_with("null", &serde_json::to_string(&im).unwrap());
        assert_eq!(on_sams.tapbacks[0].sender.as_deref(), Some("+15555550110"));
        assert!(!on_sams.tapbacks[0].is_from_me);
        assert_eq!(on_sams.tapbacks[1].sender, None);
        assert!(on_sams.tapbacks[1].is_from_me);
    }

    /// The Apple Messages reader writes each reaction as a row of its own as
    /// well as in the `tapbacks` list of the message reacted to. The row is
    /// not a message, and it carries no reaction of its own (#1213).
    #[test]
    fn a_reaction_row_is_not_a_message() {
        let header = r#"{"schema_version":4,"export":{"source":"imessage","tool":"t","tool_version":"1","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+15555550101","conversation_type":"individual","group_title":null,"participants":[{"handle":"+15555550101","display_name":"Sam"}],"stats":{"message_count":3,"attachment_count":0,"first_timestamp_unix_ms":1400773261000,"last_timestamp_unix_ms":1400773263000}}}"#.to_string();
        let target = r#"{"guid":"g-hi","timestamp_unix_ms":1400773261000,"direction":"outgoing","service":"imessage","message_kind":"imessage","sender_handle":null,"sender_display_name":null,"subject":null,"text":"hi","attachments":[],"imessage":null,"source":null}"#.to_string();
        let row = |guid: &str, kind: &str, text: &str, action: &str| {
            format!(
                r#"{{"guid":"{guid}","timestamp_unix_ms":1400773262000,"direction":"incoming","service":"imessage","message_kind":"{kind}","sender_handle":"+15555550101","sender_display_name":"Sam","subject":null,"text":"{text}","attachments":[],"imessage":{{"is_reply":false,"is_deleted":false,"associated_guid":"g-hi","associated_part":0,"tapback_kind":"loved","tapback_action":"{action}"}},"source":null}}"#
            )
        };
        let records = parse_ir_lines([
            header,
            target,
            row("g-love", "tapback", "Loved a message", "add"),
            row("g-unlove", "tapback", "Removed Heart", "remove"),
            row(
                "g-sticker",
                "sticker_tapback",
                "Reacted with a sticker",
                "add",
            ),
        ])
        .unwrap();
        let messages: Vec<_> = records
            .iter()
            .filter_map(|r| match r {
                ExportRecord::Message(m) => Some(m),
                ExportRecord::Conversation(_) => None,
            })
            .collect();
        assert_eq!(messages.len(), 1, "{messages:?}");
        assert_eq!(messages[0].guid, "g-hi");
        assert!(messages[0].tapbacks.is_empty());
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
    fn types_a_sender_by_the_address_alone() {
        assert_eq!(
            sender_handle_type(Some("alice@example.com")),
            Some(HandleType::Email)
        );
        assert_eq!(
            sender_handle_type(Some("+1 (555) 555-0101")),
            Some(HandleType::Phone)
        );
        assert_eq!(sender_handle_type(Some("AMAZON")), Some(HandleType::Other));
        assert_eq!(sender_handle_type(Some("  ")), None);
        assert_eq!(sender_handle_type(None), None);
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
            crate::imports_api::ImportFailure::NotJson { line, .. } => assert_eq!(*line, 1),
            other => panic!("expected NotJson, got {other:?}"),
        }
    }

    #[test]
    fn parse_ir_lines_reports_a_message_before_any_header_as_a_failure() {
        let err = parse_ir_lines([r#"{"guid":"m1"}"#]).unwrap_err();
        let failure = crate::imports_api::ImportFailure::in_error(&err).expect("typed failure");
        match failure {
            crate::imports_api::ImportFailure::Invalid { line, detail } => {
                assert_eq!(*line, 1);
                assert!(
                    detail.contains("before the conversation header"),
                    "{detail}"
                );
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn parse_ir_lines_reports_a_message_with_an_impossible_timestamp_as_a_failure() {
        let header = r#"{"schema_version":4,"export":{"source":"sms-backup-restore","tool":"t","tool_version":"1","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+15555550101","conversation_type":"individual","group_title":null,"participants":[{"handle":"+15555550101","display_name":"Sam"}],"stats":{"message_count":1,"attachment_count":0,"first_timestamp_unix_ms":1400773261000,"last_timestamp_unix_ms":1400773261000}}}"#;
        let msg = r#"{"guid":"g1","timestamp_unix_ms":9223372036854775807,"direction":"incoming","service":"sms","message_kind":"sms","sender_handle":"+15555550101","sender_display_name":"Sam","subject":null,"text":"hello","attachments":[],"imessage":null,"source":null}"#;
        let err = parse_ir_lines([header, msg]).unwrap_err();
        let failure = crate::imports_api::ImportFailure::in_error(&err).expect("typed failure");
        match failure {
            crate::imports_api::ImportFailure::Invalid { line, .. } => assert_eq!(*line, 2),
            other => panic!("expected Invalid, got {other:?}"),
        }
    }
    /// A message the guid index cannot see would be stored again by every
    /// retried batch (#1162), so the whole file is refused, naming every
    /// line that has no guid, before anything is staged.
    #[test]
    fn parse_ir_lines_refuses_messages_without_a_guid_naming_every_line() {
        let header = r#"{"schema_version":4,"export":{"source":"sms-backup-restore","tool":"t","tool_version":"1","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+15555550101","conversation_type":"individual","group_title":null,"participants":[{"handle":"+15555550101","display_name":"Sam"}],"stats":{"message_count":3,"attachment_count":0,"first_timestamp_unix_ms":1400773261000,"last_timestamp_unix_ms":1400773261000}}}"#;
        let msg = |guid: &str| {
            format!(
                r#"{{"guid":"{guid}","timestamp_unix_ms":1400773261000,"direction":"incoming","service":"sms","message_kind":"sms","sender_handle":"+15555550101","sender_display_name":"Sam","subject":null,"text":"hello","attachments":[],"imessage":null,"source":null}}"#
            )
        };
        let lines = [header.to_string(), msg("g1"), msg(""), msg("   ")];
        let err = parse_ir_lines(lines).unwrap_err();
        let failure = crate::imports_api::ImportFailure::in_error(&err).expect("typed failure");
        assert_eq!(
            *failure,
            crate::imports_api::ImportFailure::MissingGuid {
                lines: vec![3, 4],
                total: 2
            }
        );
    }
}
