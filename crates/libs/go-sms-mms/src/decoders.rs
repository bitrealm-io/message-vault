//! WAP-209 / WAP-230 unit decoders moved out of `mms_enc`.

use crate::mms_enc::{
    CHARSET_UCS2, CHARSET_UTF8, MMS_BCC, MMS_CC, MMS_CONTENT_LOCATION, MMS_CONTENT_TYPE, MMS_DATE,
    MMS_DELIVERY_REPORT, MMS_DELIVERY_TIME, MMS_EXPIRY, MMS_FROM, MMS_MESSAGE_CLASS,
    MMS_MESSAGE_ID, MMS_MESSAGE_SIZE, MMS_MESSAGE_TYPE, MMS_PRIORITY, MMS_READ_REPORT,
    MMS_REPORT_ALLOWED, MMS_RESPONSE_STATUS, MMS_RESPONSE_TEXT, MMS_SENDER_VISIBILITY, MMS_STATUS,
    MMS_SUBJECT, MMS_TO, MMS_TRANSACTION_ID, MMS_VERSION, MmsPart, StructuredMms,
    WELL_KNOWN_CONTENT_TYPES, WSP_CONTENT_DISPOSITION, WSP_CONTENT_ID, WSP_CONTENT_LOCATION,
    decode_bytes_with_charset, normalize_content_id,
};
use std::collections::HashMap;

/// True for MMS header fields whose value is a short integer.
pub(crate) fn is_mms_short_integer_field(field: u8) -> bool {
    matches!(
        field,
        MMS_BCC
            | MMS_CC
            | MMS_CONTENT_LOCATION
            | MMS_CONTENT_TYPE
            | MMS_DATE
            | MMS_DELIVERY_REPORT
            | MMS_DELIVERY_TIME
            | MMS_EXPIRY
            | MMS_FROM
            | MMS_MESSAGE_CLASS
            | MMS_MESSAGE_ID
            | MMS_MESSAGE_TYPE
            | MMS_VERSION
            | MMS_MESSAGE_SIZE
            | MMS_PRIORITY
            | MMS_READ_REPORT
            | MMS_REPORT_ALLOWED
            | MMS_RESPONSE_STATUS
            | MMS_RESPONSE_TEXT
            | MMS_SENDER_VISIBILITY
            | MMS_STATUS
            | MMS_SUBJECT
            | MMS_TO
            | MMS_TRANSACTION_ID
    )
}

/// The `yes`/`no` name for a boolean header byte.
pub(crate) fn yes_no_token(v: u8) -> Option<&'static str> {
    match v {
        0x00 => Some("yes"),
        0x01 => Some("no"),
        _ => None,
    }
}

/// The name for a priority header byte.
pub(crate) fn priority_token(v: u8) -> Option<&'static str> {
    match v {
        0x00 => Some("Low"),
        0x01 => Some("Normal"),
        0x02 => Some("High"),
        _ => None,
    }
}

#[derive(Debug)]
pub(crate) struct Cursor<'a> {
    pub(crate) data: &'a [u8],
    pub(crate) pos: usize,
}

impl<'a> Cursor<'a> {
    /// A cursor at the start of `data`.
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Bytes left after the cursor.
    pub(crate) fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    /// The next byte without consuming it.
    pub(crate) fn peek(&self) -> Option<u8> {
        self.data.get(self.pos).copied()
    }

    /// Consume and return the next byte.
    pub(crate) fn next_byte(&mut self) -> Result<u8, ()> {
        let b = self.peek().ok_or(())?;
        self.pos += 1;
        Ok(b)
    }

    /// Consume the next `n` bytes as a slice.
    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8], ()> {
        if self.remaining() < n {
            return Err(());
        }
        let slice = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }
}

