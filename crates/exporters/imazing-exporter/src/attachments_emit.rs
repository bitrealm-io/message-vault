//! Attachment helpers for the emitter.

use message_ir::{IrAttachment, PendingAttachment, PendingMessage};

/// What tells a message's attachments apart, for its id and for the dedupe
/// step: the content digest, so a later run that finds and copies a
/// previously missing file does not change the message id, else the path.
pub(super) fn attachment_digests(attachments: &[PendingAttachment]) -> Vec<String> {
    let mut digests: Vec<String> = attachments
        .iter()
        .map(|a| {
            a.digest_sha256
                .clone()
                .unwrap_or_else(|| a.rel_path.clone())
        })
        .collect();
    digests.sort();
    digests
}

/// Map a staged attachment onto the shared [`IrAttachment`] shape.
pub(super) fn pending_attachment_to_ir(
    a: &PendingAttachment,
    msg: &PendingMessage,
) -> IrAttachment {
    IrAttachment {
        path: a
            .rel_path
            .starts_with("attachments/")
            .then(|| a.rel_path.clone()),
        original_name: a.name_hint.clone(),
        mime_type: a.mime_type(),
        digest_sha256: a.digest_sha256.clone(),
        is_sticker: msg.extra_flag("is_sticker"),
        transcription: msg.extra_opt("transcription"),
        sticker_effect: msg.extra_opt("sticker_effect"),
        size_bytes: None,
        missing_reason: None,
        bytes: None,
    }
}
