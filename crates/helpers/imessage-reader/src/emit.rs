//! Stream every row of `chat.db` to the app as protocol events.
//!
//! Each row becomes one [`MessageRecord`], already classified (tapback,
//! announcement, balloon, plain text) with its Apple-specific fields filled
//! in. The first row of each conversation is preceded by a
//! [`ConversationRecord`] carrying the roster. Nothing is written to disk
//! here; the app owns the output.

use std::collections::HashSet;

use imessage_database::{
    message_types::variants::{Announcement, Tapback, TapbackAction, Variant},
    tables::{
        chat::Chat,
        messages::{
            Message,
            models::{GroupAction, Service},
        },
        table::{ME, ORPHANED, Table, YOU},
    },
    util::dates::TIMESTAMP_FACTOR,
};
use imessage_reader_protocol::{
    Conversation as ConversationRecord, Event, Imessage as ImessageRecord,
    Message as MessageRecord, Participant, Progress,
};
use serde_json::Value;

use crate::{
    attachments_emit::collect_parts_and_attachments,
    body::apply_body,
    error::RuntimeError,
    fields::{
        TapbackCell, balloon_kind_label, balloon_summary, build_balloon_value, build_edit_records,
        expressive_label, parse_thread_part, shared_location_label,
    },
    log::emit,
    session::MailSession,
};

/// Report often enough that long attachment decrypts between ticks do not
/// look frozen on large backups.
const MESSAGE_PROGRESS_EVERY: u64 = 1_000;

/// Poll votes and updates: noise that CSV and HTML export skip too.
fn is_poll_noise(message: &Message) -> bool {
    message.is_poll_vote() || message.is_poll_update()
}

/// Stream every row of `chat.db` as events. A row that fails to convert is
/// logged and counted, not fatal.
///
/// # Errors
///
/// Returns an error when the database cannot be read.
pub(crate) fn stream_export(session: &MailSession) -> Result<(), RuntimeError> {
    let mut announced: HashSet<String> = HashSet::new();
    let mut last_row = -1;
    let mut seen = 0u64;
    let mut failures = 0u64;
    let total = Message::get_count(session.data_source.db(), &session.options.query_context)?;
    let mut statement =
        Message::stream_rows(session.data_source.db(), &session.options.query_context)?;

    for message in Message::rows(&mut statement, [])? {
        let mut msg = message?;
        seen += 1;
        // The stream repeats a row once per attachment join; keep the first.
        if msg.rowid == last_row {
            continue;
        }
        last_row = msg.rowid;
        if !msg.is_edited() && is_poll_noise(&msg) {
            continue;
        }
        apply_body(&mut msg, session.data_source.db());
        if is_poll_noise(&msg) {
            continue;
        }

        match build_record(session, &msg) {
            Ok((conversation, record)) => {
                if announced.insert(conversation.chat_identifier.clone()) {
                    emit(&Event::Conversation(conversation));
                }
                emit(&Event::Message(Box::new(record)));
            }
            Err(why) => {
                failures += 1;
                session.options.emit_log(format!(
                    "Skipping message (rowid={}, guid={}): {}",
                    msg.rowid, msg.guid, why
                ));
            }
        }
        if seen.is_multiple_of(MESSAGE_PROGRESS_EVERY) {
            session.options.emit_log(format!("  …{seen}/{total}"));
            emit(&Event::Progress(Progress::Parse {
                done: seen,
                total: u64::try_from(total).unwrap_or(u64::MAX),
            }));
        }
    }

    // The count above only reports every MESSAGE_PROGRESS_EVERY rows; say
    // the reading is finished, or the line stops short of its total.
    let read = seen.max(u64::try_from(total).unwrap_or(u64::MAX));
    emit(&Event::Progress(Progress::Parse {
        done: read,
        total: read,
    }));
    if failures > 0 {
        session.options.emit_log(format!(
            "{failures} messages skipped due to formatting errors."
        ));
    }
    emit(&Event::ExportDone {
        messages_seen: seen,
        failures,
    });
    Ok(())
}

/// Message time as milliseconds since 1970-01-01 UTC.
fn timestamp_unix_ms(message: &Message, offset: i64) -> i64 {
    if let Ok(dt) = message.date(offset) {
        return dt.timestamp_millis();
    }
    let stamp = message.date;
    let seconds_since_2001 = if stamp >= 1_000_000_000_000 {
        stamp / TIMESTAMP_FACTOR
    } else {
        stamp
    };
    (seconds_since_2001 + offset).saturating_mul(1000)
}

/// When the message was read, as RFC 3339, or `None` for a message never
/// read. `chat.db` stores `date_read` as NULL for an unread row and the
/// library reads that back as `0`; `Message::date_read` does not treat `0`
/// as "no date", it returns Apple's epoch, so the raw stamp is checked first.
fn read_receipt_rfc3339(message: &Message, offset: i64) -> Option<String> {
    if message.date_read == 0 {
        return None;
    }
    message.date_read(offset).ok().map(|d| d.to_rfc3339())
}

/// Raw handle string for a Messages `handle_id`, if the participant is known.
fn raw_handle(session: &MailSession, handle_id: i32) -> Option<String> {
    session
        .resolve_participant(handle_id)
        .map(|name| name.details.clone())
}

/// Contact display name for a Messages `handle_id`, falling back to the handle.
fn display_name_for(session: &MailSession, handle_id: i32) -> Option<String> {
    session.resolve_participant(handle_id).map(|name| {
        if name.full.is_empty() {
            name.details.clone()
        } else {
            name.full.clone()
        }
    })
}

