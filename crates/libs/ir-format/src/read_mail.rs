//! Read an EML folder or mboxrd mailbox back into a [`ConversationDocument`].

use anyhow::{Context, Result, bail};
use mail::{MailMessage, mail_message_from_eml_bytes, mail_messages_from_mbox};
use message_crate_core::discover_files;
use message_ir::{
    ConversationDocument, ConversationMeta, ConversationStats, ExportMeta, IrAttachment,
    IrConversationType, IrMessage, IrParticipant, SCHEMA_VERSION, nonempty,
};
use std::fs;
use std::path::Path;

/// Scan a conversation directory of `.eml` files into a conversation document.
///
/// # Errors
///
/// Returns an error when `dir` is not a directory, no `.eml` files are found,
/// or a file cannot be read or parsed.
pub fn read_conversation_eml_dir(dir: &Path) -> Result<ConversationDocument> {
    if !dir.is_dir() {
        bail!("not a directory: {}", dir.display());
    }
    let mut paths = discover_files(dir, &|p| {
        p.extension()
            .and_then(|x| x.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("eml"))
    })
    .with_context(|| format!("read {}", dir.display()))?;
    paths.sort();
    if paths.is_empty() {
        bail!("no .eml files in {}", dir.display());
    }

    let mut mail_messages = Vec::with_capacity(paths.len());
    for path in &paths {
        let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
        let msg = mail_message_from_eml_bytes(&bytes)
            .with_context(|| format!("parse {}", path.display()))?;
        mail_messages.push(msg);
    }
    mail_messages.sort_by(cmp_mail_messages);

    let packaging = crate::util::packaging_suffix_from_stem(
        dir.file_name().and_then(|n| n.to_str()).unwrap_or_default(),
    );
    document_from_mail_messages(&mail_messages, packaging)
}

/// Read a conversation `.mbox` (mboxrd) into a conversation document.
///
/// # Errors
///
/// Returns an error when the mailbox cannot be read or contains no messages.
pub fn read_conversation_mbox(path: &Path) -> Result<ConversationDocument> {
    let mut mail_messages = mail_messages_from_mbox(path)?;
    if mail_messages.is_empty() {
        bail!("mbox has no messages: {}", path.display());
    }
    mail_messages.sort_by(cmp_mail_messages);
    let packaging = crate::util::packaging_suffix_from_stem(
        path.file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or_default(),
    );
    document_from_mail_messages(&mail_messages, packaging)
}

/// Order mail messages by timestamp, then GUID, so both readers agree.
fn cmp_mail_messages(a: &MailMessage, b: &MailMessage) -> std::cmp::Ordering {
    a.message
        .timestamp_unix_ms
        .cmp(&b.message.timestamp_unix_ms)
        .then_with(|| a.message.guid.cmp(&b.message.guid))
}

/// Map a list of [`MailMessage`] values from one conversation into a
/// [`ConversationDocument`].
///
/// # Errors
///
/// Returns an error when `messages` is empty.
fn document_from_mail_messages(
    messages: &[MailMessage],
    packaging_stem_suffix: Option<String>,
) -> Result<ConversationDocument> {
    if messages.is_empty() {
        bail!("document_from_mail_messages requires at least one message");
    }
    let first = &messages[0];
    let export = ExportMeta {
        source: first.export_source.clone(),
        tool: first.export_tool.clone(),
        tool_version: first.export_tool_version.clone(),
        owner_handle: nonempty(&first.owner_handle),
        owner_display_name: first.owner_display_name.as_deref().and_then(nonempty),
    };

    let participants: Vec<IrParticipant> = first
        .participants
        .iter()
        .map(participant_from_mail)
        .collect();

    let ir_messages: Vec<IrMessage> = messages.iter().map(ir_message_from_mail).collect();

    let mut doc = ConversationDocument {
        schema_version: SCHEMA_VERSION,
        export,
        conversation: ConversationMeta {
            chat_identifier: first.chat_identifier.clone(),
            conversation_type: IrConversationType::parse(&first.conversation_type),
            group_title: first.group_title.as_deref().and_then(nonempty),
            participants,
            stats: ConversationStats::default(),
        },
        messages: ir_messages,
        packaging_stem_suffix,
    };
    doc.finalize_stats();
    Ok(doc)
}

/// Build an [`IrParticipant`] from a mail participant.
///
/// EML and mbox do not store a handle type, so the type is inferred from the
/// handle string (`@` → email, digit-heavy → phone, else other).
fn participant_from_mail(p: &mail::Participant) -> IrParticipant {
    IrParticipant {
        handle: nonempty(&p.handle),
        display_name: p.display_name.as_deref().and_then(nonempty),
        handle_type: if p.handle.trim().is_empty() {
            None
        } else {
            Some(crate::util::infer_handle_type(&p.handle))
        },
    }
}

/// Map one mail message into the shared conversation message type.
///
/// The message already travels as [`IrMessage`]; only the attachment list is
/// rebuilt, from the parsed MIME parts (which carry the bytes).
fn ir_message_from_mail(msg: &MailMessage) -> IrMessage {
    let mut out = msg.message.clone();
    out.attachments = msg.attachments.iter().map(attachment_from_mail).collect();
    out
}

/// Map one mail attachment into [`IrAttachment`].
fn attachment_from_mail(a: &mail::MailAttachment) -> IrAttachment {
    let mut att: IrAttachment = a.into();
    att.bytes = if a.bytes.is_empty() {
        None
    } else {
        Some(a.bytes.clone())
    };
    att
}
