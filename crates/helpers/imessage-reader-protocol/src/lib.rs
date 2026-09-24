//! The wire protocol between the desktop app and the `imessage-reader` helper.
//!
//! The helper is a separate program because it links `imessage-database` and
//! `crabapple`, which are GPL-3.0-or-later, while the app is under the Fair
//! Core License. The two talk over pipes: the app writes one [`Request`] per
//! line on the helper's stdin and reads one [`Event`] per line from its
//! stdout. Every line is a JSON object, and every enum is tagged (`op` for
//! requests, `event` for events, `kind` for attachment sources), so a reader
//! that meets an unknown tag can say so instead of guessing.
//!
//! A session runs in this order:
//!
//! 1. The app sends [`Request::Export`] or [`Request::Identities`].
//! 2. The helper answers [`Event::Source`] once, then streams [`Event::Log`],
//!    [`Event::Progress`], [`Event::Conversation`] and [`Event::Message`]
//!    lines in any order, then
//!    [`Event::ExportDone`] (or [`Event::Identities`] for the identities
//!    request). [`Event::Error`] ends the request instead when it fails.
//! 3. For an encrypted backup the app may then send any number of
//!    [`Request::Attachment`] lines; the helper answers each with one
//!    [`Event::Attachment`].
//! 4. The app closes the helper's stdin, and the helper exits.
//!
//! This crate carries the type definitions, their serde shapes, and one rule:
//! [`bare_address`], which says what an owner address looks like on the wire.
//! It is MIT OR Apache-2.0 so that both sides can link it.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Bumped when a change to these types or to the order of a session would
/// make an older helper and a newer app (or the reverse) misread each other.
/// The helper sends it in [`Event::Source`] and the app refuses any other
/// number, and any answer that comes before it.
///
/// 2: the identities request answers [`Event::Source`] first, as an export
/// does.
/// 3: [`Event::Identities`] values and [`Message::owner_handle`] are bare
/// addresses ([`bare_address`]); the app no longer strips prefixes itself.
pub const PROTOCOL_VERSION: u32 = 3;

/// The owner address behind a raw `chat.account_login` or
/// `message.destination_caller_id` value, or `None` when nothing is left.
///
/// Apple stores the owner's addresses three ways: `P:+15550001111` and
/// `E:owner@example.com` in `chat.account_login`, and bare or as
/// `tel:+15550001111` in `message.destination_caller_id`. The helper puts
/// every one through this function before it crosses the pipe, so an
/// identity in [`Event::Identities`] and the owner of a [`Message`] are
/// spelled the same way and the app can compare them as text. Real backups
/// hold `account_login` rows that are the bare prefix `E:` with nothing after
/// it, so the emptiness test runs on the remainder.
#[must_use]
pub fn bare_address(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let stripped = trimmed
        .strip_prefix("P:")
        .or_else(|| trimmed.strip_prefix("E:"))
        .or_else(|| trimmed.strip_prefix("tel:"))
        .unwrap_or(trimmed)
        .trim();
    if stripped.is_empty() {
        None
    } else {
        Some(stripped.to_string())
    }
}

/// The file name of the helper executable, without the `.exe` Windows adds.
pub const HELPER_NAME: &str = "imessage-reader";

/// Which Messages layout the source has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    /// A `chat.db` file, as on a Mac or copied off a jailbroken iPhone.
    MacOs,
    /// An iPhone backup folder holding `Manifest.plist`.
    Ios,
}

/// Where the Messages data is and how to open it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    /// The `chat.db` file (macOS) or the backup folder (iOS).
    pub db_path: PathBuf,
    /// Which layout `db_path` has.
    pub platform: Platform,
    /// The backup password, for an encrypted iOS backup.
    pub backup_password: Option<String>,
}

/// One request line on the helper's stdin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// Read every message and stream it back as [`Event`] lines.
    Export(ExportRequest),
    /// Report the raw addresses the backup's device sent from.
    Identities(Source),
    /// Decrypt one attachment of the export just streamed into a file the
    /// app can read. Only meaningful after [`Event::Source`] reported
    /// `encrypted: true`; a plain file needs no help.
    Attachment {
        /// The `path` an [`AttachmentSource::Path`] reported.
        path: PathBuf,
    },
}

