//! Shared projection from [`PendingConversation`] to [`ConversationDocument`].
//!
//! Every exporter used to carry its own copy of this loop. The skeleton —
//! participants with the single-peer fallback, owner sender handling,
//! sent/received tallying, timestamp formatting, GUID derivation, and
//! document assembly — lives here once; the genuine per-exporter deltas
//! (vendor `source` fields, service selection, GUID materials, attachment
//! mapping) are supplied through [`ProjectionHooks`].

use crate::{
    ConversationDocument, ConversationMeta, ConversationStats, ExportMeta, HandleType,
    IrAttachment, IrConversationType, IrDirection, IrMessage, IrMessageKind, IrParticipant,
    IrService, IrSource, PendingAttachment, PendingConversation, PendingMessage, SCHEMA_VERSION,
    format_local_ts, owner_sender, stable_guid,
};
use std::collections::{BTreeMap, HashMap};

/// How the projection classifies one pending message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectedRole {
    /// Sent by the owner; counted as sent and given the owner sender identity.
    Outgoing,
    /// Received from a peer; counted as received.
    Incoming,
    /// System / notification row (e.g. an iMazing notification); counted
    /// separately and written as incoming with its own sender fields.
    Notification,
}

/// Unit of [`PendingMessage::sort_key`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKeyUnit {
    /// `sort_key` is Unix seconds (most exporters).
    Seconds,
    /// `sort_key` is Unix milliseconds (WhatsApp).
    Milliseconds,
}

impl SortKeyUnit {
    /// Convert a `sort_key` in this unit to Unix seconds.
    pub fn to_secs(self, key: i64) -> i64 {
        match self {
            Self::Seconds => key,
            Self::Milliseconds => key / 1000,
        }
    }
}

/// Counters from one projection, for the caller to fold into its report.
///
/// The projection cannot bump an exporter report directly (that type lives in
/// `message-vault-io-core`, which depends on this crate), so it returns plain
/// counts and each exporter folds the ones it tracks.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ProjectionTally {
    /// Messages projected (every role).
    pub messages: u64,
    /// Messages with [`ProjectedRole::Outgoing`].
    pub sent: u64,
    /// Messages with [`ProjectedRole::Incoming`].
    pub received: u64,
    /// Messages with [`ProjectedRole::Notification`].
    pub notifications: u64,
}

/// Per-exporter deltas of the shared [`pending_to_document`] projection.
///
/// Only [`export`](Self::export), [`service`](Self::service), and
/// [`source`](Self::source) are required; every other method has a default
/// matching the common exporter behavior.
pub trait ProjectionHooks {
    /// Export metadata (source / tool / version / owner) stamped on the document.
    fn export(&self) -> ExportMeta;

    /// Transport of one message (e.g. a constant `IrService::Sms`).
    fn service(&self, msg: &PendingMessage) -> IrService;

    /// Vendor leftovers for one message. The projection applies
    /// [`IrSource::into_option`], so an empty bag becomes `None`.
    fn source(&self, convo: &PendingConversation, msg: &PendingMessage) -> IrSource;

    /// Normalize a raw non-owner sender handle (e.g. guard phone digits).
    /// The default keeps the handle as-is.
    fn normalize_handle(&self, raw: &str) -> String {
        raw.to_string()
    }

    /// Materials fed into [`stable_guid`] alongside chat, timestamp,
    /// direction, and text. The default uses each attachment's digest
    /// (empty string when unknown), in order.
    fn guid_materials(&self, msg: &PendingMessage) -> Vec<String> {
        msg.attachments
            .iter()
            .map(|a| a.digest_sha256.clone().unwrap_or_default())
            .collect()
    }

    /// Map one queued attachment onto the shared [`IrAttachment`] shape.
    /// The default carries metadata only (no path, no bytes).
    fn attachment_to_ir(&self, att: &PendingAttachment, _msg: &PendingMessage) -> IrAttachment {
        IrAttachment {
            path: None,
            original_name: att.name_hint.clone(),
            mime_type: att.mime_type(),
            digest_sha256: att.digest_sha256.clone(),
            is_sticker: false,
            transcription: None,
            sticker_effect: None,
            size_bytes: None,
            missing_reason: None,
            bytes: None,
        }
    }

