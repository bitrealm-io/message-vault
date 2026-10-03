//! Shared parsed message types for SMS Backup+ EML conversion.

use phone::Handle;

#[derive(Debug, Clone, Default)]
pub(crate) struct AttachmentBlob {
    pub filename: String,
    pub original_name: Option<String>,
    pub mime_type: Option<String>,
    /// SHA-256 hex of [`Self::data`], computed once at extract time.
    pub digest_hex: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ParsedMessage {
    /// The peer's handle key, the group key, or empty when the mail records
    /// no address.
    pub chat_key: String,
    pub conversation_type: String,
    pub group_title: Option<String>,
    pub participants: Vec<Handle>,
    pub timestamp_secs: f64,
    /// Whether [`Self::timestamp_secs`] carries the milliseconds the phone
    /// stored (`X-smssync-date` in milliseconds). `false` when the time came
    /// in whole seconds: from the `Date` header, or `X-smssync-date` in seconds.
    pub has_milliseconds: bool,
    pub is_from_me: bool,
    pub sender: Option<Handle>,
    pub text: String,
    pub attachments: Vec<AttachmentBlob>,
    /// MIME parts dropped because their content could not be decoded.
    pub unreadable_parts: u64,
    pub name_alias: Option<String>,
    /// `X-smssync-id` when present.
    pub smssync_id: Option<String>,
    /// Raw `X-smssync-type` when present.
    pub android_type: String,
    /// Source `.eml` path (relative when under an input root).
    pub eml_path: String,
}
