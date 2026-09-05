//! Decode-oriented WAP-209 / WAP-230 helpers for MMS binary PDUs.
//!
//! Algorithm reference (not a dependency / not copied): OMA WAP-209 MMS Encapsulation,
//! WAP-230 WSP, and the decode path concepts in python-messaging's `messaging.mms`.
//!
//! # GO SMS Pro fragments
//!
//! Backups often store a partial header (From/To + named text) rather than a full
//! `m-retrieve-conf`. [`decode_mms_best_effort`] merges a strict header walk with
//! fragment scanners.
//!
//! ## Wire byte `0x8e` (three meanings)
//!
//! | Context | Meaning |
//! |---------|---------|
//! | MMS header field `0x0e` | Message-Size (Long-integer) |
//! | GO fragment | Named part marker: `0x8e` + `filename\0` + payload ([`scan_named_parts`]) |
//! | WSP part header `0x0e` | Content-Location |
//!
//! Message-Size decode requires a valid Long-integer; on failure the header walk
//! soft-stops and named-part scanning still sees the raw `0x8e` in the buffer.
//!
//! ## Other quirks
//!
//! - **Content-Type terminates headers** (WAP-209): after CT, remaining bytes are
//!   the multipart body (or GO junk / named parts).
//! - **Content-Type general-form** starts with Value-length (`peek <= 31`) and must
//!   be tried before constrained-media text, or the length octet is misread as TEXT.
//! - **GO Value-length overshoot** on From/To/encoded-strings: declared length often
//!   swallows the next short-integer header; readers stop before known MMS field bytes.
//! - **Application headers** (Token-text name) land in
//!   [`StructuredMms::application_headers`] and CSV as `app:<name>` (see
//!   <https://bitrealm.io/vault/developer/formats/go-sms-pro/mapping/>).

use crate::decoders::{
    Cursor, apply_mms_header_field, decode_content_type_value, decode_mms_header_field,
    decode_multipart_body, trim_encoded_string_junk,
};
use std::collections::BTreeMap;

/// Well-known MMS field names (WAP-209 table 8). Stored as short-integer values
/// (MSB already cleared); on the wire they appear as `value | 0x80`.
pub(crate) const MMS_BCC: u8 = 0x01;
pub(crate) const MMS_CC: u8 = 0x02;
pub(crate) const MMS_CONTENT_LOCATION: u8 = 0x03;
pub(crate) const MMS_CONTENT_TYPE: u8 = 0x04;
pub(crate) const MMS_DATE: u8 = 0x05;
pub(crate) const MMS_DELIVERY_REPORT: u8 = 0x06;
pub(crate) const MMS_DELIVERY_TIME: u8 = 0x07;
pub(crate) const MMS_EXPIRY: u8 = 0x08;
pub(crate) const MMS_FROM: u8 = 0x09;
pub(crate) const MMS_MESSAGE_CLASS: u8 = 0x0a;
pub(crate) const MMS_MESSAGE_ID: u8 = 0x0b;
pub(crate) const MMS_MESSAGE_TYPE: u8 = 0x0c;
pub(crate) const MMS_VERSION: u8 = 0x0d;
pub(crate) const MMS_MESSAGE_SIZE: u8 = 0x0e;
pub(crate) const MMS_PRIORITY: u8 = 0x0f;
pub(crate) const MMS_READ_REPORT: u8 = 0x10;
pub(crate) const MMS_REPORT_ALLOWED: u8 = 0x11;
pub(crate) const MMS_RESPONSE_STATUS: u8 = 0x12;
pub(crate) const MMS_RESPONSE_TEXT: u8 = 0x13;
pub(crate) const MMS_SENDER_VISIBILITY: u8 = 0x14;
pub(crate) const MMS_STATUS: u8 = 0x15;
pub(crate) const MMS_SUBJECT: u8 = 0x16;
pub(crate) const MMS_TO: u8 = 0x17;
pub(crate) const MMS_TRANSACTION_ID: u8 = 0x18;

/// WSP well-known headers (table 39), short-integer form (MSB cleared).
pub(crate) const WSP_CONTENT_LOCATION: u8 = 0x0e;
pub(crate) const WSP_CONTENT_DISPOSITION: u8 = 0x2e;
pub(crate) const WSP_CONTENT_ID: u8 = 0x40;
/// IANA MIBEnum UTF-8 / UCS-2.
pub(crate) const CHARSET_UTF8: u64 = 106;
pub(crate) const CHARSET_UCS2: u64 = 1000;

