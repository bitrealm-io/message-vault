//! Convert SMS Backup+ `.eml` trees into the shared conversation structure,
//! then write the chosen output format via [`ExportWriter`].

use crate::attachments_emit::{merge_attachments, queue_attachments};
use crate::identity::{chat_id_for, cover_identity, name_only_key, timestamp_ms};
use crate::parse_emit::{ParsedEmlKind, collect_eml_paths, parse_one_eml};
use crate::types::ParsedMessage;
use anyhow::{Result, bail};
use message_ir::{
    ExportMeta, IrAttachment, IrService, IrSource, PendingAttachment, PendingConversation,
    PendingMessage, ProjectionHooks, parse_android_type,
};
use message_ir_format::{AttachmentSource, ExportTransforms, ExportWriter, FormatSinkResult};
use message_vault_io_core::{
    CancelFlag, ExportReport, LogSink, OutputFormat, emit_log, prepare_outputs,
    project_conversation,
};
use phone::OwnerHandleSet;
use rayon::prelude::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

const EXPORT_SOURCE: &str = "sms-backup-plus";
const EXPORT_TOOL: &str = "SMS Backup+";
const EXPORT_TOOL_VERSION: &str = "1.5.11";

/// The EML's path relative to the input root it was found under, for the vendor `source` bag.
fn relative_eml_path(
    eml_path: &Path,
    inputs: &[PathBuf],
    file_inputs: &HashSet<PathBuf>,
) -> String {
    for root in inputs {
        if let Ok(rel) = eml_path.strip_prefix(root) {
            return rel.display().to_string();
        }
        if file_inputs.contains(root) && eml_path == root {
            return root
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_else(|| eml_path.to_str().unwrap_or(""))
                .to_string();
        }
    }
    eml_path.display().to_string()
}

/// Get or create the pending conversation for `chat_id`, unioning rosters.
///
/// Unlike the shared `ensure_conversation` (which seeds a new entry only),
/// group membership changes over time here, and a later message's smaller
/// roster must not shrink the participant list. (A roster change that yields a
/// different `chat_key` still splits the conversation into fragments; this keeps
/// each fragment's participant list complete within that key.)
fn ensure_convo<'a>(
    map: &'a mut HashMap<String, PendingConversation>,
    chat_id: &str,
    is_group: bool,
    display_name: Option<String>,
    participant_e164s: Vec<String>,
) -> &'a mut PendingConversation {
    // Avoid allocating a new String on every message for an existing chat.
    if !map.contains_key(chat_id) {
        map.insert(
            chat_id.to_string(),
            PendingConversation::new(chat_id, is_group, display_name, Vec::new()),
        );
    }
    let convo = map
        .get_mut(chat_id)
        .expect("just inserted or already present");
    convo.participant_e164s.extend(participant_e164s);
    convo.participant_e164s.sort();
    convo.participant_e164s.dedup();
    convo
}

/// Prefer flat over archive (richer metadata); otherwise keep the earlier timestamp.
fn should_replace_kept(existing: &PendingMessage, incoming: &ParsedMessage) -> bool {
    let existing_flat = existing.extra_str("source_kind") == "flat";
    let incoming_flat = incoming.source_kind == "flat";
    if incoming_flat && !existing_flat {
        return true;
    }
    if !incoming_flat && existing_flat {
        return false;
    }
    if incoming_flat
        && existing_flat
        && incoming
            .smssync_id
            .as_ref()
            .is_some_and(|s| !s.trim().is_empty())
        && existing.extra_str("smssync_id").trim().is_empty()
    {
        return true;
    }
    (incoming.timestamp_secs as i64) < existing.sort_key
}

