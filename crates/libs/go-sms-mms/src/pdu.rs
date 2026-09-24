//! A GO SMS Pro `.pdu` file as one message: who sent it, who received it,
//! its text, and its attachments.
//!
//! GO SMS Pro's backup folder holds one file per MMS beside the SMS XML,
//! named `I_<unix seconds>_<n>_<m>.pdu` for a received message and
//! `S_<unix seconds>_<n>_<m>.pdu` for a sent one. The bytes are the MMS PDU
//! the phone's MMS stack held, unchanged, so they decode by the WAP-209
//! rules in [`crate::mms`]. The rules this module adds on top:
//!
//! 1. **Direction is the message type.** `m-send-req` is sent and
//!    `m-retrieve-conf` is received. The file name prefix says the same
//!    thing and is not consulted. Any other message type is an error: a
//!    notification or a delivery report is not a message.
//! 2. **The sender is the From header's phone number**, and the recipients
//!    are To, Cc and Bcc in that order, each number once. A sent message has
//!    no From (the phone writes the Insert-address-token), so its sender is
//!    `None` and the caller knows the owner sent it. Only the digits are
//!    kept: `+14075551234/TYPE=PLMN` becomes `14075551234`, and an address
//!    with no digits (an email) is dropped.
//! 3. **The time is the Date header**, and the file name's seconds when
//!    the header is absent. Every real PDU has the header.
//! 4. **The body is the `text/plain` parts joined with a newline**, in wire
//!    order, with GO SMS Pro's `+g<hex>` emoji escapes decoded. When there
//!    is no text part, the Subject is the body, so a photo sent with a
//!    subject line keeps its words. SMIL layout parts are not text and are
//!    never shown.
//! 5. **Every part that is not text and not SMIL is an attachment**, with
//!    its content type and the name the part headers give it. Nothing is
//!    dropped for being small or having an unexpected type.
//! 6. **A stub is a file that does not start with a message type.** GO SMS
//!    Pro writes a 17-byte `application/smil\0` file for an MMS it never
//!    downloaded, and that is a stub, not an error.

use crate::emoji::decode_gosms_emojis;
use crate::mms::{self, MessageType};
use crate::wsp::{Part, decode_text};
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

/// One attachment: the part's bytes as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedAttachment {
    /// The part's media type in lower case, such as `image/jpeg`.
    pub content_type: String,
    /// The part's name from its headers, when it has one.
    pub name: Option<String>,
    /// The part's bytes, exactly as stored.
    pub data: Vec<u8>,
}

/// One decoded PDU file.
#[derive(Debug, Clone)]
pub struct ParsedPdu {
    /// The file the message came from.
    pub path: PathBuf,
    /// Message time in Unix seconds (rule 3).
    pub timestamp: i64,
    /// True for a message the owner sent (rule 1).
    pub is_sent: bool,
    /// Digits of the sender, absent on a sent message (rule 2).
    pub sender: Option<String>,
    /// Digits of every recipient, once each, in wire order (rule 2).
    pub recipients: Vec<String>,
    /// The message text (rule 4).
    pub body: String,
    /// The attachments (rule 5).
    pub attachments: Vec<ParsedAttachment>,
    /// The other headers by name: `message-id`, `priority`, `subject`, and
    /// so on, for the export's vendor fields.
    pub fields: BTreeMap<String, String>,
}

/// Why a file did not become a message.
#[derive(Debug)]
pub enum PduError {
    /// The file could not be read.
    Read(std::io::Error),
    /// A placeholder GO SMS Pro wrote for an MMS it never downloaded (rule 6).
    Stub,
    /// The bytes break a WAP-209 or WSP rule.
    Malformed(crate::wsp::Error),
    /// A transaction that is not a message, such as a delivery report.
    NotAMessage(String),
}

impl fmt::Display for PduError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(e) => write!(f, "read: {e}"),
            Self::Stub => write!(f, "stub PDU with no message"),
            Self::Malformed(e) => write!(f, "malformed PDU: expected {e}"),
            Self::NotAMessage(t) => write!(f, "{t} is not a message"),
        }
    }
}