/// Subset of WSP well-known content types (WAP-230 table 40) used for attachments.
pub(crate) const WELL_KNOWN_CONTENT_TYPES: &[&str] = &[
    "*/*",
    "text/*",
    "text/html",
    "text/plain",
    "multipart/*",
    "multipart/mixed",
    "multipart/form-data",
    "multipart/byteranges",
    "multipart/alternative",
    "application/*",
    "application/java-vm",
    "application/x-www-form-urlencoded",
    "application/hdmlc",
    "application/vnd.wap.wmlc",
    "application/vnd.wap.wmlscriptc",
    "application/vnd.wap.wta-eventc",
    "application/vnd.wap.uaprof",
    "application/vnd.wap.wtls-ca-certificate",
    "application/vnd.wap.wtls-user-certificate",
    "application/x-x509-ca-cert",
    "application/x-x509-user-cert",
    "image/*",
    "image/gif",
    "image/jpeg",
    "image/tiff",
    "image/png",
    "image/vnd.wap.wbmp",
    "application/vnd.wap.multipart.*",
    "application/vnd.wap.multipart.mixed",
    "application/vnd.wap.multipart.form-data",
    "application/vnd.wap.multipart.byteranges",
    "application/vnd.wap.multipart.alternative",
    "application/xml",
    "text/xml",
    "application/vnd.wap.wbxml",
    "application/x-x968-cross-cert",
    "application/x-x968-ca-cert",
    "application/x-x968-user-cert",
    "text/vnd.wap.si",
    "application/vnd.wap.sic",
    "text/vnd.wap.sl",
    "application/vnd.wap.slc",
    "text/vnd.wap.co",
    "application/vnd.wap.coc",
    "application/vnd.wap.multipart.related",
    "application/vnd.wap.sia",
    "text/vnd.wap.connectivity-xml",
    "application/vnd.wap.connectivity-wbxml",
    "application/pkcs7-mime",
    "application/vnd.wap.hashed-certificate",
    "application/vnd.wap.signed-certificate",
    "application/vnd.wap.cert-response",
    "application/xhtml+xml",
    "application/wml+xml",
    "text/css",
    "application/vnd.wap.mms-message",
    "application/vnd.wap.rollover-certificate",
    "application/vnd.wap.locc+wbxml",
    "application/vnd.wap.loc+xml",
    "application/vnd.syncml.dm+wbxml",
    "application/vnd.syncml.dm+xml",
    "application/vnd.syncml.notification",
    "application/vnd.wap.xhtml+xml",
    "application/vnd.wv.csp.cir",
    "application/vnd.oma.dd+xml",
    "application/vnd.oma.drm.message",
    "application/vnd.oma.drm.content",
    "application/vnd.oma.drm.rights+xml",
    "application/vnd.oma.drm.rights+wbxml",
];

/// One multipart body part (WSP headers + payload).
#[derive(Debug, Clone)]
pub(crate) struct MmsPart {
    pub content_type: String,
    pub content_location: Option<String>,
    pub content_id: Option<String>,
    /// From Content-Type `Filename` / Content-Disposition filename parameter.
    pub filename: Option<String>,
    /// IANA MIBEnum from Content-Type Charset parameter, when present.
    pub charset: Option<u64>,
    pub data: Vec<u8>,
}

/// GO Content-Location-style named payload (`0x8e` + `name\0` + bytes).
#[derive(Debug, Clone)]
pub(crate) struct NamedPart {
    pub name: String,
    pub data: Vec<u8>,
}

