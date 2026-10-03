//! Map export API messages into conversation documents.
//!
//! The export API is the Message Crate HTTP server's read path. Each document
//! is later written as JSON Lines (one JSON object per line).

use std::path::Path;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, NaiveDateTime};
use message_ir::{
    ConversationDocument, ConversationMeta, ConversationStats, ExportMeta, IrAttachment,
    IrConversationType, IrDirection, IrImessage, IrMessage, IrMessageKind, IrParticipant,
    IrService, IrSource, SCHEMA_VERSION,
};
use serde_json::{Value, json};

use message_crate_api_types::{Attachment, Message, Tapback};

/// Grouping key so messages from the same chat and backup source stay together.
pub fn conversation_key(msg: &Message) -> String {
    format!("{}::{}", msg.source, msg.conversation.chat_identifier)
}

/// Build one conversation document from a seed message and the mapped rows.
pub fn build_document(
    source: &str,
    seed: &Message,
    messages: Vec<IrMessage>,
) -> ConversationDocument {
    let conversation_type = IrConversationType::parse(&seed.conversation.conversation_type);
    let participants = participants_from_seed(seed);
    let mut attachment_count = 0u64;
    let mut first_ts = None;
    let mut last_ts = None;
    for m in &messages {
        attachment_count += m.attachments.len() as u64;
        first_ts = Some(first_ts.map_or(m.timestamp_unix_ms, |t: i64| t.min(m.timestamp_unix_ms)));
        last_ts = Some(last_ts.map_or(m.timestamp_unix_ms, |t: i64| t.max(m.timestamp_unix_ms)));
    }

    ConversationDocument {
        schema_version: SCHEMA_VERSION,
        export: ExportMeta {
            source: source.to_string(),
            tool: "message-crate".into(),
            tool_version: env!("CARGO_PKG_VERSION").into(),
            owner_handle: shared_owner(&messages),
            owner_display_name: Some("Me".into()),
        },
        conversation: ConversationMeta {
            chat_identifier: seed.conversation.chat_identifier.clone(),
            conversation_type,
            group_title: seed.conversation.group_title.clone(),
            participants,
            stats: ConversationStats {
                message_count: messages.len() as u64,
                attachment_count,
                first_timestamp_unix_ms: first_ts,
                last_timestamp_unix_ms: last_ts,
            },
        },
        messages,
        packaging_stem_suffix: None,
    }
}

/// Map one exported message into the shared conversation message type.
///
/// # Errors
///
/// Returns an error when the timestamp cannot be parsed.
pub fn to_ir_message(msg: &Message, skip_attachments: bool) -> Result<IrMessage> {
    let timestamp_unix_ms = parse_timestamp_unix_ms(msg.timestamp.as_str())
        .with_context(|| format!("message {} timestamp", msg.id))?;

    let service = IrService::parse(msg.service.as_deref().unwrap_or(""));
    let direction = if msg.is_from_me {
        IrDirection::Outgoing
    } else {
        IrDirection::Incoming
    };
    let message_kind = infer_kind(msg, service);

    let attachments = if skip_attachments {
        Vec::new()
    } else {
        msg.attachments
            .iter()
            .map(to_ir_attachment)
            .collect::<Vec<_>>()
    };

    let imessage = IrImessage {
        is_reply: msg.is_reply,
        in_reply_to_guid: msg.thread_originator_guid.clone(),
        thread_originator_part: msg
            .thread_originator_part
            .and_then(|n| u32::try_from(n).ok()),
        num_replies: u32::try_from(msg.num_replies).ok().filter(|&n| n > 0),
        // An announcement with no text carries no information, so drop it.
        announcement: msg
            .is_announcement
            .then(|| msg.text.clone().unwrap_or_default())
            .filter(|text| !text.is_empty()),
        tapbacks: tapbacks_json(&msg.tapbacks),
        ..Default::default()
    };

    let guid = msg
        .guid
        .clone()
        .filter(|g| !g.trim().is_empty())
        .unwrap_or_else(|| format!("server:{}", msg.id));

    // Keep the server's row id and source name so a later push can trace
    // each message back to the server it came from.
    let mut source_fields = serde_json::Map::new();
    source_fields.insert("server_message_id".into(), json!(msg.id));
    source_fields.insert("server_source".into(), json!(msg.source));

    Ok(IrMessage {
        guid,
        timestamp_unix_ms,
        direction,
        service,
        message_kind,
        sender_handle: msg.sender.clone(),
        sender_display_name: None,
        owner_handle: msg.owner.clone().filter(|o| !o.trim().is_empty()),
        subject: msg.subject.clone(),
        text: msg.text.clone().unwrap_or_default(),
        attachments,
        imessage: imessage.into_option(),
        source: IrSource {
            android_type: None,
            fields: source_fields,
        }
        .into_option(),
    })
}

