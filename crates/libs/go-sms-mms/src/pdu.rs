//! Parse GO SMS Pro MMS PDU backup files (`I_<timestamp>_*.pdu`).
//!
//! Prefers WAP-209 / Content-Location / SMIL structured fields ([`crate::mms_enc`]),
//! then falls back to text-marker / magic-byte heuristics only when a field is empty.

use crate::emoji::decode_gosms_emojis;
use crate::mms_enc::{
    MmsPart, NamedPart, StructuredMms, content_type_from_filename, decode_bytes_with_charset,
    decode_mms_best_effort, extension_for_content_type, normalize_content_id,
};
use std::collections::{BTreeMap, HashSet};

use anyhow::{Context, Result};
use phone::sanitize_number;
use quick_xml::Reader;
use quick_xml::events::Event;
use regex::Regex;
use regex::bytes::Regex as BytesRegex;
use std::path::Path;
use std::sync::LazyLock;

/// `I_<ts>_...` PDU file name.
static PDU_FILENAME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^I_(?P<ts>\d+)_").expect("pdu name"));
/// `+<digits>/TYPE=PLMN` phone numbers in raw bytes.
static PLMN_RE: LazyLock<BytesRegex> =
    LazyLock::new(|| BytesRegex::new(r"\+(\d{10,15})/TYPE=PLMN").expect("plmn"));
/// A `text.txt` / `text_N.txt` content marker in raw bytes.
static TEXT_CONTENT_RE: LazyLock<BytesRegex> =
    LazyLock::new(|| BytesRegex::new(r"(?-u)\x8etext(?:_\d+)?\.txt\x00").expect("txt"));
/// Text that is only a part name or reference, not a message body.
static MMS_PART_JUNK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)^(?:text_\d+\.txt|"?<text_\d+>?|"<\d+>|"<text_\d+\.txt>|IMG_\d+\.[A-Za-z]{3,4})$"#,
    )
    .expect("junk")
});
/// A run of at least eight printable ASCII bytes.
static PRINTABLE_RUN_RE: LazyLock<BytesRegex> =
    LazyLock::new(|| BytesRegex::new(r"(?-u)[\x20-\x7e\n\r\t]{8,}").expect("run"));
/// Body text ending in `!!` followed by up to twelve bytes of junk.
static TRAILING_GARBAGE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(.+!!)[^\w\s]{0,12}$").expect("trail"));
/// `text.txt` or `text_N.txt`, the message body part name.
static TEXT_PART_NAME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^text(?:_\d+)?\.txt$").expect("text name"));

const TEXT_PART_END_MARKERS: &[&[u8]] = &[
    b"\x8c",
    b"\xa0\x85",
    b"\x00\x85IMG",
    b"\x85IMG",
    b"\xff\xd8\xff",
    b"\x00\x8e",
    b"\x00\x85",
];

const ATTACHMENT_MAGICS: &[(&[u8], &str)] = &[
    (b"\xff\xd8\xff", ".jpg"),
    (b"\x89PNG\r\n\x1a\n", ".png"),
    (b"GIF87a", ".gif"),
    (b"GIF89a", ".gif"),
    (b"\x00\x00\x00\x18ftyp3gp", ".3gp"),
    (b"ftypmp42", ".mp4"),
    (b"#!AMR", ".amr"),
    (b"RIFF", ".wav"),
];

