//! Convert a GO SMS Pro backup into the shared conversation structure
//! ([`ConversationDocument`]) every exporter writes, then write the chosen
//! output format via [`FormatSink`].

use crate::attachments_emit::queue_pdu_attachments;
use crate::chat_id::{chat_id_group, chat_id_individual, guarded_phone};
use crate::xml::{SkippedBadAddrDetail, XmlMessage, parse_xml_file};
use anyhow::{Context, Result, bail};
use go_sms_mms::{ParsedPdu, parse_pdu_file};
use message_ir::{
    ExportMeta, IrAttachment, IrService, IrSource, PendingAttachment, PendingConversation,
    PendingMessage, ProjectionHooks, ensure_conversation, parse_android_type,
};
use message_staging::{AttachmentSource, ExportWriter};
use message_vault_io_core::{
    CancelFlag, ExportReport, ExportTransforms, OutputFormat, prepare_outputs, project_conversation,
};
use phone::OwnerHandleSet;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

const EXPORT_SOURCE: &str = "go-sms-pro";
const EXPORT_TOOL: &str = "GO SMS Pro";
/// Upstream app version not pinned yet (empty in CSV).
const EXPORT_TOOL_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Cap on retained skip-detail rows; overflow is counted and reported.
pub(crate) const MAX_SKIP_DETAILS: usize = 20;

/// Push one diagnostic row, keeping at most [`MAX_SKIP_DETAILS`] entries so
/// huge backups cannot grow the detail vectors without bound.
fn push_skip_detail<T>(details: &mut Vec<T>, more: &mut u64, item: T) {
    if details.len() < MAX_SKIP_DETAILS {
        details.push(item);
    } else {
        *more += 1;
    }
}

/// Skipped-row diagnostics kept out of the shared [`ExportReport`]: only used
/// to write the `skipped_*` CSV files at the end of [`convert_export`].
#[derive(Default)]
struct SkipDetails {
    invalid_address: Vec<SkippedBadAddrDetail>,
    invalid_address_more: u64,
    empty_pdu: Vec<SkippedEmptyPduDetail>,
    empty_pdu_more: u64,
    no_party: Vec<SkippedNoPartyDetail>,
    no_party_more: u64,
}

/// Diagnostic row for an empty/stub PDU file.
#[derive(Debug, Clone)]
pub(crate) struct SkippedEmptyPduDetail {
    pub pdu_filename: String,
}

/// Diagnostic row when a non-group PDU has no non-owner peer.
#[derive(Debug, Clone)]
pub(crate) struct SkippedNoPartyDetail {
    pub pdu_filename: String,
    pub participants: String,
    pub is_sent: bool,
    pub has_from: bool,
    pub has_to: bool,
}

/// Append parsed XML SMS rows to pending conversations.
fn add_xml_messages(
    conversations: &mut BTreeMap<String, PendingConversation>,
    msgs: Vec<XmlMessage>,
) {
    for msg in msgs {
        let chat_id = chat_id_individual(&msg.other_digits);
        let convo = ensure_conversation(conversations, &chat_id, false, None, Vec::new());
        let dedupe_key = format!(
            "{}|{}|{}|",
            msg.timestamp_secs as i64,
            if msg.is_from_me { "1" } else { "0" },
            msg.text
        );
        convo.messages.push(PendingMessage {
            sort_key: msg.timestamp_secs as i64,
            is_from_me: msg.is_from_me,
            sender_handle: msg.sender_digits.unwrap_or_default(),
            sender_display_name: msg.name_alias.clone(),
            text: msg.text,
            attachments: Vec::new(),
            extra: {
                let mut e = BTreeMap::new();
                e.insert("dedupe_key".into(), dedupe_key);
                e.insert("source_kind".into(), "xml".to_string());
                e.insert("android_type".into(), msg.android_type);
                e.insert("date_ms".into(), msg.date_ms);
                e.insert("contact_name".into(), msg.contact_name);
                // XML rows carry no PDU diagnostics; absent keys read back empty.
                for (k, v) in msg.xml_fields {
                    e.insert(format!("xml:{k}"), v);
                }
                e
            },
        });
    }
}

