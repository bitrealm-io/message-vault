//! Turn the records the `imessage-reader` program streams into the shared
//! conversation structure ([`ConversationDocument`]) and write them.
//!
//! The program has already classified every row; this side maps its fields
//! onto [`IrMessage`], groups messages by conversation, decides how each
//! attachment's bytes travel (staged as files, embedded, or not copied), and
//! runs the same writer every other exporter uses. For an encrypted backup
//! the bytes come back through the program one file at a time, because only
//! it holds the keys.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use imessage_reader_protocol::{
    Attachment as AttachmentRecord, AttachmentSource as SourceRecord,
    Conversation as ConversationRecord, Event, Imessage as ImessageRecord,
    Message as MessageRecord,
};
use message_ir::{
    ConversationDocument, ConversationMeta, ExportMeta, HandleType, IrAttachment,
    IrConversationType, IrDirection, IrImessage, IrMessage, IrMessageKind, IrParticipant,
    IrService, SCHEMA_VERSION, owner_sender,
};
use message_ir_format::{
    AttachmentSource, ConversationUnit, ExportWriter, ExportWriterParts, FormatSink,
    FormatSinkResult, WriteQueueOptions,
};
use message_vault_io_core::{
    MediaConfig, OutputFormat, ProgressEvent, stage_conversation_attachments,
};

use crate::{
    helper::Helper,
    run::{AttachmentEmbed, ExportOptions},
};

const EXPORT_SOURCE: &str = "imessage";
const EXPORT_TOOL: &str = "imessage-ir-exporter";
const CONVERSATION_PROGRESS_EVERY: usize = 100;

/// Messages accumulated for one Apple `chat_identifier` before projection.
struct PendingConversation {
    conversation_type: IrConversationType,
    group_title: Option<String>,
    participants: Vec<IrParticipant>,
    /// First non-empty `destination_caller_id` seen (used for `From`/`To` mapping).
    owner_handle: String,
    /// First non-empty owner display name (caller-id / Me).
    owner_display_name: Option<String>,
    messages: Vec<IrMessage>,
    /// Load keys in the same order as flattened `messages[].attachments`.
    attachment_loads: Vec<AttachmentLoad>,
}

/// How the shared runner should load one attachment after the stream.
enum AttachmentLoad {
    /// Read (or, for an encrypted backup, ask the program to decrypt) this
    /// path during the attachment pass.
    Path {
        path: PathBuf,
        size_hint: Option<u64>,
    },
    /// Already-resident bytes (handwriting SVG).
    Bytes(Vec<u8>),
    /// No source file.
    Missing,
}

/// What the stream produced.
struct Collected {
    conversations: BTreeMap<String, PendingConversation>,
    /// Attachment paths need the program to decrypt them.
    encrypted: bool,
}

/// Stream the program's records into conversations, then write the chosen
/// output format (JSON Lines, JSON, CSV, EML, MBOX, or XML).
///
/// # Errors
///
/// Returns an error when the program fails, a conversation cannot be
/// written, or the user cancels.
pub(crate) fn export(helper: &mut Helper, options: &ExportOptions) -> Result<FormatSinkResult> {
    let format = options.output_format;
    options.emit_log("");
    options.emit_log(format!(
        "Preparing {} messages in {}",
        format.as_str(),
        options.export_path.display(),
    ));

    // Open the sink before the stream. Attachment files are written after
    // the stream by the shared runner, so prior IR artifacts (including stale
    // `attachments/`) must be cleaned first, the same pattern as WhatsApp and
    // SMS Backup & Restore. A resumed run is the exception: what the
    // interrupted run wrote is exactly the work this one gets to skip.
    let ExportWriterParts {
        mut sink,
        attachments_dir,
        use_queue,
        ..
    } = ExportWriter::open(
        &options.export_path,
        format,
        options.transforms.clone(),
        options.resume,
    )
    .map_err(|e| anyhow!("open export sink: {e:#}"))?
    .into_parts();

    let mut collected = collect(helper, options)?;
    options.check_cancel()?;

    if format.is_mail_archive() && options.attachment_embed == AttachmentEmbed::Embed {
        embed_attachment_bytes(helper, options, &mut collected)?;
    }

    // The queue-or-sink decision came from `ExportWriter::open`: JSONL
    // without obfuscation is the import path and drains the write queue;
    // everything else keeps the sink path.
    if use_queue {
        return drain_conversations(helper, options, collected);
    }
    if is_file_backed(format) {
        stage_attachments(helper, options, &mut collected, &attachments_dir)?;
    }
    write_conversations(options, &mut sink, collected.conversations)?;
    sink.finish()
        .map_err(|e| anyhow!("finish export sink: {e:#}"))
}