/// Map a parsed EML message onto the pending message shape.
fn pending_from_parsed(msg: ParsedMessage, pending_atts: Vec<PendingAttachment>) -> PendingMessage {
    let date_ms = timestamp_ms(msg.timestamp_secs).to_string();
    let name = msg.name_alias.clone().unwrap_or_default();
    PendingMessage {
        sort_key: msg.timestamp_secs as i64,
        is_from_me: msg.is_from_me,
        sender_handle: msg.sender_digits.unwrap_or_default(),
        sender_display_name: msg.name_alias,
        text: msg.text,
        attachments: pending_atts,
        extra: {
            let mut e = BTreeMap::new();
            e.insert("source_kind".into(), msg.source_kind);
            e.insert("smssync_id".into(), msg.smssync_id.unwrap_or_default());
            e.insert("date_ms".into(), date_ms);
            e.insert("contact_name".into(), name);
            e.insert("android_type".into(), msg.android_type);
            e.insert("eml_path".into(), msg.eml_path);
            e
        },
    }
}

/// Add a parsed message to its conversation, replacing a kept twin when this copy carries
/// more (dedupe by cover identity).
fn add_message(
    conversations: &mut HashMap<String, PendingConversation>,
    by_identity: &mut HashMap<String, HashMap<String, usize>>,
    msg: ParsedMessage,
    pending_atts: Vec<PendingAttachment>,
    report: &mut ExportReport,
) {
    let chat_id = chat_id_for(&msg);
    let dedupe_key = cover_identity(&msg);
    let name_only = name_only_key(&msg).is_some();

    let peers: Vec<String> = peer_handles_from_digits(&msg.participant_digits);
    let convo = ensure_convo(
        conversations,
        &chat_id,
        msg.conversation_type == "group",
        msg.group_title.clone(),
        peers,
    );
    if name_only {
        convo
            .extra
            .insert(message_ir::CHAT_ID_IS_NAME.to_string(), "1".to_string());
    }

    report.bump("messages_before_dedupe", 1);

    // Online dedupe state keyed by chat id: fingerprint → index in `messages`
    // (keep earliest `sort_key`). The shared PendingConversation carries
    // document data only.
    let idx_map = by_identity.entry(chat_id.clone()).or_default();

    if let Some(&idx) = idx_map.get(&dedupe_key) {
        report.duplicates_dropped += 1;
        if should_replace_kept(&convo.messages[idx], &msg) {
            let kept_atts = std::mem::take(&mut convo.messages[idx].attachments);
            let mut pending = pending_from_parsed(msg, pending_atts);
            merge_attachments(&mut pending.attachments, kept_atts);
            convo.messages[idx] = pending;
        } else {
            merge_attachments(&mut convo.messages[idx].attachments, pending_atts);
        }
        return;
    }

    let idx = convo.messages.len();
    idx_map.insert(dedupe_key, idx);
    convo.messages.push(pending_from_parsed(msg, pending_atts));
}

/// Format each participant's digits as E.164 when unambiguous, dropping empties.
fn peer_handles_from_digits(participant_digits: &[(String, Option<String>)]) -> Vec<String> {
    participant_digits
        .iter()
        .map(|(d, _)| phone::normalize_lenient(d))
        .filter(|d| !d.is_empty())
        .collect()
}

/// True when the path has a `.eml` extension (any case).
pub(super) fn is_eml_file(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("eml"))
}

/// SMS Backup+ deltas of the shared [`message_ir::pending_to_document`] projection.
struct SbpProjection<'a> {
    export: ExportMeta,
    blob_bytes: &'a HashMap<String, Vec<u8>>,
}

impl ProjectionHooks for SbpProjection<'_> {
    fn export(&self) -> ExportMeta {
        self.export.clone()
    }

    fn service(&self, _msg: &PendingMessage) -> IrService {
        IrService::Sms
    }

    fn normalize_handle(&self, raw: &str) -> String {
        phone::normalize_lenient(raw)
    }

    fn attachment_to_ir(&self, att: &PendingAttachment, _msg: &PendingMessage) -> IrAttachment {
        att.to_ir(self.blob_bytes)
    }

    fn source(&self, convo: &PendingConversation, msg: &PendingMessage) -> IrSource {
        let mut fields = serde_json::Map::new();
        for key in ["source_kind", "smssync_id", "eml_path"] {
            let value = msg.extra_str(key);
            if !value.is_empty() {
                fields.insert(key.into(), serde_json::Value::String(value.to_string()));
            }
        }
        if let Some(title) = convo.display_name.as_deref().filter(|t| !t.is_empty()) {
            // Android group title stored as data only. Filenames do not use it.
            fields.insert(
                "android_group_title".into(),
                serde_json::Value::String(title.to_string()),
            );
        }
        IrSource {
            android_type: parse_android_type(msg.extra_str("android_type")),
            fields,
        }
    }
}