/// File name of the PDU on disk (for skip-detail rows).
fn pdu_basename(parsed: &ParsedPdu) -> String {
    parsed
        .path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string()
}

/// GO SMS Pro stub / hollow PDU: no peers, no text, no media, no From/To headers.
fn is_empty_pdu(parsed: &ParsedPdu) -> bool {
    parsed.participants.is_empty()
        && parsed.body.trim().is_empty()
        && parsed.attachments.is_empty()
        && !parsed.has_from
        && !parsed.has_to
}

/// The chat a PDU message lands in.
struct PduTarget {
    chat_id: String,
    is_group: bool,
    group_title: Option<String>,
    /// The non-owner peers of a group; empty for an individual chat.
    peers: Vec<String>,
}

/// Append one parsed PDU (binary SMS/MMS) to the conversation it belongs to.
fn add_pdu_message(
    conversations: &mut BTreeMap<String, PendingConversation>,
    parsed: ParsedPdu,
    attachments: Vec<PendingAttachment>,
    owners: &OwnerHandleSet,
    report: &mut ExportReport,
    skips: &mut SkipDetails,
) {
    if is_empty_pdu(&parsed) {
        report.bump("skipped_empty_pdu", 1);
        push_skip_detail(
            &mut skips.empty_pdu,
            &mut skips.empty_pdu_more,
            SkippedEmptyPduDetail {
                pdu_filename: pdu_basename(&parsed),
            },
        );
        return;
    }
    let Some(target) = pdu_target(&parsed, owners, report, skips) else {
        return;
    };
    report.bump("pdu_messages", 1);
    if target.is_group {
        report.bump("pdu_group_messages", 1);
    }
    let pending = pdu_pending_message(parsed, attachments);
    let convo = ensure_conversation(
        conversations,
        &target.chat_id,
        target.is_group,
        target.group_title,
        target.peers,
    );
    convo.messages.push(pending);
}

/// The chat the PDU belongs to, from the participants that are not the
/// owner: a group when the PDU says so or when there are two or more of
/// them, else the one other party. `None`, counted and detailed as a skip,
/// when nobody but the owner is on it.
fn pdu_target(
    parsed: &ParsedPdu,
    owners: &OwnerHandleSet,
    report: &mut ExportReport,
    skips: &mut SkipDetails,
) -> Option<PduTarget> {
    let others: Vec<_> = parsed
        .participants
        .iter()
        .filter(|p| !p.is_empty() && !owners.is_owner_digits(p))
        .cloned()
        .collect();
    if others.is_empty() {
        report.bump("skipped_no_other_party", 1);
        push_skip_detail(
            &mut skips.no_party,
            &mut skips.no_party_more,
            SkippedNoPartyDetail {
                pdu_filename: pdu_basename(parsed),
                participants: parsed.participants.join(";"),
                is_sent: parsed.is_sent,
                has_from: parsed.has_from,
                has_to: parsed.has_to,
            },
        );
        return None;
    }
    // Treat multi-peer MMS as a group even when the PDU flag is unset.
    if parsed.is_group || others.len() >= 2 {
        let (chat_id, title) = chat_id_group(&parsed.participants, owners);
        Some(PduTarget {
            chat_id,
            is_group: true,
            group_title: Some(title),
            peers: others.iter().map(|d| guarded_phone(d)).collect(),
        })
    } else {
        Some(PduTarget {
            chat_id: chat_id_individual(&others[0]),
            is_group: false,
            group_title: None,
            peers: Vec::new(),
        })
    }
}

