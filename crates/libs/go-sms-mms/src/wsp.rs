//! WSP binary encoding primitives (WAP-230, section 8.4) and the multipart
//! body (section 8.5).
//!
//! Every MMS PDU is a WSP header block followed by a WSP multipart body. This
//! module reads the value shapes WAP-230 defines and nothing else: there is
//! no scanning, no guessing at a boundary, and no recovery from a bad byte.
//! A value that does not have the declared shape is an [`Error`] naming the
//! offset, and the caller drops the file. The rule holds because a real
//! backup (2,004 GO SMS Pro PDUs from 2014 to 2015) decodes under it without
//! one exception; the heuristics this module replaced read JPEG bytes as
//! phone numbers.
//!
//! # Value shapes
//!
//! | Shape | Rule (WAP-230 section) |
//! |-------|------------------------|
//! | Short-integer | one byte with the high bit set; the value is the low seven bits (8.4.2.1) |
//! | Long-integer | a length byte from 1 to 30, then that many big-endian bytes (8.4.2.1) |
//! | Integer-value | Short-integer when the high bit is set, else Long-integer (8.4.2.1) |
//! | Uintvar | seven bits per byte, high bit set on every byte but the last, at most five bytes (8.1.2) |
//! | Value-length | a byte from 0 to 30 is the length; 31 means a Uintvar follows with the length (8.4.2.2) |
//! | Text-string | bytes up to a NUL; a leading 0x7f is a quote that lets the text start with a byte at or above 0x80 (8.4.2.1) |
//! | Quoted-string | a `"` byte, then text up to a NUL; the quote is not part of the value (8.4.2.1) |
//! | Encoded-string-value | Text-string, or Value-length then a charset Integer-value then Text-string (8.4.2.32) |
//! | Content-type-value | Constrained-media (a Short-integer id or a Text-string), or Value-length then the media then parameters (8.4.2.24) |
//! | Multipart | a Uintvar part count; each part is a Uintvar headers length, a Uintvar data length, the headers, the data (8.5) |

use std::collections::HashMap;
use std::fmt;

/// A decode failure: what shape was expected and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    /// Offset of the byte that broke the shape.
    pub at: usize,
    /// The shape that was being read.
    pub what: &'static str,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.what, self.at)
    }
}

impl std::error::Error for Error {}

/// The result of reading one value shape.
pub type Result<T> = std::result::Result<T, Error>;

/// A read position in a byte slice.
#[derive(Debug)]
pub struct Cursor<'a> {
    /// The bytes being read.
    pub data: &'a [u8],
    /// Offset of the next byte to read.
    pub pos: usize,
}

impl<'a> Cursor<'a> {
    /// A cursor at the start of `data`.
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Bytes left after the cursor.
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    /// The next byte without consuming it.
    pub fn peek(&self) -> Option<u8> {
        self.data.get(self.pos).copied()
    }

    fn err<T>(&self, what: &'static str) -> Result<T> {
        Err(Error { at: self.pos, what })
    }

    /// Consume and return the next byte; `what` names the shape for the error.
    pub fn next_byte(&mut self, what: &'static str) -> Result<u8> {
        let b = self.peek().ok_or(Error { at: self.pos, what })?;
        self.pos += 1;
        Ok(b)
    }