const EML_PROGRESS_EVERY: u64 = 5000;

/// Verbose-only log output: every method is a no-op unless `--verbose` was passed.
#[derive(Clone, Copy)]
struct Verbose<'a> {
    enabled: bool,
    log: Option<&'a LogSink>,
}

impl Verbose<'_> {
    /// Write one line when verbose.
    fn line(self, msg: impl AsRef<str>) {
        if self.enabled {
            emit_log(self.log, msg);
        }
    }

    /// Write a `label: N / total` line every [`EML_PROGRESS_EVERY`] items and at the end.
    fn progress(self, label: &str, processed: u64, total: u64) {
        if !self.enabled || total == 0 {
            return;
        }
        if processed == total || processed.is_multiple_of(EML_PROGRESS_EVERY) {
            emit_log(self.log, format!("{label}: {processed} / {total}"));
        }
    }

    /// List the first twenty error lines from the report, and how many more there were.
    fn errors(self, report: &ExportReport) {
        if !self.enabled || report.errors.is_empty() {
            return;
        }
        emit_log(self.log, format!("errors: {}", report.errors.len()));
        for err in report.errors.iter().take(20) {
            emit_log(self.log, format!("  {err}"));
        }
        if report.errors.len() > 20 {
            emit_log(
                self.log,
                format!("  … and {} more", report.errors.len() - 20),
            );
        }
    }
}

/// Inputs for [`convert_export`].
pub(crate) struct ConvertExportArgs<'a, P: AsRef<Path>> {
    pub inputs: &'a [P],
    pub output_dir: &'a Path,
    pub owner_phones: &'a [String],
    pub owner_emails: &'a [String],
    pub verbose: bool,
    pub transforms: ExportTransforms,
    pub output_format: OutputFormat,
    pub cancel: Option<&'a CancelFlag>,
    pub log: Option<&'a LogSink>,
    /// Continue an interrupted export: keep previous output and skip the
    /// conversations already written.
    pub resume: bool,
}

