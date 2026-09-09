//! Build fingerprint strings so duplicate EML messages collapse to one row.
//!
//! Convert dedupe uses [`cover_identity`]: chat + whole-second time + direction +
//! collapsed text. It ignores `X-smssync-id`, because the same message exported
//! twice can carry the id in one copy and not the other, and it floors the time
//! to the whole second so two exports that disagree below a second still meet.
//!
//! Collapsing whitespace in the text avoids two identities for tiny export
//! differences.

use chrono::{DateTime, Local, TimeZone, Utc};

use crate::types::ParsedMessage;

/// Who this chat is with, as a stable string (E.164 phone or `chat-…` for groups).
///
/// When the mail names the other party but records no address, the chat is
/// keyed by a stem of that name so each person gets their own conversation.
/// Collapsing them all into one `unknown` chat would merge unrelated people;
/// the vault resolves the name against contacts on import.
pub(crate) fn chat_id_for(msg: &ParsedMessage) -> String {
    if msg.conversation_type == "group" {
        format!("chat-{}", msg.chat_key)
    } else if msg.chat_key.is_empty() {
        match name_only_key(msg) {
            Some(key) => key,
            None => "unknown".to_string(),
        }
    } else {
        // Format as E.164 only when unambiguous, so a trunk-zero value stays
        // digits-as-is instead of becoming `+02079460000`.
        phone::normalize_lenient(&msg.chat_key)
    }
}

/// A stem of the peer's name, when the mail named them and recorded no
/// address. `None` when there is no usable name either.
pub(crate) fn name_only_key(msg: &ParsedMessage) -> Option<String> {
    if msg.conversation_type == "group" || !msg.chat_key.is_empty() {
        return None;
    }
    let name = msg.name_alias.as_deref().map_or("", str::trim);
    if name.is_empty() {
        return None;
    }
    Some(message_vault_io_core::name_stem(name))
}

/// Message time as milliseconds since 1970 (for identity strings).
pub(crate) fn timestamp_ms(timestamp_secs: f64) -> i64 {
    (timestamp_secs * 1000.0).round() as i64
}

/// Local wall-clock time for a Unix second, if representable.
///
/// Tries local interpretation, then UTC mapped to local. Returns `None` when
/// the instant is out of range for chrono (callers that need a non-panicking
/// filename prefix may fall back to the Unix epoch themselves).
pub(crate) fn local_datetime_from_secs(secs: i64) -> Option<DateTime<Local>> {
    Local.timestamp_opt(secs, 0).single().or_else(|| {
        Utc.timestamp_opt(secs, 0)
            .single()
            .map(|utc| utc.with_timezone(&Local))
    })
}

