//! Attachment helpers: queue blobs as [`PendingAttachment`] metadata during
//! parse, and merge attachment lists by content digest.

use crate::types::AttachmentBlob;
use anyhow::Result;
use message_ir::PendingAttachment;
use message_staging::AttachmentSpool;
use std::collections::HashSet;

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

/// Union attachment lists by content digest so dedupe does not drop media.
pub(super) fn merge_attachments(into: &mut Vec<PendingAttachment>, from: Vec<PendingAttachment>) {
    let mut seen: HashSet<String> = into
        .iter()
        .map(|a| a.digest_sha256.clone().unwrap_or_default())
        .collect();
    for att in from {
        if seen.insert(att.digest_sha256.clone().unwrap_or_default()) {
            into.push(att);
        }
    }
}
