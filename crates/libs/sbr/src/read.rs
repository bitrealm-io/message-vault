//! Streaming reader and domain parsing for SMS Backup & Restore XML.

use anyhow::{Context, Result, bail};
use base64::Engine;
use phone::sanitize_number;
use quick_xml::{Reader, XmlVersion, events::Event};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::BufRead;
use std::path::Path;
use std::sync::{Arc, LazyLock};

use message_ir::valid_filename;

const INSERT_ADDRESS_TOKEN: &str = "insert-address-token";
const MMS_ADDR_FROM: &str = "137";
const MMS_BOX_SENT: &str = "2";
const MMS_BOX_DRAFT: &str = "3";
const MMS_BOX_OUTBOX: &str = "4";
const MMS_BOX_FAILED: &str = "5";
const MMS_BOX_QUEUED: &str = "6";

/// Individual or group conversation classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConversationKind {
    /// One-to-one conversation (default).
    #[default]
    Individual,
    /// Group conversation with multiple participants.
    Group,
}

/// Raw `<part>` element: content-type, name, location, and payload columns
/// plus the full attribute map.
#[derive(Debug, Clone, Default)]
pub struct MmsPart {
    /// MIME type from the `ct` attribute.
    pub ct: String,
    /// Content name from the `name` attribute.
    pub name: String,
    /// Content-Location from the `cl` attribute.
    pub cl: String,
    /// Filename from the XML `fn` attribute (not a function attribute).
    pub filename_attr: String,
    /// Text body (SMIL) when present.
    pub text: String,
    /// Base64 payload when present.
    pub data: String,
    /// All raw attributes.
    pub attrs: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default)]
struct MmsAddr {
    address: String,
    addr_type: String,
    attrs: BTreeMap<String, String>,
}

/// Decoded MMS attachment with a content-addressed filename.
#[derive(Debug, Clone)]
pub struct AttachmentBlob {
    /// Content-addressed filename (`<sha256><ext>`).
    pub filename: String,
    /// Original part name from the XML, when present.
    pub original_name: Option<String>,
    /// MIME type from the part's `ct`.
    pub mime_type: Option<String>,
    /// Decoded payload bytes shared by reference.
    pub data: Arc<[u8]>,
    /// Lowercase hex SHA-256 of the payload.
    pub digest_hex: String,
}

/// Serde-tagged raw source bag (`kind: sms|mms`) preserved for write-back.
///
/// `Deserialize` recovers the bag from an IR message's `source.fields` on the
/// write-back path (`ir-format`'s SBR writer); `parts`/`addrs` default to
/// empty so a bag written without them still parses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum SourceFields {
    /// Raw SMS source bag.
    #[serde(rename = "sms")]
    Sms {
        /// Raw SMS attributes.
        attrs: BTreeMap<String, String>,
    },
    /// Raw MMS source bag.
    #[serde(rename = "mms")]
    Mms {
        /// Raw MMS attributes.
        attrs: BTreeMap<String, String>,
        /// Raw `<part>` attribute maps.
        #[serde(default)]
        parts: Vec<BTreeMap<String, String>>,
        /// Raw `<addr>` attribute maps.
        #[serde(default)]
        addrs: Vec<BTreeMap<String, String>>,
    },
}

/// One parsed SMS/MMS message record.
#[derive(Debug, Clone)]
pub struct Record {
    /// Conversation key (single peer number or group key).
    pub chat_key: String,
    /// Individual or group classification.
    pub conversation_kind: ConversationKind,
    /// Generated group title, if group.
    pub group_title: Option<String>,
    /// (Sanitized digits, display-name hint) pairs for participants.
    pub participant_digits: Vec<(String, Option<String>)>,
    /// Message timestamp in seconds.
    pub timestamp_secs: f64,
    /// Whether the message is outgoing.
    pub is_from_me: bool,
    /// Sender digits for incoming messages.
    pub sender_digits: Option<String>,
    /// Sender display-name hint, when present.
    pub sender_display_name: Option<String>,
    /// Message body text (HTML-entity decoded).
    pub text: String,
    /// Message subject, if any.
    pub subject: String,
    /// Decoded attachment blobs.
    pub attachments: Vec<AttachmentBlob>,
    /// `"sms"` or `"mms"`.
    pub message_kind: &'static str,
    /// Raw `date` attribute in milliseconds.
    pub date_ms: String,
    /// Raw `contact_name` attribute (may be `"null"`).
    pub contact_name: String,
    /// Raw `type` (SMS) or `msg_box` (MMS) attribute string.
    pub android_type: String,
    /// Serde-tagged raw source bag for write-back.
    pub source_fields: SourceFields,
}