/// Participants and conversation type (`individual` / `group`) for one chat room.
fn participants_for(session: &MailSession, chatroom: &Chat) -> (Vec<Participant>, &'static str) {
    let mut records = Vec::new();
    // Only non-empty handles are written, so only count those. A raw handle
    // row count over-counts empty handles and misclassifies the chat.
    let mut count = 0;
    if let Some(handles) = session.chatroom_participants.get(&chatroom.rowid) {
        for handle_id in handles {
            let name = session.resolve_participant(*handle_id);
            let (handle, display_name) = match name {
                Some(n) => (
                    n.details.clone(),
                    if n.full.is_empty() {
                        None
                    } else {
                        Some(n.full.clone())
                    },
                ),
                None => (String::new(), None),
            };
            if !handle.is_empty() {
                records.push(Participant {
                    handle,
                    display_name,
                });
                count += 1;
            }
        }
    }
    // A user-named chat is a group even when it has shrunk to two members.
    let named = chatroom.display_name().is_some();
    let conversation_type = if count > 1 || named {
        "group"
    } else {
        "individual"
    };
    (records, conversation_type)
}

/// Human-readable text for a group announcement (rename, add, leave, and similar).
fn announcement_text(session: &MailSession, msg: &Message) -> Option<String> {
    let announcement = msg.get_announcement()?;
    let mut who = session.who(
        msg.handle_id,
        msg.is_from_me(),
        msg.destination_caller_id.as_deref(),
    );
    if who == ME {
        who = YOU;
    }
    let participant_name = match &announcement {
        Announcement::GroupAction(
            GroupAction::ParticipantAdded(handle) | GroupAction::ParticipantRemoved(handle),
        ) => session.who(Some(*handle), false, msg.destination_caller_id.as_deref()),
        _ => "someone",
    };

    let body = match &announcement {
        Announcement::AudioMessageKept => "kept an audio message.".to_string(),
        Announcement::FullyUnsent => "unsent a message!".to_string(),
        Announcement::Unknown(num) => format!("performed unknown action {num}."),
        Announcement::GroupAction(group) => match group {
            GroupAction::ParticipantAdded(_) => {
                format!("added {participant_name} to the conversation.")
            }
            GroupAction::ParticipantRemoved(_) => {
                format!("removed {participant_name} from the conversation.")
            }
            GroupAction::NameChange(name) => format!("named the conversation {name}"),
            GroupAction::ParticipantLeft => "left the conversation.".to_string(),
            GroupAction::GroupIconChanged => "changed the group photo.".to_string(),
            GroupAction::GroupIconRemoved => "removed the group photo.".to_string(),
            GroupAction::ChatBackgroundChanged => "changed the chat background.".to_string(),
            GroupAction::ChatBackgroundRemoved => "removed the chat background.".to_string(),
            GroupAction::PhoneNumberChanged(_) => "changed their phone number.".to_string(),
        },
    };
    Some(format!("{who} {body}"))
}

/// Owner display name from the destination caller id, or `Me`, when that option is on.
fn owner_display_name(session: &MailSession, message: &Message) -> Option<String> {
    if session.options.use_caller_id {
        message
            .destination_caller_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or_else(|| Some(ME.to_string()))
    } else {
        None
    }
}

/// One-line description of a tapback (Loved, Liked, Removed Heart, and similar).
fn tapback_human_line(kind: &str, emoji: Option<&str>, action: &str) -> String {
    if action == "remove" {
        return match kind {
            "loved" => "Removed Heart".into(),
            "liked" => "Removed Like".into(),
            "disliked" => "Removed Dislike".into(),
            "laughed" => "Removed Laugh".into(),
            "emphasized" => "Removed Exclamation".into(),
            "questioned" => "Removed Question Mark".into(),
            "emoji" => format!("Removed {}", emoji.unwrap_or("emoji")),
            "sticker" => "Removed Sticker".into(),
            other => format!("Removed {other}"),
        };
    }
    match kind {
        "loved" => "Loved a message".into(),
        "liked" => "Liked a message".into(),
        "disliked" => "Disliked a message".into(),
        "laughed" => "Laughed at a message".into(),
        "emphasized" => "Emphasized a message".into(),
        "questioned" => "Questioned a message".into(),
        "emoji" => format!("{} reacted", emoji.unwrap_or("Emoji")),
        "sticker" => "Reacted with a sticker".into(),
        other => format!("{other} reaction"),
    }
}

/// The name of a reaction and, for an emoji reaction, the emoji itself.
fn tapback_kind(kind: Tapback<'_>) -> (&'static str, Option<String>) {
    match kind {
        Tapback::Loved => ("loved", None),
        Tapback::Liked => ("liked", None),
        Tapback::Disliked => ("disliked", None),
        Tapback::Laughed => ("laughed", None),
        Tapback::Emphasized => ("emphasized", None),
        Tapback::Questioned => ("questioned", None),
        Tapback::Emoji(e) => ("emoji", e.map(str::to_string)),
        Tapback::Sticker => ("sticker", None),
    }
}

