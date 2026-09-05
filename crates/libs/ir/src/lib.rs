//! Shared conversation structure every exporter writes.
//!
//! A [`ConversationDocument`] is the in-memory form of one chat: export
//! metadata, participants, and messages. Backup converters parse vendor
//! formats into this type. Writing files (JSON, CSV, EML, and so on) lives
//! in `message-ir-format`. Converting an existing export directory lives in
//! `message-reexport`. See the [common message](https://bitrealm.io/vault/developer/architecture/common-message/) page.
//!
//! Converters stage parsed rows in [`PendingMessage`] and
//! [`PendingConversation`] (with per-converter metadata in their `extra`
//! maps) before building a [`ConversationDocument`].

#![warn(missing_docs)]

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

mod projection;
#[cfg(feature = "testutil")]
pub mod testutil;

pub use projection::{
    ProjectedRole, ProjectionHooks, ProjectionTally, SortKeyUnit, default_participants,
    display_names_for_handles, ensure_conversation, pending_to_document, prepare_conversation,
};

/// Schema version written into every [`ConversationDocument`] (currently 3).
pub const SCHEMA_VERSION: u32 = 4;

/// `PendingConversation::extra` key marking a chat keyed by a person's name
/// rather than an address.
///
/// The rescue exporters (iMazing, OpenExtract, SMS Backup+) read formats that
/// sometimes identify the other party by name alone. They set this so the
/// projection emits a participant carrying the name and no identity, instead
/// of promoting the name stem into the handle field. The vault resolves the
/// name against contacts on import.
pub const CHAT_ID_IS_NAME: &str = "chat_id_is_name";
/// One exported chat: export metadata, conversation roster and stats, and messages.
///
/// This is the common-message schema every exporter writes and every reader
/// parses. See the [common message](https://bitrealm.io/vault/developer/architecture/common-message/) page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationDocument {
    /// Schema version written into this document (currently 3).
    pub schema_version: u32,
    /// Where and how this export was produced.
    pub export: ExportMeta,
    /// Roster and computed stats for this chat.
    pub conversation: ConversationMeta,
    /// Messages in timestamp order.
    pub messages: Vec<IrMessage>,
    /// On-disk stem suffix (e.g. `__whatsapp`). Never written into JSON or JSON Lines files.
    #[serde(skip)]
    pub packaging_stem_suffix: Option<String>,
}

/// Provenance of an export: which backup tool and account it came from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportMeta {
    /// Backup source id (e.g. `sms-backup-restore`).
    pub source: String,
    /// Human tool name (e.g. `SMS Backup & Restore`).
    pub tool: String,
    /// Version string of the tool.
    pub tool_version: String,
    /// Owner handle used for outgoing rows; `None` when the backup has no owner identity.
    pub owner_handle: Option<String>,
    /// Outgoing display name. Set when known (iMessage caller-id or `"Me"`).
    pub owner_display_name: Option<String>,
}

/// Individual or group chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IrConversationType {
    /// One-on-one chat with a single peer.
    Individual,
    /// Chat with multiple peers.
    Group,
}

impl IrConversationType {
    /// Lowercase storage id (`individual` / `group`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Individual => "individual",
            Self::Group => "group",
        }
    }

    /// Parse a storage id; anything but `group` (case-insensitive) is `Individual`.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "group" => Self::Group,
            _ => Self::Individual,
        }
    }
}

/// Kind of a participant handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HandleType {
    /// Telephone number.
    Phone,
    /// Email address.
    Email,
    /// App username (e.g. Telegram `@user`).
    Username,
    /// Any handle that is not phone, email, or username.
    Other,
}

impl HandleType {
    /// Lowercase storage id (`phone` / `email` / `username` / `other`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Phone => "phone",
            Self::Email => "email",
            Self::Username => "username",
            Self::Other => "other",
        }
    }

    /// Parse a storage id; unknown values map to `Other`.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "phone" => Self::Phone,
            "email" => Self::Email,
            "username" => Self::Username,
            _ => Self::Other,
        }
    }
}

/// Roster and computed stats for one chat.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMeta {
    /// Stable chat id from the source (E.164, group key, or app thread id).
    pub chat_identifier: String,
    /// Individual or group.
    pub conversation_type: IrConversationType,
    /// Group display title; `None` for individuals and untitled groups.
    pub group_title: Option<String>,
    /// Roster of handles and display names.
    pub participants: Vec<IrParticipant>,
    /// Computed counts and first/last timestamps.
    pub stats: ConversationStats,
}