/// Counters for seen and skipped messages.
#[derive(Debug, Default, Clone, Copy)]
pub struct ParseStats {
    /// Number of `<sms>` elements encountered.
    pub sms_seen: u64,
    /// Number of `<mms>` elements encountered.
    pub mms_seen: u64,
    /// Records dropped for an unparseable `date`.
    pub skipped_invalid_date: u64,
    /// Records dropped because no usable phone address.
    pub skipped_unknown_address: u64,
    /// SMS records dropped for an unknown `type`.
    pub skipped_unknown_type: u64,
    /// Records dropped as draft/outbox/failed/queued.
    pub skipped_draft_or_outbox: u64,
    /// MMS records dropped with no participants.
    pub skipped_empty_participants: u64,
    /// Parts with undecodable base64 `data`.
    pub skipped_bad_attachment: u64,
}

/// The element's attributes as a map with lower-case keys.
fn attrs(e: &quick_xml::events::BytesStart<'_>) -> HashMap<String, String> {
    e.attributes()
        .flatten()
        .map(|a| {
            let key = a.key.as_ref().to_ascii_lowercase();
            let value = a
                .normalized_value(XmlVersion::Implicit1_0)
                .map(|v| v.into_owned())
                .unwrap_or_default();
            (key, value)
        })
        .collect()
}

/// The attribute value, or an empty string.
fn get<'a>(attrs: &'a HashMap<String, String>, key: &str) -> &'a str {
    attrs.get(key).map(String::as_str).unwrap_or("")
}

/// The attributes as an ordered map, for the vendor bag.
fn btree(attrs: &HashMap<String, String>) -> BTreeMap<String, String> {
    attrs.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// An MMS part from a `<part>` element's attributes.
fn part(attrs: &HashMap<String, String>) -> MmsPart {
    MmsPart {
        ct: get(attrs, "ct").into(),
        name: get(attrs, "name").into(),
        cl: get(attrs, "cl").into(),
        filename_attr: get(attrs, "fn").into(),
        text: get(attrs, "text").into(),
        data: get(attrs, "data").into(),
        attrs: btree(attrs),
    }
}

/// An MMS address from an `<addr>` element's attributes.
fn addr(attrs: &HashMap<String, String>) -> MmsAddr {
    MmsAddr {
        address: get(attrs, "address").into(),
        addr_type: get(attrs, "type").into(),
        attrs: btree(attrs),
    }
}

/// A body with HTML entities decoded and line endings normalized.
fn decode_body(raw: &str) -> String {
    html_escape::decode_html_entities(raw)
        .replace("\r\n", "\n")
        .replace('\r', "\n")
}

/// The contact name from `contact_name` or `name`, when it is a real name.
fn name_alias(attrs: &HashMap<String, String>) -> Option<String> {
    let value = if get(attrs, "contact_name").is_empty() {
        get(attrs, "name")
    } else {
        get(attrs, "contact_name")
    };
    let value = value.trim();
    (!value.is_empty() && !value.eq_ignore_ascii_case("null")).then(|| value.to_string())
}

/// The contact name as written: `contact_name`, else `name`.
fn raw_name(attrs: &HashMap<String, String>) -> String {
    let value = get(attrs, "contact_name");
    if value.is_empty() {
        get(attrs, "name").into()
    } else {
        value.into()
    }
}

/// The value trimmed, with `null` treated as empty.
fn non_null(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("null") {
        String::new()
    } else {
        value.into()
    }
}

/// Every name a part goes by (name, location, file name), for SMIL matching.
fn content_keys(part: &MmsPart) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    for raw in [&part.name, &part.cl, &part.filename_attr] {
        let value = raw.trim();
        if value.is_empty()
            || value.eq_ignore_ascii_case("null")
            || value.eq_ignore_ascii_case("none")
        {
            continue;
        }
        keys.insert(value.into());
        if let Some(base) = value.rsplit('/').next().filter(|s| !s.is_empty()) {
            keys.insert(base.into());
        }
    }
    keys
}