/// JSON array of tapbacks on this message, if any exist.
fn build_parent_tapbacks(session: &MailSession, message: &Message) -> Option<Value> {
    let parts = session.tapbacks.get(&message.guid)?;
    let mut sortable: Vec<(usize, i64, i32, TapbackCell)> = Vec::new();
    for (&part_index, tapbacks) in parts {
        for tapback in tapbacks {
            let Variant::Tapback(_, action, kind) = tapback.variant() else {
                continue;
            };
            if matches!(action, TapbackAction::Removed) {
                continue;
            }
            let (kind, emoji) = tapback_kind(kind);
            let (reactor_handle, reactor_display_name) = if tapback.is_from_me() {
                (
                    None,
                    Some(owner_display_name(session, tapback).unwrap_or_else(|| ME.to_string())),
                )
            } else if let Some(handle_id) = tapback.handle_id {
                (
                    raw_handle(session, handle_id),
                    display_name_for(session, handle_id),
                )
            } else {
                (None, None)
            };
            sortable.push((
                part_index,
                tapback.date,
                tapback.rowid,
                TapbackCell {
                    part_index,
                    kind,
                    emoji,
                    reactor_handle,
                    reactor_display_name,
                },
            ));
        }
    }
    if sortable.is_empty() {
        return None;
    }
    sortable.sort_by_key(|(part, date, rowid, _)| (*part, *date, *rowid));
    let cells: Vec<_> = sortable.into_iter().map(|(_, _, _, c)| c).collect();
    serde_json::to_value(&cells).ok()
}

/// Chat id, roster and sender fields for one row.
struct RowContext {
    conversation: ConversationRecord,
    is_from_me: bool,
    sender_handle: Option<String>,
    sender_display_name: Option<String>,
    service: String,
}

/// Resolve the conversation and sender a row belongs to. A row whose chat is
/// gone lands in the `orphaned` conversation.
fn resolve_context(session: &MailSession, message: &Message) -> RowContext {
    let conversation = match session.conversation(message) {
        Some((chatroom, _)) => {
            let (participants, conversation_type) = participants_for(session, chatroom);
            ConversationRecord {
                chat_identifier: if chatroom.chat_identifier.is_empty() {
                    ORPHANED.to_string()
                } else {
                    chatroom.chat_identifier.clone()
                },
                conversation_type: conversation_type.to_string(),
                group_title: chatroom
                    .display_name()
                    .map(str::trim)
                    .filter(|n| !n.is_empty())
                    .map(str::to_string),
                participants,
            }
        }
        None => ConversationRecord {
            chat_identifier: ORPHANED.to_string(),
            conversation_type: "individual".to_string(),
            group_title: None,
            participants: Vec::new(),
        },
    };

    let is_from_me = message.is_from_me();
    let (sender_handle, sender_display_name) = if is_from_me {
        (None, None)
    } else if let Some(handle_id) = message.handle_id {
        (
            raw_handle(session, handle_id),
            display_name_for(session, handle_id),
        )
    } else {
        (None, None)
    };

    let service = match message.service() {
        Service::Unknown => String::new(),
        other => other.to_string(),
    };

    RowContext {
        conversation,
        is_from_me,
        sender_handle,
        sender_display_name,
        service,
    }
}

/// Build the conversation and message records for one row.
///
/// # Errors
///
/// Returns an error when body parts or attachments cannot be loaded.
fn build_record(
    session: &MailSession,
    message: &Message,
) -> Result<(ConversationRecord, MessageRecord), RuntimeError> {
    let context = resolve_context(session, message);
    let (parts, attachments) = collect_parts_and_attachments(session, message)?;
    let mut row = classify_row(session, message, &context.service, !attachments.is_empty());
    let kind = row.kind;
    let text = std::mem::take(&mut row.text);
    let imessage = imessage_fields(session, message, row, &parts);

    let record = MessageRecord {
        chat_identifier: context.conversation.chat_identifier.clone(),
        guid: message.guid.clone(),
        timestamp_unix_ms: timestamp_unix_ms(message, session.offset),
        outgoing: context.is_from_me,
        service: context.service,
        message_kind: kind.to_string(),
        sender_handle: context.sender_handle,
        sender_display_name: context.sender_display_name,
        subject: message.subject.clone().filter(|s| !s.is_empty()),
        text,
        owner_handle: message.destination_caller_id.clone().unwrap_or_default(),
        owner_display_name: owner_display_name(session, message),
        imessage: (!is_empty(&imessage)).then_some(imessage),
        attachments,
    };
    Ok((context.conversation, record))
}

/// Which of the message kinds a row is, the text that stands for it, and the
/// values that decided it (which the Apple-specific fields report too).
struct RowKind {
    kind: &'static str,
    text: String,
    /// The announcement's text, for announcement rows.
    announcement: Option<String>,
    /// The shared-location label, for location rows.
    shared_location: Option<String>,
    /// The send effect's label, already appended to `text`.
    send_effect: Option<String>,
    /// The app balloon's payload, for balloon rows.
    app: Option<Value>,
    tapback: Option<TapbackFields>,
}

/// A tapback row: which reaction, added or removed, on which message part.
struct TapbackFields {
    kind: &'static str,
    /// The emoji, for the `emoji` kind.
    emoji: Option<String>,
    action: &'static str,
    /// Guid of the message reacted to.
    associated_guid: Option<String>,
    /// Part index within that message.
    associated_part: Option<u32>,
}

impl TapbackFields {
    /// Read the reaction out of a row's [`Variant::Tapback`].
    fn from_variant(message: &Message, action: TapbackAction, kind: Tapback<'_>) -> Self {
        let (kind, emoji) = tapback_kind(kind);
        let action = match action {
            TapbackAction::Added => "add",
            TapbackAction::Removed => "remove",
        };
        let target = message.clean_associated_guid();
        Self {
            kind,
            emoji,
            action,
            associated_guid: target.map(|(_, guid)| guid.to_string()),
            associated_part: target.map(|(part, _)| part as u32),
        }
    }