/// Message and attachment counts plus first and last message timestamps,
/// computed from `messages` at write time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ConversationStats {
    /// Number of messages in the chat.
    pub message_count: u64,
    /// Total attachments across all messages.
    pub attachment_count: u64,
    /// Earliest message timestamp; `None` when the chat has no messages.
    pub first_timestamp_unix_ms: Option<i64>,
    /// Latest message timestamp; `None` when the chat has no messages.
    pub last_timestamp_unix_ms: Option<i64>,
}

/// One chat member: an identity, a display name, or both.
///
/// `handle` is `None` when the source named a person without recording any
/// address for them — the rescue exporters (iMazing, OpenExtract, SMS
/// Backup+) read formats that identify the other party by name alone. Such a
/// participant always carries a `display_name`; the vault reconciles it
/// against contacts on import rather than the exporter inventing an address.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IrParticipant {
    /// Phone, email, or username string; `None` when the source recorded no
    /// address for this person.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    /// Display name shown in UIs; `None` when the source has none.
    pub display_name: Option<String>,
    /// Known kind of `handle`; `None` when the source did not record one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle_type: Option<HandleType>,
}

/// Transport a message arrived on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IrService {
    /// SMS text.
    Sms,
    /// Apple iMessage (serialized as `imessage`).
    #[serde(rename = "imessage")]
    IMessage,
    /// WhatsApp.
    Whatsapp,
    /// RCS (Android).
    Rcs,
    /// Discord.
    Discord,
    /// Signal.
    Signal,
    /// Telegram.
    Telegram,
    /// Slack.
    Slack,
    /// Unrecognized or unset service.
    Unknown,
}

impl IrService {
    /// Lowercase storage id (`sms` / `imessage` / `whatsapp` / …).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sms => "sms",
            Self::IMessage => "imessage",
            Self::Whatsapp => "whatsapp",
            Self::Rcs => "rcs",
            Self::Discord => "discord",
            Self::Signal => "signal",
            Self::Telegram => "telegram",
            Self::Slack => "slack",
            Self::Unknown => "unknown",
        }
    }

    /// Parse a storage id; unknown values map to `Unknown`.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "sms" => Self::Sms,
            "imessage" => Self::IMessage,
            "whatsapp" => Self::Whatsapp,
            "rcs" => Self::Rcs,
            "discord" => Self::Discord,
            "signal" => Self::Signal,
            "telegram" => Self::Telegram,
            "slack" => Self::Slack,
            _ => Self::Unknown,
        }
    }
}

/// Platform identity stored on `handles.service` (not per-message SMS/iMessage/RCS).
///
/// UI labels: `Phone` → "Text message", `Whatsapp` → "WhatsApp".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HandleService {
    /// Phone platform (SMS/iMessage/RCS are transports, not platforms).
    Phone,
    /// WhatsApp platform.
    Whatsapp,
}

impl HandleService {
    /// Lowercase storage id (`phone` / `whatsapp`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Phone => "phone",
            Self::Whatsapp => "whatsapp",
        }
    }

    /// Parse storage ids and common aliases from documents, UI, and import.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "whatsapp" | "wa" => Self::Whatsapp,
            // Everything else, including the phone-platform aliases (`phone`,
            // `sms`, `mms`, `imessage`, `ios`, `rcs`, `text message`), is the
            // phone platform: SMS/iMessage/RCS are transports, not platforms.
            _ => Self::Phone,
        }
    }

    /// Map a per-message transport onto a handle platform bucket.
    pub fn from_ir_service(service: IrService) -> Self {
        match service {
            IrService::Whatsapp => Self::Whatsapp,
            IrService::Sms
            | IrService::IMessage
            | IrService::Rcs
            | IrService::Discord
            | IrService::Signal
            | IrService::Telegram
            | IrService::Slack
            | IrService::Unknown => Self::Phone,
        }
    }
}

