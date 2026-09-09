//! Shared parsed message types for SMS Backup+ EML → CSV exports.

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
    pub chat_key: String,
    pub conversation_type: String,
    pub group_title: Option<String>,
    pub participant_digits: Vec<(String, Option<String>)>,
    pub timestamp_secs: f64,
    pub is_from_me: bool,
    pub sender_digits: Option<String>,
    pub text: String,
    pub attachments: Vec<AttachmentBlob>,
    pub name_alias: Option<String>,
    /// `X-smssync-id` when present.
    pub smssync_id: Option<String>,
    /// Raw `X-smssync-type` when present.
    pub android_type: String,
    /// Source `.eml` path (relative when under an input root).
    pub eml_path: String,
}
