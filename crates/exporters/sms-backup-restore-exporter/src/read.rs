//! Read SMS Backup & Restore XML into [`ConversationDocument`] values.

use anyhow::{Result, bail};
use media::{CompressOptions, MediaMode};
use message_crate_core::{
    CancelFlag, LogSink, MediaConfig, ProgressSink, check_cancel, discover_files,
    document_messages, is_cancelled, stage_conversation_attachments,
};
use message_csv::format_local_ts;
use message_ir::{
    ConversationDocument, ConversationMeta, ConversationStats, ExportMeta, HandleType,
    IrAttachment, IrConversationType, IrDirection, IrMessage, IrMessageKind, IrParticipant,
    IrService, IrSource, MessageCopy, MessageGuid, MessageIdentity, SCHEMA_VERSION, TimePrecision,
    one_copy_per_message, owner_sender,
};
use message_staging::{AttachmentSpool, load_attachment_source};
use phone::{Handle, OwnerHandleSet};
use sbr::{
    AttachmentBlob, ConversationKind, ParseStats, Record, infer_owner_phones, parse_file_with,
};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

const EXPORT_SOURCE: &str = "sms-backup-restore";
const EXPORT_TOOL: &str = "SMS Backup & Restore";
const EXPORT_TOOL_VERSION: &str = "10.26.003";

/// Counts from parsing SMS Backup & Restore XML into conversation documents.
#[derive(Debug, Default)]
pub struct ReadReport {
    /// Number of conversation documents produced.
    pub conversations: u64,
    /// SMS elements parsed.
    pub sms_seen: u64,
    /// MMS elements parsed.
    pub mms_seen: u64,
    /// Attachment files staged under `attachments/`.
    pub attachments_saved: u64,
    /// Outgoing messages in produced documents.
    pub sent: u64,
    /// Incoming messages in produced documents.
    pub received: u64,
    /// Messages dropped for an invalid date.
    pub skipped_invalid_date: u64,
    /// Messages dropped outside the configured date range.
    pub skipped_out_of_range: u64,
    /// Messages dropped with no usable address.
    pub skipped_unknown_address: u64,
    /// SMS dropped for an unknown `type`.
    pub skipped_unknown_type: u64,
    /// Draft/outbox/failed/queued messages dropped.
    pub skipped_draft_or_outbox: u64,
    /// MMS dropped with no participants.
    pub skipped_empty_participants: u64,
    /// Parts with undecodable base64.
    pub skipped_bad_attachment: u64,
    /// Character references dropped because they are not a character.
    pub dropped_character_references: u64,
    /// Per-file error messages from parsing/staging.
    pub errors: Vec<String>,
}

/// Options for [`read_backup`].
#[derive(Debug)]
pub struct ReadOptions<'a> {
    /// Known owner phone numbers (empty triggers inference).
    pub owner_phones: &'a [String],
    /// Directory staged attachments are written to.
    pub attachments_dir: Option<&'a Path>,
    /// Where each attachment payload is written the moment its record is
    /// parsed, so no payload stays in memory; `None` when the run does not
    /// copy attachments, and then no payload is kept at all.
    pub spool: Option<&'a AttachmentSpool>,
    /// Whether to stage the spooled attachments into `attachments_dir`
    /// here. `false` leaves them in the spool for a caller that stages them
    /// itself; the write queue does, one conversation at a time.
    pub stage_attachments: bool,
    /// How to write attachment files after parse.
    pub media: MediaMode,
    /// Image/video compress settings used when `media` converts or compresses.
    pub compress: CompressOptions,
    /// Human-readable notes and warnings while reading.
    pub log: Option<&'a LogSink>,
    /// Typed progress events while staging attachments.
    pub progress: Option<&'a ProgressSink>,
    /// Cancellation flag checked between files.
    pub cancel: Option<&'a CancelFlag>,
}

#[derive(Debug, Clone)]
struct PendingAttachment {
    original_name: Option<String>,
    mime_type: Option<String>,
    digest: String,
    size_bytes: u64,
}

#[derive(Debug, Clone)]
struct PendingMessage {
    sort_key: f64,
    is_from_me: bool,
    /// The sender's handle key, for an incoming message.
    sender: Option<String>,
    sender_display_name: Option<String>,
    text: String,
    subject: String,
    attachments: Vec<PendingAttachment>,
    message_kind: &'static str,
    date_ms: String,
    contact_name: String,
    android_type: String,
    source_fields: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Default)]
struct PendingConversation {
    kind: ConversationKind,
    group_title: Option<String>,
    /// Each participant's handle key and kind.
    participants: Vec<(String, HandleType)>,
    messages: Vec<PendingMessage>,
}

