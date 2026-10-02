//! MMS message decoding (WAP-209, MMS Encapsulation, section 7).
//!
//! An MMS PDU is a run of headers followed by a body. Each header is a
//! Short-integer field code (table 8 of WAP-209, with the high bit set on
//! the wire) and a value in the shape that field defines. The rules this
//! decoder holds to, each with its reason:
//!
//! 1. **The first header is X-Mms-Message-Type.** WAP-209 section 7.1
//!    requires it, and it is how a stub file is told from a message: GO SMS
//!    Pro writes a 17-byte `application/smil\0` placeholder for an MMS whose
//!    body was never downloaded, and that file has no message type.
//! 2. **Headers run until Content-Type.** Section 7.1 puts Content-Type
//!    last, and the bytes after it are the body. Nothing after it is read as
//!    a header, so image bytes can never become an address.
//! 3. **Every field code has one value shape.** The shape comes from
//!    WAP-209 table 8 and, for the codes MMS 1.2 and 1.3 add (0x22 to
//!    0x3F), from OMA MMS 1.3 section 7.3 and table 25. It is read exactly;
//!    there is no fallback shape. A code this decoder does not know is an
//!    error, because its value has no length prefix and the walk cannot stay
//!    aligned past it. The real backup uses eleven codes and this decoder
//!    knows every code the 1.0 through 1.3 tables assign, except Content
//!    (0x2E) and Additional-headers (0x30): table 25 allows those only as
//!    names inside an X-Mms-Attributes list, so as a header they have no
//!    value shape and are refused like an unknown code.
//! 4. **From is Value-length then Address-present-token (0x80) and an
//!    Encoded-string-value, or Insert-address-token (0x81) alone** (section
//!    7.2.11). The insert token is what a phone writes on a message it
//!    sends: the network fills the sender in. So a sent message has no
//!    From address, and the decoder reports `from` as `None`.
//! 5. **The body is a WSP multipart when Content-Type says so** (a
//!    `multipart/*` or `application/vnd.wap.multipart.*` media type, section
//!    7.3); otherwise the body is one part of that type. Every real MMS is a
//!    `multipart.related` or `multipart.mixed`.
//! 6. **Addresses keep their wire form.** A phone number is
//!    `+14075551234/TYPE=PLMN`; an email address has no suffix. What to do
//!    with the suffix is the caller's decision, not the decoder's.
//!
//! Message-type tokens (section 7.2.14): 0x80 m-send-req, 0x81 m-send-conf,
//! 0x82 m-notification-ind, 0x83 m-notifyresp-ind, 0x84 m-retrieve-conf,
//! 0x85 m-acknowledge-ind, 0x86 m-delivery-ind, 0x87 m-read-rec-ind, 0x88
//! m-read-orig-ind. A backup holds the first (a message the owner sent) and
//! the fifth (a message the owner received).

use crate::wsp::{
    self, ContentType, Cursor, Part, Result, decode_text, encoded_string, integer_value,
    long_integer, short_integer, text_string, value_length,
};
use std::collections::BTreeMap;