/// One attachment decoded from a PDU.
#[derive(Debug, Clone)]
pub struct ParsedAttachment {
    /// File extension including the leading dot (e.g. `.jpg`).
    pub ext: String,
    /// Decoded attachment bytes.
    pub data: Vec<u8>,
    /// SMIL `src` reference the part binds to, when matched.
    pub smil_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldSource {
    Structured,
    Heuristic,
}

/// One decoded PDU message.
#[derive(Debug, Clone)]
pub struct ParsedPdu {
    /// Source `.pdu` file path.
    pub path: std::path::PathBuf,
    /// Message time in Unix seconds (structured Date header, else filename).
    pub timestamp: i64,
    /// Deduplicated, sanitized participant numbers.
    pub participants: Vec<String>,
    /// Decoded message text (possibly emoji-decoded).
    pub body: String,
    /// Decoded attachment list.
    pub attachments: Vec<ParsedAttachment>,
    /// Whether the direction is outgoing (owner was From).
    pub is_sent: bool,
    /// Whether there are at least 3 unique participants.
    pub is_group: bool,
    /// Inferred sender digits (owner when outgoing).
    pub sender_number: String,
    /// Structured MMS From header was present (before digit sanitize).
    pub has_from: bool,
    /// Structured MMS To header list was non-empty.
    pub has_to: bool,
    /// Optional MMS headers (subject, `message_id`, …).
    pub pdu_fields: BTreeMap<String, String>,
    /// `structured` | `mixed` | `heuristic`
    pub decode_quality: &'static str,
}

#[derive(Debug, Default)]
struct SmilRefs {
    text_srcs: Vec<String>,
    media_srcs: Vec<String>,
}

/// Unix timestamp from a GO SMS Pro PDU file name (`I_<ts>_...`).
fn timestamp_from_filename(name: &str) -> Option<i64> {
    PDU_FILENAME_RE
        .captures(name)
        .and_then(|c| c.name("ts"))
        .and_then(|m| m.as_str().parse().ok())
}

/// Phone numbers found as `+<digits>/TYPE=PLMN` anywhere in the raw bytes, in order, once each.
fn extract_plmn_numbers(data: &[u8]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut numbers = Vec::new();
    for caps in PLMN_RE.captures_iter(data) {
        let digits = String::from_utf8_lossy(&caps[1]).into_owned();
        if seen.insert(digits.clone()) {
            numbers.push(digits);
        }
    }
    numbers
}

/// Digits from an MMS address (`+1…/TYPE=PLMN` or bare digits).
fn digits_from_mms_address(addr: &str) -> Option<String> {
    let base = addr.split('/').next().unwrap_or(addr).trim();
    let trimmed = base.trim_start_matches('+');
    sanitize_number(trimmed).or_else(|| sanitize_number(base))
}

/// Distinct phone digit strings from the decoded addresses.
fn participants_from_structured(msg: &StructuredMms) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut numbers = Vec::new();
    for addr in msg.address_strings() {
        if let Some(digits) = digits_from_mms_address(&addr)
            && seen.insert(digits.clone())
        {
            numbers.push(digits);
        }
    }
    numbers
}

/// True for `text.txt` or `text_N.txt`, the message body part.
fn is_text_part_name(name: &str) -> bool {
    TEXT_PART_NAME_RE.is_match(name)
}

/// The body text of a part, decoded by charset, with binary tail junk removed.
fn text_from_part_data(data: &[u8], charset: Option<u64>) -> Option<String> {
    let text = decode_bytes_with_charset(data, charset)
        .replace('\0', "")
        .trim()
        .to_string();
    let text = truncate_mms_binary_tail(&text);
    if text.is_empty() || is_mms_part_junk(&text) {
        return None;
    }
    Some(decode_gosms_emojis(&text))
}

/// True when a SMIL `src` names this part.
fn smil_src_matches_name(src: &str, name: &str) -> bool {
    let a = normalize_content_id(src).to_ascii_lowercase();
    let b = normalize_content_id(name).to_ascii_lowercase();
    !a.is_empty() && (a == b || src.eq_ignore_ascii_case(name))
}

/// True when a SMIL `src` names this part by content id, location, or file name.
fn part_matches_smil_src(part: &MmsPart, src: &str) -> bool {
    if let Some(cid) = &part.content_id
        && smil_src_matches_name(src, cid)
    {
        return true;
    }
    if let Some(loc) = &part.content_location
        && smil_src_matches_name(src, loc)
    {
        return true;
    }
    if let Some(name) = &part.filename
        && smil_src_matches_name(src, name)
    {
        return true;
    }
    false
}

/// The best name for a part: location, then file name, then content id.
fn part_display_name(part: &MmsPart) -> Option<String> {
    part.content_location
        .clone()
        .or_else(|| part.filename.clone())
        .or_else(|| part.content_id.clone())
}

/// True for `application/smil`.
fn is_smil_content_type(ct: &str) -> bool {
    let base = ct
        .split(';')
        .next()
        .unwrap_or(ct)
        .trim()
        .to_ascii_lowercase();
    base.contains("smil") || base == "application/smil"
}