/// The owner address every message of a conversation carries, when they all
/// carry the same one; `None` when one has none or two differ. Each message
/// keeps its own address either way, so a conversation held at two of the
/// holder's addresses keeps the split.
fn shared_owner(messages: &[IrMessage]) -> Option<String> {
    let first = messages.first()?.owner_handle.as_deref()?;
    messages
        .iter()
        .all(|m| m.owner_handle.as_deref() == Some(first))
        .then(|| first.to_string())
}

/// Copy participant handles and display names from the seed export message.
fn participants_from_seed(seed: &Message) -> Vec<IrParticipant> {
    let mut participants = Vec::with_capacity(seed.conversation.participants.len());
    for p in &seed.conversation.participants {
        participants.push(IrParticipant {
            handle: p.identity.clone(),
            // `name` falls back to the raw handle when nothing names the
            // person (ADR-0006). Carrying a bare handle through as a display
            // name would let a later import write it onto a Contact as that
            // person's name, turning a correctly nameless Contact into a
            // wrongly-named one — so only a name distinct from the handle
            // counts as a display name here. A participant with no handle at
            // all has nothing to be identical to, so their name always counts.
            display_name: (p.identity.as_deref() != Some(p.name.as_str())).then(|| p.name.clone()),
            handle_type: None,
        });
    }
    participants
}

/// Where an export writes one attachment, relative to the output folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportPath {
    /// The path the file is written at and the conversation file names:
    /// the server's path when the check accepts it, else
    /// `attachments/{sha256}`. `None` when the attachment has neither a usable
    /// path nor a fingerprint.
    pub rel: Option<String>,
    /// The server's path, as sent, when the check refused it.
    pub refused: Option<String>,
}

/// Choose where an attachment is written under the output folder.
///
/// The server's path is input: an import that reuses a stored fingerprint
/// never read the file, so the server can hold `../x` or an absolute path.
/// Joined onto the output folder, such a path writes outside it, so it goes
/// through [`message_ir::safe_attachment_path`] like every other reader of an
/// attachment path. A refused path is not used: the attachment falls back to
/// `attachments/{sha256}`, the name an attachment with no path gets.
pub fn export_path(att: &Attachment) -> ExportPath {
    let by_fingerprint = att
        .sha256
        .as_deref()
        .and_then(message_ir::trimmed)
        .map(|sha| format!("attachments/{sha}"));
    let Some(path) = att.path.as_deref().and_then(message_ir::trimmed) else {
        return ExportPath {
            rel: by_fingerprint,
            refused: None,
        };
    };
    // The base is empty because only the verdict matters here: the caller
    // joins the accepted path onto the output folder itself.
    if message_ir::safe_attachment_path(Path::new(""), path).is_ok() {
        ExportPath {
            rel: Some(path.to_string()),
            refused: None,
        }
    } else {
        ExportPath {
            rel: by_fingerprint,
            refused: att.path.clone(),
        }
    }
}

/// Map one server attachment record onto the shared attachment type.
fn to_ir_attachment(att: &Attachment) -> IrAttachment {
    IrAttachment {
        path: export_path(att).rel,
        original_name: att.original_name.clone(),
        mime_type: att.mime_type.clone(),
        digest_sha256: att.sha256.clone(),
        is_sticker: att.is_sticker,
        transcription: att.transcription.clone(),
        sticker_effect: None,
        // The server's attachment shape carries no byte length.
        size_bytes: None,
        missing_reason: att.missing_reason.clone(),
        bytes: None,
    }
}