static TEXT_SRC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)<text[^>]+src=["']([^"']+)["']"#).expect("valid regex"));
static IMG_SRC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)<img[^>]+src=["']([^"']+)["']"#).expect("valid regex"));

/// The text and media part names a SMIL part refers to, in order.
fn smil_refs(parts: &[MmsPart], decoded: &[DecodedPartData]) -> (Vec<String>, Vec<String>) {
    let smil = parts
        .iter()
        .zip(decoded.iter())
        .find(|(p, _)| p.ct.eq_ignore_ascii_case("application/smil"))
        .map(|(p, payload)| {
            if !p.text.trim().is_empty() {
                html_escape::decode_html_entities(p.text.trim()).into_owned()
            } else {
                match payload {
                    DecodedPartData::Ok { bytes, .. } => {
                        String::from_utf8_lossy(bytes).into_owned()
                    }
                    _ => String::new(),
                }
            }
        })
        .unwrap_or_default();
    let captures = |re: &Regex| {
        re.captures_iter(&smil)
            .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
            .collect()
    };
    (captures(&TEXT_SRC), captures(&IMG_SRC))
}

/// File extension for a part's content type.
fn extension(part: &MmsPart) -> String {
    match part.ct.to_ascii_lowercase().as_str() {
        "image/jpeg" | "image/jpg" => ".jpg".into(),
        "image/png" => ".png".into(),
        "image/gif" => ".gif".into(),
        "image/webp" => ".webp".into(),
        "video/mp4" => ".mp4".into(),
        "video/3gpp" | "video/3gp" => ".3gp".into(),
        "audio/amr" => ".amr".into(),
        "audio/mpeg" => ".mp3".into(),
        "audio/mp4" => ".m4a".into(),
        ct => [&part.name, &part.cl, &part.filename_attr]
            .iter()
            .find_map(|n| {
                valid_filename(n).and_then(|n| {
                    Path::new(&n)
                        .extension()?
                        .to_str()
                        .map(|e| format!(".{}", e.to_ascii_lowercase()))
                })
            })
            .unwrap_or_else(|| {
                if ct.starts_with("image/") {
                    ".jpg".into()
                } else if ct.starts_with("video/") {
                    ".mp4".into()
                } else if ct.starts_with("audio/") {
                    ".amr".into()
                } else {
                    ".bin".into()
                }
            }),
    }
}

/// One base64 decode + SHA-256 of a part's `data` attribute.
///
/// Shared by attachment staging and source-field write-back so each part is
/// decoded once.
enum DecodedPartData {
    /// Empty or `"null"` — no payload.
    Absent,
    /// Successfully decoded payload.
    Ok {
        bytes: Arc<[u8]>,
        digest_hex: String,
    },
    /// Non-empty data that is not valid base64.
    Err { raw_len: usize, raw_sha256: String },
}

/// A part's `data` attribute decoded from base64, or why it could not be.
fn decode_part_data(raw: &str) -> DecodedPartData {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("null") {
        return DecodedPartData::Absent;
    }
    match base64::engine::general_purpose::STANDARD.decode(trimmed) {
        Ok(bytes) => {
            let digest_hex = hex::encode(Sha256::digest(&bytes));
            DecodedPartData::Ok {
                bytes: Arc::from(bytes),
                digest_hex,
            }
        }
        Err(_) => DecodedPartData::Err {
            raw_len: raw.len(),
            raw_sha256: hex::encode(Sha256::digest(raw.as_bytes())),
        },
    }
}