    /// Classify one message. The default maps `is_from_me` to
    /// [`ProjectedRole::Outgoing`] / [`ProjectedRole::Incoming`].
    fn role(&self, msg: &PendingMessage) -> ProjectedRole {
        if msg.is_from_me {
            ProjectedRole::Outgoing
        } else {
            ProjectedRole::Incoming
        }
    }

    /// Subject line of one message; the default has none.
    fn subject(&self, _msg: &PendingMessage) -> Option<String> {
        None
    }

    /// Unit of [`PendingMessage::sort_key`]; the default is seconds.
    fn sort_key_unit(&self) -> SortKeyUnit {
        SortKeyUnit::Seconds
    }

    /// Order of messages inside one conversation before projection. The
    /// default sorts by `sort_key` alone; an exporter whose rows can share a
    /// timestamp adds its own tie-break so output stays deterministic.
    fn message_order(&self, a: &PendingMessage, b: &PendingMessage) -> std::cmp::Ordering {
        a.sort_key.cmp(&b.sort_key)
    }

    /// Row shape of one message. The default maps attachment-less rows to
    /// [`IrMessageKind::Sms`] and the rest to [`IrMessageKind::Mms`].
    fn message_kind(&self, msg: &PendingMessage) -> IrMessageKind {
        if msg.attachments.is_empty() {
            IrMessageKind::Sms
        } else {
            IrMessageKind::Mms
        }
    }

    /// Roster written on the document. The default is
    /// [`default_participants`]: `participant_e164s` with display names
    /// gathered from the messages, plus the single-peer chat-id fallback.
    fn participants(&self, chat_id: &str, convo: &PendingConversation) -> Vec<IrParticipant> {
        default_participants(chat_id, convo, &|raw| self.normalize_handle(raw))
    }

    /// Group display title written on the document; the default writes none
    /// (Android group titles are stored as data, not used for filenames).
    fn group_title(&self, _convo: &PendingConversation) -> Option<String> {
        None
    }

    /// On-disk stem suffix (e.g. `__whatsapp`); the default has none.
    fn packaging_stem_suffix(&self, _convo: &PendingConversation) -> Option<String> {
        None
    }
}