/// The XML files to read: the file itself, or every `.xml` under the folder.
fn collect_xml_paths(input: &Path) -> Result<Vec<PathBuf>> {
    if input.is_file() {
        return Ok(vec![input.to_path_buf()]);
    }
    if !input.is_dir() {
        bail!("input is not a file or directory: {}", input.display());
    }
    let mut paths = discover_files(input, &|p| {
        p.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("xml"))
    })?;
    paths.sort();
    if paths.is_empty() {
        bail!("no .xml files found in {}", input.display());
    }
    Ok(paths)
}

/// Add one file's parse counts onto the report.
fn merge_stats(report: &mut ReadReport, stats: ParseStats) {
    report.sms_seen += stats.sms_seen;
    report.mms_seen += stats.mms_seen;
    report.skipped_invalid_date += stats.skipped_invalid_date;
    report.skipped_unknown_address += stats.skipped_unknown_address;
    report.skipped_unknown_type += stats.skipped_unknown_type;
    report.skipped_draft_or_outbox += stats.skipped_draft_or_outbox;
    report.skipped_empty_participants += stats.skipped_empty_participants;
    report.skipped_bad_attachment += stats.skipped_bad_attachment;
    report.dropped_character_references += stats.dropped_character_references;
}

/// Pending attachments for a message's decoded parts, each payload written
/// to the spool when there is one. The record, and the decoded bytes with
/// it, are dropped once this returns.
///
/// # Errors
///
/// Returns an error when a payload cannot be written to the spool.
fn queue_attachments(
    blobs: &[AttachmentBlob],
    spool: Option<&AttachmentSpool>,
) -> Result<Vec<PendingAttachment>> {
    blobs
        .iter()
        .map(|blob| {
            if let Some(spool) = spool {
                spool.put(&blob.data)?;
            }
            Ok(PendingAttachment {
                original_name: blob.original_name.clone(),
                mime_type: blob.mime_type.clone(),
                digest: blob.digest_hex.clone(),
                size_bytes: blob.data.len() as u64,
            })
        })
        .collect()
}

/// Stage the spooled attachments after every conversation is built, reading
/// one spooled file at a time.
fn stage_read_attachments(
    documents: &mut [ConversationDocument],
    options: &ReadOptions<'_>,
    report: &mut ReadReport,
) -> Result<()> {
    let mut sources: Vec<_> = documents
        .iter()
        .flat_map(|doc| doc.messages.iter())
        .flat_map(|msg| msg.attachments.iter())
        .map(|att| {
            options
                .spool
                .and_then(|spool| spool.source(att))
                .map(|(source, _)| source)
        })
        .collect();
    let mode = if options.spool.is_some() {
        options.media
    } else {
        MediaMode::Disabled
    };
    let attachments_dir = options.attachments_dir.unwrap_or_else(|| Path::new(""));
    report.attachments_saved += stage_conversation_attachments(
        document_messages(documents),
        attachments_dir,
        &MediaConfig {
            mode,
            compress: options.compress.clone(),
        },
        |i| match sources.get_mut(i) {
            Some(Some(source)) => load_attachment_source(source),
            _ => Ok(None),
        },
        options.log,
        options.progress,
        options.cancel,
    )
    .map_err(anyhow::Error::msg)?;
    Ok(())
}

/// The conversation id: `chat-<key>` for groups, else the peer's handle key.
fn chat_id(record: &Record) -> String {
    match record.conversation_kind {
        ConversationKind::Group => format!("chat-{}", record.chat_key),
        ConversationKind::Individual => record.chat_key.clone(),
    }
}

/// Append a parsed SMS or MMS to its conversation, creating the conversation on first sight.
fn add_record(
    conversations: &mut BTreeMap<String, PendingConversation>,
    record: Record,
    attachments: Vec<PendingAttachment>,
) -> Result<()> {
    let id = chat_id(&record);
    let peers = record
        .participants
        .iter()
        .map(|(h, _)| (h.key().to_string(), h.kind()))
        .collect();
    let conversation = conversations
        .entry(id)
        .or_insert_with(|| PendingConversation {
            kind: record.conversation_kind,
            group_title: record.group_title.clone(),
            participants: peers,
            messages: Vec::new(),
        });
    let sender = record.sender.map(Handle::into_key);
    let source_fields = serde_json::to_value(&record.source_fields)?
        .as_object()
        .cloned()
        .unwrap_or_default();
    conversation.messages.push(PendingMessage {
        sort_key: record.timestamp_secs,
        is_from_me: record.is_from_me,
        sender,
        sender_display_name: record.sender_display_name,
        text: record.text,
        subject: record.subject,
        attachments,
        message_kind: record.message_kind,
        date_ms: record.date_ms,
        contact_name: record.contact_name,
        android_type: record.android_type,
        source_fields,
    });
    Ok(())
}

