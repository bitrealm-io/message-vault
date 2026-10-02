//! Chat-id helpers: E.164-guarded phone formatting and group chat ids.

use message_ir::HandleType;
use phone::OwnerHandleSet;

/// Format as E.164 (the international phone-number format that starts with +)
/// when the digits are unambiguous for the US-centric crate. Otherwise keep
/// the digits as-is. Never invent `+0…`.
pub(super) fn guarded_phone(digits: &str) -> String {
    phone::normalize_digits_us(digits).unwrap_or_default()
}

/// Chat id for a 1:1 conversation: E.164 when unambiguous.
pub(super) fn chat_id_individual(digits: &str) -> String {
    guarded_phone(digits)
}

/// Group chat id and display title from participant digits (owner excluded).
/// A PDU that names nobody but the owner gets the one shared unknown id.
pub(super) fn chat_id_group(
    participant_digits: &[String],
    owners: &OwnerHandleSet,
) -> (String, String) {
    let others: Vec<String> = participant_digits
        .iter()
        .filter(|d| !d.is_empty() && !owners.is_owner(d, HandleType::Phone))
        .cloned()
        .collect();
    if others.is_empty() {
        return ("chat-group-unknown".to_string(), "Group".to_string());
    }
    phone::group_chat_id("chat-group-", &others)
}