/// True when the bytes contain a `<smil` tag.
fn looks_like_smil_bytes(data: &[u8]) -> bool {
    let lower = data.to_ascii_lowercase();
    lower.windows(5).any(|w| w == b"<smil")
}

/// Body text from the name-scanned parts, preferring the ones SMIL marks as text.
fn body_from_named_parts(named: &[NamedPart], smil: &SmilRefs) -> Option<String> {
    for src in &smil.text_srcs {
        for part in named {
            if smil_src_matches_name(src, &part.name)
                && let Some(text) = text_from_part_data(&part.data, None)
            {
                return Some(text);
            }
        }
    }
    let mut texts = Vec::new();
    let mut seen = HashSet::new();
    for part in named {
        if !is_text_part_name(&part.name)
            && !content_type_from_filename(&part.name).starts_with("text/")
        {
            continue;
        }
        if let Some(text) = text_from_part_data(&part.data, None)
            && seen.insert(text.clone())
        {
            texts.push(text);
        }
    }
    if texts.is_empty() {
        None
    } else {
        Some(texts.join("\n"))
    }
}

/// Body text from the structured parts, preferring the ones SMIL marks as text.
fn body_from_structured(msg: &StructuredMms, smil: &SmilRefs) -> Option<String> {
    if msg.parts.is_empty() {
        return None;
    }
    for src in &smil.text_srcs {
        for part in &msg.parts {
            if part_matches_smil_src(part, src)
                && let Some(text) = text_from_part_data(&part.data, part.charset)
            {
                return Some(text);
            }
        }
    }
    if let Some(start) = &msg.content_start {
        for part in &msg.parts {
            if !part_matches_smil_src(part, start) {
                continue;
            }
            if is_smil_content_type(&part.content_type) || looks_like_smil_bytes(&part.data) {
                continue;
            }
            let ct = part.content_type.to_ascii_lowercase();
            let base = ct.split(';').next().unwrap_or(&ct).trim();
            if base.starts_with("text/")
                && let Some(text) = text_from_part_data(&part.data, part.charset)
            {
                return Some(text);
            }
        }
    }
    let mut texts = Vec::new();
    let mut seen = HashSet::new();
    for part in &msg.parts {
        let ct = part.content_type.to_ascii_lowercase();
        let base = ct.split(';').next().unwrap_or(&ct).trim();
        if !(base.starts_with("text/plain") || base == "text/*" || base == "text/html") {
            continue;
        }
        if let Some(text) = text_from_part_data(&part.data, part.charset)
            && seen.insert(text.clone())
        {
            texts.push(text);
        }
    }
    if texts.is_empty() {
        None
    } else {
        Some(texts.join("\n"))
    }
}

/// File extension for a part name, via its guessed MIME type.
fn ext_from_filename(name: &str) -> Option<String> {
    let ct = content_type_from_filename(name);
    extension_for_content_type(&ct).map(|e| e.to_string())
}

/// False for stubs too small to be real media of that type.
fn is_usable_attachment(ext: &str, len: usize) -> bool {
    if len < 64 && matches!(ext, ".jpg" | ".png" | ".gif") {
        return false;
    }
    if ext == ".wav" && len < 10000 {
        return false;
    }
    true
}

/// Attachments from the name-scanned parts, in SMIL order when SMIL names them.
fn attachments_from_named_parts(named: &[NamedPart], smil: &SmilRefs) -> Vec<ParsedAttachment> {
    let use_smil = !smil.media_srcs.is_empty();
    let mut out = Vec::new();
    for part in named {
        if is_text_part_name(&part.name) {
            continue;
        }
        let Some(ext) = ext_from_filename(&part.name) else {
            continue;
        };
        if ext == ".txt" {
            continue;
        }
        let smil_name = if use_smil {
            smil.media_srcs
                .iter()
                .find(|src| smil_src_matches_name(src, &part.name))
                .cloned()
        } else {
            Some(part.name.clone())
        };
        if use_smil && smil_name.is_none() {
            continue;
        }
        if !is_usable_attachment(&ext, part.data.len()) {
            continue;
        }
        out.push(ParsedAttachment {
            ext,
            data: part.data.clone(),
            smil_name: smil_name.or_else(|| Some(part.name.clone())),
        });
    }
    out
}

