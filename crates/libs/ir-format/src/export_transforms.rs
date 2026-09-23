//! Media convert/compress and obfuscation applied before writing files.

use crate::util::read_attachment_file;
use anyhow::Result;
use media::MediaMode;
use message_ir::{ConversationDocument, IrAttachment, IrDirection, IrParticipant};
use message_vault_io_core::{ExportTransforms, emit_log};
use obfuscate::{
    Obfuscator, classify_attachment, materialize_placeholders, placeholder_rel_path,
    resolve_obfuscator_with_log,
};
use std::path::Path;

/// Load each attachment's bytes from the output folder into the document; unreadable
/// files are left without bytes so packaging can continue.
pub(crate) fn reload_attachment_bytes(doc: &mut ConversationDocument, output_dir: &Path) {
    for msg in &mut doc.messages {
        for att in &mut msg.attachments {
            // Lenient: IO failures leave bytes unset so packaging can continue.
            if let Ok(Some(bytes)) = read_attachment_file(att, output_dir) {
                att.bytes = Some(bytes);
            }
        }
    }
}

/// Drop attachment paths and bytes when the media mode is disabled, keeping the metadata.
pub fn clear_attachments_when_disabled(doc: &mut ConversationDocument, mode: MediaMode) {
    if !matches!(mode, MediaMode::Disabled) {
        return;
    }
    for msg in &mut doc.messages {
        for att in &mut msg.attachments {
            att.path = None;
            att.bytes = None;
            att.digest_sha256 = None;
        }
    }
}

/// Replace every handle, name, and body in the document with stable fake values.
pub(crate) fn obfuscate_document(doc: &mut ConversationDocument, anon: &mut Obfuscator) {
    doc.conversation.chat_identifier = anon.obfuscate_handle(&doc.conversation.chat_identifier);
    if let Some(title) = doc.conversation.group_title.as_mut() {
        *title = anon.obfuscate_mixed_field(title);
    }
    for p in &mut doc.conversation.participants {
        obfuscate_participant(p, anon);
    }
    if let Some(h) = doc.export.owner_handle.as_mut() {
        *h = anon.obfuscate_handle(h);
    }
    if let Some(n) = doc.export.owner_display_name.as_mut() {
        *n = anon.obfuscate_display_name(n);
    }
    for msg in &mut doc.messages {
        if let Some(h) = msg.sender_handle.as_mut() {
            *h = anon.obfuscate_handle(h);
        }
        if let Some(h) = msg.owner_handle.as_mut() {
            *h = anon.obfuscate_handle(h);
        }
        if let Some(n) = msg.sender_display_name.as_mut() {
            if msg.direction == IrDirection::Outgoing && n == "Me" {
                // Keep the conventional outgoing label.
            } else {
                *n = anon.obfuscate_display_name(n);
            }
        }
        if let Some(s) = msg.subject.as_mut() {
            *s = anon.obfuscate_text(s);
        }
        msg.text = anon.obfuscate_text(&msg.text);
        if let Some(im) = msg.imessage.as_mut()
            && let Some(a) = im.announcement.as_mut()
        {
            *a = anon.obfuscate_text(a);
        }
        for att in &mut msg.attachments {
            obfuscate_attachment(att);
        }
        // The vendor bag is raw source attributes and can carry the real
        // sender address, so obfuscated output drops it whole. `android_type`
        // goes with it: `direction` already says sent or received.
        msg.source = None;
    }
}

/// Obfuscate one participant's handle and display name.
fn obfuscate_participant(p: &mut IrParticipant, anon: &mut Obfuscator) {
    if let Some(handle) = p.handle.as_mut() {
        *handle = anon.obfuscate_handle(handle);
    }
    if let Some(n) = p.display_name.as_mut() {
        *n = anon.obfuscate_display_name(n);
    }
}

/// Replace an attachment with the placeholder file for its media class.
fn obfuscate_attachment(att: &mut IrAttachment) {
    let class = classify_attachment(att.mime_type.as_deref(), att.path.as_deref());
    let rel = placeholder_rel_path(class);
    att.path = Some(rel.to_string());
    let ext = Path::new(rel)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin");
    att.original_name = Some(format!("attachment.{ext}"));
    if att.transcription.as_deref().is_some_and(|s| !s.is_empty()) {
        att.transcription = Some("[redacted]".into());
    }
    att.digest_sha256 = None;
    att.bytes = None;
}

pub(crate) struct TransformOutcome {
    pub obfuscated_docs: usize,
}

/// Apply the export transforms (media mode, obfuscation) to every document, optionally
/// loading attachment bytes first.
pub(crate) fn apply_transforms(
    docs: &mut [ConversationDocument],
    output_dir: &Path,
    transforms: &ExportTransforms,
    load_bytes: bool,
) -> Result<TransformOutcome> {
    // Keep MIME/path for placeholder classification when obfuscating.
    if !transforms.obfuscate {
        for doc in docs.iter_mut() {
            clear_attachments_when_disabled(doc, transforms.media);
        }
    }

    // Convert/compress runs in `run_attachment_jobs` before documents are
    // written. Finish only obfuscates and packages.
    let mut obfuscated_docs = 0usize;
    if transforms.obfuscate {
        materialize_placeholders(output_dir)?;
        let log_fn = |line: &str| emit_log(transforms.log.as_ref(), line);
        let mut anon =
            resolve_obfuscator_with_log(transforms.obfuscate_seed.as_deref(), Some(&log_fn))?;
        for doc in docs.iter_mut() {
            obfuscate_document(doc, &mut anon);
            obfuscated_docs += 1;
        }
    }

    if load_bytes {
        for doc in docs.iter_mut() {
            reload_attachment_bytes(doc, output_dir);
        }
    }

    Ok(TransformOutcome { obfuscated_docs })
}