/// Shape of one message row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IrMessageKind {
    /// Plain SMS text.
    Sms,
    /// Multimedia message.
    Mms,
    /// iMessage (serialized as `imessage`).
    #[serde(rename = "imessage")]
    IMessage,
    /// iMessage tapback reaction.
    Tapback,
    /// iMessage sticker tapback.
    StickerTapback,
    /// iMessage announcement (e.g. group rename).
    Announcement,
    /// iMessage shared location.
    LocationShare,
    /// iMessage Digital Touch balloon.
    Balloon,
    /// Unrecognized or unset kind.
    Unknown,
}

impl IrMessageKind {
    /// Lowercase storage id (`sms` / `mms` / `imessage` / `tapback` / …).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sms => "sms",
            Self::Mms => "mms",
            Self::IMessage => "imessage",
            Self::Tapback => "tapback",
            Self::StickerTapback => "sticker_tapback",
            Self::Announcement => "announcement",
            Self::LocationShare => "location_share",
            Self::Balloon => "balloon",
            Self::Unknown => "unknown",
        }
    }

    /// Parse a storage id; unknown values map to `Unknown`.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "sms" => Self::Sms,
            "mms" => Self::Mms,
            "imessage" => Self::IMessage,
            "tapback" => Self::Tapback,
            "sticker_tapback" => Self::StickerTapback,
            "announcement" => Self::Announcement,
            "location_share" => Self::LocationShare,
            "balloon" => Self::Balloon,
            _ => Self::Unknown,
        }
    }
}

/// One message in a conversation: sender, body text, and attachments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IrMessage {
    /// Stable message id; derived from content when the source has no id
    /// (see message-csv's `stable_guid`).
    pub guid: String,
    /// Unix milliseconds; the chronological sort key.
    pub timestamp_unix_ms: i64,
    /// Incoming or outgoing.
    pub direction: IrDirection,
    /// Transport the message arrived on.
    pub service: IrService,
    /// Row shape.
    pub message_kind: IrMessageKind,
    /// Handle of the actual sender (the owner's handle for outgoing).
    pub sender_handle: Option<String>,
    /// Display name of the actual sender.
    pub sender_display_name: Option<String>,
    /// Message subject line (rare).
    pub subject: Option<String>,
    /// Plain-text body; never includes attachment data.
    pub text: String,
    /// Attachment metadata in order; bytes live on disk or in `bytes`.
    pub attachments: Vec<IrAttachment>,
    /// Apple extensions; `None` for non-iMessage messages.
    pub imessage: Option<IrImessage>,
    /// Vendor leftovers (Android type code and raw fields).
    pub source: Option<IrSource>,
}

/// Whether the owner sent or received the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IrDirection {
    /// Received from a peer.
    Incoming,
    /// Sent by the owner.
    Outgoing,
}

impl IrDirection {
    /// Lowercase storage id (`incoming` / `outgoing`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Incoming => "incoming",
            Self::Outgoing => "outgoing",
        }
    }
}

/// Core attachment metadata shared by the IR attachment, the CSV cell, and the
/// mail MIME layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentMeta {
    /// Relative path under `attachments/` to the staged file.
    pub path: Option<String>,
    /// Filename the sender's device had for the file.
    pub original_name: Option<String>,
    /// Detected or declared MIME type.
    pub mime_type: Option<String>,
    /// 64-hex SHA-256 of the file contents (content addressing).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest_sha256: Option<String>,
}

impl From<&IrAttachment> for AttachmentMeta {
    fn from(a: &IrAttachment) -> Self {
        Self {
            path: a.path.clone(),
            original_name: a.original_name.clone(),
            mime_type: a.mime_type.clone(),
            digest_sha256: a.digest_sha256.clone(),
        }
    }
}

/// Metadata for one attachment.
///
/// Bytes are never serialized: JSON, JSONL, and CSV carry only this metadata,
/// and the bytes live in a sidecar file under `attachments/` (or in `bytes`
/// for in-memory EML/MBOX/XML embedding).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IrAttachment {
    /// Relative path under `attachments/` to the staged file.
    pub path: Option<String>,
    /// Filename the sender's device had for the file.
    pub original_name: Option<String>,
    /// Detected or declared MIME type.
    pub mime_type: Option<String>,
    /// 64-hex SHA-256 of the file contents (content addressing).
    pub digest_sha256: Option<String>,
    /// Sticker flag.
    pub is_sticker: bool,
    /// Transcribed text of the attachment (e.g., OCR of an image or a
    /// voice-note transcript).
    pub transcription: Option<String>,
    /// iMessage sticker effect name.
    pub sticker_effect: Option<String>,
    /// On-disk / vault asset length in bytes (not file contents).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    /// None when the attachment was imported; set only when bytes were
    /// skipped, to one of a closed set: `file_missing`, `too_large`,
    /// `not_copied`, `convert_failed: <detail>`, or `unknown: <raw>`. Older
    /// exports may still carry the retired `skipped` / `embed_disabled`
    /// spellings of `not_copied`; readers keep recognizing them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missing_reason: Option<String>,
    /// In-memory bytes for EML embedding; never written to JSON.
    #[serde(skip)]
    pub bytes: Option<Vec<u8>>,
}