impl std::error::Error for PduError {}

/// Read and decode one PDU file.
///
/// # Errors
///
/// See [`PduError`].
pub fn parse_pdu_file(path: &Path) -> Result<ParsedPdu, PduError> {
    let data = std::fs::read(path).map_err(PduError::Read)?;
    parse_pdu_bytes(path, &data)
}

/// Decode the bytes of the PDU file at `path`. The path is kept on the
/// result and its name is the time of last resort (rule 3).
///
/// # Errors
///
/// See [`PduError`].
pub fn parse_pdu_bytes(path: &Path, data: &[u8]) -> Result<ParsedPdu, PduError> {
    if !mms::starts_with_message_type(data) {
        return Err(PduError::Stub);
    }
    let msg = mms::decode(data).map_err(PduError::Malformed)?;
    let is_sent = match msg.message_type {
        Some(MessageType::SendReq) => true,
        Some(MessageType::RetrieveConf) => false,
        Some(other) => return Err(PduError::NotAMessage(other.name())),
        None => return Err(PduError::NotAMessage("a PDU without a type".to_string())),
    };
    let sender = msg.from.as_deref().and_then(address_digits);
    let mut recipients: Vec<String> = Vec::new();
    for addr in msg.to.iter().chain(&msg.cc).chain(&msg.bcc) {
        if let Some(d) = address_digits(addr)
            && !recipients.contains(&d)
        {
            recipients.push(d);
        }
    }
    let timestamp = msg
        .date
        .and_then(|d| i64::try_from(d).ok())
        .or_else(|| timestamp_from_filename(path))
        .unwrap_or(0);

    let mut texts: Vec<String> = Vec::new();
    let mut attachments = Vec::new();
    for part in &msg.parts {
        let media = part.content_type.media.as_str();
        if media == "text/plain" {
            let text = decode_text(&part.data, part.content_type.charset());
            let text = text.trim_matches('\0').trim().to_string();
            if !text.is_empty() {
                texts.push(decode_gosms_emojis(&text));
            }
        } else if media != "application/smil" {
            attachments.push(attachment(part));
        }
    }
    let body = if texts.is_empty() {
        msg.subject
            .as_deref()
            .map(|s| decode_gosms_emojis(s.trim()))
            .unwrap_or_default()
    } else {
        texts.join("\n")
    };

    let mut fields = msg.headers;
    if let Some(s) = &msg.subject {
        fields.insert("subject".to_string(), s.clone());
    }
    if let Some(t) = msg.message_type {
        fields.insert("message-type".to_string(), t.name());
    }
    if let Some(ct) = &msg.content_type {
        fields.insert("content-type".to_string(), ct.media.clone());
    }

    Ok(ParsedPdu {
        path: path.to_path_buf(),
        timestamp,
        is_sent,
        sender,
        recipients,
        body,
        attachments,
        fields,
    })
}

/// The digits of a phone address: `+14075551234/TYPE=PLMN` gives
/// `14075551234`. `None` when there are none, which is what an email
/// address gives.
fn address_digits(addr: &str) -> Option<String> {
    let base = addr.split('/').next().unwrap_or(addr);
    let digits: String = base.chars().filter(char::is_ascii_digit).collect();
    (!digits.is_empty()).then_some(digits)
}

/// The seconds in `I_<seconds>_...` or `S_<seconds>_...`.
fn timestamp_from_filename(path: &Path) -> Option<i64> {
    let name = path.file_name()?.to_str()?;
    let rest = name
        .strip_prefix("I_")
        .or_else(|| name.strip_prefix("S_"))?;
    rest.split('_').next()?.parse().ok()
}

fn attachment(part: &Part) -> ParsedAttachment {
    ParsedAttachment {
        content_type: part.content_type.media.clone(),
        name: part.name().map(str::to_string),
        data: part.data.clone(),
    }
}

#[cfg(test)]
mod tests;