/// WSP uintvar: seven bits per byte, high bit set on all but the last.
pub(crate) fn decode_uint_var(cur: &mut Cursor<'_>) -> Result<u64, ()> {
    let mut value = 0u64;
    for _ in 0..5 {
        let byte = cur.next_byte()?;
        value = (value << 7) | u64::from(byte & 0x7f);
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(())
}

/// WSP Value-length: a byte up to 30, or 31 followed by a uintvar.
pub(crate) fn decode_value_length(cur: &mut Cursor<'_>) -> Result<usize, ()> {
    let byte = cur.peek().ok_or(())?;
    if byte <= 30 {
        cur.next_byte()?;
        Ok(usize::from(byte))
    } else if byte == 31 {
        cur.next_byte()?;
        Ok(decode_uint_var(cur)? as usize)
    } else {
        Err(())
    }
}

/// A NUL-terminated string (a leading quote byte is stripped).
pub(crate) fn decode_text_string(cur: &mut Cursor<'_>) -> Result<String, ()> {
    let start = cur.pos;
    while cur.pos < cur.data.len() && cur.data[cur.pos] != 0 {
        cur.pos += 1;
    }
    if cur.pos >= cur.data.len() {
        return Err(());
    }
    let s = String::from_utf8_lossy(&cur.data[start..cur.pos]).into_owned();
    cur.pos += 1; // skip NUL
    Ok(s)
}

/// WSP Short-integer: one byte with the high bit set.
pub(crate) fn decode_short_integer(cur: &mut Cursor<'_>) -> Result<u8, ()> {
    let byte = cur.peek().ok_or(())?;
    if byte & 0x80 == 0 {
        return Err(());
    }
    cur.next_byte()?;
    Ok(byte & 0x7f)
}

/// WSP Long-integer: a length byte followed by that many big-endian bytes.
pub(crate) fn decode_long_integer(cur: &mut Cursor<'_>) -> Result<u64, ()> {
    let len = cur.peek().ok_or(())?;
    if len == 0 || len > 30 {
        return Err(());
    }
    cur.next_byte()?;
    let bytes = cur.take(usize::from(len))?;
    let mut value = 0u64;
    for b in bytes {
        value = (value << 8) | u64::from(*b);
    }
    Ok(value)
}

/// Short-integer if the high bit is set, else Long-integer.
pub(crate) fn decode_integer_value(cur: &mut Cursor<'_>) -> Result<u64, ()> {
    if let Ok(v) = decode_short_integer(cur) {
        return Ok(u64::from(v));
    }
    decode_long_integer(cur)
}

/// Cut an address at its `/TYPE=PLMN` suffix; GO SMS Pro's value lengths often swallow the next header.
pub(crate) fn trim_encoded_string_junk(s: &str) -> String {
    // GO value-lengths often swallow the next header; keep a clean PLMN address.
    if let Some(idx) = s.find("/TYPE=PLMN") {
        return s[..idx + "/TYPE=PLMN".len()].to_string();
    }
    s.trim_end_matches('\0').to_string()
}

/// How many continuation bytes follow a UTF-8 lead byte: 0 for ASCII or a byte
/// that cannot start a character.
fn utf8_continuation_count(lead: u8) -> usize {
    match lead {
        0xc0..=0xdf => 1,
        0xe0..=0xef => 2,
        0xf0..=0xf7 => 3,
        _ => 0,
    }
}

/// Encoded-string-value = Text-string | Value-length Char-set Text-string
///
/// GO SMS Pro PDUs often declare a Value-length that overlaps the next MMS
/// short-integer header. Read text until NUL or a high-bit header byte; do not
/// blindly consume through `end` when that would swallow the next field.
pub(crate) fn decode_encoded_string_value(cur: &mut Cursor<'_>) -> Result<String, ()> {
    let saved = cur.pos;
    if let Ok(len) = decode_value_length(cur) {
        let end = cur.pos.checked_add(len).ok_or(())?;
        if end > cur.data.len() {
            cur.pos = saved;
            return Err(());
        }
        let after_len = cur.pos;
        let charset = match decode_integer_value(cur) {
            Ok(cs) if cur.pos <= end => Some(cs),
            _ => {
                cur.pos = after_len;
                None
            }
        };
        if cur.pos > end {
            cur.pos = saved;
            return Err(());
        }
        let start = cur.pos;
        match charset {
            Some(CHARSET_UCS2) => {
                while cur.pos + 1 < end {
                    if cur.data[cur.pos] == 0 && cur.data[cur.pos + 1] == 0 {
                        break;
                    }
                    cur.pos += 2;
                }
            }
            Some(CHARSET_UTF8) => {
                // Prefer value-length end (real UTF-8). Also stop before an obvious
                // next MMS header when GO overshoots (ASCII PLMN then 0x8x/0x9x).
                // Header codes are 0x81..=0x98, the range of UTF-8 continuation
                // bytes, so only a byte where a character would start can be one.
                while cur.pos < end && cur.data[cur.pos] != 0 {
                    let b = cur.data[cur.pos];
                    if b & 0x80 != 0 && is_mms_short_integer_field(b & 0x7f) {
                        break;
                    }
                    cur.pos += 1;
                    let mut continuation = utf8_continuation_count(b);
                    while continuation > 0 && cur.pos < end && cur.data[cur.pos] & 0xc0 == 0x80 {
                        cur.pos += 1;
                        continuation -= 1;
                    }
                }
            }
            _ => {
                // Vendor overshoot: stop before the next short-integer header.
                while cur.pos < end {
                    let b = cur.data[cur.pos];
                    if b == 0 || b & 0x80 != 0 {
                        break;
                    }
                    cur.pos += 1;
                }
            }
        }
        let text = decode_bytes_with_charset(&cur.data[start..cur.pos.min(end)], charset);
        if cur.pos < end && cur.data[cur.pos] == 0 {
            cur.pos += 1;
        }
        while cur.pos < end && cur.data[cur.pos] == 0 {
            cur.pos += 1;
        }
        // Leave a following short-integer header in place when length overshoots.
        if cur.pos < end && cur.data[cur.pos] & 0x80 != 0 {
            // keep pos
        } else if cur.pos < end {
            cur.pos = end;
        }
        if !text.is_empty() {
            return Ok(text);
        }
        cur.pos = saved;
        return Err(());
    }
    cur.pos = saved;
    decode_text_string(cur)
}

/// Delta-seconds as a Long-integer, falling back to an Integer-value.
pub(crate) fn decode_delta_seconds(cur: &mut Cursor<'_>) -> Result<u64, ()> {
    decode_long_integer(cur).or_else(|_| decode_integer_value(cur))
}

/// Absolute-token Date-value | Relative-token Delta-seconds-value inside Value-length.
pub(crate) fn decode_expiry_or_delivery_time(cur: &mut Cursor<'_>) -> Result<String, ()> {
    let saved = cur.pos;
    let len = decode_value_length(cur)?;
    let end = cur.pos.checked_add(len).ok_or(())?;
    if end > cur.data.len() {
        cur.pos = saved;
        return Err(());
    }
    let token = cur.next_byte()?;
    let result = if token == 0x80 {
        let d = decode_date_value(cur)?;
        format!("absolute:{d}")
    } else if token == 0x81 {
        let d = decode_delta_seconds(cur)?;
        format!("relative:{d}")
    } else {
        return Err(());
    };
    cur.pos = end.max(cur.pos);
    Ok(result)
}

/// MMS-version as `major.minor` from one Short-integer.
pub(crate) fn decode_mms_version(cur: &mut Cursor<'_>) -> Result<String, ()> {
    let v = decode_short_integer(cur)?;
    let major = (v >> 4) & 0x0f;
    let minor = v & 0x0f;
    Ok(format!("{major}.{minor}"))
}

/// Message-class: a well-known token name or a text string.
pub(crate) fn decode_message_class_value(cur: &mut Cursor<'_>) -> Result<String, ()> {
    let saved = cur.pos;
    if let Ok(v) = decode_short_integer(cur) {
        let name = match v {
            0x00 => "Personal",
            0x01 => "Advertisement",
            0x02 => "Informational",
            0x03 => "Auto",
            other => return Ok(format!("unknown-0x{other:02x}")),
        };
        return Ok(name.into());
    }
    cur.pos = saved;
    decode_text_string(cur)
}

/// X-Mms-Status token name.
pub(crate) fn decode_status_value(cur: &mut Cursor<'_>) -> Result<String, ()> {
    let v = decode_short_integer(cur)?;
    Ok(match v {
        0x00 => "Expired".into(),
        0x01 => "Retrieved".into(),
        0x02 => "Rejected".into(),
        0x03 => "Deferred".into(),
        0x04 => "Unrecognized".into(),
        0x05 => "Indeterminate".into(),
        0x06 => "Forwarded".into(),
        0x07 => "Unreachable".into(),
        other => format!("unknown-0x{other:02x}"),
    })
}

/// X-Mms-Response-Status token name.
pub(crate) fn decode_response_status_value(cur: &mut Cursor<'_>) -> Result<String, ()> {
    let v = decode_short_integer(cur)?;
    Ok(match v {
        0x00 => "Ok".into(),
        0x01 => "Error-unspecified".into(),
        0x02 => "Error-service-denied".into(),
        0x03 => "Error-message-format-corrupt".into(),
        0x04 => "Error-sending-address-unresolved".into(),
        0x05 => "Error-message-not-found".into(),
        0x06 => "Error-network-problem".into(),
        0x07 => "Error-content-not-accepted".into(),
        0x08 => "Error-unsupported-message".into(),
        other => format!("0x{other:02x}"),
    })
}

/// X-Mms-Sender-Visibility token name.
pub(crate) fn decode_sender_visibility_value(cur: &mut Cursor<'_>) -> Result<String, ()> {
    let v = decode_short_integer(cur)?;
    Ok(match v {
        0x00 => "Hide".into(),
        0x01 => "Show".into(),
        other => format!("unknown-0x{other:02x}"),
    })
}

/// From-value = Value-length (Address-present-token Encoded-string-value | Insert-address-token)
pub(crate) fn decode_from_value(cur: &mut Cursor<'_>) -> Result<String, ()> {
    let saved = cur.pos;
    let len = decode_value_length(cur)?;
    let end = cur.pos.checked_add(len).ok_or(())?;
    if end > cur.data.len() {
        cur.pos = saved;
        return Err(());
    }
    let token = cur.next_byte()?;
    if token == 0x81 {
        // Insert-address-token
        cur.pos = end;
        return Ok(String::new());
    }
    if token != 0x80 {
        return Err(());
    }
    let addr = decode_encoded_string_value(cur)?;
    // Same Value-length overshoot quirk as Encoded-string-value.
    while cur.pos < end && cur.data[cur.pos] == 0 {
        cur.pos += 1;
    }
    if cur.pos < end && cur.data[cur.pos] & 0x80 != 0 {
        return Ok(addr);
    }
    cur.pos = end.max(cur.pos);
    Ok(addr)
}

/// Date-value: seconds since the Unix epoch as a Long-integer.
pub(crate) fn decode_date_value(cur: &mut Cursor<'_>) -> Result<u64, ()> {
    decode_long_integer(cur)
}

/// X-Mms-Message-Type token name (`m-send-req`, `m-retrieve-conf`, and so on).
pub(crate) fn decode_message_type_value(cur: &mut Cursor<'_>) -> Result<String, ()> {
    let v = decode_short_integer(cur)?;
    let name = match v {
        0x00 => "m-send-req",
        0x01 => "m-send-conf",
        0x02 => "m-notification-ind",
        0x03 => "m-notifyresp-ind",
        0x04 => "m-retrieve-conf",
        0x05 => "m-acknowledge-ind",
        0x06 => "m-delivery-ind",
        0x07 => "m-read-rec-ind",
        0x08 => "m-read-orig-ind",
        0x09 => "m-forward-req",
        0x0a => "m-forward-conf",
        other => return Ok(format!("unknown-0x{other:02x}")),
    };
    Ok(name.to_string())
}

/// MIME type for a WSP well-known content-type id.
pub(crate) fn well_known_content_type(id: u64) -> Option<&'static str> {
    WELL_KNOWN_CONTENT_TYPES.get(id as usize).copied()
}