/// Vendor leftovers. Display names live on `sender_display_name`, not here.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IrSource {
    /// Android `type` attribute from the source (e.g. 1 = received, 2 = sent).
    pub android_type: Option<i32>,
    /// Raw vendor attributes; display names never live here.
    #[serde(default)]
    pub fields: Map<String, Value>,
}

impl IrSource {
    /// True when no vendor leftovers were recorded.
    pub fn is_empty(&self) -> bool {
        self.android_type.is_none() && self.fields.is_empty()
    }

    /// `None` when [`Self::is_empty`], else `Some(self)`.
    pub fn into_option(self) -> Option<Self> {
        if self.is_empty() { None } else { Some(self) }
    }
}

/// iMessage extensions. Nested Apple blobs remain JSON values (not strings).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IrImessage {
    /// This message is a reply to an earlier message.
    pub is_reply: bool,
    /// GUID of the message this replies to.
    pub in_reply_to_guid: Option<String>,
    /// Part index of the thread originator.
    pub thread_originator_part: Option<u32>,
    /// Number of replies under this message.
    pub num_replies: Option<u32>,
    /// Sender deleted the message.
    pub is_deleted: bool,
    /// iMessage send effect (e.g. `slam`).
    pub send_effect: Option<String>,
    /// Shared-location payload.
    pub shared_location: Option<String>,
    /// Announcement payload (e.g. group rename).
    pub announcement: Option<String>,
    /// RFC 3339 timestamp of the read receipt.
    pub read_receipt_rfc3339: Option<String>,
    /// Apple `parts` blob as a JSON value.
    pub parts: Option<Value>,
    /// Apple `edits` blob as a JSON value.
    pub edits: Option<Value>,
    /// Apple `tapbacks` blob as a JSON value.
    pub tapbacks: Option<Value>,
    /// Apple `app` blob as a JSON value.
    pub app: Option<Value>,
    /// Digital Touch balloon bundle id.
    pub balloon_bundle_id: Option<String>,
    /// Digital Touch balloon kind.
    pub balloon_kind: Option<String>,
    /// Tapback target message GUID.
    pub associated_guid: Option<String>,
    /// Tapback target part index.
    pub associated_part: Option<u32>,
    /// Tapback kind string.
    pub tapback_kind: Option<String>,
    /// Tapback emoji.
    pub tapback_emoji: Option<String>,
    /// Tapback action string.
    pub tapback_action: Option<String>,
}

impl IrImessage {
    /// True when every field is unset (`None` or `false`).
    pub fn is_empty(&self) -> bool {
        !self.is_reply
            && self.in_reply_to_guid.is_none()
            && self.thread_originator_part.is_none()
            && self.num_replies.is_none()
            && !self.is_deleted
            && self.send_effect.is_none()
            && self.shared_location.is_none()
            && self.announcement.is_none()
            && self.read_receipt_rfc3339.is_none()
            && self.parts.is_none()
            && self.edits.is_none()
            && self.tapbacks.is_none()
            && self.app.is_none()
            && self.balloon_bundle_id.is_none()
            && self.balloon_kind.is_none()
            && self.associated_guid.is_none()
            && self.associated_part.is_none()
            && self.tapback_kind.is_none()
            && self.tapback_emoji.is_none()
            && self.tapback_action.is_none()
    }

    /// `None` when [`Self::is_empty`], else `Some(self)`.
    pub fn into_option(self) -> Option<Self> {
        if self.is_empty() { None } else { Some(self) }
    }
}