/// The pending message for a PDU. Its `extra` map carries the dedupe key
/// (time, direction, body, attachment digests) and the PDU diagnostics the
/// projection reads back into the IR source fields.
fn pdu_pending_message(parsed: ParsedPdu, attachments: Vec<PendingAttachment>) -> PendingMessage {
    let att_names: Vec<String> = attachments
        .iter()
        .map(|a| a.digest_sha256.clone().unwrap_or_default())
        .collect();
    let dedupe_key = format!(
        "{}|{}|{}|{}",
        parsed.timestamp,
        if parsed.is_sent { "1" } else { "0" },
        parsed.body,
        att_names.join(",")
    );
    // The projection names the owner as the sender of every outgoing message
    // itself, so only a received PDU carries its sender here.
    let sender_handle = if parsed.is_sent {
        String::new()
    } else {
        parsed.sender_number.clone()
    };
    let mut extra = BTreeMap::new();
    extra.insert("dedupe_key".into(), dedupe_key);
    extra.insert("source_kind".into(), "pdu".to_string());
    extra.insert("android_type".into(), String::new());
    extra.insert("date_ms".into(), String::new());
    extra.insert("contact_name".into(), String::new());
    extra.insert("pdu_filename".into(), pdu_basename(&parsed));
    extra.insert("pdu_decode".into(), parsed.decode_quality.to_string());
    if !parsed.pdu_fields.is_empty() {
        extra.insert(
            "pdu_fields".into(),
            serde_json::to_string(&parsed.pdu_fields).unwrap_or_default(),
        );
    }
    PendingMessage {
        sort_key: parsed.timestamp,
        is_from_me: parsed.is_sent,
        sender_handle,
        sender_display_name: None,
        text: parsed.body,
        attachments,
        extra,
    }
}

/// Key prefix shared by XML and PDU rows: `secs|direction|text` up to the
/// trailing attachment section. The XML key (`…|text|`) is a strict prefix of
/// the PDU key (`…|body|att_names`) for the same message, so exact-key dedupe
/// alone would let both rows through and export the MMS twice.
fn dedupe_base_key(key: &str) -> &str {
    key.rsplit_once('|').map_or(key, |(base, _)| base)
}

/// Drop duplicate pending messages, keeping the row with more attachments.
fn dedupe_messages(messages: &mut Vec<PendingMessage>) {
    messages.sort_by(|a, b| {
        a.sort_key
            .partial_cmp(&b.sort_key)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut seen_base: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<PendingMessage> = Vec::with_capacity(messages.len());
    for m in messages.drain(..) {
        let base = dedupe_base_key(m.extra_str("dedupe_key")).to_string();
        match seen_base.get(&base).copied() {
            None => {
                seen_base.insert(base, out.len());
                out.push(m);
            }
            Some(idx) => {
                let existing = &out[idx];
                if existing.attachments.is_empty() && !m.attachments.is_empty() {
                    // Same message in the XML backup (no attachments) and its
                    // PDU file (with media): keep the row that carries them.
                    out[idx] = m;
                } else if existing.attachments.is_empty() || m.attachments.is_empty() {
                    // Exact duplicate or an attachment-less row shadowed by a
                    // richer one already kept: drop it.
                } else {
                    // Two attachment-bearing rows with the same prefix are
                    // distinct MMS (same second, direction, and caption but
                    // different media): keep both.
                    seen_base.insert(base, out.len());
                    out.push(m);
                }
            }
        }
    }
    *messages = out;
}

/// True when the path has a `.xml` extension (any case).
fn is_xml_file(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("xml"))
}

/// True for GO SMS Pro PDU files named `I_*.pdu` (the binary encoding of an
/// SMS/MMS on the phone).
fn is_pdu_file(p: &Path) -> bool {
    p.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("I_") && n.ends_with(".pdu"))
}

/// GO SMS Pro deltas of the shared [`message_ir::pending_to_document`] projection.
struct GoSmsProjection<'a> {
    export: ExportMeta,
    blob_bytes: &'a HashMap<String, Vec<u8>>,
}