/// Formats whose attachments are files under `attachments/` rather than
/// bytes embedded in the document.
fn is_file_backed(format: OutputFormat) -> bool {
    matches!(
        format,
        OutputFormat::Csv | OutputFormat::Json | OutputFormat::Jsonl | OutputFormat::Xml
    )
}

/// Whether this run leaves attachment files for `run_attachment_jobs` to
/// load and write later.
fn stages_attachment_files(options: &ExportOptions) -> bool {
    options.transforms.copies_attachments() && is_file_backed(options.output_format)
}

/// Read events until the program says the export is done, grouping messages
/// by conversation. Cancel is checked on every event so a cancelled run
/// stops within one message.
fn collect(helper: &mut Helper, options: &ExportOptions) -> Result<Collected> {
    let mut conversations: BTreeMap<String, PendingConversation> = BTreeMap::new();
    let mut encrypted = false;
    let stages_files = stages_attachment_files(options);
    let embed = options.attachment_embed;
    loop {
        options.check_cancel()?;
        match helper.next_event()? {
            Event::Source {
                encrypted: flag, ..
            } => encrypted = flag,
            Event::Conversation(record) => {
                conversations
                    .entry(record.chat_identifier.clone())
                    .or_insert_with(|| pending_from_record(record));
            }
            Event::Message(record) => {
                let convo = conversations
                    .get_mut(&record.chat_identifier)
                    .ok_or_else(|| {
                        anyhow!(
                            "imessage-reader sent a message for {} before its conversation",
                            record.chat_identifier
                        )
                    })?;
                if convo.owner_handle.is_empty() && !record.owner_handle.is_empty() {
                    convo.owner_handle.clone_from(&record.owner_handle);
                }
                if convo.owner_display_name.is_none() {
                    convo
                        .owner_display_name
                        .clone_from(&record.owner_display_name);
                }
                let (message, loads) = message_to_ir(*record, embed, stages_files);
                convo.attachment_loads.extend(loads);
                convo.messages.push(message);
            }
            Event::ExportDone { .. } => break,
            other => bail!("imessage-reader sent {other:?} in the middle of an export"),
        }
    }
    Ok(Collected {
        conversations,
        encrypted,
    })
}

/// A conversation's roster, as the program described it.
fn pending_from_record(record: ConversationRecord) -> PendingConversation {
    PendingConversation {
        conversation_type: IrConversationType::parse(&record.conversation_type),
        group_title: record.group_title,
        participants: record
            .participants
            .into_iter()
            .map(|p| IrParticipant {
                handle_type: Some(handle_type_for(&p.handle)),
                handle: Some(p.handle),
                display_name: p.display_name,
            })
            .collect(),
        owner_handle: String::new(),
        owner_display_name: None,
        messages: Vec::new(),
        attachment_loads: Vec::new(),
    }
}

/// iMessage stores handles as phone numbers or email addresses without
/// recording which; infer the type from the handle shape.
fn handle_type_for(handle: &str) -> HandleType {
    if handle.contains('@') {
        HandleType::Email
    } else {
        HandleType::Phone
    }
}