impl ConversationDocument {
    /// Filename stem used for CSV, JSON, and mail folders (no extension).
    pub fn filename_stem(&self) -> String {
        let handles: Vec<String> = self
            .conversation
            .participants
            .iter()
            .filter_map(|p| p.handle.clone())
            .collect();
        conversation_stem(
            self.conversation.conversation_type.as_str(),
            &self.conversation.chat_identifier,
            self.conversation.group_title.as_deref(),
            &handles,
            self.packaging_stem_suffix.as_deref(),
        )
    }

    /// Recompute [`ConversationMeta::stats`] from `messages`.
    pub fn finalize_stats(&mut self) {
        self.conversation.stats = compute_stats(&self.messages);
    }
}

/// Max peer phones included in an untitled group filename stem.
const GROUP_FILENAME_MAX_PHONES: usize = 10;

/// A file stem with anything but letters, digits, `-`, `_`, and `+` replaced by `_`.
fn sanitize_stem(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '+' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// True for a handle that is a phone number: an optional `+` then digits.
fn is_phone_handle(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() {
        return false;
    }
    if let Some(rest) = value.strip_prefix('+') {
        !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
    } else {
        value.chars().all(|c| c.is_ascii_digit())
    }
}

/// The stem with `suffix` appended when there is one.
fn with_suffix(stem: &str, suffix: Option<&str>) -> String {
    match suffix {
        Some(s) if !s.is_empty() => format!("{stem}{s}"),
        _ => stem.to_string(),
    }
}

/// Standard per-conversation filename stem (no extension — callers append
/// `.csv`, `.jsonl`, …).
///
/// - Individual → `safe_filename(chat_id)` (+ optional suffix)
/// - Group with a real `group_title` → sanitized title
/// - Untitled group → `group_+A_+B_…` (sorted unique E.164, max 10);
///   if more than 10 peers, append `_<16 hex>` of SHA-256 over the full roster
/// - Untitled group with empty roster → `group_unknown` (or hash of `chat_id`)
pub fn conversation_stem(
    conversation_type: &str,
    chat_id: &str,
    group_title: Option<&str>,
    participant_e164s: &[String],
    suffix: Option<&str>,
) -> String {
    let is_group = conversation_type.eq_ignore_ascii_case("group");
    if !is_group {
        let stem = sanitize_stem(chat_id);
        return with_suffix(&stem, suffix);
    }

    if let Some(title) = group_title.and_then(trimmed) {
        let stem = sanitize_stem(title);
        if !stem.is_empty() && !stem.chars().all(|c| c == '_') {
            return with_suffix(&stem, suffix);
        }
    }

    let phones = unique_sorted_phone_handles(participant_e164s);

    if phones.is_empty() {
        let stem = if chat_id.trim().is_empty() {
            "group_unknown".to_string()
        } else {
            let digest = hex::encode(Sha256::digest(chat_id.as_bytes()));
            format!("group_{}", &digest[..16])
        };
        return with_suffix(&stem, suffix);
    }

    let mut stem = String::from("group");
    for phone in phones.iter().take(GROUP_FILENAME_MAX_PHONES) {
        stem.push('_');
        stem.push_str(phone);
    }
    if phones.len() > GROUP_FILENAME_MAX_PHONES {
        let joined = phones.join("|");
        let digest = hex::encode(Sha256::digest(joined.as_bytes()));
        stem.push('_');
        stem.push_str(&digest[..16]);
    }
    with_suffix(&stem, suffix)
}

/// Trim, keep phone-looking handles, sort, and drop duplicates.
fn unique_sorted_phone_handles(participant_e164s: &[String]) -> Vec<String> {
    let mut phones: Vec<String> = participant_e164s
        .iter()
        .map(|p| p.trim().to_string())
        .filter(|p| is_phone_handle(p))
        .collect();
    phones.sort();
    phones.dedup();
    phones
}

/// Count messages and attachments and find first/last timestamps.
fn compute_stats(messages: &[IrMessage]) -> ConversationStats {
    let message_count = messages.len() as u64;
    let attachment_count = messages.iter().map(|m| m.attachments.len() as u64).sum();
    let mut first = None;
    let mut last = None;
    for msg in messages {
        first = Some(first.map_or(msg.timestamp_unix_ms, |f: i64| f.min(msg.timestamp_unix_ms)));
        last = Some(last.map_or(msg.timestamp_unix_ms, |l: i64| l.max(msg.timestamp_unix_ms)));
    }
    ConversationStats {
        message_count,
        attachment_count,
        first_timestamp_unix_ms: first,
        last_timestamp_unix_ms: last,
    }
}

/// Format a Unix second as local / UTC / display strings.
///
/// Returns `None` when the timestamp cannot be represented in local or UTC.
pub fn format_local_ts(secs: i64) -> Option<(String, String, String)> {
    use chrono::{Local, TimeZone, Utc};
    let local = Local.timestamp_opt(secs, 0).single().or_else(|| {
        Utc.timestamp_opt(secs, 0)
            .single()
            .map(|utc| Local.from_utc_datetime(&utc.naive_utc()))
    })?;
    let utc = local.with_timezone(&Utc);
    let display = local.format("%b %e, %Y %I:%M:%S %p").to_string();
    Some((
        local.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        utc.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        display,
    ))
}

/// Deterministic message GUID from chat + timestamp + direction + body + attachment digests.
pub fn stable_guid(
    chat_id: &str,
    timestamp: &str,
    is_from_me: bool,
    text: &str,
    att_digests: &[String],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(chat_id.as_bytes());
    hasher.update(b"|");
    hasher.update(timestamp.as_bytes());
    hasher.update(b"|");
    hasher.update(if is_from_me { b"1" } else { b"0" });
    hasher.update(b"|");
    hasher.update(text.as_bytes());
    for d in att_digests {
        hasher.update(b"|");
        hasher.update(d.as_bytes());
    }
    hex::encode(hasher.finalize())
}

/// Stream a file through SHA-256 in 64 KB chunks (no full read into memory).
///
/// Returns 64 lowercase hex digits — the same fingerprint format
/// `digest_sha256` fields carry.
///
/// # Errors
///
/// Returns an error when the file cannot be opened or read; the error message
/// names the file.
pub fn file_sha256(path: &std::path::Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)
        .map_err(|e| std::io::Error::new(e.kind(), format!("open {}: {e}", path.display())))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| std::io::Error::new(e.kind(), format!("read {}: {e}", path.display())))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// `s` trimmed, or `None` when blank. The one place "blank means absent"