/// Clean the body text before fingerprinting.
///
/// Turns newlines into spaces and squeezes repeated spaces so
/// `"Hello  \n\t from Alice\n"` matches `"Hello from Alice"`.
pub(crate) fn normalized_text(text: &str) -> String {
    let unified = text.replace("\r\n", "\n").replace('\r', "\n");
    unified.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Floor millisecond timestamp to the start of its whole second.
pub(crate) fn floor_ms_to_sec(ms: i64) -> i64 {
    ms.div_euclid(1000) * 1000
}

/// Convert dedupe key: chat + whole-second time + direction + text.
///
/// When the message has attachment digests, those digests are appended so two
/// same-second empty-caption MMS with different media stay distinct. Text-only
/// messages keep the plain key, so two exports of the same SMS still collapse.
/// Sub-second time and `X-smssync-id` stay ignored.
pub(crate) fn cover_identity(msg: &ParsedMessage) -> String {
    let mut key = cover_identity_from_parts(
        &chat_id_for(msg),
        timestamp_ms(msg.timestamp_secs),
        msg.is_from_me,
        &normalized_text(&msg.text),
    );
    let mut digests: Vec<&str> = msg
        .attachments
        .iter()
        .map(|a| a.digest_hex.as_str())
        .filter(|d| !d.is_empty())
        .collect();
    if !digests.is_empty() {
        digests.sort_unstable();
        digests.dedup();
        key.push('|');
        key.push_str(&digests.join(","));
    }
    key
}

/// The dedupe identity: chat, second-floored time, direction, and text.
pub(crate) fn cover_identity_from_parts(
    chat_id: &str,
    timestamp_ms: i64,
    is_from_me: bool,
    text: &str,
) -> String {
    format!(
        "{}|{}|{}|{}",
        chat_id,
        floor_ms_to_sec(timestamp_ms),
        if is_from_me { "1" } else { "0" },
        text,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_msg(chat_key: &str, ts: f64, is_from_me: bool, text: &str) -> ParsedMessage {
        ParsedMessage {
            chat_key: chat_key.into(),
            conversation_type: "individual".into(),
            group_title: None,
            participant_digits: if chat_key.is_empty() {
                vec![]
            } else {
                vec![(chat_key.into(), None)]
            },
            timestamp_secs: ts,
            is_from_me,
            sender_digits: if is_from_me || chat_key.is_empty() {
                None
            } else {
                Some(chat_key.into())
            },
            text: text.into(),
            attachments: vec![],
            name_alias: None,
            smssync_id: None,
            android_type: String::new(),
            eml_path: String::new(),
        }
    }

    #[test]
    fn cover_identity_floors_to_second() {
        let whole = sample_msg("4075551234", 1609459200.0, false, "Hello");
        let subsec = sample_msg("4075551234", 1609459200.488, false, "Hello");
        assert_eq!(cover_identity(&whole), cover_identity(&subsec));
        assert_eq!(cover_identity(&whole), "+14075551234|1609459200000|0|Hello");
    }

    #[test]
    fn cover_identity_ignores_smssync_id() {
        let mut a = sample_msg("4075551234", 1609459200.1, false, "Hello");
        a.smssync_id = Some("1".into());
        let mut b = sample_msg("4075551234", 1609459200.9, false, "Hello");
        b.smssync_id = Some("2".into());
        assert_eq!(cover_identity(&a), cover_identity(&b));
    }

    #[test]
    fn cover_identity_distinct_chats() {
        let a = sample_msg("5555550122", 1609459300.0, false, "Hello from Sam");
        let b = sample_msg("5555550111", 1609459200.313, true, "Hello from Alex");
        assert_ne!(cover_identity(&a), cover_identity(&b));
    }

    #[test]
    fn cover_identity_separates_same_second_mms_by_digest() {
        use crate::types::AttachmentBlob;
        let mut a = sample_msg("4075551234", 1609459200.1, false, "");
        a.attachments.push(AttachmentBlob {
            filename: "a.jpg".into(),
            digest_hex: "aaa".into(),
            ..Default::default()
        });
        let mut b = sample_msg("4075551234", 1609459200.9, false, "");
        b.attachments.push(AttachmentBlob {
            filename: "b.jpg".into(),
            digest_hex: "bbb".into(),
            ..Default::default()
        });
        assert_ne!(cover_identity(&a), cover_identity(&b));
        let mut a2 = sample_msg("4075551234", 1609459200.2, false, "");
        a2.attachments.push(AttachmentBlob {
            filename: "a-copy.jpg".into(),
            digest_hex: "aaa".into(),
            ..Default::default()
        });
        assert_eq!(cover_identity(&a), cover_identity(&a2));
    }

    #[test]
    fn cover_identity_collapses_whitespace() {
        let mut spaced = sample_msg("4075551234", 1609459200.5, false, "Hello");
        spaced.text = "Hello  \n\t from\r\nAlice\n".into();
        let compact = sample_msg("4075551234", 1609459200.5, false, "Hello from Alice");
        assert_eq!(cover_identity(&spaced), cover_identity(&compact));
    }

    #[test]
    fn normalized_text_collapses_runs() {
        assert_eq!(normalized_text("  a \n\n b\t "), "a b");
    }

    #[test]
    fn unknown_chat_id_for_empty_peer() {
        let msg = sample_msg("", 1_609_459_200.0, false, "hi");
        assert_eq!(chat_id_for(&msg), "unknown");
    }
}