/// WAP-209 table 8 and OMA MMS 1.3 table 25 field codes, without the high
/// bit.
const BCC: u8 = 0x01;
const CC: u8 = 0x02;
const CONTENT_LOCATION: u8 = 0x03;
const CONTENT_TYPE: u8 = 0x04;
const DATE: u8 = 0x05;
const DELIVERY_REPORT: u8 = 0x06;
const DELIVERY_TIME: u8 = 0x07;
const EXPIRY: u8 = 0x08;
const FROM: u8 = 0x09;
const MESSAGE_CLASS: u8 = 0x0a;
const MESSAGE_ID: u8 = 0x0b;
const MESSAGE_TYPE: u8 = 0x0c;
const MMS_VERSION: u8 = 0x0d;
const MESSAGE_SIZE: u8 = 0x0e;
const PRIORITY: u8 = 0x0f;
const READ_REPORT: u8 = 0x10;
const REPORT_ALLOWED: u8 = 0x11;
const RESPONSE_STATUS: u8 = 0x12;
const RESPONSE_TEXT: u8 = 0x13;
const SENDER_VISIBILITY: u8 = 0x14;
const STATUS: u8 = 0x15;
const SUBJECT: u8 = 0x16;
const TO: u8 = 0x17;
const TRANSACTION_ID: u8 = 0x18;
const RETRIEVE_STATUS: u8 = 0x19;
const RETRIEVE_TEXT: u8 = 0x1a;
const READ_STATUS: u8 = 0x1b;
const REPLY_CHARGING: u8 = 0x1c;
const REPLY_CHARGING_DEADLINE: u8 = 0x1d;
const REPLY_CHARGING_ID: u8 = 0x1e;
const REPLY_CHARGING_SIZE: u8 = 0x1f;
const PREVIOUSLY_SENT_BY: u8 = 0x20;
const PREVIOUSLY_SENT_DATE: u8 = 0x21;
const STORE: u8 = 0x22;
const MM_STATE: u8 = 0x23;
const MM_FLAGS: u8 = 0x24;
const STORE_STATUS: u8 = 0x25;
const STORE_STATUS_TEXT: u8 = 0x26;
const STORED: u8 = 0x27;
const ATTRIBUTES: u8 = 0x28;
const TOTALS: u8 = 0x29;
const MBOX_TOTALS: u8 = 0x2a;
const QUOTAS: u8 = 0x2b;
const MBOX_QUOTAS: u8 = 0x2c;
const MESSAGE_COUNT: u8 = 0x2d;
const START: u8 = 0x2f;
const DISTRIBUTION_INDICATOR: u8 = 0x31;
const ELEMENT_DESCRIPTOR: u8 = 0x32;
const LIMIT: u8 = 0x33;
const RECOMMENDED_RETRIEVAL_MODE: u8 = 0x34;
const RECOMMENDED_RETRIEVAL_MODE_TEXT: u8 = 0x35;
const STATUS_TEXT: u8 = 0x36;
const APPLIC_ID: u8 = 0x37;
const REPLY_APPLIC_ID: u8 = 0x38;
const AUX_APPLIC_INFO: u8 = 0x39;
const CONTENT_CLASS: u8 = 0x3a;
const DRM_CONTENT: u8 = 0x3b;
const ADAPTATION_ALLOWED: u8 = 0x3c;
const REPLACE_ID: u8 = 0x3d;
const CANCEL_ID: u8 = 0x3e;
const CANCEL_STATUS: u8 = 0x3f;

/// The kind of MMS transaction a PDU records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    /// `m-send-req`: a message the phone sent.
    SendReq,
    /// `m-retrieve-conf`: a message the phone received.
    RetrieveConf,
    /// Any other transaction: a notification, a delivery report, and so on.
    Other(u8),
}

impl MessageType {
    /// The WAP-209 token name.
    pub fn name(self) -> String {
        match self {
            Self::SendReq => "m-send-req".to_string(),
            Self::RetrieveConf => "m-retrieve-conf".to_string(),
            Self::Other(t) => match t {
                0x81 => "m-send-conf".to_string(),
                0x82 => "m-notification-ind".to_string(),
                0x83 => "m-notifyresp-ind".to_string(),
                0x85 => "m-acknowledge-ind".to_string(),
                0x86 => "m-delivery-ind".to_string(),
                0x87 => "m-read-rec-ind".to_string(),
                0x88 => "m-read-orig-ind".to_string(),
                other => format!("0x{other:02x}"),
            },
        }
    }
}

/// One decoded MMS: the headers a message uses, every other header as
/// text under its name, and the body parts.
#[derive(Debug, Clone, Default)]
pub struct Message {
    /// The transaction kind, from the first header.
    pub message_type: Option<MessageType>,
    /// The sender address as written, or `None` when the PDU carries the
    /// Insert-address-token (a sent message).
    pub from: Option<String>,
    /// To addresses as written.
    pub to: Vec<String>,
    /// Cc addresses as written.
    pub cc: Vec<String>,
    /// Bcc addresses as written.
    pub bcc: Vec<String>,
    /// Date header: seconds since the Unix epoch.
    pub date: Option<u64>,
    /// Subject header as text.
    pub subject: Option<String>,
    /// The body's content type, the last header.
    pub content_type: Option<ContentType>,
    /// Every other header, decoded to text and keyed by its WAP-209 name
    /// in lower case with dashes, such as `message-id` and `priority`.
    pub headers: BTreeMap<String, String>,
    /// The body parts, in wire order.
    pub parts: Vec<Part>,
}

/// True when the bytes begin with an X-Mms-Message-Type header (rule 1).
pub fn starts_with_message_type(data: &[u8]) -> bool {
    data.first() == Some(&(MESSAGE_TYPE | 0x80))
}

