//! The chat a parsed EML message belongs to, and its time in milliseconds.
//!
//! Which copies of a message are one message is decided by the shared
//! projection (`message_ir::one_copy_per_message`), not here.

use crate::types::ParsedMessage;

/// Who this chat is with, as a stable string (the peer's handle key, or
/// `chat-…` for groups).
///
/// When the mail names the other party but records no address, the chat is
/// keyed by that name, trimmed, so each person gets their own conversation.
/// Collapsing them all into one `unknown` chat would merge unrelated people;
/// the server resolves the name against contacts on import. The file name is
/// made from this key later, by `ConversationDocument::filename_stem`.
pub(crate) fn chat_id_for(msg: &ParsedMessage) -> String {
    if msg.is_group() {
        format!("chat-{}", msg.chat_key)
    } else if msg.chat_key.is_empty() {
        match name_only_key(msg) {
            Some(key) => key,
            None => "unknown".to_string(),
        }
    } else {
        msg.chat_key.clone()
    }
}

/// The peer's name, trimmed, when the mail named them and recorded no
/// address. `None` when there is no usable name either.
///
/// The name is kept whole: a file-name stem would give "张伟" and "李娜" one
/// key, and "José" and "Josè" another.
pub(crate) fn name_only_key(msg: &ParsedMessage) -> Option<String> {
    if msg.is_group() || !msg.chat_key.is_empty() {
        return None;
    }
    let name = msg.name_alias.as_deref().map_or("", str::trim);
    if name.is_empty() {
        return None;
    }
    Some(name.to_string())
}

/// Message time as milliseconds since 1970.
pub(crate) fn timestamp_ms(timestamp_secs: f64) -> i64 {
    (timestamp_secs * 1000.0).round() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_msg(address: &str, ts: f64, is_from_me: bool, text: &str) -> ParsedMessage {
        let peer = phone::Handle::parse(address);
        ParsedMessage {
            chat_key: peer
                .as_ref()
                .map(|p| p.key().to_string())
                .unwrap_or_default(),
            conversation_type: message_ir::IrConversationType::Individual,
            group_title: None,
            participants: peer.iter().cloned().collect(),
            timestamp_secs: ts,
            has_milliseconds: true,
            is_from_me,
            sender: peer.filter(|_| !is_from_me),
            text: text.into(),
            attachments: vec![],
            unreadable_parts: 0,
            name_alias: None,
            smssync_id: None,
            android_type: String::new(),
            eml_path: String::new(),
            owner_not_named: false,
        }
    }

    #[test]
    fn unknown_chat_id_for_empty_peer() {
        let msg = sample_msg("", 1_609_459_200.0, false, "hi");
        assert_eq!(chat_id_for(&msg), "unknown");
    }

    #[test]
    fn two_people_known_only_by_name_have_two_chats() {
        let mut a = sample_msg("", 1.0, false, "hi");
        a.name_alias = Some("张伟".into());
        let mut b = a.clone();
        b.name_alias = Some("李娜".into());
        assert_ne!(chat_id_for(&a), chat_id_for(&b));
        assert_ne!(chat_id_for(&a), "unknown");
    }

    #[test]
    fn names_that_differ_only_in_an_accent_have_two_chats() {
        let mut a = sample_msg("", 1.0, false, "hi");
        a.name_alias = Some("José".into());
        let mut b = a.clone();
        b.name_alias = Some("Josè".into());
        assert_ne!(chat_id_for(&a), chat_id_for(&b));
    }

    #[test]
    fn a_chat_known_only_by_name_is_keyed_on_the_trimmed_name() {
        let mut msg = sample_msg("", 1.0, false, "hi");
        msg.name_alias = Some("  José Ramírez \t".into());
        assert_eq!(chat_id_for(&msg), "José Ramírez");
    }
}