#[cfg(test)]
mod tests {
    use super::*;
    use media::MediaMode;
    use message_ir::{
        ConversationMeta, ConversationStats, ExportMeta, IrConversationType, IrMessage,
        IrMessageKind, IrParticipant, IrService, SCHEMA_VERSION,
    };
    use std::fs;

    fn doc_with_image_attachment() -> ConversationDocument {
        ConversationDocument {
            schema_version: SCHEMA_VERSION,
            export: ExportMeta {
                source: "test".into(),
                tool: "test".into(),
                tool_version: "0".into(),
                owner_handle: None,
                owner_display_name: None,
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
            messages: vec![IrMessage {
                guid: "guid-1".into(),
                timestamp_unix_ms: 1_400_773_261_000,
                direction: IrDirection::Incoming,
                service: IrService::Sms,
                message_kind: IrMessageKind::Sms,
                sender_handle: Some("+15555550101".into()),
                sender_display_name: Some("Sam".into()),
                owner_handle: None,
                subject: None,
                text: "hi".into(),
                attachments: vec![IrAttachment {
                    path: Some("photo.jpg".into()),
                    original_name: Some("photo.jpg".into()),
                    mime_type: Some("image/jpeg".into()),
                    digest_sha256: None,
                    is_sticker: false,
                    transcription: None,
                    sticker_effect: None,
                    size_bytes: None,
                    missing_reason: None,
                    bytes: None,
                }],
                imessage: None,
                source: None,
            }],
            packaging_stem_suffix: None,
        }
    }

    #[test]
    fn obfuscate_drops_the_vendor_bag() {
        let mut doc = message_ir::testutil::sample_document("secret");
        assert!(doc.messages[0].source.is_some());
        let mut anon = Obfuscator::new([7u8; 32]);
        obfuscate_document(&mut doc, &mut anon);
        assert!(
            doc.messages[0].source.is_none(),
            "obfuscated output must not carry vendor fields"
        );
    }

    #[test]
    fn obfuscate_replaces_the_owner_address_on_each_message() {
        let mut doc = message_ir::testutil::sample_document("secret");
        doc.messages[0].owner_handle = Some("+15555550100".into());
        let mut anon = Obfuscator::new([7u8; 32]);
        obfuscate_document(&mut doc, &mut anon);
        let owner = doc.messages[0].owner_handle.as_deref();
        assert_ne!(owner, Some("+15555550100"));
        assert_eq!(
            owner,
            doc.export.owner_handle.as_deref(),
            "one address becomes one fake address wherever it appears"
        );
    }

    #[test]
    fn obfuscate_skips_staged_media_and_writes_placeholders() {
        let tmp = tempfile::tempdir().unwrap();
        let att = tmp.path().join("attachments");
        fs::create_dir_all(&att).unwrap();
        // Pretend an exporter staged a real file (should be removed).
        fs::write(att.join("real-photo.jpg"), b"REAL_JPEG_BYTES_SHOULD_GO").unwrap();

        let mut docs = vec![doc_with_image_attachment()];
        let transforms = ExportTransforms {
            media: MediaMode::Convert,
            obfuscate: true,
            obfuscate_seed: Some(
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
            ),
            ..ExportTransforms::none()
        };
        let outcome = apply_transforms(&mut docs, tmp.path(), &transforms, false).unwrap();
        assert_eq!(outcome.obfuscated_docs, 1);

        assert!(!att.join("real-photo.jpg").exists());
        assert!(att.join("placeholder.jpg").is_file());
        assert!(att.join("placeholder.mp4").is_file());
        assert!(att.join("placeholder.bin").is_file());
        assert_eq!(
            docs[0].messages[0].attachments[0].path.as_deref(),
            Some("attachments/placeholder.jpg")
        );
    }

    #[test]
    fn obfuscate_keeps_mime_when_media_disabled() {
        let tmp = tempfile::tempdir().unwrap();
        let mut docs = vec![doc_with_image_attachment()];
        let transforms = ExportTransforms {
            media: MediaMode::Disabled,
            obfuscate: true,
            obfuscate_seed: Some(
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
            ),
            ..ExportTransforms::none()
        };
        apply_transforms(&mut docs, tmp.path(), &transforms, false).unwrap();
        assert_eq!(
            docs[0].messages[0].attachments[0].path.as_deref(),
            Some("attachments/placeholder.jpg")
        );
    }

    #[test]
    fn convert_at_finish_leaves_cloned_file() {
        let tmp = tempfile::tempdir().unwrap();
        let att = tmp.path().join("attachments");
        fs::create_dir_all(&att).unwrap();
        fs::write(att.join("keep.bin"), b"already-cloned").unwrap();
        let mut docs = vec![doc_with_image_attachment()];
        docs[0].messages[0].attachments[0].path = Some("attachments/keep.bin".into());
        let transforms = ExportTransforms {
            media: MediaMode::Convert,
            ..ExportTransforms::none()
        };
        let outcome = apply_transforms(&mut docs, tmp.path(), &transforms, false).unwrap();
        assert_eq!(outcome.obfuscated_docs, 0);
        assert_eq!(
            docs[0].messages[0].attachments[0].path.as_deref(),
            Some("attachments/keep.bin")
        );
        assert_eq!(fs::read(att.join("keep.bin")).unwrap(), b"already-cloned");
    }
}