    /// The message kind: stickers sent as reactions are their own kind.
    fn message_kind(&self) -> &'static str {
        if self.kind == "sticker" {
            "sticker_tapback"
        } else {
            "tapback"
        }
    }

    /// The human-readable line that stands in for the reaction's text.
    fn text(&self) -> String {
        tapback_human_line(self.kind, self.emoji.as_deref(), self.action)
    }
}

/// Decide a row's kind and text. The order matters: a tapback, SharePlay
/// end, or announcement wins over its (usually empty) text; a shared
/// location or app balloon over a plain text; and a plain text is iMessage,
/// MMS, or SMS by its service and whether it carries attachments.
fn classify_row(
    session: &MailSession,
    message: &Message,
    service: &str,
    has_attachments: bool,
) -> RowKind {
    let shared_location = message
        .shared_location_kind()
        .map(shared_location_label)
        .map(str::to_string);
    let app = build_balloon_value(session.data_source.db(), message);
    let plain = || message.text.clone().unwrap_or_default();

    let (kind, text, announcement, tapback) =
        if let Variant::Tapback(_, action, kind) = message.variant() {
            let tapback = TapbackFields::from_variant(message, action, kind);
            (tapback.message_kind(), tapback.text(), None, Some(tapback))
        } else if message.is_shareplay() {
            let text = "SharePlay Message Ended".to_string();
            ("announcement", text.clone(), Some(text), None)
        } else if message.is_announcement() {
            let text = announcement_text(session, message).unwrap_or_default();
            ("announcement", text.clone(), Some(text), None)
        } else if let Some(location) = shared_location.as_deref() {
            let text = message
                .text
                .clone()
                .unwrap_or_else(|| format!("Shared location {location}"));
            ("location_share", text, None, None)
        } else if let Some(app) = &app {
            (
                "balloon",
                balloon_summary(app, message.text.as_deref()),
                None,
                None,
            )
        } else if service.eq_ignore_ascii_case("imessage") {
            ("imessage", plain(), None, None)
        } else if has_attachments {
            ("mms", plain(), None, None)
        } else {
            ("sms", plain(), None, None)
        };

    let send_effect = expressive_label(message.get_expressive());
    RowKind {
        kind,
        text: with_send_effect(text, send_effect.as_deref()),
        announcement,
        shared_location,
        send_effect,
        app,
        tapback,
    }
}

/// The text with the send effect's label appended, unless it already names it.
fn with_send_effect(text: String, effect: Option<&str>) -> String {
    match effect {
        None => text,
        Some(effect) if text.is_empty() => effect.to_string(),
        Some(effect) if text.contains(effect) => text,
        Some(effect) => format!("{text}\n\n{effect}"),
    }
}

/// Reply threading for a row.
struct ThreadFields {
    /// A reply in a thread (never true for a tapback).
    is_reply: bool,
    /// The message replied to: the reacted-to message for a tapback, else
    /// the thread originator.
    in_reply_to_guid: Option<String>,
    /// Part index within the thread originator.
    thread_originator_part: Option<u32>,
}

/// Where a row points: a tapback at the message it reacts to, any other
/// reply at its thread originator, everything else nowhere.
fn thread_fields(message: &Message, tapback: Option<&TapbackFields>) -> ThreadFields {
    if let Some(tapback) = tapback {
        return ThreadFields {
            is_reply: false,
            in_reply_to_guid: tapback.associated_guid.clone(),
            thread_originator_part: None,
        };
    }
    if !message.is_reply() {
        return ThreadFields {
            is_reply: false,
            in_reply_to_guid: None,
            thread_originator_part: None,
        };
    }
    ThreadFields {
        is_reply: true,
        in_reply_to_guid: message.thread_originator_guid.clone(),
        thread_originator_part: message
            .thread_originator_part
            .as_deref()
            .and_then(parse_thread_part),
    }
}

/// Everything Apple-specific the core message fields do not carry. Blank
/// strings become `None` so the record stays small.
fn imessage_fields(
    session: &MailSession,
    message: &Message,
    row: RowKind,
    parts: &[crate::fields::PartRecord],
) -> ImessageRecord {
    let thread = thread_fields(message, row.tapback.as_ref());
    let edits = message
        .edited_parts
        .as_ref()
        .map(|edited| build_edit_records(edited, &session.offset))
        .unwrap_or_default();
    let read_receipt = read_receipt_rfc3339(message, session.offset);
    // A tapback has no tapbacks of its own.
    let tapbacks = if row.tapback.is_some() {
        None
    } else {
        build_parent_tapbacks(session, message)
    };
    let tapback = row.tapback.as_ref();
    ImessageRecord {
        is_reply: thread.is_reply,
        in_reply_to_guid: trimmed(thread.in_reply_to_guid),
        thread_originator_part: thread.thread_originator_part,
        num_replies: (message.num_replies > 0).then_some(message.num_replies as u32),
        is_deleted: message.is_deleted(),
        send_effect: trimmed(row.send_effect),
        shared_location: trimmed(row.shared_location),
        announcement: trimmed(row.announcement),
        read_receipt_rfc3339: trimmed(read_receipt),
        parts: json_if_any(parts),
        edits: json_if_any(&edits),
        tapbacks,
        balloon_kind: trimmed(row.app.as_ref().and_then(balloon_kind_label)),
        balloon_bundle_id: trimmed(message.balloon_bundle_id.clone()),
        associated_guid: trimmed(tapback.and_then(|t| t.associated_guid.clone())),
        associated_part: tapback.and_then(|t| t.associated_part),
        tapback_kind: tapback.map(|t| t.kind.to_string()),
        tapback_emoji: trimmed(tapback.and_then(|t| t.emoji.clone())),
        tapback_action: tapback.map(|t| t.action.to_string()),
        app: row.app,
    }
}