/// Attachments from the structured parts, in SMIL order when SMIL names them.
fn attachments_from_structured(msg: &StructuredMms, smil: &SmilRefs) -> Vec<ParsedAttachment> {
    let use_smil = !smil.media_srcs.is_empty();
    let mut out = Vec::new();
    for part in &msg.parts {
        if is_smil_content_type(&part.content_type) || looks_like_smil_bytes(&part.data) {
            continue;
        }
        let ext = part
            .filename
            .as_deref()
            .and_then(ext_from_filename)
            .or_else(|| part.content_location.as_deref().and_then(ext_from_filename))
            .or_else(|| extension_for_content_type(&part.content_type).map(str::to_string));
        let Some(ext) = ext else {
            continue;
        };
        if ext == ".txt" {
            continue;
        }
        let smil_name = if use_smil {
            smil.media_srcs
                .iter()
                .find(|src| part_matches_smil_src(part, src))
                .cloned()
        } else {
            part_display_name(part)
        };
        if use_smil && smil_name.is_none() {
            continue;
        }
        if !is_usable_attachment(&ext, part.data.len()) {
            continue;
        }
        out.push(ParsedAttachment {
            ext,
            data: part.data.clone(),
            smil_name,
        });
    }
    out
}

/// The bytes between `<smil` and `</smil>`, if present.
fn extract_smil_region(data: &[u8]) -> Option<&[u8]> {
    let lower = data.to_ascii_lowercase();
    let start = lower.windows(5).position(|w| w == b"<smil")?;
    let end_rel = lower[start..]
        .windows(7)
        .position(|w| w == b"</smil>")
        .map(|p| start + p + 7)?;
    Some(&data[start..end_rel])
}