/// Convert SMS Backup+ EML tree(s) into the shared conversation structure, then
/// write the chosen output format.
///
/// Deduplication runs while scanning, using [`cover_identity`] (second-floored
/// chat + direction + text) so archive and flat copies of the same SMS collapse.
/// When `cancel` is set, cooperative cancellation is checked during the EML walk
/// and while merging parse results.
///
/// # Errors
///
/// Returns an error when no `.eml` files are found, output overlaps an input,
/// a file cannot be read or written, or the user cancels.
pub(crate) fn convert_export<P: AsRef<Path>>(
    args: ConvertExportArgs<'_, P>,
) -> Result<(ExportReport, FormatSinkResult)> {
    let ConvertExportArgs {
        inputs,
        output_dir,
        owner_phones,
        owner_emails,
        verbose,
        transforms,
        output_format,
        cancel,
        log,
        resume,
    } = args;
    let verbose = Verbose {
        enabled: verbose,
        log,
    };
    let owners = OwnerHandleSet::from_phones(owner_phones)?;
    let owner_handle = owners
        .primary_owner_handle()
        .expect("from_phones guarantees a phone owner handle");
    let owner_digits = owners.all_phone_digits();
    let owner_emails_lc: Vec<String> = owner_emails
        .iter()
        .map(|e| e.trim().to_ascii_lowercase())
        .filter(|e| !e.is_empty())
        .collect();
    verbose.line(format!("owner phones: {}", owner_digits.len()));
    verbose.line(format!("owner emails: {}", owner_emails_lc.len()));
    verbose.line(format!("output: {}", output_dir.display()));

    let input_paths: Vec<PathBuf> = inputs.iter().map(|p| p.as_ref().to_path_buf()).collect();
    let (inputs, output_dir) = prepare_outputs(&input_paths, output_dir)?;
    let writer = ExportWriter::open(&output_dir, output_format, transforms, resume)?;

    let eml_paths = collect_eml_paths(&inputs, cancel)?;
    verbose.line(format!(
        "scanning {} .eml files (parallel parse)",
        eml_paths.len()
    ));
    message_vault_io_core::check_cancel(cancel)?;

    let parse = ParseInputs {
        file_inputs: inputs.iter().filter(|p| p.is_file()).cloned().collect(),
        input_roots: inputs,
        owner_digits,
        owner_emails_lc,
    };
    let mut ingest = EmlIngest::new(writer.copies_attachments(), eml_paths.len());
    parse_all_emls(&eml_paths, &parse, cancel, verbose, &mut ingest)?;
    verbose.line(ingest.parse_summary());
    let EmlIngest {
        blob_bytes,
        conversations,
        mut report,
        ..
    } = ingest;

    let hooks = SbpProjection {
        export: message_vault_io_core::export_meta(
            EXPORT_SOURCE,
            EXPORT_TOOL,
            EXPORT_TOOL_VERSION,
            Some(owner_handle),
            None,
        ),
        blob_bytes: &blob_bytes,
    };
    let mut documents = Vec::new();
    for (chat_id, mut convo) in conversations {
        message_vault_io_core::check_cancel(cancel)?;
        if let Some(doc) = project_conversation(&chat_id, &mut convo, &hooks, &mut report) {
            documents.push(doc);
        }
    }

    if !writer.use_queue() {
        verbose.line(format!(
            "writing {} conversation files (duplicates dropped so far: {})",
            documents.len(),
            report.duplicates_dropped
        ));
    }
    let sink_result = writer.finish(
        documents,
        &mut AttachmentSource::take_bytes,
        cancel,
        &mut report,
    )?;

    verbose.line(format!(
        "done: conversations={} messages={} duplicates_dropped={} attachments={}",
        report.conversations, report.messages, report.duplicates_dropped, report.attachments_saved
    ));
    verbose.errors(&report);
    Ok((report, sink_result))
}

/// Read-only inputs every parallel EML parse needs.
struct ParseInputs {
    /// The `--input` paths after output preparation; relative EML paths are
    /// computed against these.
    input_roots: Vec<PathBuf>,
    /// The subset of `input_roots` that are single files rather than folders.
    file_inputs: HashSet<PathBuf>,
    owner_digits: HashSet<String>,
    owner_emails_lc: Vec<String>,
}

/// How many EMLs one parallel batch parses before its results are folded in.
///
/// Chunking keeps attachment payloads from all being held in memory at once.
const EML_PARSE_CHUNK: usize = 256;

/// Parse every EML in parallel chunks and fold the outcomes into `ingest`.
///
/// # Errors
///
/// Returns an error when the run is cancelled.
fn parse_all_emls(
    eml_paths: &[PathBuf],
    inputs: &ParseInputs,
    cancel: Option<&CancelFlag>,
    verbose: Verbose<'_>,
    ingest: &mut EmlIngest,
) -> Result<()> {
    let total = eml_paths.len() as u64;
    let mut scanned = 0u64;
    for chunk in eml_paths.chunks(EML_PARSE_CHUNK) {
        message_vault_io_core::check_cancel(cancel)?;
        let outcomes: Vec<ParsedEmlKind> = chunk
            .par_iter()
            .map(|eml_path| parse_eml_path(eml_path, inputs, cancel))
            .collect();
        for outcome in outcomes {
            message_vault_io_core::check_cancel(cancel)?;
            scanned += 1;
            verbose.progress("scanned", scanned, total);
            ingest.absorb(outcome)?;
        }
    }
    Ok(())
}

/// Parse one EML on a worker thread. Checks cancel first so a cancelled run
/// stops reading files promptly.
fn parse_eml_path(
    eml_path: &Path,
    inputs: &ParseInputs,
    cancel: Option<&CancelFlag>,
) -> ParsedEmlKind {
    if message_vault_io_core::is_cancelled(cancel) {
        return ParsedEmlKind::Cancelled;
    }
    let rel_path = relative_eml_path(eml_path, &inputs.input_roots, &inputs.file_inputs);
    parse_one_eml(
        eml_path,
        rel_path,
        &inputs.owner_digits,
        &inputs.owner_emails_lc,
    )
}