/// One record as an [`IrMessage`] plus the load key for each attachment.
fn message_to_ir(
    record: MessageRecord,
    embed: AttachmentEmbed,
    stages_files: bool,
) -> (IrMessage, Vec<AttachmentLoad>) {
    let direction = if record.outgoing {
        IrDirection::Outgoing
    } else {
        IrDirection::Incoming
    };
    let (sender_handle, sender_display_name) = match direction {
        IrDirection::Outgoing => owner_sender(&ExportMeta {
            source: EXPORT_SOURCE.into(),
            tool: EXPORT_TOOL.into(),
            tool_version: env!("CARGO_PKG_VERSION").into(),
            owner_handle: (!record.owner_handle.is_empty()).then(|| record.owner_handle.clone()),
            owner_display_name: record.owner_display_name.clone(),
        }),
        IrDirection::Incoming => (record.sender_handle, record.sender_display_name),
    };

    let mut loads = Vec::with_capacity(record.attachments.len());
    let attachments = record
        .attachments
        .into_iter()
        .map(|attachment| {
            let (attachment, load) = attachment_to_ir(attachment, embed, stages_files);
            loads.push(load);
            attachment
        })
        .collect();

    let message = IrMessage {
        guid: record.guid,
        timestamp_unix_ms: record.timestamp_unix_ms,
        direction,
        service: IrService::parse(&record.service),
        message_kind: IrMessageKind::parse(&record.message_kind),
        sender_handle,
        sender_display_name,
        subject: record.subject,
        text: record.text,
        attachments,
        imessage: record.imessage.map(imessage_to_ir),
        source: None,
    };
    (message, loads)
}

/// The Apple-specific fields, field for field.
fn imessage_to_ir(fields: ImessageRecord) -> IrImessage {
    IrImessage {
        is_reply: fields.is_reply,
        in_reply_to_guid: fields.in_reply_to_guid,
        thread_originator_part: fields.thread_originator_part,
        num_replies: fields.num_replies,
        is_deleted: fields.is_deleted,
        send_effect: fields.send_effect,
        shared_location: fields.shared_location,
        announcement: fields.announcement,
        read_receipt_rfc3339: fields.read_receipt_rfc3339,
        parts: fields.parts,
        edits: fields.edits,
        tapbacks: fields.tapbacks,
        app: fields.app,
        balloon_bundle_id: fields.balloon_bundle_id,
        balloon_kind: fields.balloon_kind,
        associated_guid: fields.associated_guid,
        associated_part: fields.associated_part,
        tapback_kind: fields.tapback_kind,
        tapback_emoji: fields.tapback_emoji,
        tapback_action: fields.tapback_action,
    }
}

/// One attachment's shared-structure record and how its bytes will arrive.
///
/// With embedding off, nothing is loaded and the record says `not_copied`.
/// Otherwise the record carries the size the database knew, which is the
/// progress hint for the staging pass. When files are staged (CSV / JSON /
/// JSON Lines / XML with attachment copying on), the runner loads and
/// writes them under `attachments/` after the stream, so only the load key
/// travels here and the runner says `file_missing` itself. A mail archive
/// embeds bytes in the document; those are loaded by
/// [`embed_attachment_bytes`] once the stream ends. Any other run copies
/// nothing, so the record says `file_missing` when there is no file.
fn attachment_to_ir(
    attachment: AttachmentRecord,
    embed: AttachmentEmbed,
    stages_files: bool,
) -> (IrAttachment, AttachmentLoad) {
    let mut ir = IrAttachment {
        path: None,
        original_name: attachment.original_name,
        mime_type: attachment.mime_type,
        digest_sha256: None,
        is_sticker: attachment.is_sticker,
        transcription: attachment.transcription,
        sticker_effect: attachment.sticker_effect,
        size_bytes: None,
        missing_reason: None,
        bytes: None,
    };
    if embed == AttachmentEmbed::Disabled {
        ir.missing_reason = Some("not_copied".to_string());
        return (ir, AttachmentLoad::Missing);
    }
    let load = match attachment.source {
        SourceRecord::Path { path, size_hint } => AttachmentLoad::Path { path, size_hint },
        SourceRecord::Inline { text } => AttachmentLoad::Bytes(text.into_bytes()),
        SourceRecord::Missing => AttachmentLoad::Missing,
    };
    match &load {
        AttachmentLoad::Path { size_hint, .. } => ir.size_bytes = *size_hint,
        AttachmentLoad::Bytes(bytes) => ir.size_bytes = Some(bytes.len() as u64),
        // When files are staged the runner says `file_missing` itself.
        AttachmentLoad::Missing if !stages_files => {
            ir.missing_reason = Some("file_missing".to_string());
        }
        AttachmentLoad::Missing => {}
    }
    (ir, load)
}