/// Constrained-media: a well-known id or a text string.
pub(crate) fn decode_constrained_media(cur: &mut Cursor<'_>) -> Result<String, ()> {
    if let Ok(id) = decode_short_integer(cur) {
        return well_known_content_type(u64::from(id))
            .map(str::to_string)
            .ok_or(());
    }
    decode_text_string(cur)
}

/// A parameter value that may be a plain or an encoded string.
pub(crate) fn decode_wsp_text_param(cur: &mut Cursor<'_>) -> Result<String, ()> {
    decode_text_string(cur).or_else(|_| decode_encoded_string_value(cur))
}

/// Decode a well-known WSP parameter by token id (table 38).
/// Charset/Type are integer-like; Name/Filename/Start are text-like.
pub(crate) fn decode_wsp_typed_param(
    cur: &mut Cursor<'_>,
    name_id: u8,
) -> Result<(String, String), ()> {
    match name_id {
        0x08 => {
            // Charset = Well-known-charset (Integer-value)
            let v = decode_integer_value(cur)?;
            Ok(("Charset".into(), v.to_string()))
        }
        0x09 => {
            // Type (v1.2+) = Constrained-encoding
            let ct = decode_constrained_media(cur)?;
            Ok(("Type".into(), ct))
        }
        0x05 | 0x17 => Ok(("Name".into(), decode_wsp_text_param(cur)?)),
        0x06 | 0x18 => Ok(("Filename".into(), decode_wsp_text_param(cur)?)),
        0x0a | 0x19 => Ok(("Start".into(), decode_wsp_text_param(cur)?)),
        0x0b | 0x1a => Ok(("Start-info".into(), decode_wsp_text_param(cur)?)),
        _ => Err(()),
    }
}

