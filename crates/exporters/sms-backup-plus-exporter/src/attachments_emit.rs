//! Attachment helpers: queue blobs during parse, then map staged attachments
//! onto the shared [`IrAttachment`] shape after the runner writes files.

use crate::types::AttachmentBlob;
use message_ir::PendingAttachment;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Queue attachment blobs as metadata. Bytes stay in `blob_bytes` (keyed by
/// digest) until the shared runner writes them.
pub(super) fn queue_attachments(
    blobs: &[AttachmentBlob],
    copy_attachments: bool,
    blob_bytes: &mut HashMap<String, Vec<u8>>,
) -> Vec<PendingAttachment> {
    blobs
        .iter()
        .map(|blob| {
            if copy_attachments && !blob.data.is_empty() {
                blob_bytes
                    .entry(blob.digest_hex.clone())
                    .or_insert_with(|| blob.data.clone());
            }
            PendingAttachment {
                rel_path: String::new(),
                content_type: blob.mime_type.clone().unwrap_or_default(),
                extension: Path::new(&blob.filename)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_string(),
                digest_sha256: Some(blob.digest_hex.clone()),
                name_hint: blob
                    .original_name
                    .clone()
                    .or_else(|| Some(blob.filename.clone())),
            }
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
