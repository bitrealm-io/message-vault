//! What a conversation is keyed by.
//!
//! A conversation's chat id is its key: it is part of every message's
//! `guid`, and the server recognises the conversation on the next import by
//! it. [`ConversationKey`] says what kind of key a conversation has, so that
//! a group's key is never a person's address and a group's members are never
//! read back out of its id.

use crate::IrParticipant;

/// What every group chat id starts with, so a group's key can never equal an
/// address.
pub const GROUP_CHAT_ID_PREFIX: &str = "group:";

/// The key of one conversation.
#[derive(Debug, Clone)]
pub enum ConversationKey {
    /// A one-to-one conversation, keyed by the other person's address.
    OneToOne(String),
    /// A group, keyed by an id the exporter takes from the source and never
    /// from who wrote in it. The members are data: the key does not change
    /// when they do.
    Group {
        /// The group's id, unique within its source.
        vendor_id: String,
        /// The people in the group other than the account holder.
        members: Vec<IrParticipant>,
    },
    /// A one-to-one conversation with a person the source names and records
    /// no address for, keyed by the name.
    NameOnly(String),
}

impl ConversationKey {
    /// The conversation's chat id: the address for [`Self::OneToOne`],
    /// `group:` and the vendor id for [`Self::Group`], and the name made
    /// filename-safe ([`name_stem`]) for [`Self::NameOnly`].
    pub fn chat_id(&self) -> String {
        match self {
            Self::OneToOne(handle) => handle.clone(),
            Self::Group { vendor_id, .. } => format!("{GROUP_CHAT_ID_PREFIX}{vendor_id}"),
            Self::NameOnly(name) => name_stem(name),
        }
    }

    /// Whether the conversation is a group.
    pub fn is_group(&self) -> bool {
        matches!(self, Self::Group { .. })
    }

    /// Whether the conversation is keyed by a person's name.
    pub fn is_name_only(&self) -> bool {
        matches!(self, Self::NameOnly(_))
    }
}

/// Filesystem-safe stem from a display name or handle (alnum / `-` / `_` / `+`).
pub fn name_stem(value: &str) -> String {
    let mut raw = String::with_capacity(value.len());
    for c in value.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '+' {
            raw.push(c);
        } else {
            raw.push('_');
        }
    }
    if raw.is_empty() || raw.chars().all(|c| c == '_') {
        "unknown".to_string()
    } else {
        raw
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_stem_sanitizes() {
        assert_eq!(name_stem("Alice Bob"), "Alice_Bob");
        assert_eq!(name_stem("+15555550100"), "+15555550100");
        assert_eq!(name_stem("!!!"), "unknown");
        assert_eq!(name_stem(""), "unknown");
    }

    /// A group's chat id is in a namespace of its own, so even a vendor id
    /// shaped like an address never gives a group a person's key.
    #[test]
    fn a_group_chat_id_never_equals_an_address() {
        let address = "+15555550111";
        let group = ConversationKey::Group {
            vendor_id: address.to_string(),
            members: Vec::new(),
        };
        assert_ne!(
            group.chat_id(),
            ConversationKey::OneToOne(address.into()).chat_id()
        );
        assert_eq!(group.chat_id(), "group:+15555550111");
    }
}