/// Scan WSP typed/untyped parameters until `end`.
pub(crate) fn decode_wsp_parameters(cur: &mut Cursor<'_>, end: usize) -> HashMap<String, String> {
    let mut params = HashMap::new();
    while cur.pos < end {
        let pstart = cur.pos;
        if let Ok(name_id) = decode_short_integer(cur) {
            if let Ok((key, val)) = decode_wsp_typed_param(cur, name_id) {
                params.insert(key, val);
                continue;
            }
        } else if let Ok(name) = decode_text_string(cur) {
            // Untyped-parameter = Token-text Untyped-value
            if let Ok(val) = decode_wsp_text_param(cur).or_else(|_| decode_constrained_media(cur))
                && !name.is_empty()
            {
                params.insert(name, val);
                continue;
            }
        }
        cur.pos = pstart + 1;
        if cur.pos <= pstart {
            break;
        }
    }
    params
}

/// Content-type with its parameters, in either the constrained or the general form.
pub(crate) fn decode_content_type_value(
    cur: &mut Cursor<'_>,
) -> Result<(String, HashMap<String, String>), ()> {
    let saved = cur.pos;
    let peek = cur.peek().ok_or(())?;
    // Content-general-form starts with Value-length (0..=30 or 31+uintvar).
    // Must try before Constrained-media text, or length bytes look like TEXT.
    if peek <= 31 {
        let len = decode_value_length(cur)?;
        let end = cur.pos.checked_add(len).ok_or(())?;
        if end > cur.data.len() {
            cur.pos = saved;
            return Err(());
        }
        let media = if let Ok(id) = decode_integer_value(cur) {
            well_known_content_type(id).map_or_else(
                || format!("application/octet-stream;id={id}"),
                str::to_string,
            )
        } else {
            decode_text_string(cur)?
        };
        let params = decode_wsp_parameters(cur, end);
        cur.pos = end;
        return Ok((media, params));
    }
    if let Ok(ct) = decode_constrained_media(cur) {
        return Ok((ct, HashMap::new()));
    }
    cur.pos = saved;
    Err(())
}