/// The text and media `src` names a SMIL layout refers to, in order.
fn parse_smil_refs(data: &[u8]) -> SmilRefs {
    let mut refs = SmilRefs::default();
    let Some(smil_bytes) = extract_smil_region(data) else {
        return refs;
    };
    let Ok(text) = std::str::from_utf8(smil_bytes) else {
        return refs;
    };
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e) | Event::Empty(e)) => {
                let tag = e.name().as_ref().to_ascii_lowercase();
                let mut src = None;
                for attr in e.attributes().flatten() {
                    let key = attr.key.as_ref().to_ascii_lowercase();
                    if key == "src" {
                        src = Some(attr.value.into_owned());
                    }
                }
                if let Some(s) = src {
                    if s.is_empty() {
                        continue;
                    }
                    match tag.as_str() {
                        "text" => refs.text_srcs.push(s),
                        "img" | "audio" | "video" => refs.media_srcs.push(s),
                        _ => {}
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    refs
}

/// Cut body text at the first `IMG_` file name that binary data smeared onto it.
fn truncate_mms_binary_tail(text: &str) -> String {
    let mut text = text.to_string();
    if let Some(img_idx) = text.find("IMG_")
        && img_idx > 0
    {
        text.truncate(img_idx);
    }
    if let Some(caps) = TRAILING_GARBAGE_RE.captures(&text) {
        text = caps[1].to_string();
    }
    for (index, ch) in text.char_indices() {
        if ch == '\n' || ch == '\r' || ch == '\t' {
            continue;
        }
        let code = ch as u32;
        if code < 32 || code == 127 {
            return text[..index].trim_end().to_string();
        }
    }
    text.trim().to_string()
}

/// True for text that is only a part name or reference, not a message body.
fn is_mms_part_junk(text: &str) -> bool {
    MMS_PART_JUNK_RE.is_match(text)
}

/// Text from `start` up to the first known part boundary.
fn extract_text_after_marker(data: &[u8], start: usize) -> String {
    let mut end = data.len();
    for sep in TEXT_PART_END_MARKERS {
        if let Some(pos) = find_bytes(data, sep, start) {
            end = end.min(pos);
        }
    }
    text_from_part_data(&data[start..end], None).unwrap_or_default()
}

/// Index of `needle` in `haystack` at or after `start`.
fn find_bytes(haystack: &[u8], needle: &[u8], start: usize) -> Option<usize> {
    haystack[start..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| start + p)
}

/// Last-resort body when Content-Location / multipart text is missing.
fn extract_wap_text_body_fallback(data: &[u8]) -> String {
    let mut texts = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for m in TEXT_CONTENT_RE.find_iter(data) {
        let text = extract_text_after_marker(data, m.end());
        if !text.is_empty() && seen.insert(text.clone()) {
            texts.push(text);
        }
    }
    if !texts.is_empty() {
        return decode_gosms_emojis(&texts.join("\n"));
    }

    if let Some(smil_end) = find_bytes(data, b"</smil>", 0) {
        let tail = &data[smil_end + 7..];
        if let Some(m) = PRINTABLE_RUN_RE.find(tail) {
            let text = String::from_utf8_lossy(m.as_bytes()).trim().to_string();
            if !text.is_empty() && !text.starts_with('<') && !is_mms_part_junk(&text) {
                return decode_gosms_emojis(&text);
            }
        }
    }
    String::new()
}

/// Media blobs found by magic bytes when no part structure decodes: (extension, start, end) triples.
fn detect_attachment_blobs(data: &[u8]) -> Vec<(String, usize, usize)> {
    if data.len() < 32 {
        return Vec::new();
    }
    let mut hits: Vec<(usize, &str)> = Vec::new();
    for &(sig, ext) in ATTACHMENT_MAGICS {
        let mut start = 0;
        while let Some(rel) = find_bytes(data, sig, start) {
            hits.push((rel, ext));
            start = rel + 1;
        }
    }
    if hits.is_empty() {
        return Vec::new();
    }
    hits.sort_by_key(|(idx, _)| *idx);
    let mut merged = Vec::new();
    for (idx, (start, ext)) in hits.iter().enumerate() {
        let next_start = hits.get(idx + 1).map(|(s, _)| *s).unwrap_or(data.len());
        let size = next_start - start;
        if !is_usable_attachment(ext, size) {
            continue;
        }
        merged.push((ext.to_string(), *start, next_start));
    }
    merged
}

/// Participants once each, in first-seen order.
fn unique_participants(parts: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut unique = Vec::new();
    for p in parts {
        if seen.insert(p.clone()) {
            unique.push(p.clone());
        }
    }
    unique
}

/// True when the digits are one of the owner's numbers.
fn is_owner_digit(digits: &str, owners: &HashSet<String>) -> bool {
    sanitize_number(digits).is_some_and(|d| owners.contains(&d))
}

/// Sender, is-from-owner, and has-from flags from the decoded addresses.
fn roles_from_structured(
    msg: &StructuredMms,
    owners: &HashSet<String>,
) -> (Option<String>, bool, bool) {
    let from_digits = msg.from.as_ref().and_then(|a| digits_from_mms_address(a));
    let my_is_from = from_digits
        .as_ref()
        .is_some_and(|d| is_owner_digit(d, owners));
    let my_is_to = msg
        .to
        .iter()
        .chain(msg.cc.iter())
        .filter_map(|a| digits_from_mms_address(a))
        .any(|d| is_owner_digit(&d, owners));
    (from_digits, my_is_from, my_is_to)
}

/// Direction from decoded From/To/Cc when present; otherwise owner/participant rules
/// (no byte-prefix markers — sent fixtures often lack From/To headers entirely).
fn infer_pdu_direction(
    structured: &StructuredMms,
    unique_parts: &[String],
    owners: &HashSet<String>,
    primary_digits: &str,
) -> (bool, String) {
    if unique_parts.is_empty() {
        return (false, String::new());
    }

    let has_roles = structured.from.is_some()
        || !structured.to.is_empty()
        || !structured.cc.is_empty()
        || !structured.bcc.is_empty();

    if has_roles {
        let (from_digits, my_is_from, my_is_to) = roles_from_structured(structured, owners);
        if my_is_from {
            return (true, primary_digits.to_string());
        }
        if let Some(from) = from_digits {
            if !is_owner_digit(&from, owners) {
                return (false, from);
            }
            return (true, primary_digits.to_string());
        }
        if my_is_to {
            let sender = unique_parts
                .iter()
                .find(|p| !is_owner_digit(p, owners))
                .cloned()
                .unwrap_or_else(|| unique_parts[0].clone());
            return (false, sender);
        }
    }

    // Raw PLMN lists without From/To headers (e.g. sent one-to-one dumps).
    // Owner presence is the primary direction signal: a sent group MMS (owner +
    // ≥ 2 recipients, no headers) must not be read as received just because the
    // participant list is long. "Received" is only inferred when no participant
    // is an owner number, so the >= 3 branch below never sees an all-owner list.
    if unique_parts.iter().any(|p| is_owner_digit(p, owners)) {
        return (true, primary_digits.to_string());
    }

    if unique_parts.len() >= 3 {
        let sender = unique_parts
            .iter()
            .find(|p| !is_owner_digit(p, owners))
            .cloned()
            .unwrap_or_else(|| unique_parts[0].clone());
        return (false, sender);
    }

    (false, unique_parts[0].clone())
}

/// The decoded date when it is plausible, else the file-name timestamp, and which one it was.
fn resolve_timestamp(filename_ts: i64, structured: &StructuredMms) -> (i64, FieldSource) {
    match structured.date_unix {
        Some(d) if d > 0 && d <= i64::MAX as u64 => (d as i64, FieldSource::Structured),
        _ => (filename_ts, FieldSource::Heuristic),
    }
}

/// Insert `key` when the value is present and non-empty.
fn insert_nonempty(fields: &mut BTreeMap<String, String>, key: &str, value: Option<&str>) {
    if let Some(v) = value
        && !v.is_empty()
    {
        fields.insert(key.into(), v.to_string());
    }
}

/// The decoded headers as string fields for the vendor `source` bag.
fn pdu_fields_from_structured(msg: &StructuredMms) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    insert_nonempty(&mut fields, "subject", msg.subject.as_deref());
    insert_nonempty(&mut fields, "message_id", msg.message_id.as_deref());
    insert_nonempty(
        &mut fields,
        "delivery_report",
        msg.delivery_report.as_deref(),
    );
    insert_nonempty(&mut fields, "read_report", msg.read_report.as_deref());
    insert_nonempty(&mut fields, "priority", msg.priority.as_deref());
    insert_nonempty(&mut fields, "message_type", msg.message_type.as_deref());
    insert_nonempty(&mut fields, "delivery_time", msg.delivery_time.as_deref());
    insert_nonempty(&mut fields, "expiry", msg.expiry.as_deref());
    insert_nonempty(&mut fields, "message_class", msg.message_class.as_deref());
    insert_nonempty(&mut fields, "mms_version", msg.mms_version.as_deref());
    if let Some(sz) = msg.message_size {
        fields.insert("message_size".into(), sz.to_string());
    }
    insert_nonempty(&mut fields, "report_allowed", msg.report_allowed.as_deref());
    insert_nonempty(
        &mut fields,
        "response_status",
        msg.response_status.as_deref(),
    );
    insert_nonempty(&mut fields, "response_text", msg.response_text.as_deref());
    insert_nonempty(
        &mut fields,
        "sender_visibility",
        msg.sender_visibility.as_deref(),
    );
    insert_nonempty(&mut fields, "status", msg.status.as_deref());
    insert_nonempty(&mut fields, "transaction_id", msg.transaction_id.as_deref());
    if !msg.bcc.is_empty() {
        fields.insert("bcc".into(), msg.bcc.join(","));
    }
    for (name, value) in &msg.application_headers {
        if !value.is_empty() {
            fields.insert(format!("app:{name}"), value.clone());
        }
    }
    fields
}

/// A quality label from how many of the four fields came from the structured decode rather than heuristics.
fn score_decode_quality(
    body: FieldSource,
    attachments: FieldSource,
    direction: FieldSource,
    timestamp: FieldSource,
) -> &'static str {
    // Filename timestamps are normal for GO fragments; they alone do not demote
    // a row from `structured` when body/attachments/direction are structured.
    let content = [body, attachments, direction];
    if content.iter().all(|s| *s == FieldSource::Structured) {
        return "structured";
    }
    if body == FieldSource::Heuristic && attachments == FieldSource::Heuristic {
        return "heuristic";
    }
    if content.iter().all(|s| *s == FieldSource::Heuristic) && timestamp == FieldSource::Heuristic {
        return "heuristic";
    }
    "mixed"
}