impl ProjectionHooks for GoSmsProjection<'_> {
    fn export(&self) -> ExportMeta {
        self.export.clone()
    }

    fn service(&self, _msg: &PendingMessage) -> IrService {
        IrService::Sms
    }

    fn normalize_handle(&self, raw: &str) -> String {
        guarded_phone(raw)
    }

    fn attachment_to_ir(&self, att: &PendingAttachment, _msg: &PendingMessage) -> IrAttachment {
        att.to_ir(self.blob_bytes)
    }

    fn source(&self, convo: &PendingConversation, msg: &PendingMessage) -> IrSource {
        let mut fields = serde_json::Map::new();
        fields.insert(
            "source_kind".into(),
            serde_json::Value::String(msg.extra_str("source_kind").to_string()),
        );
        let pdu_filename = msg.extra_str("pdu_filename");
        if !pdu_filename.is_empty() {
            fields.insert(
                "pdu_filename".into(),
                serde_json::Value::String(pdu_filename.to_string()),
            );
        }
        let pdu_decode = msg.extra_str("pdu_decode");
        if !pdu_decode.is_empty() {
            fields.insert(
                "pdu_decode".into(),
                serde_json::Value::String(pdu_decode.to_string()),
            );
        }
        let pdu_fields = msg.extra_str("pdu_fields");
        if !pdu_fields.is_empty() {
            fields.insert(
                "pdu_fields".into(),
                serde_json::from_str(pdu_fields).unwrap_or(serde_json::Value::Null),
            );
        }
        for (k, v) in &msg.extra {
            if let Some(k) = k.strip_prefix("xml:") {
                fields
                    .entry(k.to_string())
                    .or_insert_with(|| serde_json::Value::String(v.clone()));
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

/// Inputs for [`convert_export`].
pub(crate) struct ConvertExportArgs<'a> {
    pub input_dir: &'a Path,
    pub output_dir: &'a Path,
    pub owner_phones: &'a [String],
    pub transforms: ExportTransforms,
    pub output_format: OutputFormat,
    pub cancel: Option<&'a CancelFlag>,
    /// Continue an interrupted export: keep previous output and skip the
    /// conversations already written.
    pub resume: bool,
}

/// Convert a GO SMS Pro export directory into the shared conversation structure
/// ([`ConversationDocument`]), then write the chosen output format.
///
/// When `cancel` is set, cooperative cancellation is checked between XML files
/// and between PDU files. Cancelled runs return an error with message `cancelled`.
///
/// # Errors
///
/// Returns an error when the input is not a directory, output overlaps input,
/// a file cannot be read or written, or the user cancels.
pub(crate) fn convert_export(args: ConvertExportArgs<'_>) -> Result<ExportReport> {
    let ConvertExportArgs {
        input_dir,
        output_dir,
        owner_phones,
        transforms,
        output_format,
        cancel,
        resume,
    } = args;
    if !input_dir.is_dir() {
        bail!("input is not a directory: {}", input_dir.display());
    }
    let (inputs, output_dir) = prepare_outputs(&[input_dir.to_path_buf()], output_dir)?;
    let input_dir = &inputs[0];
    let owners = OwnerHandleSet::from_phones(owner_phones)?;
    let owner_handle = owners
        .primary_owner_handle()
        .expect("from_phones guarantees a phone owner handle");

    // Clean previous CSV / mail artifacts (keep attachments if re-run; rewrite as needed).
    let writer = ExportWriter::open(&output_dir, output_format, transforms, resume)?;
    let mut ingest = Ingest {
        owners: &owners,
        copy_attachments: writer.copies_attachments(),
        blob_bytes: HashMap::new(),
        conversations: BTreeMap::new(),
        report: ExportReport::default(),
        skips: SkipDetails::default(),
    };
    for xml_path in sorted_files(input_dir, &is_xml_file)? {
        message_vault_io_core::check_cancel(cancel)?;
        ingest.ingest_xml(&xml_path);
    }
    for pdu_path in sorted_files(input_dir, &is_pdu_file)? {
        message_vault_io_core::check_cancel(cancel)?;
        ingest.ingest_pdu(&pdu_path);
    }
    message_vault_io_core::check_cancel(cancel)?;
    let Ingest {
        blob_bytes,
        conversations,
        mut report,
        skips,
        ..
    } = ingest;

    let hooks = GoSmsProjection {
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
        dedupe_messages(&mut convo.messages);
        if let Some(doc) = project_conversation(&chat_id, &mut convo, &hooks, &mut report) {
            documents.push(doc);
        }
    }

    writer.finish(
        documents,
        &mut AttachmentSource::take_bytes,
        cancel,
        &mut report,
    )?;

    write_skipped_invalid_address_csv(
        &output_dir,
        &skips.invalid_address,
        skips.invalid_address_more,
    )?;
    write_skipped_empty_pdu_csv(&output_dir, &skips.empty_pdu, skips.empty_pdu_more)?;
    write_skipped_no_party_csv(&output_dir, &skips.no_party, skips.no_party_more)?;

    Ok(report)
}

/// Every file under `dir` matching `predicate`, in path order so runs are repeatable.
///
/// # Errors
///
/// Returns an error when the folder cannot be read.
fn sorted_files(dir: &Path, predicate: &dyn Fn(&Path) -> bool) -> Result<Vec<PathBuf>> {
    let mut paths = message_vault_io_core::discover_files(dir, predicate)?;
    paths.sort();
    Ok(paths)
}

/// Parse-time state shared across every XML and PDU file in one backup.
struct Ingest<'a> {
    owners: &'a OwnerHandleSet,
    copy_attachments: bool,
    /// Attachment bytes by digest, kept until the writer asks for them.
    blob_bytes: HashMap<String, Vec<u8>>,
    conversations: BTreeMap<String, PendingConversation>,
    report: ExportReport,
    skips: SkipDetails,
}

impl Ingest<'_> {
    /// Add every SMS row from one backup XML. Parse failures are recorded in
    /// the report and the file is skipped.
    fn ingest_xml(&mut self, xml_path: &Path) {
        let (msgs, stats) = match parse_xml_file(xml_path) {
            Ok(parsed) => parsed,
            Err(err) => {
                self.report
                    .errors
                    .push(format!("{}: {err:#}", xml_path.display()));
                return;
            }
        };
        self.report.bump("xml_messages_seen", stats.messages);
        self.report.skipped_invalid_date += stats.skipped_invalid_date;
        self.report
            .bump("skipped_unknown_type", stats.skipped_unknown_type);
        self.report
            .bump("skipped_unknown_address", stats.skipped_unknown_address);
        self.skips.invalid_address_more += stats.skipped_unknown_address_details_more;
        for detail in stats.skipped_unknown_address_details {
            push_skip_detail(
                &mut self.skips.invalid_address,
                &mut self.skips.invalid_address_more,
                detail,
            );
        }
        add_xml_messages(&mut self.conversations, msgs);
    }

    /// Add the MMS in one PDU file. Unparseable and empty PDUs are counted;
    /// the first twenty unparseable ones are named in the report.
    fn ingest_pdu(&mut self, pdu_path: &Path) {
        let all_digits = self.owners.all_phone_digits();
        let primary = self.owners.primary_owner_handle().unwrap_or_default();
        let parsed = parse_pdu_file(pdu_path, &all_digits, &primary);
        let parsed = match parsed {
            Ok(Some(parsed)) => parsed,
            Ok(None) => {
                self.report.bump("skipped_unparseable_pdu", 1);
                if self.report.errors.len() < 20 {
                    self.report
                        .errors
                        .push(format!("{}: unparseable PDU", pdu_path.display()));
                }
                return;
            }
            Err(err) => {
                self.report
                    .errors
                    .push(format!("{}: {err:#}", pdu_path.display()));
                return;
            }
        };
        let atts = queue_pdu_attachments(&parsed, self.copy_attachments, &mut self.blob_bytes);
        add_pdu_message(
            &mut self.conversations,
            parsed,
            atts,
            self.owners,
            &mut self.report,
            &mut self.skips,
        );
    }
}