/// Attachment blobs from the MMS parts, in SMIL order when SMIL names them, counting the ones that failed to decode.
fn attachments(
    parts: &[MmsPart],
    decoded: &[DecodedPartData],
    refs: &[String],
    stats: &mut ParseStats,
) -> Vec<AttachmentBlob> {
    let mut by_key = HashMap::new();
    let mut order = Vec::new();
    for (part, payload) in parts.iter().zip(decoded.iter()) {
        let ct = part.ct.to_ascii_lowercase();
        if ct.starts_with("text/") || ct == "application/smil" {
            continue;
        }
        let (bytes, digest) = match payload {
            DecodedPartData::Ok { bytes, digest_hex } if !bytes.is_empty() => {
                (Arc::clone(bytes), digest_hex.clone())
            }
            DecodedPartData::Err { .. } => {
                stats.skipped_bad_attachment += 1;
                continue;
            }
            DecodedPartData::Absent | DecodedPartData::Ok { .. } => continue,
        };
        let original = valid_filename(&part.name)
            .or_else(|| valid_filename(&part.cl))
            .or_else(|| valid_filename(&part.filename_attr));
        let ext = extension(part);
        let filename = format!("{digest}{ext}");
        let blob = AttachmentBlob {
            filename: filename.clone(),
            original_name: original,
            mime_type: Some(if part.ct.trim().is_empty() {
                "application/octet-stream".into()
            } else {
                part.ct.clone()
            }),
            data: bytes,
            digest_hex: digest,
        };
        let keys = content_keys(part);
        if keys.is_empty() {
            order.push(filename.clone());
            by_key.insert(filename, blob);
        } else {
            for (index, key) in keys.into_iter().enumerate() {
                if index == 0 {
                    order.push(key.clone());
                }
                by_key.entry(key).or_insert_with(|| blob.clone());
            }
        }
    }
    let mut seen = HashSet::new();
    refs.iter()
        .chain(order.iter())
        .filter_map(|k| by_key.get(k))
        .chain(by_key.values())
        .filter(|b| seen.insert(b.filename.clone()))
        .cloned()
        .collect()
}

/// A part's attributes for the vendor bag, with the base64 data replaced by a marker.
fn part_fields(part: &MmsPart, decoded: &DecodedPartData) -> BTreeMap<String, String> {
    let mut attrs = part.attrs.clone();
    if attrs
        .remove("data")
        .is_some_and(|d| !d.trim().is_empty() && !d.eq_ignore_ascii_case("null"))
    {
        match decoded {
            DecodedPartData::Ok { bytes, digest_hex } => {
                attrs.insert("data_len".into(), bytes.len().to_string());
                attrs.insert("data_sha256".into(), digest_hex.clone());
            }
            DecodedPartData::Err {
                raw_len,
                raw_sha256,
            } => {
                attrs.insert("data_len".into(), raw_len.to_string());
                attrs.insert("data_sha256".into(), raw_sha256.clone());
                attrs.insert("data_decode_error".into(), "true".into());
            }
            DecodedPartData::Absent => {}
        }
    }
    attrs
}

/// One `<sms>` element as a record, counting rows skipped for a bad date or address.
fn parse_sms(attrs: &HashMap<String, String>, stats: &mut ParseStats) -> Option<Record> {
    stats.sms_seen += 1;
    let (date_ms, timestamp_secs) = timestamp_from_date(attrs, stats)?;
    let address = sanitize_number(get(attrs, "address")).or_else(|| {
        stats.skipped_unknown_address += 1;
        None
    })?;
    let android_type = get(attrs, "type").trim().to_string();
    let (is_from_me, sender_digits) = match android_type.as_str() {
        "1" => (false, Some(address.clone())),
        "2" => (true, None),
        // Draft (3) and outbox (4) SMS carry no delivered content; count them
        // with the descriptive counter used for MMS drafts/outbox/failed/queued
        // instead of the catch-all unknown-type counter.
        "3" | "4" => {
            stats.skipped_draft_or_outbox += 1;
            return None;
        }
        _ => {
            stats.skipped_unknown_type += 1;
            return None;
        }
    };
    let hint = name_alias(attrs);
    Some(Record {
        chat_key: address.clone(),
        conversation_kind: ConversationKind::Individual,
        group_title: None,
        participant_digits: vec![(address, hint.clone())],
        timestamp_secs,
        is_from_me,
        sender_digits,
        sender_display_name: if is_from_me { None } else { hint },
        text: decode_body(get(attrs, "body")),
        subject: non_null(get(attrs, "subject")),
        attachments: Vec::new(),
        message_kind: "sms",
        date_ms,
        contact_name: raw_name(attrs),
        android_type,
        source_fields: SourceFields::Sms {
            attrs: btree(attrs),
        },
    })
}

