//! Turn conversation documents into JSON Lines for the Message Vault import API.
//!
//! JSON Lines means one JSON object per line. The first line is the conversation
//! header. Each later line is one message.

use anyhow::{Context, Result, bail};
use message_ir::{ConversationDocument, ConversationHeader, IrMessage, check_schema_version};

/// Return the backup source name from a conversation header, or an error if the
/// schema version or source field is unusable.
///
/// # Errors
///
/// Returns an error when `schema_version` is not the current version, or when
/// `export.source` is empty.
pub fn validate_header(header: &ConversationHeader) -> Result<String> {
    check_schema_version(header.schema_version)?;
    let source = header.export.source.trim();
    if source.is_empty() {
        bail!("export.source is empty");
    }
    Ok(source.to_string())
}

/// First JSON Lines row for a conversation: the header object plus a newline.
///
/// # Errors
///
/// Returns an error when the header fails [`validate_header`] or cannot be
/// serialized.
pub fn document_header_line(doc: &ConversationDocument) -> Result<Vec<u8>> {
    let header = ConversationHeader::from_document(doc);
    validate_header(&header)?;
    let mut out =
        serde_json::to_vec(&header).context("serialize message-ir conversation header")?;
    out.push(b'\n');
    Ok(out)
}

/// How to rewrite one attachment when building an import message line.
#[derive(Debug, Clone)]
pub enum AttachmentProjection {
    /// Bytes were (or will be) uploaded under this digest.
    Digested {
        index: usize,
        digest: String,
        size: u64,
    },
    /// Bytes are absent; keep metadata and set `missing_reason`.
    Missing {
        index: usize,
        reason: String,
        size: Option<u64>,
    },
}

/// One JSON Lines message row, with attachment fingerprints or missing placeholders.
///
/// `index` is the message's position in the conversation file. It keeps resume
/// keys unique when two rows share a timestamp and have an empty guid.
///
/// # Errors
///
/// Returns an error when the message cannot be serialized.
pub fn message_line(
    msg: &IrMessage,
    projections: &[AttachmentProjection],
    index: usize,
) -> Result<(Vec<u8>, String)> {
    let mut msg = msg.clone();
    for proj in projections {
        match proj {
            AttachmentProjection::Digested {
                index,
                digest,
                size,
            } => {
                if let Some(att) = msg.attachments.get_mut(*index) {
                    att.digest_sha256 = Some(digest.clone());
                    att.size_bytes = Some(*size);
                    att.missing_reason = None;
                }
            }
            AttachmentProjection::Missing {
                index,
                reason,
                size,
            } => {
                if let Some(att) = msg.attachments.get_mut(*index) {
                    att.digest_sha256 = None;
                    att.missing_reason = Some(reason.clone());
                    if let Some(size) = size {
                        att.size_bytes = Some(*size);
                    }
                }
            }
        }
    }
    serialize_message(&msg, index)
}

/// One JSON Lines message row with attachments removed (text-only import).
///
/// # Errors
///
/// Returns an error when the message cannot be serialized.
pub fn message_line_without_attachments(
    msg: &IrMessage,
    index: usize,
) -> Result<(Vec<u8>, String)> {
    let mut msg = msg.clone();
    msg.attachments.clear();
    serialize_message(&msg, index)
}

/// Serialize one message and return `(line_bytes, resume_guid)`.
///
/// Empty guids become `unguided:{timestamp}:{index}` so a later run can skip
/// the same row.
///
/// # Errors
///
/// Returns an error when JSON serialization fails.
fn serialize_message(msg: &IrMessage, index: usize) -> Result<(Vec<u8>, String)> {
    let mut out = serde_json::to_vec(msg).context("serialize message-ir message")?;
    out.push(b'\n');
    let guid = if msg.guid.trim().is_empty() {
        format!("unguided:{}:{index}", msg.timestamp_unix_ms)
    } else {
        msg.guid.clone()
    };
    Ok((out, guid))
}

#[cfg(test)]
mod tests {
    use super::*;
    use message_ir::{
        ConversationMeta, ConversationStats, ExportMeta, IrAttachment, IrConversationType,
        IrDirection, IrMessageKind, IrParticipant, IrService, SCHEMA_VERSION,
    };

