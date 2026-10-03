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
    /// For a group, the digest of each row ([`row_digest`]), earliest first.
    /// The first one is the group's vendor id; the rest tell apart two groups
    /// whose earliest rows are the same ([`group_vendor_id`]).
    pub(super) row_digests: Vec<[u8; 32]>,
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
    if roster || people_who_wrote(kind, rows) >= 2 {
        let (members, unresolved_roster_labels) = group_members(roster, session, rows);
        let row_digests = row_digests(rows);
        return Session {
            key: ConversationKey::Group {
                vendor_id: group_vendor_id(&row_digests, 1),
                members,
            },
            contact_name: session.trim().to_string(),
            unresolved_roster_labels,
            row_digests,
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
        row_digests: Vec::new(),
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

/// How many people wrote the received rows.
///
/// In Messages a row's address and its Sender Name are one person, so two
/// addresses that share a name are one person: iMazing puts one contact's
/// number and email address, or two numbers, in one chat. Two different
/// people saved under one name are then counted as one. A WhatsApp account
/// has one number, so in WhatsApp two addresses are always two people, and
/// a name joins only a row that has no address.
fn people_who_wrote(kind: SourceKind, rows: &[&RawRow]) -> usize {
    // Union-find over the addresses and the names the rows give.
    let mut parent: HashMap<String, String> = HashMap::new();
    let mut whatsapp_named: HashSet<String> = HashSet::new();
    let rows_written: Vec<&RawRow> = rows
        .iter()
        .copied()
        .filter(|row| written_by_someone_else(row))
        .collect();
    if kind == SourceKind::WhatsApp {
        for row in &rows_written {
            let name = row.sender_name.trim().to_lowercase();
            if sender_address(&row.sender_id).is_some() && !name.is_empty() {
                whatsapp_named.insert(name);
            }
        }
    }
    for row in rows_written {
        let address = sender_address(&row.sender_id).map(|a| format!("address:{a}"));
        let name = row.sender_name.trim().to_lowercase();
        let name = if name.is_empty()
            || (kind == SourceKind::WhatsApp
                && (address.is_some() || whatsapp_named.contains(&name)))
        {
            // A WhatsApp name never joins two addresses, and a row with no
            // address whose name a row with an address gives is that person.
            None
        } else {
            Some(format!("name:{name}"))
        };
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

/// Where a row sorts when looking for the earliest row: by the date as
/// written, a row with no readable date after every dated row, then by its
/// [`RowFields`]. The field order is the comparison order.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct RowOrder<'a> {
    undated: bool,
    date: Option<NaiveDateTime>,
    fields: RowFields<'a>,
}

impl<'a> RowOrder<'a> {
    fn of(row: &'a RawRow) -> Self {
        let date = naive_date(&row.message_date);
        Self {
            undated: date.is_none(),
            date,
            fields: RowFields::of(row),
        }
    }
}

/// The address of a one-to-one chat: the one its session name gives, else
/// the smallest address the received rows give. That is the same address in
/// every export that holds both of a person's addresses, whichever one wrote
/// first. `None` when the source records no address for the person.
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
        .filter_map(|row| sender_address(&row.sender_id))
        .min()
}

/// A group's members: everyone who wrote, every number in the session name,
/// and for a Messages roster ("A & B & C") every name it lists. A listed
/// name no row pairs with an address has no address anywhere in the export,
/// so it is a member with that name and no handle; the server gives such a
/// person an identity of type `other` holding the name.
///
/// Returns the members with an address sorted by handle, then the members
/// with a name only in roster order, and how many listed names had no
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
    let mut named_only: Vec<IrParticipant> = Vec::new();
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
                let already = named_only.iter().any(|member| {
                    member
                        .display_name
                        .as_deref()
                        .is_some_and(|name| name.eq_ignore_ascii_case(label))
                });
                if !already {
                    unresolved += 1;
                    named_only.push(IrParticipant {
                        handle: None,
                        display_name: Some(label.to_string()),
                        handle_type: None,
                    });
                }
            }
        }
    }
    let mut out: Vec<IrParticipant> = members.into_values().collect();
    out.extend(named_only);
    (out, unresolved)
}

/// The columns that identify a row, as the CSV writes them. The field order
/// is the comparison order.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct RowFields<'a> {
    message_date: &'a str,
    msg_type: &'a str,
    sender_id: &'a str,
    text: &'a str,
    attachment: &'a str,
}

impl<'a> RowFields<'a> {
    fn of(row: &'a RawRow) -> Self {
        Self {
            message_date: &row.message_date,
            msg_type: &row.msg_type,
            sender_id: &row.sender_id,
            text: &row.text,
            attachment: &row.attachment,
        }
    }
}

/// SHA-256 of a row's [`RowFields`], joined by the ASCII unit separator,
/// which no CSV cell holds.
fn row_digest(fields: &RowFields<'_>) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for (index, field) in [
        fields.message_date,
        fields.msg_type,
        fields.sender_id,
        fields.text,
        fields.attachment,
    ]
    .into_iter()
    .enumerate()
    {
        if index > 0 {
            hasher.update([0x1f]);
        }
        hasher.update(field.as_bytes());
    }
    hasher.finalize().into()
}

/// The [`row_digest`] of every row, earliest first ([`RowOrder`]). Where
/// several rows share the earliest time, the smallest of them comes first,
/// so the order does not depend on the order the CSV writes them in.
fn row_digests(rows: &[&RawRow]) -> Vec<[u8; 32]> {
    let mut ordered: Vec<RowOrder<'_>> = rows.iter().map(|row| RowOrder::of(row)).collect();
    ordered.sort();
    ordered
        .iter()
        .map(|order| row_digest(&order.fields))
        .collect()
}

/// A group's vendor id, in lowercase hex. With `depth` 1 it is the
/// [`row_digest`] of the conversation's earliest row. A greater `depth`
/// hashes the digests of the earliest `depth` rows (all of them when there
/// are fewer), which tells apart two groups whose earliest rows are the same.
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
pub(super) fn group_vendor_id(row_digests: &[[u8; 32]], depth: usize) -> String {
    if depth <= 1 {
        return hex::encode(row_digests.first().copied().unwrap_or_else(|| {
            row_digest(&RowFields {
                message_date: "",
                msg_type: "",
                sender_id: "",
                text: "",
                attachment: "",
            })
        }));
    }
    let mut hasher = Sha256::new();
    for digest in row_digests.iter().take(depth) {
        hasher.update(digest);
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