/// Best-effort decoded MMS headers, parts, and GO named fragments.
///
/// Unknown text application headers are in [`Self::application_headers`] and
/// exported to CSV as `app:<name>`.
#[derive(Debug, Clone, Default)]
pub(crate) struct StructuredMms {
    pub message_type: Option<String>,
    pub from: Option<String>,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub content_type: Option<String>,
    /// Content-Type `Start` parameter (root/SMIL Content-ID), when present.
    pub content_start: Option<String>,
    pub date_unix: Option<u64>,
    pub subject: Option<String>,
    pub message_id: Option<String>,
    pub delivery_report: Option<String>,
    pub read_report: Option<String>,
    pub priority: Option<String>,
    pub delivery_time: Option<String>,
    pub expiry: Option<String>,
    pub message_class: Option<String>,
    pub mms_version: Option<String>,
    /// WAP-209 Message-Size (octets); advisory / approximate.
    pub message_size: Option<u64>,
    pub report_allowed: Option<String>,
    pub response_status: Option<String>,
    pub response_text: Option<String>,
    pub sender_visibility: Option<String>,
    pub status: Option<String>,
    pub transaction_id: Option<String>,
    /// Non-well-known MMS application headers (text name → value).
    pub application_headers: BTreeMap<String, String>,
    pub parts: Vec<MmsPart>,
    pub named_parts: Vec<NamedPart>,
}

impl StructuredMms {
    /// True when the decode produced any address, part, or body worth keeping.
    pub fn is_useful(&self) -> bool {
        self.from.is_some()
            || !self.to.is_empty()
            || !self.cc.is_empty()
            || !self.bcc.is_empty()
            || !self.parts.is_empty()
            || !self.named_parts.is_empty()
            || self.subject.is_some()
            || self.message_id.is_some()
            || self.message_type.is_some()
            || self.content_type.is_some()
            || self.transaction_id.is_some()
            || !self.application_headers.is_empty()
    }

    /// Every address on the message: from, to, cc, bcc.
    pub fn address_strings(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(from) = &self.from {
            out.push(from.clone());
        }
        out.extend(self.to.iter().cloned());
        out.extend(self.cc.iter().cloned());
        out.extend(self.bcc.iter().cloned());
        out
    }

    /// Fill `dst` from `src` when `dst` is empty.
    fn merge_opt(dst: &mut Option<String>, src: Option<String>) {
        if dst.is_none() {
            *dst = src;
        }
    }

    /// Fill every empty field of this message from `other`, and union the parts.
    fn merge_from(&mut self, other: StructuredMms) {
        Self::merge_opt(&mut self.from, other.from);
        if self.to.is_empty() {
            self.to = other.to;
        }
        if self.cc.is_empty() {
            self.cc = other.cc;
        }
        if self.bcc.is_empty() {
            self.bcc = other.bcc;
        }
        if self.date_unix.is_none() {
            self.date_unix = other.date_unix;
        }
        Self::merge_opt(&mut self.message_type, other.message_type);
        Self::merge_opt(&mut self.content_type, other.content_type);
        Self::merge_opt(&mut self.content_start, other.content_start);
        Self::merge_opt(&mut self.subject, other.subject);
        Self::merge_opt(&mut self.message_id, other.message_id);
        Self::merge_opt(&mut self.delivery_report, other.delivery_report);
        Self::merge_opt(&mut self.read_report, other.read_report);
        Self::merge_opt(&mut self.priority, other.priority);
        Self::merge_opt(&mut self.delivery_time, other.delivery_time);
        Self::merge_opt(&mut self.expiry, other.expiry);
        Self::merge_opt(&mut self.message_class, other.message_class);
        Self::merge_opt(&mut self.mms_version, other.mms_version);
        if self.message_size.is_none() {
            self.message_size = other.message_size;
        }
        Self::merge_opt(&mut self.report_allowed, other.report_allowed);
        Self::merge_opt(&mut self.response_status, other.response_status);
        Self::merge_opt(&mut self.response_text, other.response_text);
        Self::merge_opt(&mut self.sender_visibility, other.sender_visibility);
        Self::merge_opt(&mut self.status, other.status);
        Self::merge_opt(&mut self.transaction_id, other.transaction_id);
        for (k, v) in other.application_headers {
            self.application_headers.entry(k).or_insert(v);
        }
        merge_parts_into(&mut self.parts, other.parts);
        if self.named_parts.is_empty() {
            self.named_parts = other.named_parts;
        }
    }
}

/// The fields that identify a part, so the same part decoded twice is kept once.
fn part_dedupe_key(part: &MmsPart) -> (Option<&str>, Option<&str>, Option<&str>, usize, &[u8]) {
    (
        part.content_id.as_deref(),
        part.content_location.as_deref(),
        part.filename.as_deref(),
        part.data.len(),
        &part.data[..part.data.len().min(64)],
    )
}