/// Read one attachment's bytes: through the program for an encrypted backup,
/// straight from disk otherwise. Empty bytes mean the file is not there, and
/// the reason is already on the log.
fn read_attachment(
    helper: &mut Helper,
    options: &ExportOptions,
    encrypted: bool,
    path: &Path,
) -> Result<Vec<u8>, String> {
    if encrypted {
        let Some(temp) = helper
            .decrypt_attachment(path)
            .map_err(|e| format!("{e:#}"))?
        else {
            return Ok(Vec::new());
        };
        let bytes = fs::read(&temp).map_err(|e| format!("read {}: {e}", temp.display()));
        if let Err(why) = fs::remove_file(&temp) {
            options.emit_log(format!(
                "Unable to remove decrypted temp file {}: {why}",
                temp.display()
            ));
        }
        return bytes;
    }
    if !path.is_file() {
        return Ok(Vec::new());
    }
    match fs::read(path) {
        Ok(bytes) => Ok(bytes),
        Err(e) => {
            options.emit_log(format!(
                "warning: failed to read attachment {}: {e}",
                path.display()
            ));
            Ok(Vec::new())
        }
    }
}

/// Load a mail archive's attachment bytes onto the documents so the EML /
/// MBOX writer can embed them as MIME parts.
fn embed_attachment_bytes(
    helper: &mut Helper,
    options: &ExportOptions,
    collected: &mut Collected,
) -> Result<()> {
    let encrypted = collected.encrypted;
    for convo in collected.conversations.values_mut() {
        let mut loads = std::mem::take(&mut convo.attachment_loads).into_iter();
        for message in &mut convo.messages {
            for attachment in &mut message.attachments {
                options.check_cancel()?;
                let bytes = match loads.next() {
                    Some(AttachmentLoad::Path { path, .. }) => {
                        read_attachment(helper, options, encrypted, &path)
                            .map_err(|e| anyhow!("attachment {}: {e}", path.display()))?
                    }
                    Some(AttachmentLoad::Bytes(bytes)) => bytes,
                    Some(AttachmentLoad::Missing) | None => Vec::new(),
                };
                if bytes.is_empty() {
                    attachment.missing_reason = Some("file_missing".to_string());
                } else {
                    attachment.size_bytes = Some(bytes.len() as u64);
                    attachment.bytes = Some(bytes);
                }
            }
        }
    }
    Ok(())
}

/// Write every non-empty conversation through the sink, reporting progress.
fn write_conversations(
    options: &ExportOptions,
    sink: &mut FormatSink,
    conversations: BTreeMap<String, PendingConversation>,
) -> Result<()> {
    let format = options.output_format;
    let total = conversations.len();
    options.emit_log("");
    options.emit_log(format!("Preparing {total} conversation file(s)..."));
    options.emit_progress(ProgressEvent::Prepare { done: 0, total });
    let mut written = 0usize;
    for (chat_identifier, convo) in conversations {
        options.check_cancel()?;
        written += 1;
        if convo.messages.is_empty() {
            continue;
        }
        let doc = pending_to_document(chat_identifier, convo, options.request.use_caller_id);
        let document_id = doc.conversation.chat_identifier.clone();
        sink.write_document(doc)
            .map_err(|e| anyhow!("write {} for {}: {e:#}", format.as_str(), document_id))?;
        if written.is_multiple_of(CONVERSATION_PROGRESS_EVERY) || written == total {
            options.emit_log(format!("  preparing {written}/{total}"));
            options.emit_progress(ProgressEvent::Prepare {
                done: written,
                total,
            });
        }
    }
    Ok(())
}

