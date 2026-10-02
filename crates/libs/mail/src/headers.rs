//! The `X-ME-*` header names, spelled once for the writer and the reader.
//!
//! `build_eml` (writer) and `parse.rs` (reader) both name headers through
//! these constants, so a typo cannot silently break the EML roundtrip.

/// Conversation id header.
pub(crate) const CHAT_IDENTIFIER: &str = "X-ME-Chat-Identifier";
/// `individual` or `group`.
pub(crate) const CONVERSATION_TYPE: &str = "X-ME-Conversation-Type";
/// `incoming` or `outgoing`.
pub(crate) const DIRECTION: &str = "X-ME-Direction";
/// IR service (`sms`, `imessage`, …).
pub(crate) const SERVICE: &str = "X-ME-Service";
/// IR message kind (`sms`, `mms`, `tapback`, …).
pub(crate) const MESSAGE_KIND: &str = "X-ME-Message-Kind";
/// Message timestamp in Unix milliseconds.
pub(crate) const TIMESTAMP_UNIX_MS: &str = "X-ME-Timestamp-Unix-Ms";
/// Message guid.
pub(crate) const GUID: &str = "X-ME-Guid";
/// Export source id.
pub(crate) const EXPORT_SOURCE: &str = "X-ME-Export-Source";
/// Export tool name.
pub(crate) const EXPORT_TOOL: &str = "X-ME-Export-Tool";
/// Export tool version.
pub(crate) const EXPORT_TOOL_VERSION: &str = "X-ME-Export-Tool-Version";
/// Group chat title.
pub(crate) const GROUP_TITLE: &str = "X-ME-Group-Title";
/// Conversation roster as JSON.
pub(crate) const PARTICIPANTS: &str = "X-ME-Participants";
/// Sender handle.
pub(crate) const SENDER_HANDLE: &str = "X-ME-Sender-Handle";
/// Sender display name.
pub(crate) const SENDER_DISPLAY_NAME: &str = "X-ME-Sender-Display-Name";
/// The conversation's owner handle.
pub(crate) const OWNER_HANDLE: &str = "X-ME-Owner-Handle";
/// The owner handle this one message was sent from or received at, when the
/// source records one per message (Apple Messages does).
pub(crate) const MESSAGE_OWNER_HANDLE: &str = "X-ME-Message-Owner-Handle";
/// Owner display name.
pub(crate) const OWNER_DISPLAY_NAME: &str = "X-ME-Owner-Display-Name";
/// SMS/MMS subject.
pub(crate) const SUBJECT: &str = "X-ME-Subject";
/// Android SMS box type.
pub(crate) const ANDROID_TYPE: &str = "X-ME-Android-Type";
/// Vendor source fields as JSON.
pub(crate) const SOURCE_FIELDS: &str = "X-ME-Source-Fields";
/// iMessage reply flag.
pub(crate) const IS_REPLY: &str = "X-ME-Is-Reply";
/// Thread originator guid (reply parent).
pub(crate) const THREAD_ORIGINATOR_GUID: &str = "X-ME-Thread-Originator-Guid";
/// Thread originator part index.
pub(crate) const THREAD_ORIGINATOR_PART: &str = "X-ME-Thread-Originator-Part";
/// Reply count.
pub(crate) const NUM_REPLIES: &str = "X-ME-Num-Replies";
/// iMessage deleted flag.
pub(crate) const IS_DELETED: &str = "X-ME-Is-Deleted";
/// Send effect name.
pub(crate) const SEND_EFFECT: &str = "X-ME-Send-Effect";
/// Shared location payload.
pub(crate) const SHARED_LOCATION: &str = "X-ME-Shared-Location";
/// Group announcement text.
pub(crate) const ANNOUNCEMENT: &str = "X-ME-Announcement";
/// Read receipt timestamp (RFC 3339).
pub(crate) const READ_RECEIPT: &str = "X-ME-Read-Receipt";
/// Message parts as JSON.
pub(crate) const PARTS: &str = "X-ME-Parts";
/// Edit history as JSON.
pub(crate) const EDITS: &str = "X-ME-Edits";
/// Tapbacks on this message as JSON.
pub(crate) const TAPBACKS: &str = "X-ME-Tapbacks";
/// App/balloon payload as JSON.
pub(crate) const APP: &str = "X-ME-App";
/// Balloon bundle id.
pub(crate) const BALLOON_BUNDLE_ID: &str = "X-ME-Balloon-Bundle-Id";
/// Balloon kind.
pub(crate) const BALLOON_KIND: &str = "X-ME-Balloon-Kind";
/// Guid of the message a tapback targets.
pub(crate) const ASSOCIATED_GUID: &str = "X-ME-Associated-Guid";
/// Part index a tapback targets.
pub(crate) const ASSOCIATED_PART: &str = "X-ME-Associated-Part";
/// Tapback kind.
pub(crate) const TAPBACK_KIND: &str = "X-ME-Tapback-Kind";
/// Tapback emoji.
pub(crate) const TAPBACK_EMOJI: &str = "X-ME-Tapback-Emoji";
/// Tapback add/remove action.
pub(crate) const TAPBACK_ACTION: &str = "X-ME-Tapback-Action";
/// Attachment metadata as JSON.
pub(crate) const ATTACHMENT_META: &str = "X-ME-Attachment-Meta";