/// Append the parts not already present by [`part_dedupe_key`].
fn merge_parts_into(dst: &mut Vec<MmsPart>, incoming: Vec<MmsPart>) {
    for part in incoming {
        let key = part_dedupe_key(&part);
        let already = dst.iter().any(|p| part_dedupe_key(p) == key);
        if !already {
            dst.push(part);
        }
    }
}

/// Decode raw part/header bytes with an optional IANA MIBEnum charset.
pub(crate) fn decode_bytes_with_charset(bytes: &[u8], charset: Option<u64>) -> String {
    let bytes = bytes.strip_suffix(&[0]).unwrap_or(bytes);
    let text = match charset {
        Some(CHARSET_UCS2) if !bytes.is_empty() => {
            // as_chunks drops a trailing odd byte, like the manual
            // even-length truncation it replaces.
            let (pairs, _) = bytes.as_chunks::<2>();
            let units: Vec<u16> = pairs.iter().map(|&pair| u16::from_be_bytes(pair)).collect();
            String::from_utf16_lossy(&units)
        }
        _ => String::from_utf8_lossy(bytes).into_owned(),
    };
    trim_encoded_string_junk(text.trim_end_matches('\0'))
}

/// A Content-ID without its `cid:` prefix and angle brackets.
pub(crate) fn normalize_content_id(raw: &str) -> String {
    let s = raw.trim();
    let s = s.strip_prefix("cid:").unwrap_or(s);
    let s = s.strip_prefix('<').unwrap_or(s);
    let s = s.strip_suffix('>').unwrap_or(s);
    s.trim().to_string()
}

/// Attempt a full WAP-209 header + multipart decode starting at `start`.
pub(crate) fn decode_mms_at(data: &[u8], start: usize) -> Option<StructuredMms> {
    if start >= data.len() || data.len().saturating_sub(start) < 4 {
        return None;
    }
    let mut cur = Cursor { data, pos: start };
    let mut msg = StructuredMms::default();
    let mut saw_content_type = false;
    for _ in 0..64 {
        match decode_mms_header_field(&mut cur, &mut msg) {
            Ok(true) => {
                saw_content_type = true;
                break;
            }
            Ok(false) => {}
            // Soft-stop: keep headers decoded so far (e.g. GO 0x8e named part
            // misread as Message-Size). Scanners still see the raw bytes.
            Err(()) => break,
        }
    }
    if !saw_content_type && msg.from.is_none() && msg.to.is_empty() && msg.subject.is_none() {
        return None;
    }
    if let Some(ct) = &msg.content_type
        && ct.contains("multipart")
        && let Ok(parts) = decode_multipart_body(&mut cur)
    {
        msg.parts = parts;
    }
    if msg.is_useful() { Some(msg) } else { None }
}

/// Attempt a full decode from the start of `data`.
pub(crate) fn decode_mms(data: &[u8]) -> Option<StructuredMms> {
    decode_mms_at(data, 0)
}

/// Walk for Content-Type (`0x84`) and decode multipart bodies mid-file.
pub(crate) fn scan_multipart_bodies(data: &[u8]) -> Vec<MmsPart> {
    let mut parts = Vec::new();
    let mut i = 0;
    while i + 2 < data.len() {
        if data[i] != 0x84 {
            i += 1;
            continue;
        }
        let mut cur = Cursor { data, pos: i + 1 };
        let Ok((ct, _params)) = decode_content_type_value(&mut cur) else {
            i += 1;
            continue;
        };
        if !ct.to_ascii_lowercase().contains("multipart") {
            i += 1;
            continue;
        }
        if let Ok(decoded) = decode_multipart_body(&mut cur) {
            merge_parts_into(&mut parts, decoded);
            i = cur.pos.max(i + 1);
        } else {
            i += 1;
        }
    }
    parts
}

/// Try a structured decode at every `X-Mms-Message-Type` byte in the blob, for PDUs with junk before the headers.
fn scan_message_type_starts(data: &[u8]) -> Vec<StructuredMms> {
    let mut out = Vec::new();
    let mut attempts = 0usize;
    let mut i = 0;
    while i + 2 < data.len() && attempts < 32 {
        if data[i] != 0x8c {
            i += 1;
            continue;
        }
        attempts += 1;
        if let Some(msg) = decode_mms_at(data, i) {
            out.push(msg);
        }
        i += 1;
    }
    out
}