/// Decode one MMS PDU.
///
/// # Errors
///
/// Returns the first byte that breaks a rule and which shape it broke.
pub fn decode(data: &[u8]) -> Result<Message> {
    let mut cur = Cursor::new(data);
    let mut msg = Message::default();
    if !starts_with_message_type(data) {
        return Err(wsp::Error {
            at: 0,
            what: "X-Mms-Message-Type header",
        });
    }
    loop {
        let at = cur.pos;
        let code = short_integer(&mut cur).map_err(|_| wsp::Error {
            at,
            what: "header field code",
        })?;
        if code == CONTENT_TYPE {
            let ct = wsp::content_type(&mut cur)?;
            msg.parts = if ct.is_multipart() {
                wsp::multipart(&mut cur)?
            } else {
                vec![Part {
                    content_type: ct.clone(),
                    content_location: None,
                    content_id: None,
                    data: data[cur.pos..].to_vec(),
                }]
            };
            msg.content_type = Some(ct);
            return Ok(msg);
        }
        header(code, &mut cur, &mut msg)?;
    }
}

/// Decode and store the header with `code` (rule 3).
fn header(code: u8, cur: &mut Cursor<'_>, msg: &mut Message) -> Result<()> {
    match code {
        MESSAGE_TYPE => {
            let at = cur.pos;
            let t = short_integer(cur).map_err(|_| wsp::Error {
                at,
                what: "message-type token",
            })?;
            msg.message_type = Some(match t {
                0x00 => MessageType::SendReq,
                0x04 => MessageType::RetrieveConf,
                other => MessageType::Other(other | 0x80),
            });
            return Ok(());
        }
        FROM => {
            msg.from = from_value(cur)?;
            return Ok(());
        }
        TO => {
            msg.to.push(string(cur)?);
            return Ok(());
        }
        CC => {
            msg.cc.push(string(cur)?);
            return Ok(());
        }
        BCC => {
            msg.bcc.push(string(cur)?);
            return Ok(());
        }
        DATE => {
            msg.date = Some(long_integer(cur)?);
            return Ok(());
        }
        SUBJECT => {
            msg.subject = Some(string(cur)?);
            return Ok(());
        }
        _ => {}
    };
    let (name, value): (&str, String) = match code {
        TRANSACTION_ID => ("transaction-id", string(cur)?),
        MESSAGE_ID => ("message-id", string(cur)?),
        CONTENT_LOCATION => ("content-location", string(cur)?),
        RESPONSE_TEXT => ("response-text", string(cur)?),
        RETRIEVE_TEXT => ("retrieve-text", string(cur)?),
        REPLY_CHARGING_ID => ("reply-charging-id", string(cur)?),
        MMS_VERSION => {
            let v = short_integer(cur)?;
            ("mms-version", format!("{}.{}", v >> 4, v & 0x0f))
        }
        MESSAGE_SIZE => ("message-size", long_integer(cur)?.to_string()),
        REPLY_CHARGING_SIZE => ("reply-charging-size", long_integer(cur)?.to_string()),
        PREVIOUSLY_SENT_BY => ("previously-sent-by", previously_sent(cur, false)?),
        PREVIOUSLY_SENT_DATE => ("previously-sent-date", previously_sent(cur, true)?),
        DELIVERY_TIME => ("delivery-time", absolute_or_relative(cur)?),
        EXPIRY => ("expiry", absolute_or_relative(cur)?),
        REPLY_CHARGING_DEADLINE => ("reply-charging-deadline", absolute_or_relative(cur)?),
        MESSAGE_CLASS => ("message-class", message_class(cur)?),
        DELIVERY_REPORT => ("delivery-report", yes_no(cur)?),
        READ_REPORT => ("read-report", yes_no(cur)?),
        REPORT_ALLOWED => ("report-allowed", yes_no(cur)?),
        PRIORITY => ("priority", token(cur, &["Low", "Normal", "High"])?),
        SENDER_VISIBILITY => ("sender-visibility", token(cur, &["Hide", "Show"])?),
        STATUS => (
            "status",
            token(
                cur,
                &[
                    "Expired",
                    "Retrieved",
                    "Rejected",
                    "Deferred",
                    "Unrecognized",
                    "Indeterminate",
                    "Forwarded",
                    "Unreachable",
                ],
            )?,
        ),
        RESPONSE_STATUS => ("response-status", short_integer(cur)?.to_string()),
        RETRIEVE_STATUS => ("retrieve-status", short_integer(cur)?.to_string()),
        READ_STATUS => (
            "read-status",
            token(cur, &["Read", "Deleted without being read"])?,
        ),
        REPLY_CHARGING => (
            "reply-charging",
            token(
                cur,
                &[
                    "Requested",
                    "Requested text only",
                    "Accepted",
                    "Accepted text only",
                ],
            )?,
        ),
        STORE => ("store", yes_no(cur)?),
        STORED => ("stored", yes_no(cur)?),
        TOTALS => ("totals", yes_no(cur)?),
        QUOTAS => ("quotas", yes_no(cur)?),
        DISTRIBUTION_INDICATOR => ("distribution-indicator", yes_no(cur)?),
        DRM_CONTENT => ("drm-content", yes_no(cur)?),
        ADAPTATION_ALLOWED => ("adaptation-allowed", yes_no(cur)?),
        MM_STATE => (
            "mm-state",
            token(cur, &["Draft", "Sent", "New", "Retrieved", "Forwarded"])?,
        ),
        MM_FLAGS => ("mm-flags", mm_flags(cur)?),
        STORE_STATUS => ("store-status", short_integer(cur)?.to_string()),
        STORE_STATUS_TEXT => ("store-status-text", string(cur)?),
        ATTRIBUTES => ("attributes", format!("0x{:02x}", short_integer(cur)?)),
        MBOX_TOTALS => ("mbox-totals", mbox_count(cur)?),
        MBOX_QUOTAS => ("mbox-quotas", mbox_count(cur)?),
        MESSAGE_COUNT => ("message-count", integer_value(cur)?.to_string()),
        START => ("start", integer_value(cur)?.to_string()),
        LIMIT => ("limit", integer_value(cur)?.to_string()),
        ELEMENT_DESCRIPTOR => ("element-descriptor", element_descriptor(cur)?),
        RECOMMENDED_RETRIEVAL_MODE => ("recommended-retrieval-mode", token(cur, &["Manual"])?),
        RECOMMENDED_RETRIEVAL_MODE_TEXT => ("recommended-retrieval-mode-text", string(cur)?),
        STATUS_TEXT => ("status-text", string(cur)?),
        APPLIC_ID => ("applic-id", string(cur)?),
        REPLY_APPLIC_ID => ("reply-applic-id", string(cur)?),
        AUX_APPLIC_INFO => ("aux-applic-info", string(cur)?),
        REPLACE_ID => ("replace-id", string(cur)?),
        CANCEL_ID => ("cancel-id", string(cur)?),
        CONTENT_CLASS => (
            "content-class",
            token(
                cur,
                &[
                    "text",
                    "image-basic",
                    "image-rich",
                    "video-basic",
                    "video-rich",
                    "megapixel",
                    "content-basic",
                    "content-rich",
                ],
            )?,
        ),
        CANCEL_STATUS => (
            "cancel-status",
            token(
                cur,
                &[
                    "Cancel Request Successfully received",
                    "Cancel Request corrupted",
                ],
            )?,
        ),
        _ => {
            return Err(wsp::Error {
                at: cur.pos - 1,
                what: "unknown header field code",
            });
        }
    };
    msg.headers.insert(name.to_string(), value);
    Ok(())
}

