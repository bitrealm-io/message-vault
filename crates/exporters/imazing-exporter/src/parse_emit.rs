//! Conversation key, sender, date, and row-classification helpers for the
//! emitter.

use crate::emit::TransportFamily;
use crate::parse::{RawRow, SourceKind};
use chrono::NaiveDateTime;
use message_csv::Zone;
use message_ir::{ConversationKey, HandleType, IrParticipant};
use phone::{sanitize_number, sanitize_phone_shaped};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};

impl TransportFamily {
    /// The transport family for a source kind.
    pub(super) fn from_kind(kind: SourceKind) -> Self {
        match kind {
            SourceKind::Messages => Self::Messages,
            SourceKind::WhatsApp => Self::WhatsApp,
        }
    }
}

/// Who one chat session is with: its key, and the name the source gives the
/// chat.
#[derive(Debug)]
pub(super) struct Session {
    pub(super) key: ConversationKey,
    /// The session name, or empty when the session name is the chat's address.
    pub(super) contact_name: String,
    /// Members a Messages group's session name lists by a name no row pairs
    /// with an address.
    pub(super) unresolved_roster_labels: u64,
}

/// Work out the key of one chat session from its rows.
///
/// A Messages session named as a roster ("A & B") is a group, and so is any
/// session in which two or more people wrote. Two addresses that the rows
/// give one Sender Name are one person, so one contact writing from a number
/// and an email address is a one-to-one conversation.
///
/// A group's key is a hash of its earliest row ([`group_vendor_id`]), never
/// who wrote, the session name, or the file it came from: iMazing gives a
/// group no id, and every name it does give changes or repeats.
pub(super) fn session_key(kind: SourceKind, session: &str, rows: &[&RawRow]) -> Session {
    let roster = kind == SourceKind::Messages && session.contains(" & ");
    if roster || people_who_wrote(rows) >= 2 {
        let (members, unresolved_roster_labels) = group_members(roster, session, rows);
        return Session {
            key: ConversationKey::Group {
                vendor_id: group_vendor_id(rows),
                members,
            },
            contact_name: session.trim().to_string(),
            unresolved_roster_labels,
        };
    }
    let session = session.trim();
    let named_by_address = session.contains('@') || sanitize_phone_shaped(session).is_some();
    let key = match direct_handle(session, rows) {
        Some(handle) => ConversationKey::Direct(handle),
        None => ConversationKey::NameOnly(session.to_string()),
    };
    Session {
        contact_name: if named_by_address && !key.is_name_only() {
            String::new()
        } else {
            session.to_string()
        },
        key,
        unresolved_roster_labels: 0,
    }
}

/// The address a `Sender ID` holds, or `None` when it holds none.
///
/// Email first: a sender like `bob2024@gmail.com` has 4+ digits and must
/// never be reduced to a phone number. A number is formatted as E.164 (the
/// international phone-number format that starts with +) when unambiguous,
/// otherwise kept as digits; never an invented `+0…`.
fn sender_address(sender_id: &str) -> Option<String> {
    let sid = sender_id.trim();
    if sid.contains('@') {
        Some(sid.to_string())
    } else if sanitize_number(sid).is_some() {
        Some(phone::normalize_lenient(sid))
    } else {
        None
    }
}

/// True for a row someone other than the account holder wrote.
fn written_by_someone_else(row: &RawRow) -> bool {
    !is_outgoing(&row.msg_type) && !is_notification(&row.msg_type)
}

/// The node `node` is joined to in a union-find map.
fn root(parent: &HashMap<String, String>, node: &str) -> String {
    let mut current = node.to_string();
    while let Some(next) = parent.get(&current).filter(|next| **next != current) {
        current.clone_from(next);
    }
    current
}