impl PendingMessage {
    /// The UTC instant in milliseconds and how finely the XML recorded it:
    /// the `date` attribute's milliseconds, else the sort key's whole second.
    fn time(&self) -> (i64, TimePrecision) {
        match self.date_ms.trim().parse::<i64>() {
            Ok(ms) => (ms, TimePrecision::Milliseconds),
            Err(_) => (
                (self.sort_key as i64).saturating_mul(1000),
                TimePrecision::Seconds,
            ),
        }
    }

    /// Digests of the attachments, for the message's identity.
    fn attachment_digests(&self) -> Vec<String> {
        self.attachments.iter().map(|a| a.digest.clone()).collect()
    }
}

/// Sort by time and keep one copy of each message
/// ([`one_copy_per_message`]), each with the time it keeps.
fn dedupe(messages: &mut Vec<PendingMessage>) {
    messages.sort_by(|a, b| a.sort_key.total_cmp(&b.sort_key));
    let prepared: Vec<_> = messages
        .iter()
        .map(|m| (m.time(), m.attachment_digests()))
        .collect();
    let copies: Vec<MessageCopy<'_>> = messages
        .iter()
        .zip(&prepared)
        .map(|(m, ((ms, precision), digests))| MessageCopy {
            is_from_me: m.is_from_me,
            sender: m.sender.as_deref(),
            timestamp_unix_ms: *ms,
            precision: *precision,
            text: &m.text,
            attachment_digests: digests,
            vendor_key: None,
        })
        .collect();
    let kept = one_copy_per_message(&copies);
    let mut kept = kept.into_iter();
    messages.retain_mut(|m| match kept.next().flatten() {
        Some(ms) => {
            if m.time().0 != ms {
                m.date_ms = ms.to_string();
            }
            true
        }
        None => false,
    });
}

/// Display names seen per sender handle across the conversation.
fn names_by_handle(conversation: &PendingConversation) -> HashMap<String, String> {
    let mut names = HashMap::new();
    for message in &conversation.messages {
        if let (Some(sender), Some(name)) = (
            &message.sender,
            message
                .sender_display_name
                .as_deref()
                .and_then(message_ir::trimmed),
        ) {
            names
                .entry(sender.clone())
                .or_insert_with(|| name.to_string());
        }
        if let Some(name) = sbr::contact_name(&message.contact_name, conversation.kind) {
            for (peer, _) in &conversation.participants {
                names
                    .entry(peer.clone())
                    .or_insert_with(|| name.to_string());
            }
        }
    }
    names
}

/// Project one pending conversation into a document and fold its counts into the report.
fn to_document(
    id: &str,
    conversation: &PendingConversation,
    owner_handle: Option<&str>,
    report: &mut ReadReport,
) -> ConversationDocument {
    let export = ExportMeta {
        source: EXPORT_SOURCE.into(),
        tool: EXPORT_TOOL.into(),
        tool_version: EXPORT_TOOL_VERSION.into(),
        owner_handle: owner_handle.map(str::to_string),
        owner_display_name: None,
    };
    let owner = owner_sender(&export);
    let messages = conversation
        .messages
        .iter()
        .map(|message| {
            if message.is_from_me {
                report.sent += 1;
            } else {
                report.received += 1;
            }
            ir_message(id, message, &owner)
        })
        .collect();
    let mut document = ConversationDocument {
        schema_version: SCHEMA_VERSION,
        export,
        conversation: ConversationMeta {
            chat_identifier: id.into(),
            conversation_type: match conversation.kind {
                ConversationKind::Individual => IrConversationType::Individual,
                ConversationKind::Group => IrConversationType::Group,
            },
            group_title: conversation.group_title.clone(),
            participants: ir_participants(conversation),
            stats: ConversationStats::default(),
        },
        messages,
        packaging_stem_suffix: None,
    };
    document.finalize_stats();
    document
}

/// The IR message for one pending message. `owner` (handle, display name)
/// stands in as the sender of anything sent from this phone; the timestamp
/// is the record's own milliseconds when it parses, else the sort key.
fn ir_message(
    chat_id: &str,
    message: &PendingMessage,
    owner: &(Option<String>, Option<String>),
) -> IrMessage {
    let (timestamp_unix_ms, _) = message.time();
    let digests = message.attachment_digests();
    let (sender_handle, sender_display_name) = if message.is_from_me {
        owner.clone()
    } else {
        (message.sender.clone(), message.sender_display_name.clone())
    };
    IrMessage {
        guid: MessageGuid::new(&MessageIdentity {
            chat: chat_id,
            is_from_me: message.is_from_me,
            sender: message.sender.as_deref(),
            timestamp_unix_ms,
            text: &message.text,
            attachment_digests: &digests,
            vendor_key: None,
        })
        .into_string(),
        timestamp_unix_ms,
        direction: if message.is_from_me {
            IrDirection::Outgoing
        } else {
            IrDirection::Incoming
        },
        service: IrService::Sms,
        message_kind: IrMessageKind::parse(message.message_kind),
        sender_handle,
        sender_display_name,
        owner_handle: None,
        subject: (!message.subject.is_empty()).then(|| message.subject.clone()),
        text: message.text.clone(),
        attachments: message.attachments.iter().map(ir_attachment).collect(),
        imessage: None,
        source: IrSource {
            android_type: message.android_type.trim().parse().ok(),
            fields: message.source_fields.clone(),
        }
        .into_option(),
    }
}