/// Unix seconds from an element's millisecond `date` attribute, with the raw
/// value. Counts and drops an unreadable date.
fn timestamp_from_date(
    attrs: &HashMap<String, String>,
    stats: &mut ParseStats,
) -> Option<(String, f64)> {
    let date_ms = get(attrs, "date").to_string();
    let Ok(millis) = date_ms.parse::<f64>() else {
        stats.skipped_invalid_date += 1;
        return None;
    };
    Some((date_ms, millis / 1000.0))
}

/// One `<mms>` element as a [`Record`], or `None` (counted in `stats`) when it
/// is a draft, has no participants, or names nobody but the owner.
fn parse_mms(
    attrs: &HashMap<String, String>,
    parts: &[MmsPart],
    addrs: &[MmsAddr],
    owners: &HashSet<String>,
    stats: &mut ParseStats,
) -> Option<Record> {
    stats.mms_seen += 1;
    let (date_ms, timestamp_secs) = timestamp_from_date(attrs, stats)?;
    let msg_box = get(attrs, "msg_box").trim().to_string();
    if matches!(
        msg_box.as_str(),
        MMS_BOX_DRAFT | MMS_BOX_OUTBOX | MMS_BOX_FAILED | MMS_BOX_QUEUED
    ) {
        stats.skipped_draft_or_outbox += 1;
        return None;
    }
    let participants = mms_participants(attrs, addrs);
    if participants.is_empty() {
        stats.skipped_empty_participants += 1;
        return None;
    }
    let is_from_me = msg_box == MMS_BOX_SENT;
    let sender_digits = if is_from_me {
        None
    } else {
        mms_sender(addrs, &participants, owners)
    };
    let peers = mms_peers(&participants, owners);
    if peers.is_empty() {
        stats.skipped_unknown_address += 1;
        return None;
    }
    let decoded: Vec<DecodedPartData> = parts.iter().map(|p| decode_part_data(&p.data)).collect();
    let (text_refs, image_refs) = smil_refs(parts, &decoded);
    let hint = name_alias(attrs);
    let conversation = MmsConversation::for_peers(peers, hint.clone());
    Some(Record {
        chat_key: conversation.chat_key,
        conversation_kind: conversation.kind,
        group_title: conversation.group_title,
        participant_digits: conversation.participant_digits,
        timestamp_secs,
        is_from_me,
        sender_digits,
        sender_display_name: if is_from_me { None } else { hint },
        text: mms_text(parts, &text_refs),
        subject: non_null(get(attrs, "sub")),
        attachments: attachments(parts, &decoded, &image_refs, stats),
        message_kind: "mms",
        date_ms,
        contact_name: raw_name(attrs),
        android_type: msg_box,
        source_fields: SourceFields::Mms {
            attrs: btree(attrs),
            parts: parts
                .iter()
                .zip(decoded.iter())
                .map(|(p, d)| part_fields(p, d))
                .collect(),
            addrs: addrs.iter().map(|a| a.attrs.clone()).collect(),
        },
    })
}