fn remove_if_exists(path: &Path) {
    if path.exists() {
        let _ = fs::remove_file(path);
    }
}

/// Write `skipped_invalid_address.csv` (or remove a stale one) listing rows dropped for an unusable address.
fn write_skipped_invalid_address_csv(
    output_dir: &Path,
    details: &[SkippedBadAddrDetail],
    more: u64,
) -> Result<()> {
    let path = output_dir.join("skipped_invalid_address.csv");
    // Remove legacy filename from earlier builds.
    remove_if_exists(&output_dir.join("skipped_bad_addr.csv"));
    if details.is_empty() && more == 0 {
        remove_if_exists(&path);
        return Ok(());
    }
    let mut wtr =
        csv::Writer::from_path(&path).with_context(|| format!("create {}", path.display()))?;
    wtr.write_record([
        "xml_file",
        "address",
        "contact_name",
        "android_type",
        "date_ms",
        "body",
    ])?;
    for d in details {
        wtr.write_record([
            d.xml_file.as_str(),
            d.address.as_str(),
            d.contact_name.as_str(),
            d.android_type.as_str(),
            d.date_ms.as_str(),
            d.body.as_str(),
        ])?;
    }
    if more > 0 {
        wtr.write_record([
            "",
            "",
            "",
            "",
            "",
            &format!("...and {more} more entries not shown"),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

/// Write `skipped_empty_pdu.csv` (or remove a stale one) listing stub PDU files.
fn write_skipped_empty_pdu_csv(
    output_dir: &Path,
    details: &[SkippedEmptyPduDetail],
    more: u64,
) -> Result<()> {
    let path = output_dir.join("skipped_empty_pdu.csv");
    if details.is_empty() && more == 0 {
        remove_if_exists(&path);
        return Ok(());
    }
    let mut wtr =
        csv::Writer::from_path(&path).with_context(|| format!("create {}", path.display()))?;
    wtr.write_record(["pdu_filename"])?;
    for d in details {
        wtr.write_record([d.pdu_filename.as_str()])?;
    }
    if more > 0 {
        wtr.write_record([&format!("...and {more} more entries not shown")])?;
    }
    wtr.flush()?;
    Ok(())
}

/// Write `skipped_no_party.csv` (or remove a stale one) listing MMS with no non-owner participant.
fn write_skipped_no_party_csv(
    output_dir: &Path,
    details: &[SkippedNoPartyDetail],
    more: u64,
) -> Result<()> {
    let path = output_dir.join("skipped_no_party.csv");
    if details.is_empty() && more == 0 {
        remove_if_exists(&path);
        return Ok(());
    }
    let mut wtr =
        csv::Writer::from_path(&path).with_context(|| format!("create {}", path.display()))?;
    wtr.write_record([
        "pdu_filename",
        "participants",
        "is_sent",
        "has_from",
        "has_to",
    ])?;
    for d in details {
        wtr.write_record([
            d.pdu_filename.as_str(),
            d.participants.as_str(),
            if d.is_sent { "1" } else { "0" },
            if d.has_from { "1" } else { "0" },
            if d.has_to { "1" } else { "0" },
        ])?;
    }
    if more > 0 {
        wtr.write_record([
            "",
            "",
            "",
            "",
            &format!("...and {more} more entries not shown"),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_chat_ids_do_not_collide_on_digit_boundaries() {
        let owners = OwnerHandleSet::from_phones(&["+15555550100".into()]).unwrap();
        let (a, _) = chat_id_group(&["12".into(), "34".into()], &owners);
        let (b, _) = chat_id_group(&["123".into(), "4".into()], &owners);
        assert_ne!(a, b);
        assert!(a.contains("2:12"));
        assert!(b.contains("3:123"));
    }

    #[test]
    fn multi_peer_pdu_without_group_flag_uses_group_chat_id() {
        let owners = OwnerHandleSet::from_phones(&["+15555550100".into()]).unwrap();
        let parsed = ParsedPdu {
            path: std::path::PathBuf::from("I_1609459200_x.pdu"),
            timestamp: 1_609_459_200,
            participants: vec![
                "15555550100".into(),
                "15555550122".into(),
                "15555550133".into(),
            ],
            body: "hi".into(),
            attachments: Vec::new(),
            is_sent: true,
            is_group: false,
            sender_number: String::new(),
            has_from: false,
            has_to: true,
            pdu_fields: BTreeMap::new(),
            decode_quality: "structured",
        };
        let mut conversations = BTreeMap::new();
        let mut report = ExportReport::default();
        let mut skips = SkipDetails::default();
        add_pdu_message(
            &mut conversations,
            parsed,
            Vec::new(),
            &owners,
            &mut report,
            &mut skips,
        );
        assert_eq!(conversations.len(), 1);
        let convo = conversations.values().next().unwrap();
        assert!(convo.is_group);
        assert!(convo.chat_id.starts_with("chat-group-"));
        assert_eq!(report.extra("pdu_group_messages"), 1);
    }

    fn test_msg(key: &str, attachments: usize) -> PendingMessage {
        PendingMessage {
            sort_key: 1609459200,
            is_from_me: false,
            sender_handle: String::new(),
            sender_display_name: None,
            text: String::new(),
            attachments: (0..attachments)
                .map(|i| PendingAttachment {
                    rel_path: format!("attachments/a{i}.jpg"),
                    content_type: String::new(),
                    extension: "jpg".into(),
                    digest_sha256: None,
                    name_hint: None,
                })
                .collect(),
            extra: {
                let mut e = BTreeMap::new();
                e.insert("dedupe_key".into(), key.to_string());
                e.insert("source_kind".into(), "xml".to_string());
                e
            },
        }
    }

    #[test]
    fn dedupe_base_key_prefix() {
        assert_eq!(dedupe_base_key("1609459200|1|hello|"), "1609459200|1|hello");
        assert_eq!(
            dedupe_base_key("1609459200|1|hello|attachments/a1.jpg"),
            "1609459200|1|hello"
        );
        // Pipes inside the text must not split the base key.
        assert_eq!(
            dedupe_base_key("1609459200|1|he|llo|attachments/a1.jpg"),
            "1609459200|1|he|llo"
        );
    }

    #[test]
    fn xml_and_pdu_mms_rows_collapse_keeping_attachments() {
        // The same MMS appears in the XML backup (no attachments) and as a PDU
        // row with media. Exact-key dedupe would export it twice.
        let mut pdu_row = test_msg("1609459200|1|hello|attachments/a1.jpg", 1);
        pdu_row.extra.insert("source_kind".into(), "pdu".into());
        let mut msgs = vec![test_msg("1609459200|1|hello|", 0), pdu_row];
        dedupe_messages(&mut msgs);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].attachments.len(), 1);
        assert_eq!(msgs[0].extra_str("source_kind"), "pdu");
    }

    #[test]
    fn distinct_mms_with_same_prefix_both_kept() {
        // Two MMS sharing second, direction, and caption but with different
        // media are distinct messages: both rows survive.
        let mut msgs = vec![
            test_msg("1609459200|1|photo|attachments/a1.jpg", 1),
            test_msg("1609459200|1|photo|attachments/a2.jpg", 1),
        ];
        dedupe_messages(&mut msgs);
        assert_eq!(msgs.len(), 2);
    }

    #[test]
    fn plain_sms_duplicates_dropped() {
        let mut msgs = vec![
            test_msg("1609459200|1|hi|", 0),
            test_msg("1609459200|1|hi|", 0),
        ];
        dedupe_messages(&mut msgs);
        assert_eq!(msgs.len(), 1);
    }

    #[test]
    fn pdu_row_shadowed_by_xml_row_keeps_attachments() {
        // Defensive: XML pass runs first, but if a PDU row with media ever
        // precedes its XML twin, the attachment row still wins.
        let mut pdu_row = test_msg("1609459200|1|hello|attachments/a1.jpg", 1);
        pdu_row.extra.insert("source_kind".into(), "pdu".into());
        let mut msgs = vec![pdu_row, test_msg("1609459200|1|hello|", 0)];
        dedupe_messages(&mut msgs);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].attachments.len(), 1);
        assert_eq!(msgs[0].extra_str("source_kind"), "pdu");
    }
}