/// Build a [`ConversationDocument`] from one pending conversation.
///
/// The skeleton shared by every exporter: participants (with the single-peer
/// chat-id fallback), the owner sender for outgoing rows, `format_local_ts` +
/// [`stable_guid`] per message, the `date_ms`-extra-with-fallback timestamp,
/// and document assembly. Timestamps must already be representable — run
/// [`prepare_conversation`] (or an equivalent prune) first.
///
/// Returns the document and a [`ProjectionTally`] for the caller to fold into
/// its report.
pub fn pending_to_document<H: ProjectionHooks + ?Sized>(
    chat_id: &str,
    convo: &PendingConversation,
    hooks: &H,
) -> (ConversationDocument, ProjectionTally) {
    let export = hooks.export();
    let (owner_sender_handle, owner_sender_display) = owner_sender(&export);

    let mut tally = ProjectionTally::default();
    let mut messages = Vec::with_capacity(convo.messages.len());
    for msg in &convo.messages {
        let role = hooks.role(msg);
        tally.messages += 1;
        match role {
            ProjectedRole::Outgoing => tally.sent += 1,
            ProjectedRole::Incoming => tally.received += 1,
            ProjectedRole::Notification => tally.notifications += 1,
        }

        let (secs, fallback_ms) = match hooks.sort_key_unit() {
            SortKeyUnit::Seconds => (msg.sort_key, msg.sort_key.saturating_mul(1000)),
            SortKeyUnit::Milliseconds => (msg.sort_key / 1000, msg.sort_key),
        };
        let (ts_local, _, _) = format_local_ts(secs).expect("timestamp validated above");
        let guid = stable_guid(
            chat_id,
            &ts_local,
            msg.is_from_me,
            &msg.text,
            &hooks.guid_materials(msg),
        );
        let timestamp_unix_ms = msg
            .extra_str("date_ms")
            .parse::<i64>()
            .unwrap_or(fallback_ms);

        let outgoing = role == ProjectedRole::Outgoing;
        let (sender_handle, sender_display_name) = if outgoing {
            (owner_sender_handle.clone(), owner_sender_display.clone())
        } else {
            (
                (!msg.sender_handle.is_empty()).then(|| hooks.normalize_handle(&msg.sender_handle)),
                msg.sender_display_name.clone(),
            )
        };
        let attachments: Vec<IrAttachment> = msg
            .attachments
            .iter()
            .map(|a| hooks.attachment_to_ir(a, msg))
            .collect();

        messages.push(IrMessage {
            guid,
            timestamp_unix_ms,
            direction: if outgoing {
                IrDirection::Outgoing
            } else {
                IrDirection::Incoming
            },
            service: hooks.service(msg),
            message_kind: hooks.message_kind(msg),
            sender_handle,
            sender_display_name,
            owner_handle: None,
            subject: hooks.subject(msg),
            text: msg.text.clone(),
            attachments,
            imessage: None,
            source: hooks.source(convo, msg).into_option(),
        });
    }

    let doc = ConversationDocument {
        schema_version: SCHEMA_VERSION,
        export,
        conversation: ConversationMeta {
            chat_identifier: chat_id.to_string(),
            conversation_type: if convo.is_group {
                IrConversationType::Group
            } else {
                IrConversationType::Individual
            },
            group_title: hooks.group_title(convo),
            participants: hooks.participants(chat_id, convo),
            stats: ConversationStats::default(),
        },
        messages,
        packaging_stem_suffix: hooks.packaging_stem_suffix(convo),
    };
    (doc, tally)
}

/// Map of handle → display name from message extras and sender fields.
///
/// `normalize_handle` maps a raw sender handle onto the same form the
/// participant roster uses (e.g. guarded phone normalization).
pub fn display_names_for_handles(
    convo: &PendingConversation,
    normalize_handle: &dyn Fn(&str) -> String,
) -> HashMap<String, String> {
    let mut names = HashMap::new();
    for msg in &convo.messages {
        if !msg.sender_handle.is_empty() {
            let handle = normalize_handle(&msg.sender_handle);
            if let Some(name) = msg.sender_display_name.as_deref().and_then(crate::trimmed) {
                names.entry(handle).or_insert_with(|| name.to_string());
            }
        }
        if !convo.is_group {
            let name = msg.extra_str("contact_name").trim();
            if !name.is_empty() {
                for peer in &convo.participant_e164s {
                    names
                        .entry(peer.clone())
                        .or_insert_with(|| name.to_string());
                }
            }
        }
    }
    names
}

/// The default roster: `participant_e164s` (with display names gathered from
/// the messages) plus the single-peer chat-id fallback when the roster is
/// empty for an individual chat.
pub fn default_participants(
    chat_id: &str,
    convo: &PendingConversation,
    normalize_handle: &dyn Fn(&str) -> String,
) -> Vec<IrParticipant> {
    let name_by_handle = display_names_for_handles(convo, normalize_handle);
    let mut participants: Vec<IrParticipant> = convo
        .participant_e164s
        .iter()
        .filter(|h| !h.is_empty())
        .map(|h| IrParticipant {
            handle: Some(h.clone()),
            display_name: name_by_handle.get(h).cloned(),
            handle_type: Some(HandleType::Phone),
        })
        .collect();
    if participants.is_empty() && !convo.is_group && !chat_id.is_empty() {
        if convo.extra.contains_key(crate::CHAT_ID_IS_NAME) {
            // The source named this person and recorded no address for them,
            // so the chat id is a stem of the name — not something to store
            // as an identity.
            participants.push(IrParticipant {
                handle: None,
                display_name: convo.first_contact_name(),
                handle_type: None,
            });
        } else {
            participants.push(IrParticipant {
                handle: Some(chat_id.to_string()),
                display_name: name_by_handle
                    .get(chat_id)
                    .cloned()
                    .or_else(|| convo.first_contact_name()),
                handle_type: Some(HandleType::Phone),
            });
        }
    }
    participants
}