/// How many people wrote the received rows. A row's address and its Sender
/// Name are one person, so two addresses that share a name are one person.
fn people_who_wrote(rows: &[&RawRow]) -> usize {
    // Union-find over the addresses and the names the rows give.
    let mut parent: HashMap<String, String> = HashMap::new();
    for row in rows.iter().filter(|row| written_by_someone_else(row)) {
        let address = sender_address(&row.sender_id).map(|a| format!("address:{a}"));
        let name = row.sender_name.trim();
        let name = (!name.is_empty()).then(|| format!("name:{}", name.to_lowercase()));
        let nodes: Vec<String> = address.into_iter().chain(name).collect();
        for node in &nodes {
            parent.entry(node.clone()).or_insert_with(|| node.clone());
        }
        if let [a, b] = nodes.as_slice() {
            let (ra, rb) = (root(&parent, a), root(&parent, b));
            if ra != rb {
                parent.insert(ra, rb);
            }
        }
    }
    parent
        .keys()
        .map(|node| root(&parent, node))
        .collect::<HashSet<_>>()
        .len()
}

/// The order rows are compared in to find the earliest: by the date as
/// written, a row with no readable date after every dated row, then by
/// [`row_fields`].
type RowOrder<'a> = (bool, Option<NaiveDateTime>, RowFields<'a>);

/// Where `row` sorts when looking for the earliest row.
fn row_order<'a>(row: &'a RawRow) -> RowOrder<'a> {
    let date = naive_date(&row.message_date);
    (date.is_none(), date, row_fields(row))
}

/// The address of a one-to-one chat: the one its session name gives, else
/// the address of the earliest received row that has one. `None` when the
/// source records no address for the person.
fn direct_handle(session: &str, rows: &[&RawRow]) -> Option<String> {
    if let Some(phone) = phones_in_text(session).into_iter().next() {
        return Some(phone);
    }
    // Email first: an address like `bob2024@gmail.com` has 4+ digits and must
    // not be treated as a phone number.
    if session.contains('@') {
        return Some(session.to_string());
    }
    if sanitize_phone_shaped(session).is_some() {
        return Some(phone::normalize_lenient(session));
    }
    rows.iter()
        .filter(|row| written_by_someone_else(row))
        .filter_map(|row| Some((row_order(row), sender_address(&row.sender_id)?)))
        .min_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, address)| address)
}

/// A group's members: everyone who wrote, every number in the session name,
/// and for a Messages roster ("A & B & C") every name it lists. A listed
/// name no row pairs with an address has no address anywhere in the export,
/// so it becomes an identity of type `other` whose value is the name.
///
/// Returns the members sorted by handle, and how many listed names had no
/// address.
fn group_members(roster: bool, session: &str, rows: &[&RawRow]) -> (Vec<IrParticipant>, u64) {
    let mut members: BTreeMap<String, IrParticipant> = BTreeMap::new();
    let mut add = |handle: String, name: &str, handle_type: HandleType| {
        let member = members.entry(handle.clone()).or_insert(IrParticipant {
            handle: Some(handle),
            display_name: None,
            handle_type: Some(handle_type),
        });
        let name = name.trim();
        if member.display_name.is_none() && !name.is_empty() {
            member.display_name = Some(name.to_string());
        }
    };

    // The rows themselves pair a sender's name with their address. That is
    // the only name-to-address mapping the source gives.
    let mut handle_by_sender_name: HashMap<String, String> = HashMap::new();
    for row in rows.iter().filter(|row| !is_outgoing(&row.msg_type)) {
        let Some(address) = sender_address(&row.sender_id) else {
            continue;
        };
        let name = row.sender_name.trim();
        if !name.is_empty() {
            handle_by_sender_name
                .entry(name.to_lowercase())
                .or_insert_with(|| address.clone());
        }
        let handle_type = handle_type_for(&address);
        add(address, name, handle_type);
    }
    for phone in phones_in_text(session) {
        add(phone, "", HandleType::Phone);
    }

    let mut unresolved = 0u64;
    if roster {
        for label in session.split(" & ").map(str::trim) {
            if label.is_empty() {
                continue;
            }
            if label.contains('@') {
                add(label.to_string(), "", HandleType::Email);
            } else if sanitize_phone_shaped(label).is_some() {
                add(phone::normalize_lenient(label), "", HandleType::Phone);
            } else if let Some(handle) = handle_by_sender_name.get(&label.to_lowercase()) {
                add(handle.clone(), label, handle_type_for(handle));
            } else {
                // A member who never wrote, shown by name: the export holds
                // no address for them.
                unresolved += 1;
                add(label.to_string(), label, HandleType::Other);
            }
        }
    }
    (members.into_values().collect(), unresolved)
}