/// is spelled out; pass it to `Option::and_then` for optional fields.
pub fn trimmed(s: &str) -> Option<&str> {
    let t = s.trim();
    if t.is_empty() { None } else { Some(t) }
}

/// Trimmed owned copy of `s`, or `None` when blank.
pub fn nonempty(s: &str) -> Option<String> {
    trimmed(s).map(str::to_string)
}

/// `value` trimmed, unless it is blank or the literal `null` / `none` that
/// some backups write where an attachment name is missing.
pub fn valid_filename(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()
        && !value.eq_ignore_ascii_case("null")
        && !value.eq_ignore_ascii_case("none"))
    .then(|| value.to_string())
}

/// One mebibyte, for byte counts shown or compared in MiB.
pub const MIB: u64 = 1024 * 1024;

/// Owner identity for outgoing rows: handle + display (`"Me"` if handle set but name missing).
pub fn owner_sender(export: &ExportMeta) -> (Option<String>, Option<String>) {
    let handle = export.owner_handle.as_deref().and_then(nonempty);
    let display = export
        .owner_display_name
        .as_deref()
        .and_then(nonempty)
        .or_else(|| handle.as_ref().map(|_| "Me".into()));
    (handle, display)
}

/// Parse Android type strings / numbers into `i32`.
pub fn parse_android_type(s: &str) -> Option<i32> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    t.parse::<i32>().ok()
}

/// Parse a JSON string into a [`Value`], or return the string as a JSON string value.
pub fn parse_json_value(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or_else(|_| json!(s))
}

/// Export and conversation metadata without messages (JSONL header line
/// and CSV header row).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationHeader {
    /// Schema version written into this header (currently 3).
    pub schema_version: u32,
    /// Where and how this export was produced.
    pub export: ExportMeta,
    /// Roster and computed stats for this chat.
    pub conversation: ConversationMeta,
}

impl ConversationHeader {
    /// Copy export and conversation metadata from a full document.
    pub fn from_document(doc: &ConversationDocument) -> Self {
        Self {
            schema_version: doc.schema_version,
            export: doc.export.clone(),
            conversation: doc.conversation.clone(),
        }
    }