/// An Encoded-string-value as text.
fn string(cur: &mut Cursor<'_>) -> Result<String> {
    let s = encoded_string(cur)?;
    Ok(decode_text(&s.bytes, s.charset))
}

/// From-value (rule 4): `None` for the Insert-address-token.
fn from_value(cur: &mut Cursor<'_>) -> Result<Option<String>> {
    let start = cur.pos;
    let len = value_length(cur)?;
    let end = cur.pos + len;
    if end > cur.data.len() {
        return Err(wsp::Error {
            at: start,
            what: "from length past the end",
        });
    }
    let token = cur.next_byte("from token")?;
    let from = match token {
        0x81 => None,
        0x80 => Some(string(cur)?),
        _ => {
            return Err(wsp::Error {
                at: cur.pos - 1,
                what: "from address token",
            });
        }
    };
    if cur.pos != end {
        return Err(wsp::Error {
            at: start,
            what: "from length does not match its address",
        });
    }
    Ok(from)
}

/// Expiry-value and Delivery-time-value: Value-length, then Absolute-token
/// (0x80) and a Date-value or Relative-token (0x81) and Delta-seconds.
fn absolute_or_relative(cur: &mut Cursor<'_>) -> Result<String> {
    let start = cur.pos;
    let len = value_length(cur)?;
    let end = cur.pos + len;
    let token = cur.next_byte("time token")?;
    let seconds = integer_value(cur)?;
    if cur.pos != end {
        return Err(wsp::Error {
            at: start,
            what: "time value length does not match",
        });
    }
    Ok(match token {
        0x80 => format!("absolute:{seconds}"),
        0x81 => format!("relative:{seconds}"),
        _ => {
            return Err(wsp::Error {
                at: start + 1,
                what: "time token",
            });
        }
    })
}