/// The columns that identify a row: Message Date, Type, Sender ID, Text and
/// Attachment, as the CSV writes them.
type RowFields<'a> = (&'a str, &'a str, &'a str, &'a str, &'a str);

/// The [`RowFields`] of `row`.
fn row_fields(row: &RawRow) -> RowFields<'_> {
    (
        &row.message_date,
        &row.msg_type,
        &row.sender_id,
        &row.text,
        &row.attachment,
    )
}

/// A group's vendor id: SHA-256 of the conversation's earliest row
/// ([`row_fields`]), in lowercase hex.
///
/// Within one export one CSV file is one conversation, and across exports
/// its first message stays the same while new ones arrive, so the id stays
/// the same when someone new writes, a member is renamed, or the session
/// name changes. Where several rows share the earliest time, the smallest of
/// them is taken, so the choice does not depend on row order. The date is
/// read as written, with no time zone, so the id does not depend on the zone
/// the export is converted in.
///
/// The id changes when the oldest messages are gone from the phone or an
/// export covers only a date range: the group then comes in as a second
/// conversation, never merged with another group.
fn group_vendor_id(rows: &[&RawRow]) -> String {
    let (date, msg_type, sender_id, text, attachment) = rows
        .iter()
        .map(|row| row_order(row))
        .min()
        .map(|(_, _, fields)| fields)
        .unwrap_or_default();
    let mut hasher = Sha256::new();
    for (index, field) in [date, msg_type, sender_id, text, attachment]
        .into_iter()
        .enumerate()
    {
        if index > 0 {
            // The ASCII unit separator, which no CSV cell holds.
            hasher.update([0x1f]);
        }
        hasher.update(field.as_bytes());
    }
    hex::encode(hasher.finalize())
}

/// iMazing identifiers are E.164 phones or emails; infer the type from the
/// handle shape.
pub(super) fn handle_type_for(handle: &str) -> HandleType {
    if handle.contains('@') {
        HandleType::Email
    } else {
        HandleType::Phone
    }
}

/// An iMazing date string (`YYYY-MM-DD HH:MM[:SS]`, no zone) as written.
fn naive_date(raw: &str) -> Option<NaiveDateTime> {
    let raw = raw.trim();
    NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M"))
        .ok()
}

/// Parse an iMazing date string (`YYYY-MM-DD HH:MM[:SS]`, no zone) in `zone`
/// into Unix seconds. iMazing records whole seconds only. [`Zone::instant`]
/// settles a wall clock that a daylight-saving change repeats or skips, so
/// every parsable row is kept.
pub(super) fn parse_message_date(raw: &str, zone: Zone) -> Option<i64> {
    Some(zone.instant(naive_date(raw)?)?.timestamp())
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

/// The sender handle and display name for a row: empty for outgoing, and
/// otherwise the row's own Sender ID and Sender Name.
///
/// Only a one-to-one chat with a number or short code fills a received row
/// that names no sender: it can only be from the chat's one other person. A
/// group's row with no sender has no sender, and a chat keyed by a name has
/// no address to give.
pub(super) fn resolve_sender(
    row: &RawRow,
    is_from_me: bool,
    is_notification: bool,
    session: &Session,
) -> (String, String) {
    if is_from_me {
        return (String::new(), String::new());
    }
    let address = sender_address(&row.sender_id);
    if is_notification {
        // Keep any available identity from the notification row; often empty.
        return (
            address.unwrap_or_default(),
            row.sender_name.trim().to_string(),
        );
    }
    let handle = address.unwrap_or_else(|| match &session.key {
        ConversationKey::Direct(handle) if !handle.contains('@') => handle.clone(),
        _ => String::new(),
    });
    let mut display = row.sender_name.trim().to_string();
    if display.is_empty() && !session.key.is_group() {
        display.clone_from(&session.contact_name);
    }
    (handle, display)
}