    /// Consume the next `n` bytes; `what` names the shape for the error.
    pub fn take(&mut self, n: usize, what: &'static str) -> Result<&'a [u8]> {
        if self.remaining() < n {
            return self.err(what);
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
}

/// Uintvar: seven bits per byte, high bit set on all but the last, at most
/// five bytes (WAP-230 8.1.2).
pub fn uintvar(cur: &mut Cursor<'_>) -> Result<u64> {
    let start = cur.pos;
    let mut value = 0u64;
    for _ in 0..5 {
        let byte = cur.next_byte("uintvar")?;
        value = (value << 7) | u64::from(byte & 0x7f);
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(Error {
        at: start,
        what: "uintvar longer than five bytes",
    })
}

/// Value-length: a byte from 0 to 30, or 31 followed by a Uintvar
/// (WAP-230 8.4.2.2).
pub fn value_length(cur: &mut Cursor<'_>) -> Result<usize> {
    let byte = cur.next_byte("value-length")?;
    match byte {
        0..=30 => Ok(usize::from(byte)),
        31 => Ok(uintvar(cur)? as usize),
        _ => Err(Error {
            at: cur.pos - 1,
            what: "value-length above 31",
        }),
    }
}

/// Short-integer: one byte with the high bit set (WAP-230 8.4.2.1).
pub fn short_integer(cur: &mut Cursor<'_>) -> Result<u8> {
    match cur.peek() {
        Some(b) if b & 0x80 != 0 => {
            cur.pos += 1;
            Ok(b & 0x7f)
        }
        _ => cur.err("short-integer"),
    }
}

/// Long-integer: a length byte from 1 to 30, then that many big-endian bytes
/// (WAP-230 8.4.2.1).
pub fn long_integer(cur: &mut Cursor<'_>) -> Result<u64> {
    let len = cur.next_byte("long-integer")?;
    if len == 0 || len > 30 {
        return Err(Error {
            at: cur.pos - 1,
            what: "long-integer length outside 1 to 30",
        });
    }
    let bytes = cur.take(usize::from(len), "long-integer")?;
    // Bytes past eight would overflow; the values in a PDU (dates, sizes) fit.
    Ok(bytes.iter().fold(0u64, |acc, b| (acc << 8) | u64::from(*b)))
}

/// Integer-value: Short-integer when the high bit is set, else Long-integer.
pub fn integer_value(cur: &mut Cursor<'_>) -> Result<u64> {
    match cur.peek() {
        Some(b) if b & 0x80 != 0 => short_integer(cur).map(u64::from),
        _ => long_integer(cur),
    }
}

/// Text-string or Quoted-string: bytes up to a NUL. A leading 0x7f (the
/// Text-string quote) or `"` (the Quoted-string quote) is dropped; the quote
/// is there so the text can start with a byte the decoder would otherwise
/// read as a Short-integer (WAP-230 8.4.2.1).
pub fn text_string(cur: &mut Cursor<'_>) -> Result<Vec<u8>> {
    if matches!(cur.peek(), Some(0x7f | b'"')) {
        cur.pos += 1;
    }
    let start = cur.pos;
    let Some(len) = cur.data[start..].iter().position(|b| *b == 0) else {
        return Err(Error {
            at: start,
            what: "text-string without a NUL",
        });
    };
    let text = cur.data[start..start + len].to_vec();
    cur.pos = start + len + 1;
    Ok(text)
}

/// A decoded string and the charset it declared, so the caller can turn the
/// bytes into text with the right decoder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedString {
    /// The IANA MIBenum charset, when one was declared.
    pub charset: Option<u64>,
    /// The text bytes without their NUL.
    pub bytes: Vec<u8>,
}

/// Encoded-string-value: Text-string, or Value-length then a charset
/// Integer-value then Text-string (WAP-230 8.4.2.32). The first byte tells
/// them apart: 0 to 31 is a Value-length, anything else starts text.
pub fn encoded_string(cur: &mut Cursor<'_>) -> Result<EncodedString> {
    match cur.peek() {
        Some(0..=31) => {
            let start = cur.pos;
            let len = value_length(cur)?;
            let end = cur.pos + len;
            if end > cur.data.len() {
                return Err(Error {
                    at: start,
                    what: "encoded-string length past the end",
                });
            }
            let charset = integer_value(cur)?;
            let bytes = text_string(cur)?;
            if cur.pos != end {
                return Err(Error {
                    at: start,
                    what: "encoded-string length does not match its text",
                });
            }
            Ok(EncodedString {
                charset: Some(charset),
                bytes,
            })
        }
        _ => Ok(EncodedString {
            charset: None,
            bytes: text_string(cur)?,
        }),
    }
}

/// IANA MIBenum for US-ASCII.
pub const CHARSET_US_ASCII: u64 = 3;
/// IANA MIBenum for ISO-8859-1.
pub const CHARSET_ISO_8859_1: u64 = 4;
/// IANA MIBenum for UTF-8, the charset every phone writes.
pub const CHARSET_UTF8: u64 = 106;
/// IANA MIBenum for UCS-2, big-endian 16-bit units.
pub const CHARSET_UCS2: u64 = 1000;
/// IANA MIBenum for UTF-16.
pub const CHARSET_UTF16: u64 = 1015;

/// Text from bytes in a declared charset. UTF-8 is the default: it is what
/// every phone writes, and ASCII is a subset of it. ISO-8859-1 maps each
/// byte to the code point of the same value. UCS-2 and UTF-16 are big-endian
/// 16-bit units. Bytes that do not decode become U+FFFD rather than an error,
/// because a bad byte in a body should cost one character, not the message.
pub fn decode_text(bytes: &[u8], charset: Option<u64>) -> String {
    match charset {
        Some(CHARSET_ISO_8859_1) => bytes.iter().map(|b| char::from(*b)).collect(),
        Some(CHARSET_UCS2 | CHARSET_UTF16) => {
            let (pairs, _) = bytes.as_chunks::<2>();
            let units: Vec<u16> = pairs.iter().map(|p| u16::from_be_bytes(*p)).collect();
            String::from_utf16_lossy(&units)
        }
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// Well-known content types by their assigned number (WAP-230 table 40;
/// entries 0x00 to 0x4b). A Short-integer media id indexes this table.
pub const WELL_KNOWN_CONTENT_TYPES: &[&str] = &[
    "*/*",
    "text/*",
    "text/html",
    "text/plain",
    "text/x-hdml",
    "text/x-ttml",
    "text/x-vCalendar",
    "text/x-vCard",
    "text/vnd.wap.wml",
    "text/vnd.wap.wmlscript",
    "text/vnd.wap.wta-event",
    "multipart/*",
    "multipart/mixed",
    "multipart/form-data",
    "multipart/byteranges",
    "multipart/alternative",
    "application/*",
    "application/java-vm",
    "application/x-www-form-urlencoded",
    "application/x-hdmlc",
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

/// A content type with the parameters this crate uses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContentType {
    /// The media type in lower case, such as `image/jpeg`.
    pub media: String,
    /// Parameters by their WAP-230 name: `Charset` (a MIBenum number),
    /// `Name`, `Filename`, `Start`, `Start-info`, `Type`.
    pub params: HashMap<String, String>,
}

impl ContentType {
    /// True for `multipart/*` and `application/vnd.wap.multipart.*`, the
    /// container types whose body is a WSP multipart.
    pub fn is_multipart(&self) -> bool {
        self.media.starts_with("multipart/")
            || self.media.starts_with("application/vnd.wap.multipart.")
    }

    /// The declared charset, when there is one.
    pub fn charset(&self) -> Option<u64> {
        self.params.get("Charset").and_then(|s| s.parse().ok())
    }
}

/// Constrained-media: a well-known Short-integer id, or a Text-string.
fn constrained_media(cur: &mut Cursor<'_>) -> Result<String> {
    if let Some(b) = cur.peek()
        && b & 0x80 != 0
    {
        let at = cur.pos;
        let id = short_integer(cur)?;
        return WELL_KNOWN_CONTENT_TYPES
            .get(usize::from(id))
            .map(|s| s.to_string())
            .ok_or(Error {
                at,
                what: "unassigned well-known content type",
            });
    }
    let bytes = text_string(cur)?;
    Ok(String::from_utf8_lossy(&bytes).to_ascii_lowercase())
}

/// Content-type-value (WAP-230 8.4.2.24): Constrained-media alone, or
/// Value-length, then the media (a Short-integer id, a Long-integer id, or
/// a Text-string), then parameters up to the declared end.
///
/// Parameter codes are WAP-230 table 38. Only the ones a message part uses
/// are read; at the first code this decoder does not know, the rest of the
/// parameters are dropped and the cursor moves to the declared end, since
/// an unknown parameter's value has no length prefix to skip. The value
/// length keeps the walk aligned either way.
pub fn content_type(cur: &mut Cursor<'_>) -> Result<ContentType> {
    let Some(first) = cur.peek() else {
        return cur.err("content-type");
    };
    if first > 31 {
        return Ok(ContentType {
            media: constrained_media(cur)?,
            params: HashMap::new(),
        });
    }
    let start = cur.pos;
    let len = value_length(cur)?;
    let end = cur.pos + len;
    if end > cur.data.len() {
        return Err(Error {
            at: start,
            what: "content-type length past the end",
        });
    }
    let media = match cur.peek() {
        Some(0..=30) => {
            let at = cur.pos;
            let id = long_integer(cur)?;
            WELL_KNOWN_CONTENT_TYPES
                .get(id as usize)
                .map(|s| s.to_string())
                .ok_or(Error {
                    at,
                    what: "unassigned well-known content type",
                })?
        }
        _ => constrained_media(cur)?,
    };
    let mut params = HashMap::new();
    while cur.pos < end {
        let Ok(code) = short_integer(cur) else {
            // An untyped parameter: Token-text then a text or integer value.
            let name = String::from_utf8_lossy(&text_string(cur)?).into_owned();
            let value = match cur.peek() {
                Some(b) if b & 0x80 != 0 => integer_value(cur)?.to_string(),
                _ => String::from_utf8_lossy(&text_string(cur)?).into_owned(),
            };
            params.insert(name, value);
            continue;
        };
        let (name, value) = match code {
            0x01 => ("Charset", integer_value(cur)?.to_string()),
            0x03 => ("Type", integer_value(cur)?.to_string()),
            0x09 => ("Type", constrained_media(cur)?),
            0x05 | 0x17 => ("Name", param_text(cur)?),
            0x06 | 0x18 => ("Filename", param_text(cur)?),
            0x0a | 0x19 => ("Start", param_text(cur)?),
            0x0b | 0x1a => ("Start-info", param_text(cur)?),
            _ => break,
        };
        params.insert(name.to_string(), value);
    }
    cur.pos = end;
    Ok(ContentType { media, params })
}

/// A text parameter value: Text-string, or an Encoded-string-value for the
/// 1.4 forms (`Name` 0x17, `Filename` 0x18 and the rest).
fn param_text(cur: &mut Cursor<'_>) -> Result<String> {
    let s = encoded_string(cur)?;
    Ok(decode_text(&s.bytes, s.charset))
}

/// One part of a multipart body: its content type, the header values a
/// message uses, and the data bytes exactly as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    /// The part's content type and parameters.
    pub content_type: ContentType,
    /// Content-Location (WSP header 0x0e): the part's file name.
    pub content_location: Option<String>,
    /// Content-ID (WSP header 0x40) without its angle brackets.
    pub content_id: Option<String>,
    /// The part's bytes, exactly as stored.
    pub data: Vec<u8>,
}

impl Part {
    /// The best name for the part: the content type's Filename or Name
    /// parameter, then Content-Location, then Content-ID.
    pub fn name(&self) -> Option<&str> {
        self.content_type
            .params
            .get("Filename")
            .or_else(|| self.content_type.params.get("Name"))
            .map(String::as_str)
            .or(self.content_location.as_deref())
            .or(self.content_id.as_deref())
    }
}

/// WSP well-known header codes used in part headers (WAP-230 table 39).
const WSP_CONTENT_LOCATION: u8 = 0x0e;
const WSP_CONTENT_DISPOSITION: u8 = 0x2e;
const WSP_CONTENT_ID: u8 = 0x40;

/// Multipart body (WAP-230 8.5): a Uintvar part count, then for each part a
/// Uintvar headers length, a Uintvar data length, the headers, and the
/// data. The headers start with a Content-type-value and continue with WSP
/// headers, each a Short-integer code or a Token-text name and a value.
///
/// The two lengths are the whole rule for finding a part's bytes: nothing
/// in the data is inspected to find where the part ends, which is why a
/// JPEG can hold any byte. A part count above 256 is refused as corrupt.
pub fn multipart(cur: &mut Cursor<'_>) -> Result<Vec<Part>> {
    let start = cur.pos;
    let n = uintvar(cur)?;
    if n > 256 {
        return Err(Error {
            at: start,
            what: "multipart with more than 256 parts",
        });
    }
    let mut parts = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let headers_len = uintvar(cur)? as usize;
        let data_len = uintvar(cur)? as usize;
        let headers = cur.take(headers_len, "part headers")?;
        let data = cur.take(data_len, "part data")?;
        parts.push(part_from_headers(headers, data)?);
    }
    Ok(parts)
}

/// A part from its header bytes and data.
fn part_from_headers(headers: &[u8], data: &[u8]) -> Result<Part> {
    let mut cur = Cursor::new(headers);
    let content_type = content_type(&mut cur)?;
    let mut part = Part {
        content_type,
        content_location: None,
        content_id: None,
        data: data.to_vec(),
    };
    while cur.remaining() > 0 {
        if let Ok(code) = short_integer(&mut cur) {
            match code {
                WSP_CONTENT_LOCATION => {
                    part.content_location = Some(param_text(&mut cur)?);
                }
                WSP_CONTENT_ID => {
                    let raw = String::from_utf8_lossy(&text_string(&mut cur)?).into_owned();
                    part.content_id = Some(strip_angle_brackets(&raw));
                }
                WSP_CONTENT_DISPOSITION => {
                    // Value-length, a disposition token, then parameters. The
                    // filename parameter is the only one a message uses, and
                    // the content type's own name takes precedence, so the
                    // whole value is skipped by its length.
                    let len = value_length(&mut cur)?;
                    cur.take(len, "content-disposition")?;
                }
                _ => {
                    // Another well-known header: its value shape is unknown
                    // here, and the part headers are bounded, so stop reading
                    // headers. Content type, location and id come first in
                    // every phone's encoding.
                    break;
                }
            }
        } else {
            // A Token-text header name with a Text-string value.
            let name = String::from_utf8_lossy(&text_string(&mut cur)?).into_owned();
            let value = String::from_utf8_lossy(&text_string(&mut cur)?).into_owned();
            match name.to_ascii_lowercase().as_str() {
                "content-id" => part.content_id = Some(strip_angle_brackets(&value)),
                "content-location" => part.content_location = Some(value),
                _ => {}
            }
        }
    }
    Ok(part)
}

/// `<img1>` becomes `img1`; a `cid:` prefix goes too.
pub fn strip_angle_brackets(raw: &str) -> String {
    let s = raw.trim();
    let s = s.strip_prefix("cid:").unwrap_or(s);
    let s = s.strip_prefix('<').unwrap_or(s);
    let s = s.strip_suffix('>').unwrap_or(s);
    s.to_string()
}

#[cfg(test)]
mod tests;