/// Parse one PDU file. Returns `None` for unparseable files or bad filenames.
///
/// # Errors
///
/// Returns an error when the file cannot be read.
pub fn parse_pdu_file(
    path: &Path,
    owners: &HashSet<String>,
    primary_digits: &str,
) -> Result<Option<ParsedPdu>> {
    let data = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    if data.len() < 10 {
        return Ok(None);
    }
    let Some(filename_ts) =
        timestamp_from_filename(path.file_name().and_then(|s| s.to_str()).unwrap_or(""))
    else {
        return Ok(None);
    };

    let structured = decode_mms_best_effort(&data);
    let smil = parse_smil_refs(&data);
    let (timestamp, ts_src) = resolve_timestamp(filename_ts, &structured);
    let pdu_fields = pdu_fields_from_structured(&structured);
    let (body, body_src) = pdu_body(&structured, &smil, &data);
    let (attachments, atts_src) = pdu_attachments(&structured, &smil, &data);

    let unique_parts = unique_participants(&pdu_participants(&structured, &data));
    let is_group = unique_parts.len() >= 3;
    let has_roles = structured.from.is_some()
        || !structured.to.is_empty()
        || !structured.cc.is_empty()
        || !structured.bcc.is_empty();
    let dir_src = if has_roles {
        FieldSource::Structured
    } else {
        FieldSource::Heuristic
    };
    let (is_sent, sender_number) =
        infer_pdu_direction(&structured, &unique_parts, owners, primary_digits);

    let decode_quality = score_decode_quality(body_src, atts_src, dir_src, ts_src);

    Ok(Some(ParsedPdu {
        path: path.to_path_buf(),
        timestamp,
        participants: unique_parts,
        body,
        attachments,
        is_sent,
        is_group,
        sender_number,
        has_from: structured.from.is_some(),
        has_to: !structured.to.is_empty(),
        pdu_fields,
        decode_quality,
    }))
}