/// What an export run needs beyond the source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportRequest {
    /// The Messages data to read.
    pub source: Source,
    /// A folder attachments live under, when the database's own paths do
    /// not apply (a jailbroken phone's `sms.db` copied beside its files).
    pub attachment_root: Option<String>,
    /// An Apple Contacts database to take names from. macOS only.
    pub contacts_path: Option<PathBuf>,
    /// Name the owner by the destination caller id instead of `Me`.
    pub use_caller_id: bool,
    /// A folder the helper may write decrypted files into. The app owns it
    /// and deletes it when the run ends, so a killed helper leaves nothing
    /// behind. The system temp folder is used when this is `None`.
    pub scratch_dir: Option<PathBuf>,
}

/// One event line on the helper's stdout.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    /// The source opened. Sent once, before any message.
    Source {
        /// The helper's [`PROTOCOL_VERSION`].
        protocol_version: u32,
        /// Whether attachment paths need [`Request::Attachment`] to read.
        encrypted: bool,
    },
    /// A progress or warning line for the app's log.
    Log {
        /// The line, without a trailing newline.
        line: String,
    },
    /// A count for the app's progress bar. Log lines are for people; nothing
    /// reads numbers back out of them.
    Progress(Progress),
    /// A conversation seen for the first time. Always precedes the first
    /// [`Event::Message`] that names its `chat_identifier`.
    Conversation(Conversation),
    /// One message. Boxed because it is several times the size of any other
    /// event, and events are passed around by value.
    Message(Box<Message>),
    /// The export stream ended.
    ExportDone {
        /// Rows read from the database, repeats included.
        messages_seen: u64,
        /// Rows that could not be converted and were skipped.
        failures: u64,
    },
    /// The answer to [`Request::Identities`].
    Identities {
        /// The distinct `chat.account_login` and
        /// `message.destination_caller_id` values, each through
        /// [`bare_address`]: prefixes stripped, blanks dropped. One address
        /// can still appear more than once when the columns spell it
        /// differently; the app deduplicates.
        values: Vec<String>,
    },
    /// The answer to [`Request::Attachment`].
    Attachment {
        /// The decrypted file, or `None` when the backup does not hold it.
        path: Option<PathBuf>,
    },
    /// The request failed. The helper writes nothing after this line.
    Error {
        /// A sentence for the person, in the app's own words where the
        /// helper knows them.
        message: String,
    },
}

/// Where the reader is, as counts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum Progress {
    /// A numbered setup step before any message is read: deriving backup
    /// keys, decrypting a database, caching a table.
    Setup {
        /// What the step does, for the bar's caption.
        label: String,
        /// This step's number, from 1.
        step: u64,
        /// How many setup steps there are.
        total: u64,
    },
    /// Rows read so far, out of the rows the database holds.
    Parse {
        /// Rows read.
        done: u64,
        /// Rows in total.
        total: u64,
    },
}

/// A conversation's roster and shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    /// Apple's `chat.chat_identifier`, or `orphaned` for messages whose chat
    /// row is gone.
    pub chat_identifier: String,
    /// `individual` or `group`.
    pub conversation_type: String,
    /// The name a person gave the group, if any.
    pub group_title: Option<String>,
    /// Everyone in the chat other than the owner.
    pub participants: Vec<Participant>,
}

/// One person in a conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Participant {
    /// Phone number or email address as Messages stores it.
    pub handle: String,
    /// The contact name, when the address book knows one.
    pub display_name: Option<String>,
}

/// One message row, already classified.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// The conversation this row belongs to.
    pub chat_identifier: String,
    /// Apple's message GUID.
    pub guid: String,
    /// Sent time as milliseconds since 1970-01-01 UTC.
    pub timestamp_unix_ms: i64,
    /// `true` for a message the owner sent.
    pub outgoing: bool,
    /// `iMessage`, `SMS`, `RCS`, or empty when unknown.
    pub service: String,
    /// `imessage`, `sms`, `mms`, `tapback`, `sticker_tapback`,
    /// `announcement`, `location_share`, or `balloon`.
    pub message_kind: String,
    /// The sender's address, for an incoming message.
    pub sender_handle: Option<String>,
    /// The sender's contact name, for an incoming message.
    pub sender_display_name: Option<String>,
    /// The subject line, when the message has one.
    pub subject: Option<String>,
    /// The body, or the sentence that stands in for a tapback or
    /// announcement.
    pub text: String,
    /// The owner's address on this row (`destination_caller_id` through
    /// [`bare_address`]), or empty when the row carries none.
    pub owner_handle: String,
    /// The owner's display name, when `use_caller_id` asked for one.
    pub owner_display_name: Option<String>,
    /// Apple-specific fields; `None` when every field is empty.
    pub imessage: Option<Imessage>,
    /// Attachments the body references, in body order.
    pub attachments: Vec<Attachment>,
}