/// Content-Disposition-value = Value-length Disposition *(Parameter) | text
pub(crate) fn decode_content_disposition_value(
    cur: &mut Cursor<'_>,
) -> Result<(String, HashMap<String, String>), ()> {
    let saved = cur.pos;
    if let Ok(len) = decode_value_length(cur) {
        let end = cur.pos.checked_add(len).ok_or(())?;
        if end > cur.data.len() {
            cur.pos = saved;
            return Err(());
        }
        let token = cur.next_byte()?;
        let disposition = match token {
            0x80 => "form-data".into(),
            0x81 => "attachment".into(),
            0x82 => "inline".into(),
            _ => format!("0x{token:02x}"),
        };
        let params = decode_wsp_parameters(cur, end);
        cur.pos = end;
        return Ok((disposition, params));
    }
    cur.pos = saved;
    Ok((decode_text_string(cur)?, HashMap::new()))
}

/// An application header value: a text string, else a text-string parameter.
pub(crate) fn decode_application_header_value(cur: &mut Cursor<'_>) -> Result<String, ()> {
    let saved = cur.pos;
    if let Ok(s) = decode_text_string(cur)
        && !s.is_empty()
    {
        return Ok(s);
    }
    cur.pos = saved;
    if let Ok(s) = decode_encoded_string_value(cur)
        && !s.is_empty()
    {
        return Ok(s);
    }
    cur.pos = saved;
    if let Ok(v) = decode_short_integer(cur) {
        return Ok(format!("0x{v:02x}"));
    }
    cur.pos = saved;
    if let Ok(v) = decode_long_integer(cur) {
        return Ok(v.to_string());
    }
    cur.pos = saved;
    skip_unknown_mms_value(cur)?;
    Err(())
}

/// Skip a header value this decoder does not understand, trying the value-length,
/// short-integer, and text-string shapes in turn.
pub(crate) fn skip_unknown_mms_value(cur: &mut Cursor<'_>) -> Result<(), ()> {
    // Best-effort: value-length blob, short-integer, or text-string.
    let saved = cur.pos;
    if let Ok(len) = decode_value_length(cur) {
        if cur.take(len).is_err() {
            cur.pos = saved;
            return Err(());
        }
        return Ok(());
    }
    cur.pos = saved;
    if decode_short_integer(cur).is_ok() {
        return Ok(());
    }
    cur.pos = saved;
    let _ = decode_text_string(cur)?;
    Ok(())
}