/// Sanitized participant numbers: the address headers, plus any
/// PLMN-encoded number in the raw bytes the headers did not name. With no
/// address headers at all, the raw scan is all there is.
fn pdu_participants(structured: &StructuredMms, data: &[u8]) -> Vec<String> {
    let mut parts = participants_from_structured(structured);
    if parts.is_empty() {
        parts = extract_plmn_numbers(data);
    } else {
        let mut seen: HashSet<String> = parts.iter().cloned().collect();
        for n in extract_plmn_numbers(data) {
            if seen.insert(n.clone()) {
                parts.push(n);
            }
        }
    }
    parts.iter().filter_map(|p| sanitize_number(p)).collect()
}

/// The text body and where it came from: a named text part, a text part the
/// SMIL names, the Subject header, or, as a last resort, a scan of the raw
/// bytes for WAP text, which is heuristic when it finds anything.
fn pdu_body(structured: &StructuredMms, smil: &SmilRefs, data: &[u8]) -> (String, FieldSource) {
    if let Some(b) = body_from_named_parts(&structured.named_parts, smil) {
        return (b, FieldSource::Structured);
    }
    if let Some(b) = body_from_structured(structured, smil) {
        return (b, FieldSource::Structured);
    }
    if let Some(subject) = structured
        .subject
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return (decode_gosms_emojis(subject), FieldSource::Structured);
    }
    let b = extract_wap_text_body_fallback(data);
    let src = if b.is_empty() {
        FieldSource::Structured
    } else {
        FieldSource::Heuristic
    };
    (b, src)
}

/// The attachments and where they came from: named parts, parts the SMIL
/// names, or, as a last resort, media blobs detected in the raw bytes
/// (heuristic) paired with the SMIL media names in order.
fn pdu_attachments(
    structured: &StructuredMms,
    smil: &SmilRefs,
    data: &[u8],
) -> (Vec<ParsedAttachment>, FieldSource) {
    let mut attachments = attachments_from_named_parts(&structured.named_parts, smil);
    if attachments.is_empty() {
        attachments = attachments_from_structured(structured, smil);
    }
    if !attachments.is_empty() {
        return (attachments, FieldSource::Structured);
    }
    let attachments: Vec<ParsedAttachment> = detect_attachment_blobs(data)
        .into_iter()
        .enumerate()
        .map(|(i, (ext, start, end))| ParsedAttachment {
            ext,
            data: data[start..end].to_vec(),
            smil_name: smil.media_srcs.get(i).cloned(),
        })
        .collect();
    let src = if attachments.is_empty() {
        FieldSource::Structured
    } else {
        FieldSource::Heuristic
    };
    (attachments, src)
}

#[cfg(test)]
mod tests;