    /// The document a reader builds from a header and the messages that
    /// followed it, at the current schema version, with its stats computed.
    pub fn into_document(
        self,
        messages: Vec<IrMessage>,
        packaging_stem_suffix: Option<String>,
    ) -> ConversationDocument {
        let mut doc = ConversationDocument {
            schema_version: SCHEMA_VERSION,
            export: self.export,
            conversation: self.conversation,
            messages,
            packaging_stem_suffix,
        };
        doc.finalize_stats();
        doc
    }
}

/// Intermediate message before conversion to [`IrMessage`].
///
/// Exporters parse vendor formats into these, then convert to
/// [`ConversationDocument`] in their `pending_to_document` functions.
/// Exporter-specific metadata goes in [`Self::extra`].
#[derive(Debug, Clone)]
pub struct PendingMessage {
    /// Unix timestamp for chronological sort (seconds or milliseconds).
    pub sort_key: i64,
    /// Whether the owner sent the message.
    pub is_from_me: bool,
    /// Handle of the sender (the owner's handle when `is_from_me`).
    pub sender_handle: String,
    /// Display name of the sender; `None` when the source has none.
    pub sender_display_name: Option<String>,
    /// Plain-text body; never includes attachment data.
    pub text: String,
    /// Relative paths to staged attachment files.
    pub attachments: Vec<PendingAttachment>,
    /// Per-exporter metadata (e.g., `key_id`, `android_type`, `xml_fields`).
    pub extra: std::collections::BTreeMap<String, String>,
}

impl PendingMessage {
    /// Read an exporter-specific string field from [`Self::extra`].
    pub fn extra_str(&self, key: &str) -> &str {
        self.extra.get(key).map(String::as_str).unwrap_or("")
    }

    /// Read a boolean stored in [`Self::extra`] (values `"true"` / `"false"`).
    pub fn extra_flag(&self, key: &str) -> bool {
        self.extra.get(key).is_some_and(|v| v == "true")
    }

    /// Read an optional string field from [`Self::extra`] (empty = `None`).
    pub fn extra_opt(&self, key: &str) -> Option<String> {
        let v = self.extra_str(key);
        (!v.is_empty()).then(|| v.to_string())
    }
}

/// Intermediate attachment reference before conversion to [`IrAttachment`].
#[derive(Debug, Clone)]
pub struct PendingAttachment {
    /// Relative path to the staged file.
    pub rel_path: String,
    /// MIME content type.
    pub content_type: String,
    /// File extension.
    pub extension: String,
    /// SHA-256 of the file contents; `None` when unknown.
    pub digest_sha256: Option<String>,
    /// Optional SMIL/content-location name.
    pub name_hint: Option<String>,
}

impl PendingAttachment {
    /// MIME type as an option; empty [`Self::content_type`] means `None`.
    pub fn mime_type(&self) -> Option<String> {
        (!self.content_type.is_empty()).then(|| self.content_type.clone())
    }

    /// The IR attachment for a queued one, carrying its bytes when
    /// `blob_bytes` holds them under its digest. No path: the runner that
    /// writes the file fills that in.
    pub fn to_ir(&self, blob_bytes: &HashMap<String, Vec<u8>>) -> IrAttachment {
        let digest = self.digest_sha256.clone();
        let bytes = digest.as_ref().and_then(|d| blob_bytes.get(d).cloned());
        IrAttachment {
            path: None,
            original_name: self.name_hint.clone(),
            mime_type: self.mime_type(),
            digest_sha256: digest,
            is_sticker: false,
            transcription: None,
            sticker_effect: None,
            size_bytes: bytes.as_ref().map(|b| b.len() as u64),
            missing_reason: None,
            bytes,
        }
    }
}

/// Intermediate conversation before conversion to [`ConversationDocument`].
#[derive(Debug, Clone)]
pub struct PendingConversation {
    /// Stable chat id from the source.
    pub chat_id: String,
    /// Display name for the chat; `None` when the source has none.
    pub display_name: Option<String>,
    /// Participant handles in E.164 form.
    pub participant_e164s: Vec<String>,
    /// Messages awaiting conversion to [`IrMessage`].
    pub messages: Vec<PendingMessage>,
    /// Whether this is a group chat.
    pub is_group: bool,
    /// Whether any message in this conversation has attachments.
    pub has_attachments: bool,
    /// Per-exporter metadata.
    pub extra: std::collections::BTreeMap<String, String>,
}