/// One decoded MMS header value, before it is stored on a [`StructuredMms`].
/// Decoding and storing are separate steps so the strict header parse and
/// the lenient byte scan share one decoder and differ only in what they do
/// with a value.
pub(crate) enum HeaderValue {
    From(String),
    To(String),
    Cc(String),
    Bcc(String),
    MessageType(String),
    Date(u64),
    Subject(String),
    MessageId(String),
    TransactionId(String),
    Version(String),
    MessageSize(u64),
    MessageClass(String),
    DeliveryTime(String),
    Expiry(String),
    /// A `yes`/`no` token; `None` when the byte is neither.
    DeliveryReport(Option<&'static str>),
    ReadReport(Option<&'static str>),
    ReportAllowed(Option<&'static str>),
    /// A priority token; `None` when the byte is not one.
    Priority(Option<&'static str>),
    Status(String),
    ResponseStatus(String),
    ResponseText(String),
    SenderVisibility(String),
    ContentType(String, HashMap<String, String>),
    /// Content-Location and fields this decoder does not model: consumed, not kept.
    Skipped,
}

/// How [`StructuredMms::store`] treats a field that already has a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Store {
    /// The header section is authoritative: a later header replaces an earlier one.
    Overwrite,
    /// A byte scan meets stray copies: the first value found stays.
    KeepFirst,
}

/// Decode the value of well-known header `field` at the cursor.
///
/// # Errors
///
/// Returns `Err` when the bytes are not a valid value for the field. For
/// Message-Size the cursor is put back where it was: GO names parts with the
/// same wire byte (`\x8etext.txt\0`), and the named-part scanner wants to
/// read those bytes as text.
pub(crate) fn decode_header_value(field: u8, cur: &mut Cursor<'_>) -> Result<HeaderValue, ()> {
    use HeaderValue as H;
    Ok(match field {
        MMS_FROM => H::From(decode_from_value(cur)?),
        MMS_TO => H::To(decode_encoded_string_value(cur)?),
        MMS_CC => H::Cc(decode_encoded_string_value(cur)?),
        MMS_BCC => H::Bcc(decode_encoded_string_value(cur)?),
        MMS_MESSAGE_TYPE => H::MessageType(decode_message_type_value(cur)?),
        MMS_DATE => H::Date(decode_date_value(cur)?),
        MMS_SUBJECT => H::Subject(decode_encoded_string_value(cur)?),
        MMS_MESSAGE_ID => {
            H::MessageId(decode_text_string(cur).or_else(|_| decode_encoded_string_value(cur))?)
        }
        MMS_TRANSACTION_ID => H::TransactionId(decode_text_string(cur)?),
        MMS_VERSION => H::Version(decode_mms_version(cur)?),
        MMS_MESSAGE_SIZE => {
            let saved = cur.pos;
            H::MessageSize(decode_long_integer(cur).inspect_err(|()| cur.pos = saved)?)
        }
        MMS_MESSAGE_CLASS => H::MessageClass(decode_message_class_value(cur)?),
        MMS_DELIVERY_TIME => H::DeliveryTime(decode_expiry_or_delivery_time(cur)?),
        MMS_EXPIRY => H::Expiry(decode_expiry_or_delivery_time(cur)?),
        MMS_DELIVERY_REPORT => H::DeliveryReport(yes_no_token(decode_short_integer(cur)?)),
        MMS_READ_REPORT => H::ReadReport(yes_no_token(decode_short_integer(cur)?)),
        MMS_REPORT_ALLOWED => H::ReportAllowed(yes_no_token(decode_short_integer(cur)?)),
        MMS_PRIORITY => H::Priority(priority_token(decode_short_integer(cur)?)),
        MMS_STATUS => H::Status(decode_status_value(cur)?),
        MMS_RESPONSE_STATUS => H::ResponseStatus(decode_response_status_value(cur)?),
        MMS_RESPONSE_TEXT => H::ResponseText(decode_encoded_string_value(cur)?),
        MMS_SENDER_VISIBILITY => H::SenderVisibility(decode_sender_visibility_value(cur)?),
        MMS_CONTENT_TYPE => {
            let (content_type, params) = decode_content_type_value(cur)?;
            H::ContentType(content_type, params)
        }
        MMS_CONTENT_LOCATION => {
            decode_encoded_string_value(cur)
                .or_else(|_| decode_text_string(cur))
                .or_else(|_| skip_unknown_mms_value(cur).map(|()| String::new()))?;
            H::Skipped
        }
        _ => {
            skip_unknown_mms_value(cur)?;
            H::Skipped
        }
    })
}

impl StructuredMms {
    /// Store one decoded header. A blank string, a zero date, and a byte that
    /// is not a known yes/no or priority token store nothing. Returns whether
    /// anything was stored.
    pub(crate) fn store(&mut self, value: HeaderValue, mode: Store) -> bool {
        use HeaderValue as H;
        match value {
            H::From(a) => put_text(&mut self.from, a, mode),
            H::To(a) => push_address(&mut self.to, a),
            H::Cc(a) => push_address(&mut self.cc, a),
            H::Bcc(a) => push_address(&mut self.bcc, a),
            H::MessageType(v) => put(&mut self.message_type, v, mode),
            H::Date(d) => d > 0 && put(&mut self.date_unix, d, mode),
            H::Subject(s) => put_text(&mut self.subject, s, mode),
            H::MessageId(s) => put_text(&mut self.message_id, s, mode),
            H::TransactionId(s) => put_text(&mut self.transaction_id, s, mode),
            H::Version(v) => put(&mut self.mms_version, v, mode),
            H::MessageSize(n) => put(&mut self.message_size, n, mode),
            H::MessageClass(v) => put(&mut self.message_class, v, mode),
            H::DeliveryTime(v) => put(&mut self.delivery_time, v, mode),
            H::Expiry(v) => put(&mut self.expiry, v, mode),
            H::DeliveryReport(t) => put_token(&mut self.delivery_report, t, mode),
            H::ReadReport(t) => put_token(&mut self.read_report, t, mode),
            H::ReportAllowed(t) => put_token(&mut self.report_allowed, t, mode),
            H::Priority(t) => put_token(&mut self.priority, t, mode),
            H::Status(v) => put(&mut self.status, v, mode),
            H::ResponseStatus(v) => put(&mut self.response_status, v, mode),
            H::ResponseText(v) => put_text(&mut self.response_text, v, mode),
            H::SenderVisibility(v) => put(&mut self.sender_visibility, v, mode),
            H::ContentType(content_type, params) => {
                self.content_type = Some(content_type);
                if let Some(start) = params.get("Start").or_else(|| params.get("Start-info")) {
                    self.content_start = Some(normalize_content_id(start));
                }
                true
            }
            H::Skipped => false,
        }
    }
}

/// Set `slot` unless `mode` keeps an existing value. Returns whether it was set.
fn put<T>(slot: &mut Option<T>, value: T, mode: Store) -> bool {
    if mode == Store::KeepFirst && slot.is_some() {
        return false;
    }
    *slot = Some(value);
    true
}

/// [`put`] for text: a blank string is never stored.
fn put_text(slot: &mut Option<String>, value: String, mode: Store) -> bool {
    !value.is_empty() && put(slot, value, mode)
}

/// [`put`] for a token that may not have decoded to a known word.
fn put_token(slot: &mut Option<String>, token: Option<&str>, mode: Store) -> bool {
    match token {
        Some(token) => put(slot, token.to_string(), mode),
        None => false,
    }
}

/// Append a recipient; a blank address is never stored.
fn push_address(list: &mut Vec<String>, address: String) -> bool {
    if address.is_empty() {
        return false;
    }
    list.push(address);
    true
}

/// Decode one MMS header into `msg`. Returns `true` at Content-Type, which
/// ends the header section.
///
/// # Errors
///
/// Returns `Err` at the first byte that is not a well-formed header, where
/// the caller stops reading headers. Recipients and the subject are the
/// exception: a malformed one is dropped and decoding continues, because GO
/// writes them loosely and the rest of the PDU is still worth keeping.
pub(crate) fn decode_mms_header_field(
    cur: &mut Cursor<'_>,
    msg: &mut StructuredMms,
) -> Result<bool, ()> {
    let byte = cur.peek().ok_or(())?;
    if byte & 0x80 == 0 {
        // Application-header = Token-text Text-string (or other value forms).
        let name = decode_text_string(cur)?;
        if let Ok(value) = decode_application_header_value(cur)
            && !name.is_empty()
            && !value.is_empty()
        {
            msg.application_headers.entry(name).or_insert(value);
        }
        return Ok(false);
    }
    let field = decode_short_integer(cur)?;
    let value = match decode_header_value(field, cur) {
        Ok(value) => value,
        Err(()) if matches!(field, MMS_TO | MMS_CC | MMS_BCC | MMS_SUBJECT) => return Ok(false),
        Err(()) => return Err(()),
    };
    let ends_headers = matches!(value, HeaderValue::ContentType(..));
    msg.store(value, Store::Overwrite);
    Ok(ends_headers)
}

/// Try to read `field` as a header at `cur` during a byte scan. Returns
/// `true` when a value was decoded and stored, in which case the scan
/// continues from `cur.pos`. Content-Type and Content-Location are never
/// taken from a scan: the strict parse owns them, and a stray byte followed
/// by anything would otherwise replace a real one.
pub(crate) fn apply_mms_header_field(
    field: u8,
    cur: &mut Cursor<'_>,
    msg: &mut StructuredMms,
) -> bool {
    if matches!(field, MMS_CONTENT_TYPE | MMS_CONTENT_LOCATION) {
        return false;
    }
    match decode_header_value(field, cur) {
        Ok(value) => msg.store(value, Store::KeepFirst),
        Err(()) => false,
    }
}

/// WSP multipart body: a part count, then each part's headers and data.
pub(crate) fn decode_multipart_body(cur: &mut Cursor<'_>) -> Result<Vec<MmsPart>, ()> {
    let n = decode_uint_var(cur)? as usize;
    if n > 256 {
        return Err(());
    }
    let mut parts = Vec::with_capacity(n);
    for _ in 0..n {
        let headers_len = decode_uint_var(cur)? as usize;
        let data_len = decode_uint_var(cur)? as usize;
        if headers_len + data_len > cur.remaining() {
            return Err(());
        }
        let header_bytes = cur.take(headers_len)?;
        let mut hcur = Cursor::new(header_bytes);
        let (ctype, params) = decode_content_type_value(&mut hcur)
            .unwrap_or_else(|_| ("application/octet-stream".into(), HashMap::new()));
        let mut content_location = params
            .get("Name")
            .cloned()
            .or_else(|| params.get("Filename").cloned());
        let mut filename = params
            .get("Filename")
            .cloned()
            .or_else(|| params.get("Name").cloned());
        let charset = params.get("Charset").and_then(|s| s.parse::<u64>().ok());
        let mut content_id = None;
        while hcur.remaining() > 0 {
            let before = hcur.pos;
            if let Ok(field) = decode_short_integer(&mut hcur) {
                // Part headers use the WSP table: Content-Location is 0x0e, not
                // MMS 0x03 (Accept-Language in WSP).
                if field == WSP_CONTENT_LOCATION {
                    if let Ok(v) = decode_encoded_string_value(&mut hcur)
                        .or_else(|_| decode_text_string(&mut hcur))
                    {
                        content_location = Some(v);
                        continue;
                    }
                } else if field == WSP_CONTENT_ID {
                    if let Ok(v) = decode_encoded_string_value(&mut hcur)
                        .or_else(|_| decode_text_string(&mut hcur))
                    {
                        content_id = Some(normalize_content_id(&v));
                        continue;
                    }
                } else if field == WSP_CONTENT_DISPOSITION {
                    if let Ok((_disp, dparams)) = decode_content_disposition_value(&mut hcur) {
                        if let Some(fnm) = dparams.get("Filename").or_else(|| dparams.get("Name")) {
                            filename = Some(fnm.clone());
                            if content_location.is_none() {
                                content_location = Some(fnm.clone());
                            }
                        }
                        continue;
                    }
                } else {
                    let _ = skip_unknown_mms_value(&mut hcur);
                }
            } else if let Ok(name) = decode_text_string(&mut hcur) {
                if name.eq_ignore_ascii_case("Content-ID") {
                    if let Ok(v) = decode_encoded_string_value(&mut hcur)
                        .or_else(|_| decode_text_string(&mut hcur))
                    {
                        content_id = Some(normalize_content_id(&v));
                        continue;
                    }
                } else if name.eq_ignore_ascii_case("Content-Disposition") {
                    if let Ok((_disp, dparams)) = decode_content_disposition_value(&mut hcur) {
                        if let Some(fnm) = dparams.get("Filename").or_else(|| dparams.get("Name")) {
                            filename = Some(fnm.clone());
                            if content_location.is_none() {
                                content_location = Some(fnm.clone());
                            }
                        }
                        continue;
                    }
                } else if name.eq_ignore_ascii_case("Content-Location")
                    && let Ok(v) = decode_encoded_string_value(&mut hcur)
                        .or_else(|_| decode_text_string(&mut hcur))
                {
                    content_location = Some(v);
                    continue;
                }
                let _ = skip_unknown_mms_value(&mut hcur);
            }
            if hcur.pos == before {
                hcur.pos += 1;
            }
        }
        let data = cur.take(data_len)?.to_vec();
        parts.push(MmsPart {
            content_type: ctype,
            content_location,
            content_id,
            filename,
            charset,
            data,
        });
    }
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Value-length, charset UTF-8 (0xEA), then `text` and a NUL.
    fn utf8_value(text: &str) -> Vec<u8> {
        let mut v = vec![(text.len() + 2) as u8, 0xEA];
        v.extend_from_slice(text.as_bytes());
        v.push(0);
        v
    }

    #[test]
    fn utf8_encoded_string_keeps_non_latin_text_whole() {
        // "т" is d1 82 and "—" is e2 80 94: continuation bytes whose low bits
        // are header codes. "😀" is f0 9f 98 80 (0x98 is Transaction-Id).
        // A tab (0x09) is the From code without the high bit, so it stays text.
        let text = "Привет — café\t😀";
        let v = utf8_value(text);
        let mut cur = Cursor::new(&v);
        assert_eq!(decode_encoded_string_value(&mut cur).unwrap(), text);
        assert_eq!(cur.pos, v.len());
    }

    #[test]
    fn utf8_encoded_string_stops_at_a_real_next_header() {
        // GO SMS Pro overshoot: the value length runs past the address into the
        // Subject header (0x96) that follows it.
        let addr = b"+15555550101/TYPE=PLMN";
        let mut v = vec![(addr.len() + 4) as u8, 0xEA];
        v.extend_from_slice(addr);
        v.extend_from_slice(&[0x96, b'h', b'i']);
        let mut cur = Cursor::new(&v);
        assert_eq!(
            decode_encoded_string_value(&mut cur).unwrap(),
            "+15555550101/TYPE=PLMN"
        );
        assert_eq!(cur.data[cur.pos], 0x96);

        // The same overshoot after a value that ends in a two-byte character.
        let text = "café";
        let mut v = vec![(text.len() + 4) as u8, 0xEA];
        v.extend_from_slice(text.as_bytes());
        v.extend_from_slice(&[0x96, b'h', b'i']);
        let mut cur = Cursor::new(&v);
        assert_eq!(decode_encoded_string_value(&mut cur).unwrap(), "café");
        assert_eq!(cur.data[cur.pos], 0x96);
    }
}
