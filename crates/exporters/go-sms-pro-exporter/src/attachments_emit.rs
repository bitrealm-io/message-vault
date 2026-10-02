//! Attachment helpers for the emitter.

use anyhow::Result;
use go_sms_mms::ParsedPdu;
use message_crate_core::digest_prefix;
use message_ir::PendingAttachment;
use message_staging::AttachmentSpool;
use sha2::{Digest, Sha256};

/// Queue PDU attachment parts as metadata. With a `spool`, each payload is
/// written to it here, so no attachment's bytes stay in memory until the
/// shared runner writes them. The extension comes from the part's content
/// type; a type the media table does not know gets `.bin` so the bytes are
/// still kept.
///
/// # Errors
///
/// Returns an error when a payload cannot be written to the spool.
pub(super) fn queue_pdu_attachments(
    parsed: &ParsedPdu,
    spool: Option<&AttachmentSpool>,
) -> Result<Vec<PendingAttachment>> {
    let mut out = Vec::new();
    for (idx, att) in parsed.attachments.iter().enumerate() {
        let digest_hex = match spool {
            Some(spool) if !att.data.is_empty() => spool.put(&att.data)?,
            _ => hex::encode(Sha256::digest(&att.data)),
        };
        let ext = media::ext_for_mime(&att.content_type).unwrap_or(".bin");
        let name = format!(
            "I_{}_{}_{}{}",
            parsed.timestamp,
            digest_prefix(&digest_hex),
            idx + 1,
            ext
        );
        out.push(PendingAttachment {
            rel_path: String::new(),
            content_type: att.content_type.clone(),
            digest_sha256: Some(digest_hex),
            name_hint: att.name.clone().or(Some(name)),
        });
    }
    Ok(out)
}
