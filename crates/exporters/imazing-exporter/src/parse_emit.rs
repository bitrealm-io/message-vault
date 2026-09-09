//! Peer, timezone, and row-classification helpers for the emitter.

use crate::emit::TransportFamily;
use crate::parse::{RawRow, SourceKind};
use chrono::{Local, LocalResult, NaiveDateTime, TimeZone};
use chrono_tz::Tz;
use phone::sanitize_number;
use std::collections::{HashMap, HashSet};

impl TransportFamily {
    /// The transport family for a source kind.
    pub(super) fn from_kind(kind: SourceKind) -> Self {
        match kind {
            SourceKind::Messages => Self::Messages,
            SourceKind::WhatsApp => Self::WhatsApp,
        }
    }
}

#[derive(Debug)]
pub(super) struct PeerInfo {
    pub(super) chat_id: String,
    pub(super) contact_name: String,
    pub(super) group: bool,
    pub(super) unresolved_chat: bool,
    pub(super) unresolved_roster_labels: u64,
}

/// Work out who a chat session is with from its rows: the chat id, contact name, whether
/// it is a group, and what could not be resolved.
pub(super) fn collect_peer_info(kind: SourceKind, session: &str, rows: &[&RawRow]) -> PeerInfo {
    let mut handles: HashSet<String> = HashSet::new();
    for row in rows {
        let sid = row.sender_id.trim();
        // Email first: a sender like `bob2024@gmail.com` has 4+ digits and
        // must never be reduced to a phone number.
        if sid.contains('@') {
            handles.insert(sid.to_string());
        } else if sanitize_number(sid).is_some() {
            // Format as E.164 (the international phone-number format that starts
            // with +) when unambiguous. Otherwise keep digits as-is. Never invent `+0…`.
            handles.insert(phone::normalize_lenient(sid));
        }
        for phone in phones_in_text(&row.chat_session) {
            handles.insert(phone);
        }
    }

    // The rows themselves pair a sender's name with their address. That is the
    // only name-to-address mapping available now that no contacts file is
    // supplied, and it comes from the source rather than from outside it.
    let mut handle_by_sender_name: HashMap<String, String> = HashMap::new();
    for row in rows {
        let name = row.sender_name.trim();
        let sid = row.sender_id.trim();
        if name.is_empty() || sid.is_empty() {
            continue;
        }
        let handle = if sid.contains('@') {
            sid.to_string()
        } else if sanitize_number(sid).is_some() {
            phone::normalize_lenient(sid)
        } else {
            continue;
        };
        handle_by_sender_name
            .entry(name.to_ascii_lowercase())
            .or_insert(handle);
    }

    let mut unresolved_roster_labels = 0u64;
    // Messages group rosters encode members as "A & B & C". A member who never
    // sent a message has no address anywhere in the source; report them rather
    // than invent one.
    if kind == SourceKind::Messages && session.contains(" & ") {
        for part in session.split(" & ") {
            let label = part.trim();
            if label.is_empty() {
                continue;
            }
            if label.contains('@') {
                handles.insert(label.to_string());
                continue;
            }
            if sanitize_number(label).is_some() {
                handles.insert(phone::normalize_lenient(label));
                continue;
            }
            match handle_by_sender_name.get(&label.to_ascii_lowercase()) {
                Some(handle) => {
                    handles.insert(handle.clone());
                }
                None => unresolved_roster_labels += 1,
            }
        }
    }

    let mut peer_handles: Vec<String> = handles.into_iter().collect();
    peer_handles.sort();

    let group = match kind {
        SourceKind::Messages => session.contains(" & ") || peer_handles.len() >= 2,
        // WhatsApp has no roster column; multiple distinct senders imply a group.
        SourceKind::WhatsApp => peer_handles.len() >= 2,
    };

    let (chat_id, contact_name, unresolved_chat) =
        resolve_chat_identifier(session, &peer_handles, group);
    PeerInfo {
        chat_id,
        contact_name,
        group,
        unresolved_chat,
        unresolved_roster_labels,
    }
}