/// Everything the scan accumulates: conversations, dedupe state, attachment
/// bytes, and the counts that end up in the report.
struct EmlIngest {
    copy_attachments: bool,
    /// Attachment bytes by digest, kept until the writer asks for them.
    blob_bytes: HashMap<String, Vec<u8>>,
    conversations: HashMap<String, PendingConversation>,
    /// Online dedupe state (fingerprint → message index) keyed by chat id;
    /// the shared `PendingConversation` carries document data only.
    by_identity: HashMap<String, HashMap<String, usize>>,
    report: ExportReport,
}

impl EmlIngest {
    /// Empty state, pre-sized for the typical ratio of chats to EML files.
    fn new(copy_attachments: bool, eml_count: usize) -> Self {
        Self {
            copy_attachments,
            blob_bytes: HashMap::new(),
            conversations: HashMap::with_capacity((eml_count / 4).min(50_000)),
            by_identity: HashMap::new(),
            report: ExportReport::default(),
        }
    }

    /// Fold one parsed EML into the pending conversations.
    ///
    /// # Errors
    ///
    /// Returns an error when a worker saw the cancel flag.
    fn absorb(&mut self, outcome: ParsedEmlKind) -> Result<()> {
        match outcome {
            ParsedEmlKind::Cancelled => bail!("cancelled"),
            ParsedEmlKind::Archive {
                msgs,
                skipped_dates,
                path_display,
            } => {
                self.report.bump("archive_eml", 1);
                self.report.skipped_invalid_date += skipped_dates;
                // An archive that yields nothing is a whole conversation lost.
                // Say which file, because the export otherwise finishes looking
                // healthy and the person has no way to notice.
                if msgs.is_empty() {
                    self.report.bump("empty_archive_eml", 1);
                    self.report
                        .errors
                        .push(format!("{path_display}: archive held no messages"));
                }
                for msg in msgs {
                    self.add_parsed(msg);
                }
            }
            ParsedEmlKind::Flat { msg } => {
                self.report.bump("flat_eml", 1);
                self.add_parsed(*msg);
            }
            ParsedEmlKind::FlatNone => self.report.bump("skipped_parse_error", 1),
            ParsedEmlKind::NotSms => self.report.bump("skipped_not_sms_backup_plus", 1),
            ParsedEmlKind::IoError(msg) => self.report.errors.push(msg),
            ParsedEmlKind::ParseError(msg) => {
                self.report.bump("skipped_parse_error", 1);
                self.report.errors.push(msg);
            }
        }
        Ok(())
    }

    /// Queue one message's attachments and add it to its conversation.
    fn add_parsed(&mut self, msg: ParsedMessage) {
        if msg.chat_key.is_empty() {
            self.report.bump("unknown_chat_messages", 1);
        }
        let atts = queue_attachments(
            &msg.attachments,
            self.copy_attachments,
            &mut self.blob_bytes,
        );
        add_message(
            &mut self.conversations,
            &mut self.by_identity,
            msg,
            atts,
            &mut self.report,
        );
    }