/// The IR attachment for one pending attachment. Neither a path nor the
/// bytes: whoever stages it reads the payload from the spool by its digest.
fn ir_attachment(a: &PendingAttachment) -> IrAttachment {
    IrAttachment {
        path: None,
        original_name: a.original_name.clone(),
        mime_type: a.mime_type.clone(),
        digest_sha256: (!a.digest.is_empty()).then(|| a.digest.clone()),
        is_sticker: false,
        transcription: None,
        sticker_effect: None,
        size_bytes: Some(a.size_bytes),
        missing_reason: None,
        bytes: None,
    }
}

/// Every participant with the kind of address it is, named when the XML
/// named it.
fn ir_participants(conversation: &PendingConversation) -> Vec<IrParticipant> {
    let names = names_by_handle(conversation);
    conversation
        .participants
        .iter()
        .map(|(handle, kind)| IrParticipant {
            handle: Some(handle.clone()),
            display_name: names.get(handle).cloned(),
            handle_type: Some(*kind),
        })
        .collect()
}

/// Parse SMS Backup & Restore XML into conversation documents.
///
/// Stages attachments and drops duplicate messages.
///
/// # Errors
///
/// Returns an error when no XML files are found, owner phones cannot be
/// inferred, or a file cannot be parsed.
pub fn read_backup(
    input: &Path,
    options: ReadOptions<'_>,
) -> Result<(Vec<ConversationDocument>, ReadReport)> {
    let paths = collect_xml_paths(input)?;
    let mut owner_phones = options.owner_phones.to_vec();
    if owner_phones.is_empty() {
        // Owner inference is best-effort: the main pass below already reports
        // per-file parse errors, so one malformed file must not abort the whole
        // export. Only give up when no file could be parsed at all.
        let mut parse_errors = Vec::new();
        for path in &paths {
            match infer_owner_phones(path) {
                Ok(phones) => owner_phones.extend(phones),
                Err(error) => parse_errors.push(format!("{}: {error:#}", path.display())),
            }
        }
        if owner_phones.is_empty() && !parse_errors.is_empty() && parse_errors.len() == paths.len()
        {
            bail!(
                "could not infer owner phones from any input file ({}), and none were supplied",
                parse_errors.join("; ")
            );
        }
        owner_phones.sort();
        owner_phones.dedup();
    }
    let owners = if owner_phones.is_empty() {
        None
    } else {
        Some(OwnerHandleSet::from_phones(&owner_phones)?)
    };
    // from_phones guarantees at least one phone handle in the set.
    let owner_handle = owners
        .as_ref()
        .and_then(OwnerHandleSet::primary_owner_handle);
    let mut report = ReadReport::default();
    let mut conversations = BTreeMap::new();
    for path in paths {
        check_cancel(options.cancel)?;
        // Each record's attachment payloads go to the spool as the record is
        // parsed; staging waits until every conversation is built. Messages
        // that parse before an XML error are kept; stats are merged even
        // when the file is truncated.
        let mut stats = ParseStats::default();
        // A spool that cannot be written stops the read rather than counting
        // as an error in one file.
        let mut spool_error = None;
        let parse_result = parse_file_with(&path, owners.as_ref(), &mut stats, |record| {
            check_cancel(options.cancel)?;
            let attachments = match queue_attachments(&record.attachments, options.spool) {
                Ok(attachments) => attachments,
                Err(error) => {
                    let stop = anyhow::anyhow!("{error:#}");
                    spool_error = Some(error);
                    return Err(stop);
                }
            };
            match add_record(&mut conversations, record, attachments) {
                Ok(()) => Ok(()),
                Err(error) => {
                    // Keep parsing the rest of the file; one bad record
                    // must not abort the whole backup.
                    report.errors.push(format!("{}: {error:#}", path.display()));
                    Ok(())
                }
            }
        });
        merge_stats(&mut report, stats);
        if let Some(error) = spool_error {
            return Err(error);
        }
        if let Err(error) = parse_result {
            if is_cancelled(options.cancel) || error.to_string() == "cancelled" {
                return Err(error);
            }
            report.errors.push(format!("{}: {error:#}", path.display()));
        }
    }
    check_cancel(options.cancel)?;
    let mut documents = Vec::new();
    for (id, mut conversation) in conversations {
        dedupe(&mut conversation.messages);
        conversation.messages.retain(|message| {
            let valid = format_local_ts(message.sort_key as i64).is_some();
            if !valid {
                report.skipped_invalid_date += 1;
            }
            valid
        });
        if conversation.messages.is_empty() {
            continue;
        }
        documents.push(to_document(
            &id,
            &conversation,
            owner_handle.as_deref(),
            &mut report,
        ));
        report.conversations += 1;
    }
    if options.stage_attachments {
        stage_read_attachments(&mut documents, &options, &mut report)?;
    }
    Ok((documents, report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::write::SbrBackupSession;
    use std::fs;

    fn opts<'a>(
        owner_phones: &'a [String],
        attachments_dir: Option<&'a Path>,
        spool: Option<&'a AttachmentSpool>,
    ) -> ReadOptions<'a> {
        ReadOptions {
            owner_phones,
            attachments_dir,
            spool,
            stage_attachments: true,
            media: if spool.is_some() {
                MediaMode::Clone
            } else {
                MediaMode::Disabled
            },
            compress: CompressOptions::default(),
            log: None,
            progress: None,
            cancel: None,
        }
    }

    #[test]
    fn reads_then_writes_source_fields_and_attachment() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.xml");
        fs::write(&input, r#"<smses><mms date="1400773400000" msg_box="2" address="+15555550101" extra="yes"><parts><part seq="0" ct="image/jpeg" name="pic.jpg" data="aGVsbG8="/></parts><addrs><addr address="+15555550100" type="137" charset="106"/><addr address="+15555550101" type="151"/></addrs></mms></smses>"#).unwrap();
        let output = dir.path().join("output");
        let stage = output.join("attachments");
        let spool = AttachmentSpool::open(dir.path()).unwrap();
        let (docs, report) = read_backup(&input, opts(&[], Some(&stage), Some(&spool))).unwrap();
        assert_eq!(report.attachments_saved, 1);
        let staged: Vec<_> = fs::read_dir(&stage)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(staged.len(), 1);
        assert_eq!(fs::metadata(&staged[0]).unwrap().len(), 5);
        assert_eq!(docs[0].export.owner_handle.as_deref(), Some("+15555550100"));
        assert_eq!(
            docs[0].messages[0].attachments[0].size_bytes,
            Some(5),
            "decoded aGVsbG8= is five bytes; size_bytes lets message-crate-push skip re-hashing"
        );
        assert_eq!(
            docs[0].messages[0].source.as_ref().unwrap().fields["attrs"]["extra"],
            "yes"
        );
        let mut writer = SbrBackupSession::create(&output).unwrap();
        writer.append_document(&docs[0]).unwrap();
        let xml = fs::read_to_string(writer.finish().unwrap()).unwrap();
        assert!(xml.contains(r#"extra="yes""#));
        assert!(xml.contains(r#"data="aGVsbG8=""#));
        assert!(xml.contains(r#"charset="106""#));
    }

    #[test]
    fn write_back_matches_parts_to_attachments_by_digest() {
        // An empty-data part must not consume the next part's attachment, and
        // identical payloads dedupe into one staged file that both parts share.
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.xml");
        fs::write(
            &input,
            r#"<smses><mms date="1400773400000" msg_box="2" address="+15555550101"><parts><part seq="0" ct="image/jpeg" name="empty.jpg" data=""/><part seq="1" ct="image/jpeg" name="pic.jpg" data="aGVsbG8="/><part seq="2" ct="image/jpeg" name="pic-copy.jpg" data="aGVsbG8="/></parts><addrs><addr address="+15555550100" type="137" charset="106"/><addr address="+15555550101" type="151"/></addrs></mms></smses>"#,
        )
        .unwrap();
        let output = dir.path().join("output");
        let stage = output.join("attachments");
        let spool = AttachmentSpool::open(dir.path()).unwrap();
        let (docs, report) = read_backup(&input, opts(&[], Some(&stage), Some(&spool))).unwrap();
        assert_eq!(report.attachments_saved, 1);
        let mut writer = SbrBackupSession::create(&output).unwrap();
        writer.append_document(&docs[0]).unwrap();
        let xml = fs::read_to_string(writer.finish().unwrap()).unwrap();
        // Both payload parts carry the decoded bytes; the empty part does not.
        assert_eq!(xml.match_indices(r#"data="aGVsbG8=""#).count(), 2);
    }

    /// A group MMS is credited to the `type="137"` addr, whichever position
    /// that number holds in the `address` list, and to nobody when the
    /// backup names no sender. Every form of the owner's number counts as
    /// the owner, so all four messages share one conversation.
    #[test]
    fn group_mms_sender_direction_and_conversation() {
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/group_mms_sender.xml");
        let (docs, report) = read_backup(&fixture, opts(&[], None, None)).unwrap();
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(docs.len(), 1, "every message lands in the same group");
        let doc = &docs[0];
        assert_eq!(
            doc.conversation.chat_identifier,
            "chat-group-+15555550101_+15555550102_+15555550103"
        );
        assert_eq!(doc.export.owner_handle.as_deref(), Some("+15555550100"));
        let mut participants: Vec<_> = doc
            .conversation
            .participants
            .iter()
            .filter_map(|p| p.handle.as_deref())
            .collect();
        participants.sort_unstable();
        assert_eq!(
            participants,
            ["+15555550101", "+15555550102", "+15555550103"],
            "the owner is not a participant, in any spelling"
        );
        let seen: Vec<(&str, IrDirection, Option<&str>)> = doc
            .messages
            .iter()
            .map(|m| (m.text.as_str(), m.direction, m.sender_handle.as_deref()))
            .collect();
        assert_eq!(
            seen,
            [
                ("from lee", IrDirection::Incoming, Some("+15555550103")),
                ("no from", IrDirection::Incoming, None),
                ("sent by me", IrDirection::Outgoing, Some("+15555550100")),
                ("from ana", IrDirection::Incoming, Some("+15555550102")),
            ]
        );
    }

    #[test]
    fn owner_inference_tolerates_malformed_files() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input");
        fs::create_dir_all(&input).unwrap();
        fs::write(
            input.join("ok.xml"),
            r#"<smses><mms date="1400773400000" msg_box="2" address="+15555550101"><parts/><addrs><addr address="+15555550100" type="137"/><addr address="+15555550101" type="151"/></addrs></mms></smses>"#,
        )
        .unwrap();
        fs::write(input.join("broken.xml"), "<smses><mms date=").unwrap();
        let (docs, report) = read_backup(&input, opts(&[], None, None)).unwrap();
        assert_eq!(docs[0].export.owner_handle.as_deref(), Some("+15555550100"));
        assert_eq!(report.errors.len(), 1);
    }

    #[test]
    fn truncated_xml_keeps_messages_parsed_before_the_error() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.xml");
        fs::write(
            &input,
            r#"<smses><sms protocol="0" address="+15555550101" date="1400773261000" type="1" body="kept"/><sms date=""#,
        )
        .unwrap();
        let owner = vec!["+15555550100".to_string()];
        let (docs, report) = read_backup(&input, opts(&owner, None, None)).unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].messages.len(), 1);
        assert_eq!(docs[0].messages[0].text, "kept");
        assert_eq!(
            report.sms_seen, 1,
            "stats from the completed message must survive the XML error"
        );
        assert_eq!(report.errors.len(), 1);
    }

    /// Every payload is on disk in the spool once the read returns, and no
    /// document holds a byte of one, so a backup full of video is never in
    /// memory at once (issue #1128).
    #[test]
    fn reading_a_backup_spools_every_payload_and_holds_none() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.xml");
        fs::write(&input, r#"<smses><mms date="1400773400000" msg_box="1" address="+15555550101"><parts><part ct="image/jpeg" name="a.jpg" data="aGVsbG8="/></parts><addrs><addr address="+15555550101" type="137"/></addrs></mms><mms date="1400773500000" msg_box="1" address="+15555550101"><parts><part ct="image/jpeg" name="b.jpg" data="d29ybGQ="/></parts><addrs><addr address="+15555550101" type="137"/></addrs></mms></smses>"#).unwrap();
        let stage = dir.path().join("output").join("attachments");
        let spool = AttachmentSpool::open(dir.path()).unwrap();
        let owner = vec!["+15555550100".to_string()];

        let mut options = opts(&owner, Some(&stage), Some(&spool));
        options.stage_attachments = false;
        let (docs, report) = read_backup(&input, options).unwrap();

        let attachments: Vec<_> = docs[0]
            .messages
            .iter()
            .flat_map(|m| &m.attachments)
            .collect();
        let held: usize = attachments
            .iter()
            .filter_map(|a| a.bytes.as_ref())
            .map(Vec::len)
            .sum();
        assert_eq!(held, 0, "no payload stays on a document");
        let spooled: Vec<Vec<u8>> = attachments
            .iter()
            .map(|a| fs::read(spool.path(a.digest_sha256.as_deref().unwrap()).unwrap()).unwrap())
            .collect();
        assert_eq!(spooled, [b"hello".to_vec(), b"world".to_vec()]);
        assert_eq!(attachments[0].size_bytes, Some(5));
        assert!(
            attachments.iter().all(|a| a.path.is_none()),
            "nothing was staged, so nothing to point at"
        );
        assert!(!stage.exists(), "no attachment files were written");
        assert_eq!(report.attachments_saved, 0);
    }

    /// A run that does not copy attachments keeps no payload anywhere.
    #[test]
    fn without_a_spool_no_payload_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.xml");
        fs::write(&input, r#"<smses><mms date="1400773400000" msg_box="1" address="+15555550101"><parts><part ct="image/jpeg" name="a.jpg" data="aGVsbG8="/></parts><addrs><addr address="+15555550101" type="137"/></addrs></mms></smses>"#).unwrap();
        let owner = vec!["+15555550100".to_string()];
        let (docs, _) = read_backup(&input, opts(&owner, None, None)).unwrap();
        let att = &docs[0].messages[0].attachments[0];
        assert!(att.bytes.is_none() && att.path.is_none());
        assert_eq!(att.size_bytes, Some(5), "the size is still recorded");
    }

    /// Read `messages` wrapped in `<smses>` with +15555550100 as the owner.
    fn read_xml(messages: &str) -> Vec<ConversationDocument> {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.xml");
        fs::write(&input, format!("<smses>{messages}</smses>")).unwrap();
        let owner = vec!["+15555550100".to_string()];
        let (docs, report) = read_backup(&input, opts(&owner, None, None)).unwrap();
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        docs
    }

    fn roster(doc: &ConversationDocument) -> Vec<(&str, Option<&str>)> {
        doc.conversation
            .participants
            .iter()
            .map(|p| (p.handle.as_deref().unwrap(), p.display_name.as_deref()))
            .collect()
    }

    #[test]
    fn a_contact_name_names_the_peer_of_a_direct_conversation() {
        // Only sent messages, which name the peer but carry no sender.
        let docs = read_xml(
            r#"<sms protocol="0" address="+15555550101" date="1400773261000" type="2" body="hi" contact_name="Sam"/>"#,
        );
        assert_eq!(roster(&docs[0]), [("+15555550101", Some("Sam"))]);
    }

    #[test]
    fn a_placeholder_contact_name_names_nobody() {
        for placeholder in ["null", "(Unknown)"] {
            let docs = read_xml(&format!(
                r#"<sms protocol="0" address="+15555550101" date="1400773261000" type="2" body="hi" contact_name="{placeholder}"/><sms protocol="0" address="+15555550101" date="1400773262000" type="1" body="hey" contact_name="{placeholder}"/>"#,
            ));
            assert_eq!(roster(&docs[0]), [("+15555550101", None)], "{placeholder}");
            assert_eq!(docs[0].messages[1].sender_display_name, None);
        }
    }

    /// A group MMS's `contact_name` is the members' names joined by ", ",
    /// which names the group, so no participant takes it as a name.
    #[test]
    fn a_group_contact_name_names_no_sender() {
        let docs = read_xml(
            r#"<mms date="1400773400000" msg_box="1" address="+15555550101~+15555550102~+15555550100" contact_name="Ana, Lee"><parts><part ct="text/plain" text="hi"/></parts><addrs><addr address="+15555550102" type="137"/><addr address="+15555550101" type="151"/><addr address="+15555550100" type="151"/></addrs></mms>"#,
        );
        assert_eq!(
            roster(&docs[0]),
            [("+15555550101", None), ("+15555550102", None)]
        );
        assert_eq!(docs[0].messages[0].sender_display_name, None);
    }

    #[test]
    fn a_subject_is_kept_and_a_null_one_dropped() {
        let docs = read_xml(
            r#"<sms protocol="0" address="+15555550101" date="1400773261000" type="1" body="one" subject="Plans"/><sms protocol="0" address="+15555550101" date="1400773262000" type="1" body="two" subject="null"/>"#,
        );
        let subjects: Vec<_> = docs[0]
            .messages
            .iter()
            .map(|m| m.subject.as_deref())
            .collect();
        assert_eq!(subjects, [Some("Plans"), None]);
    }

    #[test]
    fn an_attachment_carries_its_payload_digest() {
        let docs = read_xml(
            r#"<mms date="1400773400000" msg_box="1" address="+15555550101"><parts><part ct="image/jpeg" name="pic.jpg" data="aGVsbG8="/></parts><addrs><addr address="+15555550101" type="137"/></addrs></mms>"#,
        );
        assert_eq!(
            docs[0].messages[0].attachments[0].digest_sha256.as_deref(),
            // SHA-256 of "hello", the decoded payload.
            Some("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")
        );
    }

    /// A carrier often lists the owner's number in national form in an MMS,
    /// so `07700900123` is the owner `+447700900123`, and the MMS is the
    /// one-to-one conversation with the other person, not a group.
    #[test]
    fn the_owner_in_national_form_is_not_a_participant() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.xml");
        fs::write(&input, r#"<smses><mms date="1400773400000" msg_box="1" address="+447911123456~07700900123"><parts><part ct="text/plain" text="hi"/></parts><addrs><addr address="+447911123456" type="137"/><addr address="07700900123" type="151"/></addrs></mms></smses>"#).unwrap();
        let owner = vec!["+447700900123".to_string()];
        let (docs, _) = read_backup(&input, opts(&owner, None, None)).unwrap();
        assert_eq!(
            docs[0].conversation.participants.len(),
            1,
            "{:?}",
            docs[0].conversation.participants
        );
    }

    /// With more than one owner number, a sent message is credited to the
    /// first one given, in every run.
    #[test]
    fn the_first_owner_number_given_is_the_owner_handle() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.xml");
        fs::write(&input, r#"<smses><sms protocol="0" address="+15555550101" date="1400773261000" type="2" body="hi"/></smses>"#).unwrap();
        // Nine numbers, so a set's order would pick the first one given only
        // by chance; the first given is not the smallest either.
        let owners: Vec<String> = [9, 0, 2, 3, 4, 5, 6, 7, 8]
            .iter()
            .map(|i| format!("+1555555010{i}"))
            .collect();
        let (docs, _) = read_backup(&input, opts(&owners, None, None)).unwrap();
        assert_eq!(docs[0].export.owner_handle.as_deref(), Some("+15555550109"));
    }

    #[test]
    fn an_international_number_keeps_its_country() {
        let docs = read_xml(
            r#"<sms protocol="0" address="+6591234567" date="1400773261000" type="1" body="hi"/><sms protocol="0" address="+447700900123" date="1400773261000" type="1" body="hi"/>"#,
        );
        let ids: Vec<_> = docs
            .iter()
            .map(|d| d.conversation.chat_identifier.as_str())
            .collect();
        assert_eq!(ids, ["+447700900123", "+6591234567"]);
    }

    #[test]
    fn an_email_sender_is_an_email_identity() {
        let docs = read_xml(
            r#"<sms protocol="0" address="john1985@example.com" date="1400773261000" type="1" body="hi"/>"#,
        );
        assert_eq!(docs[0].conversation.chat_identifier, "john1985@example.com");
        let participant = &docs[0].conversation.participants[0];
        assert_eq!(participant.handle.as_deref(), Some("john1985@example.com"));
        assert_eq!(participant.handle_type, Some(HandleType::Email));
        assert_eq!(
            docs[0].messages[0].sender_handle.as_deref(),
            Some("john1985@example.com")
        );
    }

    #[test]
    fn a_sender_name_is_an_identity_of_type_other() {
        let docs = read_xml(
            r#"<sms protocol="0" address="AMAZON" date="1400773261000" type="1" body="Your parcel"/>"#,
        );
        let participant = &docs[0].conversation.participants[0];
        assert_eq!(participant.handle.as_deref(), Some("AMAZON"));
        assert_eq!(participant.handle_type, Some(HandleType::Other));
    }

    /// With no owner on the form, the owner comes from the sent MMS, and a
    /// UK owner keeps its country, as the account's identity does.
    #[test]
    fn an_inferred_owner_outside_the_us_keeps_its_country() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.xml");
        fs::write(&input, r#"<smses><mms date="1400773400000" msg_box="2" address="+447700900123"><parts><part ct="text/plain" text="hi"/></parts><addrs><addr address="+447911123456" type="137"/><addr address="+447700900123" type="151"/></addrs></mms></smses>"#).unwrap();
        let (docs, _) = read_backup(&input, opts(&[], None, None)).unwrap();
        assert_eq!(
            docs[0].export.owner_handle.as_deref(),
            Some("+447911123456")
        );
        assert_eq!(docs[0].conversation.chat_identifier, "+447700900123");
    }

    #[test]
    fn verify_e1_4_two_group_senders_in_one_second_get_two_guids() {
        let docs = read_xml(
            r#"<mms date="1400773400100" msg_box="1" address="+15555550101~+15555550102~+15555550100"><parts><part ct="text/plain" text="lol"/></parts><addrs><addr address="+15555550101" type="137"/><addr address="+15555550102" type="151"/><addr address="+15555550100" type="151"/></addrs></mms><mms date="1400773400200" msg_box="1" address="+15555550101~+15555550102~+15555550100"><parts><part ct="text/plain" text="lol"/></parts><addrs><addr address="+15555550102" type="137"/><addr address="+15555550101" type="151"/><addr address="+15555550100" type="151"/></addrs></mms>"#,
        );
        assert_eq!(docs[0].messages.len(), 2, "the exporter keeps both");
        assert_ne!(docs[0].messages[0].guid, docs[0].messages[1].guid);
    }
}