/// True for a byte that can appear in a part file name.
fn is_printable_name_byte(b: u8) -> bool {
    matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-')
}

/// True for a short, printable name with an extension.
fn looks_like_part_name(name: &str) -> bool {
    let name = name.trim();
    if name.is_empty() || name.len() > 128 {
        return false;
    }
    if !name.contains('.') {
        return false;
    }
    name.bytes().all(is_printable_name_byte)
}

/// Parse a Content-Location name at a `0x8e` byte; returns the name and the index after its NUL.
fn try_parse_cloc_name_at(data: &[u8], at: usize) -> Option<(String, usize)> {
    // at points at 0x8e; returns (name, index of byte after NUL).
    if at >= data.len() || data[at] != 0x8e || at + 1 >= data.len() {
        return None;
    }
    let next = data[at + 1];
    if next & 0x80 != 0 || !is_printable_name_byte(next) {
        return None;
    }
    let name_start = at + 1;
    let mut name_end = name_start;
    while name_end < data.len() && data[name_end] != 0 {
        if !is_printable_name_byte(data[name_end]) {
            return None;
        }
        name_end += 1;
    }
    if name_end >= data.len() || data[name_end] != 0 {
        return None;
    }
    let name = String::from_utf8_lossy(&data[name_start..name_end]).into_owned();
    if !looks_like_part_name(&name) {
        return None;
    }
    Some((name, name_end + 1))
}