    /// One line of parse counters for the verbose log.
    fn parse_summary(&self) -> String {
        format!(
            "parsed: flat_eml={} archive_eml={} messages={} unknown_chat={} skipped_not_sms_backup_plus={} skipped_parse_error={} skipped_bad_date={}",
            self.report.extra("flat_eml"),
            self.report.extra("archive_eml"),
            self.report.extra("messages_before_dedupe"),
            self.report.extra("unknown_chat_messages"),
            self.report.extra("skipped_not_sms_backup_plus"),
            self.report.extra("skipped_parse_error"),
            self.report.skipped_invalid_date
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::AttachmentBlob;
    use message_ir::{
        ConversationDocument, ConversationMeta, ConversationStats, IrConversationType, IrDirection,
        IrMessage, IrMessageKind, SCHEMA_VERSION,
    };

    #[test]
    fn merge_attachments_unions_by_digest() {
        let mut into = vec![PendingAttachment {
            rel_path: "attachments/a.jpg".into(),
            content_type: "image/jpeg".into(),
            extension: "jpg".into(),
            digest_sha256: Some("aaa".into()),
            name_hint: Some("a.jpg".into()),
        }];
        let from = vec![
            PendingAttachment {
                rel_path: "attachments/a.jpg".into(),
                content_type: "image/jpeg".into(),
                extension: "jpg".into(),
                digest_sha256: Some("aaa".into()),
                name_hint: Some("a.jpg".into()),
            },
            PendingAttachment {
                rel_path: "attachments/b.jpg".into(),
                content_type: "image/jpeg".into(),
                extension: "jpg".into(),
                digest_sha256: Some("bbb".into()),
                name_hint: Some("b.jpg".into()),
            },
        ];
        merge_attachments(&mut into, from);
        assert_eq!(into.len(), 2);
        assert_eq!(into[1].digest_sha256.as_deref(), Some("bbb"));
    }

    #[test]
    fn queue_attachments_keeps_message_on_single_failure() {
        let dir = tempfile::tempdir().unwrap();
        let att_dir = dir.path().join("attachments");
        std::fs::create_dir_all(&att_dir).unwrap();
        // Empty bytes: the runner records file_missing and continues.
        let blobs = vec![
            AttachmentBlob {
                filename: "missing.jpg".into(),
                original_name: None,
                mime_type: Some("image/jpeg".into()),
                digest_hex: "aaa".into(),
                data: vec![],
            },
            AttachmentBlob {
                filename: "ok.jpg".into(),
                original_name: None,
                mime_type: Some("image/jpeg".into()),
                digest_hex: "bbb".into(),
                data: vec![4, 5, 6],
            },
        ];
        let mut blob_bytes = std::collections::HashMap::new();
        let queued = queue_attachments(&blobs, true, &mut blob_bytes);
        assert_eq!(queued.len(), 2);

        let mut atts: Vec<_> = queued.iter().map(|a| a.to_ir(&blob_bytes)).collect();
        let mut report = ExportReport::default();
        let mut doc = ConversationDocument {
            schema_version: SCHEMA_VERSION,
            export: ExportMeta {
                source: String::new(),
                tool: String::new(),
                tool_version: String::new(),
                owner_handle: None,
                owner_display_name: None,
            },
            conversation: ConversationMeta {
                chat_identifier: "test".into(),
                conversation_type: IrConversationType::Individual,
                group_title: None,
                participants: Vec::new(),
                stats: ConversationStats::default(),
            },
            messages: vec![IrMessage {
                guid: "g".into(),
                timestamp_unix_ms: 0,
                direction: IrDirection::Incoming,
                service: IrService::Sms,
                message_kind: IrMessageKind::Mms,
                sender_handle: None,
                sender_display_name: None,
                subject: None,
                text: "hi".into(),
                attachments: std::mem::take(&mut atts),
                imessage: None,
                source: None,
            }],
            packaging_stem_suffix: None,
        };
        let payloads: Vec<Option<Vec<u8>>> = doc
            .messages
            .iter()
            .flat_map(|msg| msg.attachments.iter().map(|att| att.bytes.clone()))
            .collect();
        message_vault_io_core::stage_conversation_attachments(
            std::slice::from_mut(&mut doc),
            &att_dir,
            &message_vault_io_core::MediaConfig::default(),
            |i| Ok(payloads.get(i).cloned().flatten()),
            None,
            None,
            None,
            &mut report,
        )
        .unwrap();
        // The missing source stays on the message; the good one is staged.
        assert_eq!(doc.messages[0].attachments.len(), 2);
        assert_eq!(
            doc.messages[0].attachments[0].missing_reason.as_deref(),
            Some("file_missing")
        );
        assert!(doc.messages[0].attachments[1].path.is_some());
        assert_eq!(report.attachments_saved, 1);
        assert_eq!(std::fs::read_dir(&att_dir).unwrap().count(), 1);
    }

    /// Which copy of a duplicated message is kept.
    ///
    /// When two `.eml` files describe the same message, `should_replace_kept`
    /// decides whether the one arriving now replaces the one already held.
    /// It has four rules and no test had exercised any of them: the whole
    /// function could be replaced with `true` or `false` and the suite stayed
    /// green, which means the export silently kept the worse copy — an archive
    /// transcript line rather than the flat `.eml` that carries the headers.
    ///
    /// The rules, in the order the function applies them:
    ///
    /// 1. A flat message always beats an archive one, because a flat `.eml`
    ///    carries `X-smssync-*` headers and an archive transcript line does
    ///    not.
    /// 2. An archive message never displaces a flat one.
    /// 3. Between two flat messages, one with an `X-smssync-id` beats one
    ///    without.
    /// 4. Otherwise the earlier timestamp wins, so re-running an import does
    ///    not shuffle the order.
    #[test]
    fn a_flat_message_beats_an_archive_one_whichever_arrives_first() {
        let archive_kept = pending(1_000, "archive", "");
        let flat_incoming = parsed(1_000.0, "flat", None);
        assert!(
            should_replace_kept(&archive_kept, &flat_incoming),
            "a flat message replaces an archive one"
        );

        let flat_kept = pending(1_000, "flat", "");
        let archive_incoming = parsed(1_000.0, "archive", None);
        assert!(
            !should_replace_kept(&flat_kept, &archive_incoming),
            "an archive message does not displace a flat one"
        );
    }

    #[test]
    fn between_two_flat_messages_the_one_with_an_smssync_id_wins() {
        let without_id = pending(1_000, "flat", "");
        let with_id = parsed(1_000.0, "flat", Some("276"));
        assert!(
            should_replace_kept(&without_id, &with_id),
            "an id is more than no id"
        );

        // And not the other way round: a message with an id is not replaced by
        // one without, even at the same instant.
        let kept_with_id = pending(1_000, "flat", "276");
        let incoming_without = parsed(1_000.0, "flat", None);
        assert!(!should_replace_kept(&kept_with_id, &incoming_without));

        // A blank or whitespace id is no id at all.
        for blank in ["", "   "] {
            let incoming_blank = parsed(1_000.0, "flat", Some(blank));
            assert!(
                !should_replace_kept(&without_id, &incoming_blank),
                "an id of {blank:?} is not an id"
            );
        }
    }

    #[test]
    fn otherwise_the_earlier_message_is_the_one_kept() {
        let kept = pending(1_000, "flat", "276");

        let earlier = parsed(999.0, "flat", Some("276"));
        assert!(
            should_replace_kept(&kept, &earlier),
            "an earlier copy replaces a later one, so a re-import is stable"
        );

        let later = parsed(1_001.0, "flat", Some("276"));
        assert!(!should_replace_kept(&kept, &later));

        // The same instant is not earlier, so the first one seen stays.
        let same = parsed(1_000.0, "flat", Some("276"));
        assert!(!should_replace_kept(&kept, &same));
    }

    /// A `PendingMessage` already held, with the two fields the decision reads.
    fn pending(sort_key: i64, source_kind: &str, smssync_id: &str) -> PendingMessage {
        let mut extra = std::collections::BTreeMap::new();
        extra.insert("source_kind".to_string(), source_kind.to_string());
        extra.insert("smssync_id".to_string(), smssync_id.to_string());
        PendingMessage {
            sort_key,
            is_from_me: false,
            sender_handle: "+15555550101".into(),
            sender_display_name: None,
            text: "hello".into(),
            attachments: Vec::new(),
            extra,
        }
    }

    /// A `ParsedMessage` arriving now, with the three fields the decision reads.
    fn parsed(timestamp_secs: f64, source_kind: &str, smssync_id: Option<&str>) -> ParsedMessage {
        ParsedMessage {
            chat_key: "+15555550101".into(),
            conversation_type: "individual".into(),
            group_title: None,
            participant_digits: Vec::new(),
            timestamp_secs,
            is_from_me: false,
            sender_digits: Some("15555550101".into()),
            text: "hello".into(),
            attachments: Vec::new(),
            name_alias: None,
            smssync_id: smssync_id.map(str::to_string),
            source_kind: source_kind.into(),
            android_type: "1".into(),
            eml_path: "flat.eml".into(),
        }
    }
}