/// JSON array of tapbacks (reactions), or `None` when the message has none.
fn tapbacks_json(tapbacks: &[Tapback]) -> Option<Value> {
    if tapbacks.is_empty() {
        return None;
    }
    let mut items = Vec::with_capacity(tapbacks.len());
    for t in tapbacks {
        items.push(json!({
            "part_index": t.part_index,
            "kind": t.kind,
            "emoji": t.emoji,
            "is_from_me": t.is_from_me,
            "reactor_handle": t.sender,
        }));
    }
    Some(Value::Array(items))
}

/// Choose SMS, MMS, iMessage, or announcement from service and attachments.
fn infer_kind(msg: &Message, service: IrService) -> IrMessageKind {
    if msg.is_announcement {
        return IrMessageKind::Announcement;
    }
    if !msg.attachments.is_empty() && matches!(service, IrService::Sms | IrService::Rcs) {
        return IrMessageKind::Mms;
    }
    match service {
        IrService::IMessage => IrMessageKind::IMessage,
        IrService::Sms | IrService::Rcs => IrMessageKind::Sms,
        IrService::Whatsapp
        | IrService::Discord
        | IrService::Signal
        | IrService::Telegram
        | IrService::Slack => IrMessageKind::Unknown,
        IrService::Unknown => {
            if msg.attachments.is_empty() {
                IrMessageKind::Sms
            } else {
                IrMessageKind::Mms
            }
        }
    }
}