/// Everything Apple-specific the core message fields do not carry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Imessage {
    /// A reply inside a thread.
    pub is_reply: bool,
    /// The message replied to, or reacted to.
    pub in_reply_to_guid: Option<String>,
    /// Part index within the thread originator.
    pub thread_originator_part: Option<u32>,
    /// How many replies this message has.
    pub num_replies: Option<u32>,
    /// Deleted in Messages but still in the database.
    pub is_deleted: bool,
    /// A send effect's label.
    pub send_effect: Option<String>,
    /// A shared-location label.
    pub shared_location: Option<String>,
    /// An announcement's text.
    pub announcement: Option<String>,
    /// When the message was read, RFC 3339.
    pub read_receipt_rfc3339: Option<String>,
    /// Body parts as a JSON array.
    pub parts: Option<Value>,
    /// Edit history as a JSON array.
    pub edits: Option<Value>,
    /// Reactions on this message as a JSON array.
    pub tapbacks: Option<Value>,
    /// An app balloon's payload.
    pub app: Option<Value>,
    /// The balloon's bundle id.
    pub balloon_bundle_id: Option<String>,
    /// The balloon's kind label.
    pub balloon_kind: Option<String>,
    /// For a tapback, the GUID it reacts to.
    pub associated_guid: Option<String>,
    /// For a tapback, the part index it reacts to.
    pub associated_part: Option<u32>,
    /// For a tapback, `loved`, `liked`, `emoji`, and so on.
    pub tapback_kind: Option<String>,
    /// For an emoji tapback, the emoji.
    pub tapback_emoji: Option<String>,
    /// For a tapback, `add` or `remove`.
    pub tapback_action: Option<String>,
}

/// One attachment's metadata and where its bytes are.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    /// The file name Messages transferred.
    pub original_name: Option<String>,
    /// The MIME type Messages recorded.
    pub mime_type: Option<String>,
    /// A sticker rather than a file.
    pub is_sticker: bool,
    /// Transcription of an audio message.
    pub transcription: Option<String>,
    /// The sticker's effect name.
    pub sticker_effect: Option<String>,
    /// Where the bytes are.
    pub source: AttachmentSource,
}

/// Where an attachment's bytes are.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AttachmentSource {
    /// A path the helper resolved. Readable as-is for a plain source; for an
    /// encrypted backup it names the entry to pass to [`Request::Attachment`].
    Path {
        /// The resolved path.
        path: PathBuf,
        /// The size Messages recorded, when it is known.
        size_hint: Option<u64>,
    },
    /// Text the helper rendered itself: a handwriting message as SVG.
    Inline {
        /// The rendered document.
        text: String,
    },
    /// No file to read.
    Missing,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_and_events_round_trip_with_their_tags() {
        let request = Request::Export(ExportRequest {
            source: Source {
                db_path: "/tmp/chat.db".into(),
                platform: Platform::MacOs,
                backup_password: None,
            },
            attachment_root: None,
            contacts_path: None,
            use_caller_id: true,
            scratch_dir: None,
        });
        let line = serde_json::to_string(&request).unwrap();
        assert!(line.starts_with(r#"{"op":"export""#), "{line}");
        let back: Request = serde_json::from_str(&line).unwrap();
        assert!(matches!(back, Request::Export(_)));

        let event = Event::Attachment { path: None };
        let line = serde_json::to_string(&event).unwrap();
        assert_eq!(line, r#"{"event":"attachment","path":null}"#);

        let source = AttachmentSource::Inline {
            text: "<svg/>".into(),
        };
        let line = serde_json::to_string(&source).unwrap();
        assert_eq!(line, r#"{"kind":"inline","text":"<svg/>"}"#);
    }

    #[test]
    fn bare_address_strips_each_prefix_and_drops_what_is_then_empty() {
        assert_eq!(
            bare_address("P:+15550001111").as_deref(),
            Some("+15550001111")
        );
        assert_eq!(
            bare_address("E:owner@example.com").as_deref(),
            Some("owner@example.com")
        );
        assert_eq!(
            bare_address("tel:+15550001111").as_deref(),
            Some("+15550001111")
        );
        assert_eq!(
            bare_address(" +15550001111 ").as_deref(),
            Some("+15550001111")
        );
        assert_eq!(bare_address("E:"), None);
        assert_eq!(bare_address("tel: "), None);
        assert_eq!(bare_address("  "), None);
    }
}