impl PendingConversation {
    /// Fresh conversation with no messages and an empty `extra` map.
    pub fn new(
        chat_id: impl Into<String>,
        is_group: bool,
        display_name: Option<String>,
        participant_e164s: Vec<String>,
    ) -> Self {
        Self {
            chat_id: chat_id.into(),
            display_name,
            participant_e164s,
            messages: Vec::new(),
            is_group,
            has_attachments: false,
            extra: std::collections::BTreeMap::new(),
        }
    }

    /// Read an exporter-specific string field from [`Self::extra`].
    pub fn extra_str(&self, key: &str) -> &str {
        self.extra.get(key).map(String::as_str).unwrap_or("")
    }

    /// First non-empty `contact_name` extra on a message in this conversation.
    pub fn first_contact_name(&self) -> Option<String> {
        self.messages
            .iter()
            .map(|m| m.extra_str("contact_name").trim())
            .find(|n| !n.is_empty())
            .map(str::to_string)
    }
}

#[cfg(test)]
mod conversation_stem_tests {
    use super::conversation_stem;

    #[test]
    fn individual_uses_chat_id() {
        assert_eq!(
            conversation_stem("individual", "+15551212", None, &[], None),
            "+15551212"
        );
    }

    #[test]
    fn group_with_title_uses_title() {
        assert_eq!(
            conversation_stem("group", "chat-x", Some("Family Chat"), &[], None),
            "Family_Chat"
        );
    }

    #[test]
    fn untitled_group_lists_sorted_phones() {
        let peers = vec!["+18285532527".into(), "+14073109632".into()];
        assert_eq!(
            conversation_stem("group", "chat-group-x", None, &peers, None),
            "group_+14073109632_+18285532527"
        );
    }

    #[test]
    fn untitled_group_over_ten_appends_hash() {
        let peers: Vec<String> = (1..=13).map(|i| format!("+1555555{i:04}")).collect();
        let stem = conversation_stem("group", "chat-x", None, &peers, None);
        assert!(stem.starts_with("group_+15555550001_"));
        assert!(stem.contains("+15555550010_"));
        assert!(!stem.contains("+15555550011"));
        let hash = stem.rsplit('_').next().unwrap();
        assert_eq!(hash.len(), 16);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(
            stem,
            conversation_stem("group", "other-id", None, &peers, None)
        );
    }

    #[test]
    fn whatsapp_suffix() {
        let peers = vec!["+15555550100".into()];
        assert_eq!(
            conversation_stem("group", "x", None, &peers, Some("__whatsapp")),
            "group_+15555550100__whatsapp"
        );
    }

    #[test]
    fn none_title_uses_phones_not_synthetic() {
        let peers = vec!["+15555550100".into()];
        assert_eq!(
            conversation_stem("group", "chat-group-x", None, &peers, None),
            "group_+15555550100"
        );
    }
}

#[cfg(test)]
mod handle_service_tests {
    use super::{HandleService, IrService};

    #[test]
    fn parse_phone_aliases() {
        for s in [
            "phone",
            "SMS",
            "imessage",
            "rcs",
            "mms",
            "Text message",
            "text_message",
        ] {
            assert_eq!(HandleService::parse(s), HandleService::Phone, "{s}");
        }
    }

    #[test]
    fn parse_whatsapp() {
        assert_eq!(HandleService::parse("whatsapp"), HandleService::Whatsapp);
        assert_eq!(HandleService::parse("WA"), HandleService::Whatsapp);
    }

    #[test]
    fn map_ir_service() {
        assert_eq!(
            HandleService::from_ir_service(IrService::Whatsapp),
            HandleService::Whatsapp
        );
        assert_eq!(
            HandleService::from_ir_service(IrService::IMessage),
            HandleService::Phone
        );
        assert_eq!(
            HandleService::from_ir_service(IrService::Sms),
            HandleService::Phone
        );
        assert_eq!(
            HandleService::from_ir_service(IrService::Rcs),
            HandleService::Phone
        );
    }

    #[test]
    fn as_str_storage_ids() {
        assert_eq!(HandleService::Phone.as_str(), "phone");
        assert_eq!(HandleService::Whatsapp.as_str(), "whatsapp");
    }
}
