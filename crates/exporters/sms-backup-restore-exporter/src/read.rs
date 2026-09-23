//! Read SMS Backup & Restore XML into [`ConversationDocument`] values.

use anyhow::{Result, bail};
use media::{CompressOptions, MediaMode};
use message_csv::{format_local_ts, stable_guid};
use message_ir::{
    ConversationDocument, ConversationMeta, ConversationStats, ExportMeta, HandleType,
    IrAttachment, IrConversationType, IrDirection, IrMessage, IrMessageKind, IrParticipant,
    IrService, IrSource, SCHEMA_VERSION, owner_sender,
};
use message_vault_io_core::{
    CancelFlag, LogSink, MediaConfig, ProgressSink, check_cancel, discover_files,
    document_messages, is_cancelled, stage_conversation_attachments,
};
use phone::OwnerHandleSet;
use sbr::{
    AttachmentBlob, ConversationKind, ParseStats, Record, infer_owner_phones, parse_file_with,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

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
    /// Per-file error messages from parsing/staging.
    pub errors: Vec<String>,
}

/// Options for [`read_backup`].
#[derive(Debug)]
pub struct ReadOptions<'a> {
    /// Known owner phone numbers (empty triggers inference).
    pub owner_phones: &'a [String],
    /// Date window messages must fall inside.
    /// Directory staged attachments are written to.
    pub attachments_dir: Option<&'a Path>,
    /// Whether to write staged attachment files.
    pub copy_attachments: bool,
    /// Whether to write the staged attachment files here. `false` leaves
    /// the bytes on the records for a caller that stages them itself —
    /// the write queue does, one conversation at a time.
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
    bytes: Option<Arc<[u8]>>,
}

#[derive(Debug, Clone)]
struct PendingMessage {
    sort_key: f64,
    is_from_me: bool,
    sender_digits: Option<String>,
    sender_display_name: Option<String>,
    text: String,
    subject: String,
    attachments: Vec<PendingAttachment>,
    dedupe_key: String,
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
    participant_e164s: Vec<String>,
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
}

/// Pending attachments for a message's decoded parts, carrying bytes only when the caller keeps them.
fn queue_attachments(blobs: &[AttachmentBlob], keep_bytes: bool) -> Vec<PendingAttachment> {
    blobs
        .iter()
        .map(|blob| PendingAttachment {
            original_name: blob.original_name.clone(),
            mime_type: blob.mime_type.clone(),
            digest: blob.digest_hex.clone(),
            size_bytes: blob.data.len() as u64,
            bytes: keep_bytes.then(|| Arc::clone(&blob.data)),
        })
        .collect()
}

/// Write queued attachment bytes after every conversation is built.
fn stage_read_attachments(
    documents: &mut [ConversationDocument],
    options: &ReadOptions<'_>,
    report: &mut ReadReport,
) -> Result<()> {
    let payloads: Vec<Option<Vec<u8>>> = documents
        .iter()
        .flat_map(|doc| {
            doc.messages
                .iter()
                .flat_map(|msg| msg.attachments.iter().map(|att| att.bytes.clone()))
        })
        .collect();
    let mode = if options.copy_attachments {
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
        |i| Ok(payloads.get(i).cloned().flatten()),
        options.log,
        options.progress,
        options.cancel,
    )
    .map_err(anyhow::Error::msg)?;
    Ok(())
}