/// Previously-sent-by-value and Previously-sent-date-value (WAP-209 1.2,
/// sections 7.2.32 and 7.2.33): Value-length, a forwarded count as an
/// Integer-value, then an Encoded-string-value address or a Date-value.
/// Decoded as `<count>:<address or seconds>`.
fn previously_sent(cur: &mut Cursor<'_>, is_date: bool) -> Result<String> {
    let start = cur.pos;
    let len = value_length(cur)?;
    let end = cur.pos + len;
    let count = integer_value(cur)?;
    let value = if is_date {
        long_integer(cur)?.to_string()
    } else {
        string(cur)?
    };
    if cur.pos != end {
        return Err(wsp::Error {
            at: start,
            what: "previously-sent length does not match",
        });
    }
    Ok(format!("{count}:{value}"))
}

/// MM-flags-value (OMA MMS 1.3 section 7.3.32): Value-length, then
/// Add-token (0x80), Remove-token (0x81) or Filter-token (0x82), then an
/// Encoded-string-value keyword. Decoded as `<token>:<keyword>`.
fn mm_flags(cur: &mut Cursor<'_>) -> Result<String> {
    let start = cur.pos;
    let len = value_length(cur)?;
    let end = cur.pos + len;
    let action = token(cur, &["Add", "Remove", "Filter"])?;
    let keyword = string(cur)?;
    if cur.pos != end {
        return Err(wsp::Error {
            at: start,
            what: "mm-flags length does not match",
        });
    }
    Ok(format!("{action}:{keyword}"))
}

/// Mbox-totals-value and Mbox-quotas-value (OMA MMS 1.3 sections 7.3.25
/// and 7.3.26): Value-length, a Message (0x80) or Size (0x81) token, then
/// an Integer-value. Decoded as `<token>:<count>`.
fn mbox_count(cur: &mut Cursor<'_>) -> Result<String> {
    let start = cur.pos;
    let len = value_length(cur)?;
    let end = cur.pos + len;
    let kind = token(cur, &["Message", "Size"])?;
    let count = integer_value(cur)?;
    if cur.pos != end {
        return Err(wsp::Error {
            at: start,
            what: "mbox count length does not match",
        });
    }
    Ok(format!("{kind}:{count}"))
}

/// Element-Descriptor-value (OMA MMS 1.3 section 7.3.18): Value-length, a
/// Text-string content reference, then parameters. Decoded as the content
/// reference; the parameters are skipped by the length.
fn element_descriptor(cur: &mut Cursor<'_>) -> Result<String> {
    let start = cur.pos;
    let len = value_length(cur)?;
    let end = cur.pos + len;
    if end > cur.data.len() {
        return Err(wsp::Error {
            at: start,
            what: "element-descriptor length past the end",
        });
    }
    let reference = String::from_utf8_lossy(&text_string(cur)?).into_owned();
    if cur.pos > end {
        return Err(wsp::Error {
            at: start,
            what: "element-descriptor length does not match",
        });
    }
    cur.pos = end;
    Ok(reference)
}

/// Message-class-value: a well-known token or a Token-text.
fn message_class(cur: &mut Cursor<'_>) -> Result<String> {
    match cur.peek() {
        Some(b) if b & 0x80 != 0 => {
            token(cur, &["Personal", "Advertisement", "Informational", "Auto"])
        }
        _ => Ok(String::from_utf8_lossy(&text_string(cur)?).into_owned()),
    }
}

/// A Short-integer token whose names are listed in order from 0.
fn token(cur: &mut Cursor<'_>, names: &[&str]) -> Result<String> {
    let v = short_integer(cur)?;
    Ok(names
        .get(usize::from(v))
        .map_or_else(|| format!("0x{v:02x}"), |n| n.to_string()))
}

/// Yes (0x80) or No (0x81) on the wire.
fn yes_no(cur: &mut Cursor<'_>) -> Result<String> {
    token(cur, &["yes", "no"])
}

#[cfg(test)]
mod tests;