/// Index of the next Content-Location name at or after `start`.
fn find_next_cloc_name(data: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    while i + 2 < data.len() {
        if data[i] == 0x8e && try_parse_cloc_name_at(data, i).is_some() {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Matches `text.txt` or `text_<digits>.txt` (same rule as `pdu`).
fn is_text_part_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower == "text.txt" {
        return true;
    }
    let Some(rest) = lower.strip_prefix("text_") else {
        return false;
    };
    let Some(stem) = rest.strip_suffix(".txt") else {
        return false;
    };
    !stem.is_empty() && stem.bytes().all(|b| b.is_ascii_digit())
}

/// Text-part payload end: advance through valid UTF-8 bytes so non-ASCII text
/// (accented Latin, Cyrillic, CJK) survives. Stops at the first byte that cannot
/// be part of a UTF-8 sequence — in GO dumps that is the next MMS header byte
/// (e.g. `0x8c`), a next named-part marker (`0x8e`), or a truncated sequence.
fn text_part_payload_end(data: &[u8], start: usize) -> usize {
    let mut end = start;
    while end < data.len() {
        let b = data[end];
        if b < 0x80 {
            end += 1;
        } else if (0xc2..=0xdf).contains(&b) && end + 1 < data.len() && data[end + 1] & 0xc0 == 0x80
        {
            end += 2;
        } else if (0xe0..=0xef).contains(&b)
            && end + 2 < data.len()
            && data[end + 1] & 0xc0 == 0x80
            && data[end + 2] & 0xc0 == 0x80
        {
            end += 3;
        } else if (0xf0..=0xf4).contains(&b)
            && end + 3 < data.len()
            && data[end + 1] & 0xc0 == 0x80
            && data[end + 2] & 0xc0 == 0x80
            && data[end + 3] & 0xc0 == 0x80
        {
            end += 4;
        } else {
            break;
        }
    }
    end
}

/// Scan GO named parts: wire `0x8e` + NUL-terminated filename + payload.
///
/// This is **not** WAP-209 Message-Size (same wire id). Text parts end at the
/// next byte that cannot be part of a UTF-8 text payload (a header byte or the
/// next `0x8e` marker); media parts end at the next `0x8e` name (or EOF) so
/// JPEG high bytes are kept intact.
pub(crate) fn scan_named_parts(data: &[u8]) -> Vec<NamedPart> {
    let mut parts = Vec::new();
    let mut i = 0;
    while i + 2 < data.len() {
        let Some((name, payload_start)) = try_parse_cloc_name_at(data, i) else {
            i += 1;
            continue;
        };
        let payload_end = if is_text_part_name(&name) {
            text_part_payload_end(data, payload_start)
        } else {
            find_next_cloc_name(data, payload_start).unwrap_or(data.len())
        };
        let payload = data[payload_start..payload_end].to_vec();
        parts.push(NamedPart {
            name,
            data: payload,
        });
        i = payload_end.max(i + 1);
    }
    parts
}

/// MIME type guessed from a part file name's extension.
pub(crate) fn content_type_from_filename(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    match ext {
        "txt" => "text/plain".into(),
        "html" | "htm" => "text/html".into(),
        "jpg" | "jpeg" => "image/jpeg".into(),
        "png" => "image/png".into(),
        "gif" => "image/gif".into(),
        "amr" => "audio/amr".into(),
        "mp3" => "audio/mpeg".into(),
        "wav" => "audio/wav".into(),
        "3gp" => "video/3gpp".into(),
        "mp4" => "video/mp4".into(),
        "smil" => "application/smil".into(),
        _ => "application/octet-stream".into(),
    }
}

/// Attach the parts found by name scanning, skipping those the structured decode already has.
fn merge_named_parts(msg: &mut StructuredMms, named: Vec<NamedPart>) {
    if named.is_empty() {
        return;
    }
    let mut out_named = Vec::with_capacity(named.len());
    for np in named {
        let already = msg.parts.iter().any(|p| {
            p.content_location.as_deref() == Some(np.name.as_str())
                || (p.data == np.data && !np.data.is_empty())
        });
        let NamedPart { name, data } = np;
        if !already {
            msg.parts.push(MmsPart {
                content_type: content_type_from_filename(&name),
                content_location: Some(name.clone()),
                content_id: None,
                filename: Some(name.clone()),
                charset: None,
                data,
            });
            let data = msg.parts.last().expect("just pushed").data.clone();
            out_named.push(NamedPart { name, data });
        } else {
            out_named.push(NamedPart { name, data });
        }
    }
    msg.named_parts = out_named;
}

/// Scan for embedded From/To/Cc/Date short-integer headers (GO SMS Pro fragments).
pub(crate) fn scan_mms_addresses(data: &[u8]) -> StructuredMms {
    let mut msg = StructuredMms::default();
    let mut i = 0;
    while i < data.len() {
        let byte = data[i];
        if byte & 0x80 == 0 {
            i += 1;
            continue;
        }
        let field = byte & 0x7f;
        if i + 1 >= data.len() {
            break;
        }
        let mut cur = Cursor { data, pos: i + 1 };
        if apply_mms_header_field(field, &mut cur, &mut msg) {
            i = cur.pos;
            continue;
        }
        i += 1;
    }
    msg
}

/// Merge full/offset decode, address/header scan, mid-file multipart, and GO named parts.
///
/// Prefer this entry point for GO backup files: individual paths alone are incomplete.
pub(crate) fn decode_mms_best_effort(data: &[u8]) -> StructuredMms {
    let named = scan_named_parts(data);
    let mut msg = decode_mms(data).unwrap_or_default();
    msg.merge_from(scan_mms_addresses(data));
    for candidate in scan_message_type_starts(data) {
        msg.merge_from(candidate);
    }
    merge_parts_into(&mut msg.parts, scan_multipart_bodies(data));
    merge_named_parts(&mut msg, named);
    msg
}

/// File extension for a MIME type, or `None` when unknown.
pub(crate) fn extension_for_content_type(content_type: &str) -> Option<&'static str> {
    let ct = content_type.to_ascii_lowercase();
    let base = ct.split(';').next().unwrap_or(&ct).trim();
    match base {
        "image/jpeg" | "image/jpg" => Some(".jpg"),
        "image/png" => Some(".png"),
        "image/gif" => Some(".gif"),
        "image/tiff" => Some(".tiff"),
        "image/vnd.wap.wbmp" => Some(".wbmp"),
        "audio/amr" | "audio/3gpp" => Some(".amr"),
        "audio/mpeg" | "audio/mp3" => Some(".mp3"),
        "audio/wav" | "audio/x-wav" => Some(".wav"),
        "video/3gpp" => Some(".3gp"),
        "video/mp4" => Some(".mp4"),
        "text/plain" | "text/*" => Some(".txt"),
        "application/smil" | "application/vnd.wap.multipart.related" => None,
        _ if base.starts_with("image/") => Some(".bin"),
        _ if base.starts_with("audio/") => Some(".bin"),
        _ if base.starts_with("video/") => Some(".bin"),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