/// The conversation id: `chat-<key>` for groups, else the guarded-normalized address.
fn chat_id(record: &Record) -> String {
    match record.conversation_kind {
        ConversationKind::Group => format!("chat-{}", record.chat_key),
        // Guarded policy on the raw address: E.164 only when unambiguous, so
        // a trunk-zero `020 7946 0000` stays digits-as-is instead of being
        // fabricated into `+02079460000`.
        ConversationKind::Individual => phone::normalize_lenient(&record.chat_key),
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
        .participant_digits
        .iter()
        .map(|(d, _)| phone::normalize_lenient(d))
        .filter(|d| !d.is_empty())
        .collect();
    let conversation = conversations
        .entry(id)
        .or_insert_with(|| PendingConversation {
            kind: record.conversation_kind,
            group_title: record.group_title.clone(),
            participant_e164s: peers,
            messages: Vec::new(),
        });
    let names: Vec<_> = attachments.iter().map(|a| a.digest.as_str()).collect();
    // Include the full fractional timestamp and sender to avoid false deduplication
    // of distinct messages within the same second.
    let dedupe_key = format!(
        "{}|{}|{}|{}|{}",
        record.timestamp_secs,
        u8::from(record.is_from_me),
        record.sender_digits.as_deref().unwrap_or(""),
        record.text,
        names.join(",")
    );
    let source_fields = serde_json::to_value(&record.source_fields)?
        .as_object()
        .cloned()
        .unwrap_or_default();
    conversation.messages.push(PendingMessage {
        sort_key: record.timestamp_secs,
        is_from_me: record.is_from_me,
        sender_digits: record.sender_digits,
        sender_display_name: record.sender_display_name,
        text: record.text,
        subject: record.subject,
        attachments,
        dedupe_key,
        message_kind: record.message_kind,
        date_ms: record.date_ms,
        contact_name: record.contact_name,
        android_type: record.android_type,
        source_fields,
    });
    Ok(())
}

/// Sort by time and drop later messages with the same dedupe key.
fn dedupe(messages: &mut Vec<PendingMessage>) {
    messages.sort_by(|a, b| a.sort_key.total_cmp(&b.sort_key));
    let mut seen = HashSet::new();
    messages.retain(|m| seen.insert(m.dedupe_key.clone()));
}

/// Display names seen per sender handle across the conversation.
fn names_by_handle(conversation: &PendingConversation) -> HashMap<String, String> {
    let mut names = HashMap::new();
    for message in &conversation.messages {
        if let (Some(digits), Some(name)) = (
            &message.sender_digits,
            message
                .sender_display_name
                .as_deref()
                .and_then(message_ir::trimmed),
        ) {
            names
                .entry(phone::normalize_lenient(digits))
                .or_insert_with(|| name.to_string());
        }
        if conversation.kind == ConversationKind::Individual {
            let name = message.contact_name.trim();
            if !name.is_empty() {
                for peer in &conversation.participant_e164s {
                    names
                        .entry(peer.clone())
                        .or_insert_with(|| name.to_string());
                }
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
    let timestamp_unix_ms = message
        .date_ms
        .parse()
        .unwrap_or_else(|_| (message.sort_key as i64).saturating_mul(1000));
    let timestamp = format_local_ts(message.sort_key as i64).expect("timestamps validated");
    let digests: Vec<_> = message
        .attachments
        .iter()
        .map(|a| a.digest.clone())
        .collect();
    let (sender_handle, sender_display_name) = if message.is_from_me {
        owner.clone()
    } else {
        (
            message
                .sender_digits
                .as_deref()
                .map(phone::normalize_lenient),
            message.sender_display_name.clone(),
        )
    };
    IrMessage {
        guid: stable_guid(
            chat_id,
            &timestamp.0,
            message.is_from_me,
            &message.text,
            &digests,
        ),
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

/// The IR attachment for one pending attachment. The bytes travel with it
/// when the XML carried them inline; a path is never known.
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
        bytes: a.bytes.as_ref().map(|b| b.as_ref().to_vec()),
    }
}

/// Every participant as a phone handle, named when the XML named it. SBR
/// participants are E.164 numbers by construction (`participant_e164s`),
/// so the type is always Phone.
fn ir_participants(conversation: &PendingConversation) -> Vec<IrParticipant> {
    let names = names_by_handle(conversation);
    conversation
        .participant_e164s
        .iter()
        .filter(|h| !h.is_empty())
        .map(|handle| IrParticipant {
            handle: Some(handle.clone()),
            display_name: names.get(handle).cloned(),
            handle_type: Some(HandleType::Phone),
        })
        .collect()
}

/// Parse SMS Backup & Restore XML into conversation documents.
///
/// Stages attachments, applies the date filter, and drops duplicate messages.
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
    let (owners, owner_handle) = if owner_phones.is_empty() {
        (HashSet::new(), None)
    } else {
        let owners = OwnerHandleSet::from_phones(&owner_phones)?;
        // from_phones guarantees at least one phone handle in the set.
        (owners.all_phone_digits(), owners.primary_owner_handle())
    };
    let mut report = ReadReport::default();
    let mut conversations = BTreeMap::new();
    for path in paths {
        check_cancel(options.cancel)?;
        // Decode attachment bytes during parse; file writes wait until every
        // conversation is built. Messages that parse before an XML error are
        // kept; stats are merged even when the file is truncated.
        let mut stats = ParseStats::default();
        let parse_result = parse_file_with(&path, &owners, &mut stats, |record| {
            check_cancel(options.cancel)?;
            let attachments = queue_attachments(&record.attachments, options.copy_attachments);
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
        copy_attachments: bool,
    ) -> ReadOptions<'a> {
        ReadOptions {
            owner_phones,
            attachments_dir,
            copy_attachments,
            stage_attachments: true,
            media: if copy_attachments {
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
        let (docs, report) = read_backup(&input, opts(&[], Some(&stage), true)).unwrap();
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
            "decoded aGVsbG8= is five bytes; size_bytes lets vault-push skip re-hashing"
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
        let (docs, report) = read_backup(&input, opts(&[], Some(&stage), true)).unwrap();
        assert_eq!(report.attachments_saved, 1);
        let mut writer = SbrBackupSession::create(&output).unwrap();
        writer.append_document(&docs[0]).unwrap();
        let xml = fs::read_to_string(writer.finish().unwrap()).unwrap();
        // Both payload parts carry the decoded bytes; the empty part does not.
        assert_eq!(xml.match_indices(r#"data="aGVsbG8=""#).count(), 2);
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
        let (docs, report) = read_backup(&input, opts(&[], None, false)).unwrap();
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
        let (docs, report) = read_backup(&input, opts(&owner, None, false)).unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].messages.len(), 1);
        assert_eq!(docs[0].messages[0].text, "kept");
        assert_eq!(
            report.sms_seen, 1,
            "stats from the completed message must survive the XML error"
        );
        assert_eq!(report.errors.len(), 1);
    }

    #[test]
    fn stage_attachments_false_leaves_the_bytes_for_the_caller() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.xml");
        fs::write(&input, r#"<smses><mms date="1400773400000" msg_box="2" address="+15555550101"><parts><part seq="0" ct="image/jpeg" name="pic.jpg" data="aGVsbG8="/></parts><addrs><addr address="+15555550100" type="137" charset="106"/><addr address="+15555550101" type="151"/></addrs></mms></smses>"#).unwrap();
        let stage = dir.path().join("output").join("attachments");

        let mut options = opts(&[], Some(&stage), true);
        options.stage_attachments = false;
        let (docs, report) = read_backup(&input, options).unwrap();

        let att = &docs[0].messages[0].attachments[0];
        assert_eq!(
            att.bytes.as_deref(),
            Some(&b"hello"[..]),
            "the bytes stay on the record for the caller to stage"
        );
        assert!(
            att.path.is_none(),
            "nothing was staged, so nothing to point at"
        );
        assert!(!stage.exists(), "no attachment files were written");
        assert_eq!(report.attachments_saved, 0);
    }
}