/// Get or create the pending conversation for `chat_id`.
///
/// A conversation already in `map` is returned as-is; `is_group`,
/// `display_name`, and `participant_e164s` seed a new entry only.
pub fn ensure_conversation<'a>(
    map: &'a mut BTreeMap<String, PendingConversation>,
    chat_id: &str,
    is_group: bool,
    display_name: Option<String>,
    participant_e164s: Vec<String>,
) -> &'a mut PendingConversation {
    map.entry(chat_id.to_string()).or_insert_with(|| {
        PendingConversation::new(chat_id, is_group, display_name, participant_e164s)
    })
}

/// Sort messages with `cmp`, drop messages whose timestamp cannot be
/// represented, and set `has_attachments`.
///
/// `to_secs` converts a message sort key to Unix seconds (exporters that
/// store milliseconds pass `|k| k / 1000`).
///
/// Returns `(any_messages_remain, skipped_invalid_date_count)`; the caller
/// folds the skipped count into its report.
pub fn prepare_conversation(
    convo: &mut PendingConversation,
    cmp: impl FnMut(&PendingMessage, &PendingMessage) -> std::cmp::Ordering,
    to_secs: impl Fn(i64) -> i64,
) -> (bool, u64) {
    convo.messages.sort_by(cmp);
    let mut skipped = 0u64;
    convo.messages.retain(|m| {
        if format_local_ts(to_secs(m.sort_key)).is_some() {
            true
        } else {
            skipped += 1;
            false
        }
    });
    convo.has_attachments = convo.messages.iter().any(|m| !m.attachments.is_empty());
    (!convo.messages.is_empty(), skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestHooks;

    impl ProjectionHooks for TestHooks {
        fn export(&self) -> ExportMeta {
            ExportMeta {
                source: "test".into(),
                tool: "Test".into(),
                tool_version: "0".into(),
                owner_handle: Some("+15555550100".into()),
                owner_display_name: None,
            }
        }

        fn service(&self, _msg: &PendingMessage) -> IrService {
            IrService::Sms
        }

        fn source(&self, _convo: &PendingConversation, _msg: &PendingMessage) -> IrSource {
            IrSource::default()
        }
    }

    fn msg(secs: i64, from_me: bool, text: &str) -> PendingMessage {
        PendingMessage {
            sort_key: secs,
            is_from_me: from_me,
            sender_handle: if from_me {
                String::new()
            } else {
                "+15555550122".into()
            },
            sender_display_name: (!from_me).then(|| "Peer".to_string()),
            text: text.into(),
            attachments: Vec::new(),
            extra: BTreeMap::new(),
        }
    }

    #[test]
    fn projects_owner_sender_and_tallies_directions() {
        let mut convo =
            PendingConversation::new("+15555550122", false, None, vec!["+15555550122".into()]);
        convo.messages = vec![
            msg(1_609_459_200, false, "hi"),
            msg(1_609_459_260, true, "yo"),
        ];

        let (doc, tally) = pending_to_document("+15555550122", &convo, &TestHooks);
        assert_eq!(tally.messages, 2);
        assert_eq!(tally.sent, 1);
        assert_eq!(tally.received, 1);
        assert_eq!(doc.messages.len(), 2);
        assert_eq!(doc.messages[0].direction, IrDirection::Incoming);
        assert_eq!(
            doc.messages[0].sender_handle.as_deref(),
            Some("+15555550122")
        );
        assert_eq!(doc.messages[1].direction, IrDirection::Outgoing);
        assert_eq!(
            doc.messages[1].sender_handle.as_deref(),
            Some("+15555550100")
        );
        assert_eq!(doc.messages[1].sender_display_name.as_deref(), Some("Me"));
        assert_eq!(
            doc.messages[0].timestamp_unix_ms, 1_609_459_200_000,
            "seconds sort keys fall back to secs * 1000"
        );
        assert_eq!(doc.conversation.participants.len(), 1);
        assert_eq!(
            doc.conversation.participants[0].display_name.as_deref(),
            Some("Peer")
        );
    }

    #[test]
    fn single_peer_fallback_uses_chat_id_and_contact_name() {
        let mut convo = PendingConversation::new("+15555550122", false, None, Vec::new());
        let mut m = msg(1_609_459_200, false, "hi");
        m.sender_handle = String::new();
        m.sender_display_name = None;
        m.extra.insert("contact_name".into(), "Bob".into());
        convo.messages = vec![m];

        let (doc, _) = pending_to_document("+15555550122", &convo, &TestHooks);
        assert_eq!(doc.conversation.participants.len(), 1);
        assert_eq!(
            doc.conversation.participants[0].handle.as_deref(),
            Some("+15555550122")
        );
        assert_eq!(
            doc.conversation.participants[0].display_name.as_deref(),
            Some("Bob")
        );
    }

    #[test]
    fn prepare_conversation_sorts_prunes_and_counts() {
        let mut convo = PendingConversation::new("x", false, None, Vec::new());
        convo.messages = vec![
            msg(1_609_459_260, true, "later"),
            msg(i64::MAX, false, "unrepresentable"),
            msg(1_609_459_200, false, "earlier"),
        ];
        let (keep, skipped) =
            prepare_conversation(&mut convo, |a, b| a.sort_key.cmp(&b.sort_key), |k| k);
        assert!(keep);
        assert_eq!(skipped, 1);
        assert_eq!(convo.messages.len(), 2);
        assert_eq!(convo.messages[0].text, "earlier");
    }

    /// Hooks whose sort keys are milliseconds, as WhatsApp's are.
    struct MillisecondHooks;

    impl ProjectionHooks for MillisecondHooks {
        fn export(&self) -> ExportMeta {
            TestHooks.export()
        }

        fn service(&self, msg: &PendingMessage) -> IrService {
            TestHooks.service(msg)
        }

        fn source(&self, convo: &PendingConversation, msg: &PendingMessage) -> IrSource {
            TestHooks.source(convo, msg)
        }

        fn sort_key_unit(&self) -> SortKeyUnit {
            SortKeyUnit::Milliseconds
        }
    }

    #[test]
    fn sort_key_unit_converts_to_seconds() {
        assert_eq!(SortKeyUnit::Seconds.to_secs(1_609_459_200), 1_609_459_200);
        assert_eq!(
            SortKeyUnit::Milliseconds.to_secs(1_609_459_200_999),
            1_609_459_200
        );
    }

    #[test]
    fn millisecond_sort_keys_keep_their_milliseconds() {
        let mut convo = PendingConversation::new("+15555550122", false, None, Vec::new());
        convo.messages = vec![msg(1_609_459_200_123, false, "hi")];

        let (doc, _) = pending_to_document("+15555550122", &convo, &MillisecondHooks);
        assert_eq!(doc.messages[0].timestamp_unix_ms, 1_609_459_200_123);
        // The GUID is built from the message's second, not its millisecond.
        let (ts_local, _, _) = format_local_ts(1_609_459_200).unwrap();
        assert_eq!(
            doc.messages[0].guid,
            stable_guid("+15555550122", &ts_local, false, "hi", &[])
        );
    }

    #[test]
    fn photo_only_messages_in_the_same_second_get_their_own_guids() {
        let photo = |digest: &str| {
            let mut m = msg(1_609_459_200, true, "");
            m.attachments = vec![PendingAttachment {
                rel_path: format!("attachments/{digest}.jpg"),
                content_type: "image/jpeg".into(),
                extension: "jpg".into(),
                digest_sha256: Some(digest.to_string()),
                name_hint: None,
            }];
            m
        };
        let mut convo = PendingConversation::new("+15555550122", false, None, Vec::new());
        convo.messages = vec![photo(&"a".repeat(64)), photo(&"b".repeat(64))];

        let (doc, _) = pending_to_document("+15555550122", &convo, &TestHooks);
        assert_ne!(doc.messages[0].guid, doc.messages[1].guid);
    }
}
