//! Attachment helpers: queue blobs as [`PendingAttachment`] metadata during
//! parse.

use crate::types::AttachmentBlob;
use anyhow::Result;
use message_ir::PendingAttachment;
use message_staging::AttachmentSpool;

/// Queue attachment blobs as metadata. With a `spool`, each payload is
/// written to it here, so no attachment's bytes stay in memory until the
/// shared runner writes them.
///
/// # Errors
///
/// Returns an error when a payload cannot be written to the spool.
pub(super) fn queue_attachments(
    blobs: &[AttachmentBlob],
    spool: Option<&AttachmentSpool>,
) -> Result<Vec<PendingAttachment>> {
    blobs
        .iter()
        .map(|blob| {
            let digest = match spool {
                Some(spool) if !blob.data.is_empty() => spool.put(&blob.data)?,
                _ => blob.digest_hex.clone(),
            };
            Ok(PendingAttachment {
                rel_path: String::new(),
                content_type: blob.mime_type.clone().unwrap_or_default(),
                digest_sha256: Some(digest),
                name_hint: blob
                    .original_name
                    .clone()
                    .or_else(|| Some(blob.filename.clone())),
            })
        })
        .collect()
}