/// Parse an iMazing date string into `(unix_secs, date_ms)`; DST-ambiguous
/// times resolve to the earliest occurrence.
///
/// `zone` is the named zone the wall clock is read in. `None` falls back to the
/// host zone, which makes the result depend on the machine — the same defect as
/// issue #523, still open for this exporter because nothing sets a zone for it
/// yet.
pub(super) fn parse_message_date(raw: &str, zone: Option<Tz>) -> Option<(i64, String)> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let naive = NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M"))
        .ok()?;
    // Ambiguous (DST fall-back) hours resolve to the earliest instant instead
    // of silently dropping the message.
    let secs = match zone {
        None => match Local.from_local_datetime(&naive) {
            LocalResult::Single(dt) => dt.timestamp(),
            LocalResult::Ambiguous(earliest, _latest) => earliest.timestamp(),
            LocalResult::None => return None,
        },
        Some(zone) => match zone.from_local_datetime(&naive) {
            LocalResult::Single(dt) => dt.timestamp(),
            LocalResult::Ambiguous(earliest, _latest) => earliest.timestamp(),
            LocalResult::None => return None,
        },
    };
    Some((secs, (secs * 1000).to_string()))
}

/// True for rows the exporter treats as sent (`outgoing`/`sent` types).
pub(super) fn is_outgoing(msg_type: &str) -> bool {
    matches!(
        msg_type.trim().to_ascii_lowercase().as_str(),
        "outgoing" | "sent"
    )
}

/// True for iMazing's `Notification` message type.
pub(super) fn is_notification(msg_type: &str) -> bool {
    msg_type.trim().eq_ignore_ascii_case("notification")
}

/// Every `+`-prefixed phone number mentioned in the text.
fn phones_in_text(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'+' {
            let start = i;
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i > start + 1 && sanitize_number(&text[start..i]).is_some() {
                let e164 = phone::normalize_lenient(&text[start..i]);
                if !out.contains(&e164) {
                    out.push(e164);
                }
            }
        } else {
            i += 1;
        }
    }
    out
}

/// Resolve a session into `(chat_identifier, contact_name, name_only)`.
///
/// The third value is `true` when the session names a person and the source
/// recorded no address for them; the chat is then keyed by a stem of the name
/// and the vault reconciles it on import.
fn resolve_chat_identifier(
    session: &str,
    peer_handles: &[String],
    group: bool,
) -> (String, String, bool) {
    if group {
        if !peer_handles.is_empty() {
            let title = session.trim().to_string();
            return (peer_handles.join(","), title, false);
        }
        return (
            message_vault_io_core::name_stem(session),
            session.trim().to_string(),
            true,
        );
    }

    if let Some(handle) = peer_handles.first() {
        return (handle.clone(), session.trim().to_string(), false);
    }

    let session = session.trim();
    if session.is_empty() {
        return ("unknown".to_string(), String::new(), true);
    }
    // Email first: an address like `bob2024@gmail.com` has 4+ digits and must
    // not be treated as a phone number.
    if session.contains('@') {
        return (session.to_string(), String::new(), false);
    }
    if sanitize_number(session).is_some() {
        // Format as E.164 when unambiguous. Otherwise keep digits as-is. Never
        // invent `+0…`.
        return (phone::normalize_lenient(session), String::new(), false);
    }
    (
        message_vault_io_core::name_stem(session),
        session.to_string(),
        true,
    )
}

/// The sender handle and display name for a row: empty for outgoing, the sender for group rows, else the peer.
pub(super) fn resolve_sender(
    row: &RawRow,
    is_from_me: bool,
    is_notification: bool,
    chat_id: &str,
    contact_name: &str,
) -> (String, String) {
    if is_from_me {
        return (String::new(), String::new());
    }
    if is_notification {
        // Keep any available identity from the notification row; often empty.
        // Email first: an address like `bob2024@gmail.com` has 4+ digits and
        // must not be reduced to a phone number.
        let handle = if row.sender_id.contains('@') {
            row.sender_id.trim().to_string()
        } else if sanitize_number(&row.sender_id).is_some() {
            phone::normalize_lenient(&row.sender_id)
        } else {
            String::new()
        };
        return (handle, row.sender_name.trim().to_string());
    }

    let mut handle = String::new();
    if row.sender_id.contains('@') {
        handle = row.sender_id.trim().to_string();
    } else if sanitize_number(&row.sender_id).is_some() {
        // Format as E.164 when unambiguous. Otherwise keep digits as-is. Never invent `+0…`.
        handle = phone::normalize_lenient(&row.sender_id);
    } else if !chat_id.contains('@')
        && (chat_id.starts_with('+') || sanitize_number(chat_id).is_some())
    {
        handle = phone::normalize_lenient(chat_id);
    }

    let mut display = row.sender_name.trim().to_string();
    if display.is_empty() && !contact_name.is_empty() {
        display = contact_name.to_string();
    }

    (handle, display)
}