/// Parse a server timestamp into milliseconds since Unix epoch.
///
/// Accepts a millisecond or second integer, RFC 3339, or a few common server
/// date strings without a timezone (treated as UTC).
///
/// # Errors
///
/// Returns an error when the string is empty or none of the formats match.
fn parse_timestamp_unix_ms(raw: &str) -> Result<i64> {
    let t = raw.trim();
    if t.is_empty() {
        bail!("empty timestamp");
    }
    if let Ok(ms) = t.parse::<i64>() {
        // Heuristic: seconds vs millis.
        // Any numeric value below 10^10 is a seconds timestamp (year 2286 in
        // seconds, well beyond any real SMS data). Values at or above 10^10
        // are millisecond timestamps (10^10 ms is 1970-04-26, long before any
        // SMS, so real data won't hit the ambiguity).
        return Ok(if ms.abs() < 10_000_000_000 {
            ms.saturating_mul(1000)
        } else {
            ms
        });
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(t) {
        return Ok(dt.timestamp_millis());
    }
    // Common server form without offset: treat as UTC.
    if let Ok(ndt) = NaiveDateTime::parse_from_str(t, "%Y-%m-%dT%H:%M:%S%.f") {
        return Ok(ndt.and_utc().timestamp_millis());
    }
    if let Ok(ndt) = NaiveDateTime::parse_from_str(t, "%Y-%m-%dT%H:%M:%S") {
        return Ok(ndt.and_utc().timestamp_millis());
    }
    // Last resort: chrono's RFC3339-ish with space
    if let Ok(dt) = DateTime::parse_from_str(t, "%Y-%m-%d %H:%M:%S %z") {
        return Ok(dt.timestamp_millis());
    }
    bail!("unrecognized timestamp: {t}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use message_crate_api_types::{MessageConversation, Participant, Tapback};

    /// One page of `GET /v1/exports/{id}/messages` exactly as the server serializes
    /// it: `service` on the message rather than on the conversation, an
    /// attachment with no byte length, and a participant the source named
    /// without recording an address, whose `identity` and `service` are `null`.
    ///
    /// This is a string literal on purpose. Every other fixture in this module
    /// builds the `message_crate_api_types` shapes in Rust, which is what let
    /// three of them drift away from what the server sends without the
    /// compiler or the suite noticing: `handle: String` rejected `"handle": null` and aborted every
    /// pull of a conversation holding an address-less participant, and
    /// `conversation.service` read a field the server has never sent, so every
    /// pulled message came out `IrService::Unknown`.
    const EXPORT_PAGE_JSON: &str = r#"{
      "items": [
        {
          "id": 4021,
          "source": "imessage",
          "service": "iMessage",
          "guid": "3A9E-0001",
          "timestamp": "2015-03-12T18:05:22Z",
          "sort_order": 0,
          "is_from_me": false,
          "sender": "+15555550100",
          "subject": null,
          "text": "dinner at seven?",
          "is_announcement": false,
          "is_reply": false,
          "thread_originator_guid": null,
          "thread_originator_part": null,
          "num_replies": 0,
          "conversation": {
            "id": 9,
            "chat_identifier": "chat9000",
            "conversation_type": "group",
            "group_title": "Book Club",
            "participants": [
              { "name": "Robert Smith", "identity": "+15555550100", "service": "imessage", "contact_id": 3 },
              { "name": "Sarah Vale", "identity": null, "service": null, "contact_id": 7 },
              { "name": "+15555550200", "identity": "+15555550200", "service": "imessage" }
            ]
          },
          "attachments": [
            {
              "path": "attachments/ab",
              "original_name": "menu.pdf",
              "mime_type": "application/pdf",
              "sha256": "ab",
              "transcription": null
            }
          ],
          "tapbacks": [
            { "part_index": 0, "kind": "loved", "is_from_me": true }
          ]
        }
      ],
      "total": 1,
      "limit": 500,
      "offset": 0
    }"#;

    /// The whole page parses, an address-less participant survives it, and the
    /// service the server sent reaches the IR message.
    #[test]
    fn a_real_export_page_parses_with_an_address_less_participant() {
        let page: message_crate_api_types::Page<message_crate_api_types::Message> =
            serde_json::from_str(EXPORT_PAGE_JSON).expect("the server's own page shape must parse");
        assert_eq!((page.items.len(), page.total), (1, 1));

        let participants = participants_from_seed(&page.items[0]);
        assert_eq!(participants.len(), 3);
        assert_eq!(participants[0].handle.as_deref(), Some("+15555550100"));
        assert_eq!(
            participants[0].display_name.as_deref(),
            Some("Robert Smith")
        );
        // No address at all: the name is all the server has for this person, so
        // it carries through as their display name.
        assert_eq!(participants[1].handle, None);
        assert_eq!(participants[1].display_name.as_deref(), Some("Sarah Vale"));
        // A name that is only the handle is still not a display name.
        assert_eq!(participants[2].handle.as_deref(), Some("+15555550200"));
        assert_eq!(participants[2].display_name, None);
    }

    /// The service rides on the message, so the IR message gets iMessage and
    /// the kind that follows from it — not `Unknown`/`Mms`, which is what
    /// reading `conversation.service` produced (issue #324).
    #[test]
    fn the_message_service_round_trips_from_a_real_export_page() {
        let page: message_crate_api_types::Page<message_crate_api_types::Message> =
            serde_json::from_str(EXPORT_PAGE_JSON).unwrap();
        assert_eq!(page.items[0].service.as_deref(), Some("iMessage"));

        let ir = to_ir_message(&page.items[0], false).unwrap();
        assert_eq!(ir.service, IrService::IMessage);
        assert_eq!(ir.message_kind, IrMessageKind::IMessage);
        // The attachment maps without a byte length: the server never sends one.
        assert_eq!(ir.attachments.len(), 1);
        assert_eq!(ir.attachments[0].size_bytes, None);
    }

    /// A conversation the holder used at two addresses keeps the split: each
    /// message carries its own owner, and the header names none, because no
    /// one address is the owner of every message (#1098).
    #[test]
    fn each_message_keeps_its_owner_and_a_split_conversation_names_none_in_the_header() {
        let mut by_phone = seed_message_with_participant(Participant {
            identity: Some("+15555550101".into()),
            name: "Sam".into(),
            service: None,
            contact_id: None,
        });
        by_phone.owner = Some("+15555550100".into());
        let mut by_email = by_phone.clone();
        by_email.owner = Some("me@example.com".into());
        let mut blank = by_phone.clone();
        blank.owner = Some("  ".into());

        let messages = vec![
            to_ir_message(&by_phone, false).unwrap(),
            to_ir_message(&by_email, false).unwrap(),
            to_ir_message(&blank, false).unwrap(),
        ];
        assert_eq!(
            messages
                .iter()
                .map(|m| m.owner_handle.as_deref())
                .collect::<Vec<_>>(),
            [Some("+15555550100"), Some("me@example.com"), None]
        );

        let doc = build_document("imessage", &by_phone, messages);
        assert_eq!(doc.export.owner_handle, None);
    }

    /// RFC 3339, whole seconds and milliseconds all land on the same instant,
    /// and a number at the cut-off (10^10) is read as milliseconds.
    #[test]
    fn parses_each_timestamp_form_to_the_exact_millisecond() {
        const MS: i64 = 1_426_183_522_000;
        assert_eq!(parse_timestamp_unix_ms("2015-03-12T18:05:22Z").unwrap(), MS);
        assert_eq!(
            parse_timestamp_unix_ms("2015-03-12T19:05:22.250+01:00").unwrap(),
            MS + 250
        );
        assert_eq!(parse_timestamp_unix_ms("1426183522").unwrap(), MS);
        assert_eq!(parse_timestamp_unix_ms("1426183522250").unwrap(), MS + 250);
        assert_eq!(
            parse_timestamp_unix_ms("9999999999").unwrap(),
            9_999_999_999_000
        );
        assert_eq!(
            parse_timestamp_unix_ms("10000000000").unwrap(),
            10_000_000_000
        );
    }

    /// Reply threading, reactions and an announcement's text come through
    /// the pull into the message's iMessage fields.
    #[test]
    fn a_reply_with_tapbacks_keeps_its_threading_and_reactions() {
        let mut msg = seed_message_with_participant(Participant {
            identity: Some("+1".into()),
            name: "Sam".into(),
            service: None,
            contact_id: None,
        });
        msg.timestamp = "1426183522250".into();
        msg.is_reply = true;
        msg.thread_originator_guid = Some("origin-guid".into());
        msg.thread_originator_part = Some(2);
        msg.num_replies = 3;
        msg.tapbacks = vec![
            Tapback {
                part_index: 0,
                kind: "loved".into(),
                emoji: None,
                is_from_me: true,
                sender: None,
            },
            Tapback {
                part_index: 1,
                kind: "emoji".into(),
                emoji: Some("🎉".into()),
                is_from_me: false,
                sender: Some("+2".into()),
            },
        ];

        let ir = to_ir_message(&msg, false).unwrap();

        assert_eq!(ir.timestamp_unix_ms, 1_426_183_522_250);
        let imessage = ir.imessage.expect("reply fields make an iMessage block");
        assert!(imessage.is_reply);
        assert_eq!(imessage.in_reply_to_guid.as_deref(), Some("origin-guid"));
        assert_eq!(imessage.thread_originator_part, Some(2));
        assert_eq!(imessage.num_replies, Some(3));
        assert_eq!(imessage.announcement, None);
        assert_eq!(
            imessage.tapbacks,
            Some(json!([
                { "part_index": 0, "kind": "loved", "emoji": null, "is_from_me": true, "reactor_handle": null },
                { "part_index": 1, "kind": "emoji", "emoji": "🎉", "is_from_me": false, "reactor_handle": "+2" },
            ]))
        );
    }

    /// An SMS that carries a file is an MMS, and the document counts every
    /// attachment across its messages.
    #[test]
    fn an_sms_with_a_file_is_an_mms_and_counts_toward_the_document() {
        let mut sms = seed_message_with_participant(Participant {
            identity: Some("+1".into()),
            name: "Sam".into(),
            service: None,
            contact_id: None,
        });
        sms.service = Some("SMS".into());
        let plain = to_ir_message(&sms, false).unwrap();
        assert_eq!(plain.message_kind, IrMessageKind::Sms);

        let file: Attachment = serde_json::from_value(json!({ "sha256": "ab" })).unwrap();
        sms.attachments = vec![file.clone(), file.clone(), file];
        let with_files = to_ir_message(&sms, false).unwrap();
        assert_eq!(with_files.message_kind, IrMessageKind::Mms);

        let doc = build_document("sms", &sms, vec![with_files, plain]);
        assert_eq!(doc.conversation.stats.attachment_count, 3);
        assert_eq!(doc.conversation.stats.message_count, 2);
    }

    /// An announcement keeps its text; one with no text carries nothing.
    #[test]
    fn an_announcement_keeps_its_text() {
        let mut msg = seed_message_with_participant(Participant {
            identity: Some("+1".into()),
            name: "Sam".into(),
            service: None,
            contact_id: None,
        });
        msg.is_announcement = true;
        msg.text = Some("Sam named the conversation \"Book Club\"".into());

        let ir = to_ir_message(&msg, false).unwrap();

        assert_eq!(ir.message_kind, IrMessageKind::Announcement);
        let imessage = ir
            .imessage
            .expect("an announcement makes an iMessage block");
        assert_eq!(
            imessage.announcement.as_deref(),
            Some("Sam named the conversation \"Book Club\"")
        );
        assert_eq!(imessage.tapbacks, None);

        msg.text = None;
        let ir = to_ir_message(&msg, false).unwrap();
        assert!(ir.imessage.is_none());
    }

    #[test]
    fn maps_basic_message() {
        let msg = Message {
            id: 1,
            source: "imessage".into(),
            service: Some("iMessage".into()),
            guid: Some("g1".into()),
            timestamp: "2015-03-12T18:05:22Z".into(),
            is_from_me: false,
            sender: Some("+1".into()),
            owner: None,
            subject: None,
            text: Some("hi".into()),
            is_announcement: false,
            is_reply: false,
            thread_originator_guid: None,
            thread_originator_part: None,
            num_replies: 0,
            sort_order: 0,
            conversation: MessageConversation {
                id: 9,
                chat_identifier: "+1".into(),
                conversation_type: "individual".into(),
                group_title: None,
                participants: vec![Participant {
                    identity: Some("+1".into()),
                    name: "Sam".into(),
                    service: None,
                    contact_id: None,
                }],
            },
            attachments: vec![],
            tapbacks: vec![],
        };
        let ir = to_ir_message(&msg, false).unwrap();
        assert_eq!(ir.guid, "g1");
        assert_eq!(ir.text, "hi");
        assert_eq!(ir.service, IrService::IMessage);
    }

    /// A participant `name` distinct from the handle carries through as the
    /// IR participant's display name.
    #[test]
    fn participants_from_seed_carries_a_real_name() {
        let seed = seed_message_with_participant(Participant {
            identity: Some("+1".into()),
            name: "Sam".into(),
            service: None,
            contact_id: None,
        });
        let participants = participants_from_seed(&seed);
        assert_eq!(participants[0].handle.as_deref(), Some("+1"));
        assert_eq!(participants[0].display_name.as_deref(), Some("Sam"));
    }

    /// When the server has nothing to name the person, `name` falls back to
    /// the handle (ADR-0006). That must not become a display name here — see
    /// the comment on `participants_from_seed` for why.
    #[test]
    fn participants_from_seed_drops_a_name_that_is_just_the_handle() {
        let seed = seed_message_with_participant(Participant {
            identity: Some("+1".into()),
            name: "+1".into(),
            service: None,
            contact_id: None,
        });
        let participants = participants_from_seed(&seed);
        assert_eq!(participants[0].display_name, None);
    }

    /// A minimal `Message` carrying exactly one conversation participant.
    fn seed_message_with_participant(participant: Participant) -> Message {
        Message {
            id: 1,
            source: "imessage".into(),
            service: Some("iMessage".into()),
            guid: Some("g1".into()),
            timestamp: "2015-03-12T18:05:22Z".into(),
            is_from_me: false,
            sender: Some("+1".into()),
            owner: None,
            subject: None,
            text: Some("hi".into()),
            is_announcement: false,
            is_reply: false,
            thread_originator_guid: None,
            thread_originator_part: None,
            num_replies: 0,
            sort_order: 0,
            conversation: MessageConversation {
                id: 9,
                chat_identifier: "+1".into(),
                conversation_type: "individual".into(),
                group_title: None,
                participants: vec![participant],
            },
            attachments: vec![],
            tapbacks: vec![],
        }
    }
}
