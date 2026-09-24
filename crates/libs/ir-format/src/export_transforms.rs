//! Media convert/compress and obfuscation applied before writing files.

use anyhow::Result;
use media::MediaMode;
use message_ir::{ConversationDocument, IrAttachment, IrDirection, IrImessage, IrParticipant};
use message_vault_io_core::{ExportTransforms, emit_log};
use obfuscate::{
    Obfuscator, classify_attachment, materialize_placeholders, placeholder_rel_path,
    resolve_obfuscator_with_log,
};
use serde_json::{Map, Value};
use std::path::Path;

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
    // A group title is chosen by people and often names them, so every word
    // goes, not only the addresses in it.
    if let Some(title) = doc.conversation.group_title.as_mut() {
        *title = anon.obfuscate_text(title);
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
        if let Some(im) = msg.imessage.as_mut() {
            obfuscate_imessage(im, anon);
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

/// Obfuscate the iMessage extension's announcement and tapback reactors.
///
/// `parts` repeats the body, `edits` holds its earlier wording, `app` holds
/// link previews, and `shared_location` holds a place. None of them is read
/// on import, so obfuscated output drops them whole.
fn obfuscate_imessage(im: &mut IrImessage, anon: &mut Obfuscator) {
    if let Some(a) = im.announcement.as_mut() {
        *a = anon.obfuscate_text(a);
    }
    im.parts = None;
    im.edits = None;
    im.app = None;
    im.shared_location = None;
    // Tapbacks are imported, so they stay with only the reactor rewritten.
    // Anything that is not a tapback object goes.
    match im.tapbacks.as_mut() {
        Some(Value::Array(tapbacks)) => {
            tapbacks.retain_mut(|tapback| match tapback {
                Value::Object(fields) => {
                    obfuscate_tapback(fields, anon);
                    true
                }
                _ => false,
            });
        }
        Some(Value::Object(fields)) => obfuscate_tapback(fields, anon),
        _ => im.tapbacks = None,
    }
}

/// Obfuscate who reacted, keep what the reaction was, and drop any other key.
fn obfuscate_tapback(fields: &mut Map<String, Value>, anon: &mut Obfuscator) {
    fields.retain(|key, value| {
        match (key.as_str(), value) {
            ("part_index" | "kind" | "emoji" | "is_from_me", _) => {}
            ("reactor_handle" | "sender", Value::String(h)) => *h = anon.obfuscate_handle(h),
            ("reactor_display_name", Value::String(n)) if n != "Me" => {
                *n = anon.obfuscate_display_name(n);
            }
            ("reactor_display_name", Value::String(_)) => {}
            _ => return false,
        }
        true
    });
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

/// Apply the export transforms (media mode, obfuscation) to every document.
///
/// Attachment bytes are not loaded here. Every writer that embeds them reads
/// each file through [`crate::load_attachment_bytes`] before the sink removes
/// the staged `attachments/`.
pub(crate) fn apply_transforms(
    docs: &mut [ConversationDocument],
    output_dir: &Path,
    transforms: &ExportTransforms,
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

    Ok(TransformOutcome { obfuscated_docs })
}

#[cfg(test)]
mod tests;