/// Project one accumulated conversation into the shared document shape.
fn pending_to_document(
    chat_identifier: String,
    convo: PendingConversation,
    use_caller_id: bool,
) -> ConversationDocument {
    let export = ExportMeta {
        source: EXPORT_SOURCE.into(),
        tool: EXPORT_TOOL.into(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
        owner_handle: (!convo.owner_handle.is_empty()).then(|| convo.owner_handle.clone()),
        owner_display_name: convo
            .owner_display_name
            .or_else(|| use_caller_id.then(|| "Me".to_string())),
    };
    let (owner_handle, owner_display_name) = owner_sender(&export);
    let mut messages = convo.messages;
    for msg in &mut messages {
        if msg.direction == IrDirection::Outgoing {
            msg.sender_handle.clone_from(&owner_handle);
            msg.sender_display_name.clone_from(&owner_display_name);
        }
    }
    ConversationDocument {
        schema_version: SCHEMA_VERSION,
        export,
        conversation: ConversationMeta {
            chat_identifier,
            conversation_type: convo.conversation_type,
            group_title: convo.group_title,
            participants: convo.participants,
            stats: Default::default(),
        },
        messages,
        packaging_stem_suffix: None,
    }
}

/// Pair a conversation's document with its attachment sources.
///
/// `attachment_loads` is positional: it runs in the same order as the
/// conversation's flattened `messages[].attachments`, so the sources are
/// consumed in that order and land on the attachment each was collected for.
fn pending_to_unit(
    chat_identifier: String,
    mut convo: PendingConversation,
    use_caller_id: bool,
) -> ConversationUnit {
    let loads = std::mem::take(&mut convo.attachment_loads);
    let doc = pending_to_document(chat_identifier, convo, use_caller_id);
    let mut loads = loads.into_iter();
    ConversationUnit::from_doc(doc, |_, att| match loads.next() {
        Some(AttachmentLoad::Path { path, size_hint }) => (AttachmentSource::Path(path), size_hint),
        Some(AttachmentLoad::Bytes(bytes)) => {
            let hint = Some(bytes.len() as u64);
            (AttachmentSource::Bytes(bytes), hint)
        }
        Some(AttachmentLoad::Missing) | None => (AttachmentSource::Missing, att.size_bytes),
    })
}

/// Write every conversation through the shared write queue.
fn drain_conversations(
    helper: &mut Helper,
    options: &ExportOptions,
    collected: Collected,
) -> Result<FormatSinkResult> {
    let use_caller_id = options.request.use_caller_id;
    let units: Vec<ConversationUnit> = collected
        .conversations
        .into_iter()
        .filter(|(_, convo)| !convo.messages.is_empty())
        .map(|(chat_identifier, convo)| pending_to_unit(chat_identifier, convo, use_caller_id))
        .collect();

    let queue = WriteQueueOptions {
        media: options.transforms.media,
        compress: options.transforms.compress.clone(),
        resume: options.resume,
        writer_count: 0,
    };
    let log = options.log.clone();
    let progress = options.progress.clone();
    let cancel = options.cancel.as_ref();

    let report = if collected.encrypted {
        // The program decrypts one file at a time over one pipe, so the
        // drain runs on one writer. Decrypt-bound throughput would not have
        // parallelized well anyway.
        let mut load = |source: &mut AttachmentSource| match source {
            AttachmentSource::Path(path) => {
                let bytes = read_attachment(helper, options, true, path).map_err(|e| {
                    // Say why before it becomes a chip: a systemic failure
                    // otherwise reads as a run's worth of unexplained gaps.
                    options.emit_log(format!(
                        "warning: attachment {} could not be read: {e}",
                        path.display()
                    ));
                    e
                })?;
                Ok((!bytes.is_empty()).then_some(bytes))
            }
            other => message_ir_format::load_attachment_source(other),
        };
        message_ir_format::drain_write_queue_with_loader(
            &options.export_path,
            units,
            &queue,
            &mut load,
            log.as_ref(),
            progress.as_ref(),
            cancel,
        )
    } else {
        message_ir_format::drain_write_queue(
            &options.export_path,
            units,
            &queue,
            log.as_ref(),
            progress.as_ref(),
            cancel,
        )
    }
    .map_err(|e| anyhow!("write conversations: {e:#}"))?;

    Ok(FormatSinkResult {
        xml_path: None,
        media: report.media,
        obfuscated_docs: 0,
    })
}

/// Write staged attachment bytes after the stream and before conversation
/// files, through the shared step every exporter uses. The loads travel in
/// the same order as the flattened `messages[].attachments`, which is the
/// order the step hands out indexes in.
fn stage_attachments(
    helper: &mut Helper,
    options: &ExportOptions,
    collected: &mut Collected,
    attachments_dir: &Path,
) -> Result<()> {
    let media = MediaConfig {
        mode: options.transforms.media,
        compress: options.transforms.compress.clone(),
    };
    let encrypted = collected.encrypted;
    let mut loads = Vec::new();
    for convo in collected.conversations.values_mut() {
        loads.append(&mut convo.attachment_loads);
    }

    // The shared step calls the loader from one thread, so the program
    // handle can be borrowed by the closure for the whole pass.
    let helper = std::cell::RefCell::new(helper);
    let saved = stage_conversation_attachments(
        collected
            .conversations
            .values_mut()
            .flat_map(|convo| convo.messages.iter_mut()),
        attachments_dir,
        &media,
        |i| match loads.get(i) {
            Some(AttachmentLoad::Path { path, .. }) => {
                let bytes = read_attachment(&mut helper.borrow_mut(), options, encrypted, path)
                    .map_err(|e| {
                        // The shared step turns any Err other than "canceled" into a
                        // file_missing attachment and moves on. Log the real reason here
                        // first, or a systemic failure (a revoked Full Disk Access, a
                        // failing disk) degrades into a run's worth of unexplained chips.
                        options.emit_log(format!(
                            "warning: attachment {} could not be read: {e}",
                            path.display()
                        ));
                        e
                    })?;
                Ok((!bytes.is_empty()).then_some(bytes))
            }
            Some(AttachmentLoad::Bytes(bytes)) => Ok(Some(bytes.clone())),
            _ => Ok(None),
        },
        options.log.as_ref(),
        options.progress.as_ref(),
        options.cancel.as_ref(),
    )
    .map_err(|e| anyhow!(e))
    .context("stage attachments")?;
    if saved > 0 {
        options.emit_log(format!("  saved {saved} attachments"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record_with_attachment(source: SourceRecord) -> AttachmentRecord {
        AttachmentRecord {
            original_name: Some("a.jpg".into()),
            mime_type: Some("image/jpeg".into()),
            is_sticker: false,
            transcription: None,
            sticker_effect: None,
            source,
        }
    }

    fn path_source() -> SourceRecord {
        SourceRecord::Path {
            path: PathBuf::from("/nowhere/a.jpg"),
            size_hint: Some(11),
        }
    }

    #[test]
    fn disabled_embedding_marks_not_copied() {
        let (ir, load) = attachment_to_ir(
            record_with_attachment(path_source()),
            AttachmentEmbed::Disabled,
            true,
        );
        assert_eq!(ir.missing_reason.as_deref(), Some("not_copied"));
        assert!(matches!(load, AttachmentLoad::Missing));
    }

    #[test]
    fn staged_files_carry_the_size_hint_and_defer_the_rest_to_the_runner() {
        let (ir, load) = attachment_to_ir(
            record_with_attachment(path_source()),
            AttachmentEmbed::Embed,
            true,
        );
        assert_eq!(ir.missing_reason, None);
        assert_eq!(ir.size_bytes, Some(11));
        assert!(ir.bytes.is_none());
        match load {
            AttachmentLoad::Path { path, size_hint } => {
                assert_eq!(path, PathBuf::from("/nowhere/a.jpg"));
                assert_eq!(size_hint, Some(11));
            }
            _ => panic!("path source lost"),
        }
    }

    #[test]
    fn unstaged_records_carry_size_and_missing_reason() {
        let (ir, _) = attachment_to_ir(
            record_with_attachment(path_source()),
            AttachmentEmbed::Embed,
            false,
        );
        assert_eq!(ir.size_bytes, Some(11));
        assert_eq!(ir.missing_reason, None);

        let (ir, _) = attachment_to_ir(
            record_with_attachment(SourceRecord::Missing),
            AttachmentEmbed::Embed,
            false,
        );
        assert_eq!(ir.missing_reason.as_deref(), Some("file_missing"));

        let (ir, load) = attachment_to_ir(
            record_with_attachment(SourceRecord::Inline {
                text: "<svg/>".into(),
            }),
            AttachmentEmbed::Embed,
            false,
        );
        assert_eq!(ir.size_bytes, Some(6));
        assert!(matches!(load, AttachmentLoad::Bytes(b) if b == b"<svg/>"));
    }

    fn message_record(chat: &str, guid: &str, outgoing: bool) -> MessageRecord {
        MessageRecord {
            chat_identifier: chat.into(),
            guid: guid.into(),
            timestamp_unix_ms: 1_609_459_200_000,
            outgoing,
            service: "iMessage".into(),
            message_kind: "imessage".into(),
            sender_handle: (!outgoing).then(|| "+15555550122".to_string()),
            sender_display_name: None,
            subject: None,
            text: "hi".into(),
            owner_handle: "+15555550100".into(),
            owner_display_name: None,
            imessage: None,
            attachments: Vec::new(),
        }
    }

    #[test]
    fn outgoing_rows_take_the_owner_as_sender() {
        let (incoming, _) = message_to_ir(
            message_record("+15555550122", "g1", false),
            AttachmentEmbed::Embed,
            true,
        );
        assert_eq!(incoming.direction, IrDirection::Incoming);
        assert_eq!(incoming.sender_handle.as_deref(), Some("+15555550122"));
        assert_eq!(incoming.service, IrService::IMessage);
        assert_eq!(incoming.message_kind, IrMessageKind::IMessage);

        let (outgoing, _) = message_to_ir(
            message_record("+15555550122", "g2", true),
            AttachmentEmbed::Embed,
            true,
        );
        assert_eq!(outgoing.direction, IrDirection::Outgoing);
        assert_eq!(outgoing.sender_handle.as_deref(), Some("+15555550100"));
        assert_eq!(outgoing.sender_display_name.as_deref(), Some("Me"));
    }

    /// A bare message carrying `count` attachments, for pairing tests.
    fn msg_with_attachments(ts: i64, count: usize) -> IrMessage {
        IrMessage {
            guid: format!("guid-{ts}"),
            timestamp_unix_ms: ts,
            direction: IrDirection::Incoming,
            service: IrService::IMessage,
            message_kind: IrMessageKind::IMessage,
            sender_handle: Some("+15555550101".into()),
            sender_display_name: None,
            subject: None,
            text: "hi".into(),
            attachments: (0..count)
                .map(|i| IrAttachment {
                    path: None,
                    original_name: Some(format!("a{i}.jpg")),
                    mime_type: None,
                    digest_sha256: None,
                    is_sticker: false,
                    transcription: None,
                    sticker_effect: None,
                    size_bytes: None,
                    missing_reason: None,
                    bytes: None,
                })
                .collect(),
            imessage: None,
            source: None,
        }
    }

    #[test]
    fn unit_sources_land_on_the_attachment_each_was_collected_for() {
        // attachment_loads is positional against the conversation's flattened
        // attachments, so the first load belongs to the first message's
        // attachment and the second to the next message's.
        let first = PathBuf::from("first.jpg");
        let convo = PendingConversation {
            conversation_type: IrConversationType::Individual,
            group_title: None,
            participants: Vec::new(),
            owner_handle: String::new(),
            owner_display_name: None,
            messages: vec![msg_with_attachments(1000, 1), msg_with_attachments(2000, 1)],
            attachment_loads: vec![
                AttachmentLoad::Path {
                    path: first.clone(),
                    size_hint: Some(11),
                },
                AttachmentLoad::Bytes(b"second".to_vec()),
            ],
        };

        let unit = pending_to_unit("+15555550101".into(), convo, false);

        assert_eq!(unit.attachments.len(), 2);
        assert_eq!(unit.attachments[0].message_index, 0);
        assert_eq!(unit.attachments[0].attachment_index, 0);
        assert_eq!(unit.attachments[0].timestamp_unix_ms, 1000);
        assert_eq!(unit.attachments[0].size_hint, Some(11));
        match &unit.attachments[0].source {
            AttachmentSource::Path(p) => assert_eq!(p, &first),
            other => panic!("first attachment lost its path source: {other:?}"),
        }

        assert_eq!(unit.attachments[1].message_index, 1);
        assert_eq!(unit.attachments[1].timestamp_unix_ms, 2000);
        match &unit.attachments[1].source {
            AttachmentSource::Bytes(b) => assert_eq!(b, b"second"),
            other => panic!("second attachment lost its bytes source: {other:?}"),
        }
    }
}