/// Every address on the element: the `~`-joined `address` attribute, then
/// each `<addr>` child. Blank entries are dropped; owners are not.
fn mms_participants(attrs: &HashMap<String, String>, addrs: &[MmsAddr]) -> Vec<String> {
    get(attrs, "address")
        .split('~')
        .chain(addrs.iter().map(|a| a.address.as_str()))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// The sender of an incoming MMS: the `FROM` address when it is a number
/// other than the owner's, else the first participant number that is not
/// the owner's.
fn mms_sender(
    addrs: &[MmsAddr],
    participants: &[String],
    owners: &HashSet<String>,
) -> Option<String> {
    addrs
        .iter()
        .find(|a| a.addr_type == MMS_ADDR_FROM)
        .and_then(|a| sanitize_number(&a.address))
        .filter(|d| !owners.contains(d))
        .or_else(|| {
            participants
                .iter()
                .filter_map(|p| sanitize_number(p))
                .find(|d| !owners.contains(d))
        })
}

/// The other parties: every participant number that is not the owner's,
/// sorted and de-duplicated so the same group always gets the same key.
fn mms_peers(participants: &[String], owners: &HashSet<String>) -> Vec<String> {
    let mut peers: Vec<String> = participants
        .iter()
        .filter_map(|p| sanitize_number(p))
        .filter(|p| !owners.contains(p))
        .collect();
    peers.sort();
    peers.dedup();
    peers
}

/// The message text: the text parts the SMIL references, in its order, or
/// when there is no SMIL every text part sorted and de-duplicated.
fn mms_text(parts: &[MmsPart], text_refs: &[String]) -> String {
    let mut text_by_key = HashMap::new();
    for part in parts
        .iter()
        .filter(|p| p.ct.to_ascii_lowercase().starts_with("text/"))
    {
        let text = decode_body(&part.text);
        if !text.is_empty() && !text.eq_ignore_ascii_case("null") {
            for key in content_keys(part) {
                text_by_key.entry(key).or_insert_with(|| text.clone());
            }
        }
    }
    if text_refs.is_empty() {
        let mut values: Vec<String> = text_by_key.into_values().collect();
        values.sort();
        values.dedup();
        return values.join("\n");
    }
    text_refs
        .iter()
        .filter_map(|r| text_by_key.get(r))
        .cloned()
        .collect::<Vec<_>>()
        .join("\n")
}

/// Where an MMS lands: a one-to-one conversation keyed by the peer's number,
/// or a group keyed by the sorted peer set.
struct MmsConversation {
    chat_key: String,
    kind: ConversationKind,
    group_title: Option<String>,
    participant_digits: Vec<(String, Option<String>)>,
}

impl MmsConversation {
    /// `peers` is sorted and non-empty; `hint` is the element's contact name,
    /// which names the one peer of an individual conversation.
    fn for_peers(mut peers: Vec<String>, hint: Option<String>) -> Self {
        if peers.len() == 1 {
            let peer = peers.remove(0);
            return Self {
                chat_key: peer.clone(),
                kind: ConversationKind::Individual,
                group_title: None,
                participant_digits: vec![(peer, hint)],
            };
        }
        Self {
            chat_key: group_chat_key(&peers),
            kind: ConversationKind::Group,
            group_title: Some(group_title(&peers)),
            participant_digits: peers.into_iter().map(|d| (d, None)).collect(),
        }
    }
}

/// `Group: <up to four numbers>`, with a count for the rest.
fn group_title(peers: &[String]) -> String {
    let shown: Vec<String> = peers
        .iter()
        .take(4)
        .map(|d| phone::normalize_lenient(d))
        .collect();
    if peers.len() <= 4 {
        format!("Group: {}", shown.join(", "))
    } else {
        format!(
            "Group: {}, and {} others",
            shown.join(", "),
            peers.len() - 4
        )
    }
}

/// Group chats are keyed by the sorted participant set because the format
/// has no stable thread ID. When the roster changes (someone is added or
/// removed), messages before and after the change land in different
/// conversations, an inherent limitation of the source, documented at
/// https://bitrealm.io/vault/developer/formats/sms-backup-restore/mapping/.
/// A very long roster is keyed by a hash so the key stays a usable file stem.
fn group_chat_key(peers: &[String]) -> String {
    let raw_key = format!("group-{}", peers.join("_"));
    if raw_key.len() > 180 {
        format!(
            "group-{}",
            &hex::encode(Sha256::digest(raw_key.as_bytes()))[..16]
        )
    } else {
        raw_key
    }
}

/// Parse one XML file, calling `on_record` for each message as soon as it is
/// complete.
///
/// The callback owns the record (including decoded attachment bytes). Staging
/// those bytes and dropping the record frees the payload before the next
/// message is parsed.
///
/// `stats` is updated as messages are seen, including when this function later
/// returns an error. Callers that keep records from the callback can merge
/// those counters even if the XML is truncated.
///
/// # Errors
///
/// Returns an error when the file cannot be opened, the XML cannot be parsed,
/// or `on_record` returns an error.
pub fn parse_file_with<F>(
    path: &Path,
    owners: &HashSet<String>,
    stats: &mut ParseStats,
    on_record: F,
) -> Result<()>
where
    F: FnMut(Record) -> Result<()>,
{
    let file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    parse_reader_with(std::io::BufReader::new(file), owners, stats, on_record)
}

/// Stream the XML, calling `on_record` for each SMS or MMS as it completes.
fn parse_reader_with<R, F>(
    reader: R,
    owners: &HashSet<String>,
    stats: &mut ParseStats,
    mut on_record: F,
) -> Result<()>
where
    R: BufRead,
    F: FnMut(Record) -> Result<()>,
{
    let mut xml = Reader::from_reader(reader);
    xml.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let (mut sms, mut mms, mut parts, mut addrs) =
        (HashMap::new(), HashMap::new(), Vec::new(), Vec::new());
    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match e.name().as_ref().to_ascii_lowercase().as_str() {
                "sms" => sms = attrs(&e),
                "mms" => {
                    mms = attrs(&e);
                    parts.clear();
                    addrs.clear();
                }
                "part" => parts.push(part(&attrs(&e))),
                "addr" => addrs.push(addr(&attrs(&e))),
                _ => {}
            },
            Ok(Event::Empty(e)) => match e.name().as_ref().to_ascii_lowercase().as_str() {
                "sms" => {
                    if let Some(r) = parse_sms(&attrs(&e), stats) {
                        on_record(r)?;
                    }
                }
                "part" => parts.push(part(&attrs(&e))),
                "addr" => addrs.push(addr(&attrs(&e))),
                "mms" => {
                    if let Some(r) = parse_mms(&attrs(&e), &[], &[], owners, stats) {
                        on_record(r)?;
                    }
                }
                _ => {}
            },
            Ok(Event::End(e)) => match e.name().as_ref().to_ascii_lowercase().as_str() {
                "sms" => {
                    if let Some(r) = parse_sms(&sms, stats) {
                        on_record(r)?;
                    }
                }
                "mms" => {
                    let record = parse_mms(&mms, &parts, &addrs, owners, stats);
                    // Drop the base64 `data` strings before the callback stages
                    // decoded bytes, so peak RAM is one payload, not payload plus
                    // the still-resident encoding.
                    parts.clear();
                    addrs.clear();
                    if let Some(r) = record {
                        on_record(r)?;
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(error) => return Err(error).context("XML parse error"),
            _ => {}
        }
        buf.clear();
    }
    Ok(())
}

#[cfg(test)]
fn parse_reader<R: BufRead>(
    reader: R,
    owners: &HashSet<String>,
) -> Result<(Vec<Record>, ParseStats)> {
    let mut records = Vec::new();
    let mut stats = ParseStats::default();
    parse_reader_with(reader, owners, &mut stats, |record| {
        records.push(record);
        Ok(())
    })?;
    Ok((records, stats))
}

/// Infer owner phones from nested `<addr type="137">` elements in sent MMS.
///
/// # Errors
///
/// Returns an error when the file cannot be opened or parsed.
pub fn infer_owner_phones(path: &Path) -> Result<Vec<String>> {
    let file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut xml = Reader::from_reader(std::io::BufReader::new(file));
    let (mut buf, mut in_sent, mut counts) = (Vec::new(), false, HashMap::<String, u64>::new());
    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Start(e) | Event::Empty(e)) => {
                match e.name().as_ref().to_ascii_lowercase().as_str() {
                    "mms" => in_sent = get(&attrs(&e), "msg_box").trim() == MMS_BOX_SENT,
                    "addr" if in_sent => {
                        let a = attrs(&e);
                        if get(&a, "type").trim() == MMS_ADDR_FROM {
                            let raw = get(&a, "address");
                            if !raw.eq_ignore_ascii_case(INSERT_ADDRESS_TOKEN)
                                // Guarded (US-digit form, matching
                                // OwnerPhoneSet): never a fabricated `+0…`.
                                && let Some(normalized) = phone::normalize_digits_us(raw)
                            {
                                *counts.entry(normalized).or_default() += 1;
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) if e.name().as_ref().eq_ignore_ascii_case("mms") => in_sent = false,
            Ok(Event::Eof) => break,
            Err(error) => bail!("parse {}: {error}", path.display()),
            _ => {}
        }
        buf.clear();
    }
    let mut ranked: Vec<_> = counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Ok(ranked.into_iter().map(|(phone, _)| phone).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_owner_from_nested_addr() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("smses.xml");
        std::fs::write(&path, r#"<smses><mms msg_box="2"><parts/><addrs><addr address="+15555550100" type="137"/></addrs></mms></smses>"#).unwrap();
        assert_eq!(infer_owner_phones(&path).unwrap(), vec!["+15555550100"]);
    }

    #[test]
    fn parses_attachment_and_preserves_fields() {
        let xml = br#"<smses><mms date="1400773400000" msg_box="1" address="+15555550101" extra="x"><parts><part seq="0" ct="image/jpeg" name="pic.jpg" data="aGVsbG8="/></parts><addrs><addr address="+15555550101" type="137" charset="106"/></addrs></mms></smses>"#;
        let (records, stats) = parse_reader(xml.as_slice(), &HashSet::new()).unwrap();
        assert_eq!(stats.mms_seen, 1);
        assert_eq!(records[0].attachments[0].data.as_ref(), b"hello");
        let SourceFields::Mms {
            attrs,
            parts,
            addrs,
        } = &records[0].source_fields
        else {
            panic!("mms")
        };
        assert_eq!(attrs.get("extra").map(String::as_str), Some("x"));
        assert!(parts[0].contains_key("data_sha256"));
        assert_eq!(addrs[0].get("charset").map(String::as_str), Some("106"));
    }

    #[test]
    fn attachment_filename_is_content_addressed() {
        let xml = br#"<smses><mms date="1" msg_box="1" address="+15555550101"><parts><part ct="image/jpeg" name="first.jpg" data="aGVsbG8="/><part ct="image/jpeg" name="second.jpg" data="aGVsbG8="/></parts><addrs><addr address="+15555550101" type="137"/></addrs></mms></smses>"#;
        let (records, _) = parse_reader(xml.as_slice(), &HashSet::new()).unwrap();
        assert_eq!(records[0].attachments.len(), 1);
        let attachment = &records[0].attachments[0];
        assert!(attachment.filename.starts_with(&attachment.digest_hex));
        assert_eq!(attachment.digest_hex.len(), 64);
    }

    #[test]
    fn parse_file_with_calls_back_per_message() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("smses.xml");
        std::fs::write(
            &path,
            r#"<smses>
            <sms protocol="0" address="+15555550101" date="1400773261000" type="1" body="hi"/>
            <mms date="1400773400000" msg_box="1" address="+15555550101">
                <parts><part seq="0" ct="text/plain" text="mms"/></parts>
                <addrs><addr address="+15555550101" type="137"/></addrs>
            </mms>
        </smses>"#,
        )
        .unwrap();
        let mut n = 0u32;
        let mut stats = ParseStats::default();
        parse_file_with(&path, &HashSet::new(), &mut stats, |_| {
            n += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(n, 2);
        assert_eq!(stats.sms_seen, 1);
        assert_eq!(stats.mms_seen, 1);
    }

    #[test]
    fn skipped_bad_attachment_records_decode_error() {
        let xml = br#"<smses><mms date="1" msg_box="1" address="+15555550101"><parts><part ct="image/jpeg" name="pic.jpg" data="@@@not-base64@@@"/></parts><addrs><addr address="+15555550101" type="137"/></addrs></mms></smses>"#;
        let (records, stats) = parse_reader(xml.as_slice(), &HashSet::new()).unwrap();
        assert_eq!(stats.skipped_bad_attachment, 1);
        assert!(records[0].attachments.is_empty());
        let SourceFields::Mms { parts, .. } = &records[0].source_fields else {
            panic!("mms")
        };
        assert_eq!(
            parts[0].get("data_decode_error").map(String::as_str),
            Some("true")
        );
    }

    #[test]
    fn smil_src_orders_attachment_from_decoded_payload() {
        let smil = "PHNtaWw+PGJvZHk+PGltZyBzcmM9InBpYy5qcGciLz48L2JvZHk+PC9zbWlsPg==";
        let xml = format!(
            r#"<smses><mms date="1" msg_box="1" address="+15555550101"><parts><part ct="application/smil" data="{smil}"/><part ct="image/jpeg" name="pic.jpg" data="aGVsbG8="/></parts><addrs><addr address="+15555550101" type="137"/></addrs></mms></smses>"#
        );
        let (records, stats) = parse_reader(xml.as_bytes(), &HashSet::new()).unwrap();
        assert_eq!(stats.mms_seen, 1);
        assert_eq!(records[0].attachments.len(), 1);
        assert_eq!(records[0].attachments[0].data.as_ref(), b"hello");
    }
}