    #[test]
    fn serializes_ir_sms() {
        let doc = ConversationDocument {
            schema_version: SCHEMA_VERSION,
            export: ExportMeta {
                source: "sms-backup-restore".into(),
                tool: "SMS Backup & Restore".into(),
                tool_version: "10.26.003".into(),
                owner_handle: Some("+15555550100".into()),
                owner_display_name: Some("Me".into()),
            },
            conversation: ConversationMeta {
                chat_identifier: "+15555550101".into(),
                conversation_type: IrConversationType::Individual,
                group_title: None,
                participants: vec![IrParticipant {
                    handle: Some("+15555550101".into()),
                    display_name: Some("Sam".into()),
                    handle_type: None,
                }],
                stats: ConversationStats::default(),
            },
            messages: vec![],
            packaging_stem_suffix: None,
        };
        let header = String::from_utf8(document_header_line(&doc).unwrap()).unwrap();
        assert!(header.contains(r#""schema_version":4"#));
        assert!(header.contains(r#""sms-backup-restore""#));
        assert!(!header.contains(r#""record":"conversation""#));

        let msg = IrMessage {
            guid: "g1".into(),
            timestamp_unix_ms: 1_400_773_261_000,
            direction: IrDirection::Incoming,
            service: IrService::Sms,
            message_kind: IrMessageKind::Sms,
            sender_handle: Some("+15555550101".into()),
            sender_display_name: Some("Sam".into()),
            owner_handle: None,
            subject: None,
            text: "hello".into(),
            attachments: vec![],
            imessage: None,
            source: None,
        };
        let (line, guid) = message_line(&msg, &[], 0).unwrap();
        assert_eq!(guid, "g1");
        let s = String::from_utf8(line).unwrap();
        assert!(s.contains(r#""direction":"incoming""#));
        assert!(!s.contains(r#""record":"message""#));
    }

    #[test]
    fn unguided_keys_include_message_index() {
        let msg = IrMessage {
            guid: "  ".into(),
            timestamp_unix_ms: 42,
            direction: IrDirection::Incoming,
            service: IrService::Sms,
            message_kind: IrMessageKind::Sms,
            sender_handle: None,
            sender_display_name: None,
            owner_handle: None,
            subject: None,
            text: "x".into(),
            attachments: vec![],
            imessage: None,
            source: None,
        };
        let (_, g0) = message_line(&msg, &[], 0).unwrap();
        let (_, g1) = message_line(&msg, &[], 1).unwrap();
        assert_eq!(g0, "unguided:42:0");
        assert_eq!(g1, "unguided:42:1");
    }

    #[test]
    fn projects_missing_reason_and_clears_digest() {
        let msg = IrMessage {
            guid: "g1".into(),
            timestamp_unix_ms: 1,
            direction: IrDirection::Incoming,
            service: IrService::Sms,
            message_kind: IrMessageKind::Sms,
            sender_handle: None,
            sender_display_name: None,
            owner_handle: None,
            subject: None,
            text: "with attachment".into(),
            attachments: vec![IrAttachment {
                path: Some("attachments/big.bin".into()),
                original_name: Some("big.bin".into()),
                mime_type: Some("application/octet-stream".into()),
                digest_sha256: Some("deadbeef".into()),
                is_sticker: false,
                transcription: None,
                sticker_effect: None,
                size_bytes: Some(99),
                missing_reason: None,
                bytes: None,
            }],
            imessage: None,
            source: None,
        };
        let (line, _) = message_line(
            &msg,
            &[AttachmentProjection::Missing {
                index: 0,
                reason: "too_large".into(),
                size: Some(5_000_000),
            }],
            0,
        )
        .unwrap();
        let parsed: IrMessage = serde_json::from_slice(&line).unwrap();
        assert!(parsed.attachments[0].digest_sha256.is_none());
        assert_eq!(
            parsed.attachments[0].missing_reason.as_deref(),
            Some("too_large")
        );
        assert_eq!(parsed.attachments[0].size_bytes, Some(5_000_000));
        assert_eq!(
            parsed.attachments[0].original_name.as_deref(),
            Some("big.bin")
        );
    }
}