/// Whether every Apple-specific field is empty, so the record can be dropped.
fn is_empty(fields: &ImessageRecord) -> bool {
    !fields.is_reply
        && fields.in_reply_to_guid.is_none()
        && fields.thread_originator_part.is_none()
        && fields.num_replies.is_none()
        && !fields.is_deleted
        && fields.send_effect.is_none()
        && fields.shared_location.is_none()
        && fields.announcement.is_none()
        && fields.read_receipt_rfc3339.is_none()
        && fields.parts.is_none()
        && fields.edits.is_none()
        && fields.tapbacks.is_none()
        && fields.app.is_none()
        && fields.balloon_bundle_id.is_none()
        && fields.balloon_kind.is_none()
        && fields.associated_guid.is_none()
        && fields.associated_part.is_none()
        && fields.tapback_kind.is_none()
        && fields.tapback_emoji.is_none()
        && fields.tapback_action.is_none()
}

/// The string trimmed, or `None` when blank.
fn trimmed(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// The items as a JSON array, or `None` when there are none.
fn json_if_any<T: serde::Serialize>(items: &[T]) -> Option<Value> {
    if items.is_empty() {
        return None;
    }
    serde_json::to_value(items).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::FixtureDb;
    use chat_db_fixture::{
        FRIEND_EMAIL, FRIEND_PHONE, GROUP_CHAT_IDENTIFIER, GROUP_TITLE, OWNER, OWNER_EMAIL,
    };
    use imessage_reader_protocol::AttachmentSource;
    use std::collections::HashMap;

    #[test]
    fn send_effect_is_appended_once() {
        assert_eq!(with_send_effect("hi".into(), None), "hi");
        assert_eq!(with_send_effect(String::new(), Some("Slam")), "Slam");
        assert_eq!(with_send_effect("hi".into(), Some("Slam")), "hi\n\nSlam");
        assert_eq!(with_send_effect("hi Slam".into(), Some("Slam")), "hi Slam");
    }

    #[test]
    fn tapback_lines_name_the_reaction() {
        assert_eq!(tapback_human_line("loved", None, "add"), "Loved a message");
        assert_eq!(tapback_human_line("loved", None, "remove"), "Removed Heart");
        assert_eq!(tapback_human_line("emoji", Some("🔥"), "add"), "🔥 reacted");
    }

    #[test]
    fn empty_fields_are_dropped() {
        assert!(is_empty(&ImessageRecord::default()));
        let fields = ImessageRecord {
            is_deleted: true,
            ..ImessageRecord::default()
        };
        assert!(!is_empty(&fields));
    }

    #[test]
    fn blank_strings_become_none() {
        assert_eq!(trimmed(Some("  a  ".to_string())).as_deref(), Some("a"));
        assert_eq!(trimmed(Some("   ".to_string())), None);
        assert_eq!(trimmed(None), None);
        assert_eq!(json_if_any::<u8>(&[]), None);
        assert_eq!(json_if_any(&[1u8]), Some(serde_json::json!([1])));
    }

    #[test]
    fn a_tapback_kind_is_its_name_and_an_emoji_keeps_the_emoji() {
        assert_eq!(tapback_kind(Tapback::Loved), ("loved", None));
        assert_eq!(tapback_kind(Tapback::Sticker), ("sticker", None));
        assert_eq!(
            tapback_kind(Tapback::Emoji(Some("🔥"))),
            ("emoji", Some("🔥".to_string()))
        );
    }

    #[test]
    fn a_reply_points_at_its_originator_and_a_tapback_at_its_target() {
        let fixture = FixtureDb::write();
        let session = fixture.session();
        let mut message = FixtureDb::messages(&session).remove(1);

        let plain = thread_fields(&message, None);
        assert!(!plain.is_reply);
        assert_eq!(plain.in_reply_to_guid, None);

        message.thread_originator_guid = Some("guid-1".to_string());
        message.thread_originator_part = Some("0/1".to_string());
        let reply = thread_fields(&message, None);
        assert!(reply.is_reply);
        assert_eq!(reply.in_reply_to_guid.as_deref(), Some("guid-1"));
        assert_eq!(reply.thread_originator_part, Some(0));

        let tapback = TapbackFields {
            kind: "loved",
            emoji: None,
            action: "add",
            associated_guid: Some("guid-1".to_string()),
            associated_part: Some(0),
        };
        assert_eq!(tapback.message_kind(), "tapback");
        assert_eq!(tapback.text(), "Loved a message");
        let pointed = thread_fields(&message, Some(&tapback));
        assert!(!pointed.is_reply, "a tapback is never a reply");
        assert_eq!(pointed.in_reply_to_guid.as_deref(), Some("guid-1"));
    }

    /// A poll is exported; a vote on it and a row that adds an option are
    /// noise Apple's own export skips. Messages writes a vote as reaction
    /// type 4000, and an update as a poll balloon that points at another
    /// row's poll.
    #[test]
    fn a_poll_is_kept_and_its_votes_and_updates_are_noise() {
        const POLLS: &str = "com.apple.messages.MSMessageExtensionBalloonPlugin:0000000000:com.apple.messages.Polls";
        let fixture = FixtureDb::write();
        let session = fixture.session();
        let base = || FixtureDb::messages(&session).remove(1);

        assert!(!is_poll_noise(&base()), "a plain message");

        let mut poll = base();
        poll.balloon_bundle_id = Some(POLLS.to_string());
        assert!(poll.is_poll());
        assert!(!is_poll_noise(&poll), "the poll itself");

        let mut vote = base();
        vote.associated_message_type = Some(4000);
        vote.associated_message_guid = Some(poll.guid.clone());
        assert!(is_poll_noise(&vote), "a vote");

        let mut update = base();
        update.guid = "guid-update".to_string();
        update.balloon_bundle_id = Some(POLLS.to_string());
        update.associated_message_guid = Some(poll.guid.clone());
        assert!(is_poll_noise(&update), "an added option");
    }

    /// A tapback row read off the message's own fields: reaction 2000 is a
    /// heart added, 3000 a heart removed, and the target guid is the part
    /// prefix stripped off `associated_message_guid`.
    #[test]
    fn a_tapback_row_is_read_off_the_message() {
        let fixture = FixtureDb::write();
        let session = fixture.session();
        let mut message = FixtureDb::messages(&session).remove(1);
        // Messages stores a target as `p:<part>/<36-character guid>`.
        let target = "0F3A3C1E-9B7D-4E2A-8C6F-1D2E3F4A5B6C";
        message.associated_message_type = Some(2000);
        message.associated_message_guid = Some(format!("p:1/{target}"));
        let Variant::Tapback(_, action, kind) = message.variant() else {
            panic!("a heart tapback");
        };
        let tapback = TapbackFields::from_variant(&message, action, kind);
        assert_eq!((tapback.kind, tapback.action), ("loved", "add"));
        assert_eq!(tapback.associated_guid.as_deref(), Some(target));
        assert_eq!(tapback.associated_part, Some(1));

        message.associated_message_type = Some(1000);
        let Variant::Tapback(_, action, kind) = message.variant() else {
            panic!("a sticker tapback");
        };
        let sticker = TapbackFields::from_variant(&message, action, kind);
        assert_eq!(sticker.message_kind(), "sticker_tapback");
    }

    /// Every fixture row becomes a record: the photo message is an iMessage
    /// carrying one attachment on disk, the reply is outgoing and named by
    /// the caller id, and the group message carries the group's roster and
    /// title.
    #[test]
    fn every_fixture_row_builds_a_record() {
        let fixture = FixtureDb::write();
        let session = fixture.session_with_contacts();
        let messages = FixtureDb::messages(&session);

        let (conversation, photo) = build_record(&session, &messages[0]).unwrap();
        assert_eq!(conversation.chat_identifier, FRIEND_PHONE);
        assert_eq!(conversation.conversation_type, "individual");
        assert_eq!(conversation.group_title, None);
        assert_eq!(conversation.participants.len(), 1);
        assert_eq!(
            conversation.participants[0].display_name.as_deref(),
            Some("Sam Example")
        );
        assert_eq!(photo.guid, "guid-1");
        assert!(!photo.outgoing);
        assert_eq!(photo.message_kind, "imessage");
        assert_eq!(photo.service, "iMessage");
        assert_eq!(photo.sender_handle.as_deref(), Some(FRIEND_PHONE));
        assert_eq!(photo.sender_display_name.as_deref(), Some("Sam Example"));
        assert_eq!(photo.owner_handle, OWNER);
        assert_eq!(photo.owner_display_name.as_deref(), Some(OWNER));
        assert_eq!(photo.attachments.len(), 1);
        assert_eq!(
            photo.attachments[0].original_name.as_deref(),
            Some("photo.jpg")
        );
        assert!(matches!(
            &photo.attachments[0].source,
            AttachmentSource::Path { path, size_hint: Some(17) } if path.ends_with("photo.jpg")
        ));
        assert_eq!(
            photo.timestamp_unix_ms, 1_578_307_200_000,
            "2001 + 600,000,000 s"
        );

        let (_, reply) = build_record(&session, &messages[1]).unwrap();
        assert!(reply.outgoing);
        assert_eq!(reply.text, "Nice");
        assert_eq!(reply.sender_handle, None);
        let fields = reply.imessage.expect("the parsed body is one part");
        assert_eq!(
            fields.parts,
            Some(serde_json::json!([{ "index": 0, "kind": "run", "text": "Nice" }]))
        );
        assert!(!fields.is_reply);
        assert_eq!(fields.tapback_kind, None);
        assert_eq!(fields.app, None);
        assert!(reply.attachments.is_empty());

        let (group, message) = build_record(&session, &messages[2]).unwrap();
        assert_eq!(group.chat_identifier, GROUP_CHAT_IDENTIFIER);
        assert_eq!(group.conversation_type, "group");
        assert_eq!(group.group_title.as_deref(), Some(GROUP_TITLE));
        let mut handles: Vec<_> = group
            .participants
            .iter()
            .map(|p| p.handle.as_str())
            .collect();
        handles.sort_unstable();
        assert_eq!(handles, vec![FRIEND_PHONE, FRIEND_EMAIL]);
        assert_eq!(message.text, "Saturday works");
        assert_eq!(message.sender_display_name.as_deref(), Some("Robin"));
    }

    /// The owner's address rides on each row as `destination_caller_id`
    /// carries it: the email for a row sent from the email account, bare
    /// (the `E:` prefix lives only in `chat.account_login`), and empty for a
    /// row Apple wrote with NULL, which is still outgoing and still named
    /// `Me` when the caller id option is on.
    #[test]
    fn the_owner_address_on_a_row_is_the_bare_caller_id_or_nothing() {
        let fixture = FixtureDb::write();
        let session = fixture.session();
        let messages = FixtureDb::messages(&session);

        let (conversation, from_mac) = build_record(&session, &messages[3]).unwrap();
        assert_eq!(conversation.chat_identifier, FRIEND_EMAIL);
        assert_eq!(conversation.conversation_type, "individual");
        let handles: Vec<_> = conversation
            .participants
            .iter()
            .map(|p| p.handle.as_str())
            .collect();
        assert_eq!(
            handles,
            vec![FRIEND_EMAIL],
            "the owner is not a participant"
        );
        assert!(from_mac.outgoing);
        assert_eq!(from_mac.text, "From my Mac");
        assert_eq!(from_mac.sender_handle, None);
        assert_eq!(from_mac.owner_handle, OWNER_EMAIL);
        assert_eq!(from_mac.owner_display_name.as_deref(), Some(OWNER_EMAIL));

        let (conversation, still_me) = build_record(&session, &messages[4]).unwrap();
        assert_eq!(conversation.chat_identifier, FRIEND_PHONE);
        assert!(still_me.outgoing);
        assert_eq!(still_me.text, "Still me");
        assert_eq!(still_me.sender_handle, None);
        assert_eq!(still_me.owner_handle, "", "NULL comes through as empty");
        assert_eq!(still_me.owner_display_name.as_deref(), Some(ME));
    }

    /// The fixture's photo message was read a minute after it arrived, so
    /// it carries that stamp; the outgoing "Nice" row has `date_read` NULL
    /// and must carry no receipt rather than Apple's epoch (issue #630).
    #[test]
    fn an_unread_message_has_no_read_receipt_and_a_read_one_has_the_stamp() {
        let fixture = FixtureDb::write();
        let session = fixture.session();
        let messages = FixtureDb::messages(&session);

        let (_, photo) = build_record(&session, &messages[0]).unwrap();
        let fields = photo.imessage.expect("the photo row has Apple fields");
        let receipt = fields
            .read_receipt_rfc3339
            .expect("a read message carries its receipt");
        let read_at = chrono::DateTime::parse_from_rfc3339(&receipt).unwrap();
        assert_eq!(
            read_at.timestamp_millis(),
            1_578_307_260_000,
            "2001 + 600,000,060 s"
        );

        let (_, reply) = build_record(&session, &messages[1]).unwrap();
        assert_eq!(reply.imessage.unwrap().read_receipt_rfc3339, None);
    }

    /// A row whose chat is gone lands in the orphaned conversation, and one
    /// whose service is unknown is SMS or MMS by its attachments.
    #[test]
    fn an_orphaned_row_and_a_non_imessage_row_are_classified() {
        let fixture = FixtureDb::write();
        let session = fixture.session();
        let mut message = FixtureDb::messages(&session).remove(1);
        message.chat_id = Some(42);
        message.service = Some("SMS".to_string());

        let context = resolve_context(&session, &message);
        assert_eq!(context.conversation.chat_identifier, ORPHANED);
        assert!(context.conversation.participants.is_empty());
        assert_eq!(context.service, "SMS");

        assert_eq!(classify_row(&session, &message, "SMS", false).kind, "sms");
        assert_eq!(classify_row(&session, &message, "SMS", true).kind, "mms");
        assert_eq!(
            classify_row(&session, &message, "iMessage", true).kind,
            "imessage"
        );
    }

    /// The classifier's other branches over a fixture row with its fields
    /// changed: a rename announcement, a location share, a balloon and a
    /// send effect.
    #[test]
    fn announcements_locations_balloons_and_effects_are_classified() {
        let fixture = FixtureDb::write();
        let session = fixture.session_with_contacts();
        let base = FixtureDb::messages(&session).remove(2);

        let mut rename = FixtureDb::messages(&session).remove(2);
        rename.item_type = 2;
        rename.group_title = Some("New name".to_string());
        assert!(rename.is_announcement());
        let row = classify_row(&session, &rename, "iMessage", false);
        assert_eq!(row.kind, "announcement");
        assert_eq!(row.text, "Robin named the conversation New name");
        assert_eq!(
            announcement_text(&session, &rename).as_deref(),
            Some("Robin named the conversation New name")
        );

        let mut added = FixtureDb::messages(&session).remove(2);
        added.item_type = 1;
        added.group_action_type = 0;
        added.other_handle = Some(1);
        added.is_from_me = true;
        // The owner is named by the caller id when that option is on; only
        // a bare `Me` becomes `You`.
        assert_eq!(
            announcement_text(&session, &added).as_deref(),
            Some("+15550000001 added Sam Example to the conversation.")
        );
        added.destination_caller_id = None;
        assert_eq!(
            announcement_text(&session, &added).as_deref(),
            Some("You added Sam Example to the conversation.")
        );

        let mut location = FixtureDb::messages(&session).remove(2);
        location.item_type = 4;
        location.share_status = false;
        location.share_direction = Some(true);
        location.text = None;
        let row = classify_row(&session, &location, "iMessage", false);
        assert_eq!(row.kind, "location_share");
        assert!(row.text.starts_with("Shared location "), "{}", row.text);
        assert!(row.shared_location.is_some());

        let mut balloon = FixtureDb::messages(&session).remove(2);
        balloon.balloon_bundle_id =
            Some("com.apple.PassbookUIService.PeerPaymentMessagesExtension".to_string());
        let row = classify_row(&session, &balloon, "iMessage", false);
        assert_eq!(row.kind, "balloon");
        assert_eq!(row.text, "Saturday works");
        assert_eq!(
            row.app.as_ref().and_then(balloon_kind_label).as_deref(),
            Some("apple_pay")
        );

        let mut slam = base;
        slam.expressive_send_style_id =
            Some("com.apple.MobileSMS.expressivesend.impact".to_string());
        let row = classify_row(&session, &slam, "iMessage", false);
        assert_eq!(row.text, "Saturday works\n\nSent with Slam");
        assert_eq!(row.send_effect.as_deref(), Some("Sent with Slam"));
        let fields = imessage_fields(&session, &slam, row, &[]);
        assert_eq!(fields.send_effect.as_deref(), Some("Sent with Slam"));
        assert!(!is_empty(&fields));
    }

    /// Tapbacks on a parent are listed in part, date and rowid order, with
    /// the reactor named; a removed tapback is left out.
    #[test]
    fn parent_tapbacks_are_listed_in_order_and_named() {
        let fixture = FixtureDb::write();
        let mut session = fixture.session_with_contacts();
        let messages = FixtureDb::messages(&session);
        let parent = &messages[1];

        let mut heart = FixtureDb::messages(&session).remove(0);
        heart.associated_message_type = Some(2000);
        heart.associated_message_guid = Some(format!("p:0/{}", parent.guid));
        heart.date = 20;
        let mut fire = FixtureDb::messages(&session).remove(2);
        fire.associated_message_type = Some(2006);
        fire.associated_message_emoji = Some("🔥".to_string());
        fire.associated_message_guid = Some(format!("p:0/{}", parent.guid));
        fire.date = 10;
        fire.is_from_me = true;
        let mut removed = FixtureDb::messages(&session).remove(0);
        removed.associated_message_type = Some(3000);
        removed.associated_message_guid = Some(format!("p:0/{}", parent.guid));
        session.tapbacks.insert(
            parent.guid.clone(),
            HashMap::from([(0usize, vec![heart, fire, removed])]),
        );

        let value = build_parent_tapbacks(&session, parent).expect("two tapbacks");
        let cells = value.as_array().unwrap();
        assert_eq!(cells.len(), 2, "{value}");
        assert_eq!(cells[0]["kind"], "emoji");
        assert_eq!(cells[0]["emoji"], "🔥");
        assert_eq!(cells[0]["reactor_display_name"], OWNER);
        assert_eq!(cells[1]["kind"], "loved");
        assert_eq!(cells[1]["reactor_handle"], FRIEND_PHONE);
        assert_eq!(cells[1]["reactor_display_name"], "Sam Example");

        assert_eq!(build_parent_tapbacks(&session, &messages[0]), None);
    }

    /// The whole stream over the fixture: five rows seen, none skipped. Each
    /// conversation is announced once, before its first message, and the
    /// stream ends with the full parse count and the done event.
    #[test]
    fn the_fixture_streams_without_a_failure() {
        let fixture = FixtureDb::write();
        let session = fixture.session();
        let events = crate::log::capture::events(|| stream_export(&session).unwrap());

        let lines: Vec<String> = events
            .iter()
            .map(|event| match event["event"].as_str().unwrap() {
                "conversation" => format!("conversation {}", event["chat_identifier"]),
                "message" => format!("message {} in {}", event["guid"], event["chat_identifier"]),
                other => other.to_string(),
            })
            .collect();
        assert_eq!(
            lines,
            [
                r#"conversation "+15550000002""#,
                r#"message "guid-1" in "+15550000002""#,
                r#"message "guid-2" in "+15550000002""#,
                r#"conversation "chat100""#,
                r#"message "guid-3" in "chat100""#,
                r#"conversation "friend@example.com""#,
                r#"message "guid-4" in "friend@example.com""#,
                r#"message "guid-5" in "+15550000002""#,
                "progress",
                "export_done",
            ]
        );
        assert_eq!(
            events[8],
            serde_json::json!({"event": "progress", "stage": "parse", "done": 5, "total": 5})
        );
        assert_eq!(
            events[9],
            serde_json::json!({"event": "export_done", "messages_seen": 5, "failures": 0})
        );
    }

    #[test]
    fn a_seconds_stamp_from_an_older_database_reads_the_same_as_a_nanoseconds_one() {
        let fixture = FixtureDb::write();
        let session = fixture.session();
        let mut message = FixtureDb::messages(&session).remove(1);
        assert_eq!(
            timestamp_unix_ms(&message, session.offset),
            1_578_307_260_000
        );
        message.date = 600_000_060;
        assert_eq!(
            timestamp_unix_ms(&message, session.offset),
            1_578_307_260_000,
            "a seconds stamp from an older database reads the same"
        );
    }

    #[test]
    fn a_timestamp_falls_back_to_the_raw_stamp_when_the_date_is_invalid() {
        let fixture = FixtureDb::write();
        let session = fixture.session();
        let mut message = FixtureDb::messages(&session).remove(1);
        // Ten trillion seconds before 2001 is outside the range chrono can
        // hold, so `Message::date` fails and the raw stamp is read instead.
        message.date = -10_000_000_000_000;
        assert!(message.date(session.offset).is_err());
        assert_eq!(
            timestamp_unix_ms(&message, session.offset),
            (-10_000_000_000_000 + session.offset) * 1000
        );
    }
}
