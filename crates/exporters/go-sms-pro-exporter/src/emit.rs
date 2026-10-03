//! Convert a GO SMS Pro backup into the shared conversation structure
//! ([`ConversationDocument`]) every exporter writes, then write the chosen
//! output format via [`ExportWriter`].

use crate::attachments_emit::queue_pdu_attachments;
use crate::xml::{SkippedBadAddrDetail, XmlMessage, parse_xml_file};
use anyhow::{Context, Result, bail};
use go_sms_mms::{ParsedPdu, PduError, parse_pdu_file};
use message_crate_core::{
    CancelFlag, ExportReport, ExportTransforms, OutputFormat, prepare_outputs, project_conversation,
};
use message_ir::{
    ExportMeta, IrAttachment, IrParticipant, IrService, IrSource, PendingAttachment,
    PendingConversation, PendingMessage, ProjectionHooks, default_participants,
    ensure_conversation, parse_android_type,
};
use message_staging::{AttachmentSource, AttachmentSpool, ExportWriter};
use phone::{Handle, OwnerHandleSet};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const EXPORT_SOURCE: &str = "go-sms-pro";
const EXPORT_TOOL: &str = "GO SMS Pro";
/// This crate's version, recorded as the export tool version.
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

/// Diagnostic row for a PDU whose every address is the owner's.
#[derive(Debug, Clone)]
pub(crate) struct SkippedNoPartyDetail {
    pub pdu_filename: String,
    pub sender: String,
    pub recipients: String,
    pub is_sent: bool,
}

/// Append parsed XML SMS rows to pending conversations.
fn add_xml_messages(
    conversations: &mut BTreeMap<String, PendingConversation>,
    msgs: Vec<XmlMessage>,
) {
    for msg in msgs {
        let chat_id = msg.other.key();
        let convo = ensure_conversation(conversations, chat_id, false, None, Vec::new());
        convo.messages.push(PendingMessage {
            sort_key: msg.timestamp_secs as i64,
            is_from_me: msg.is_from_me,
            sender_handle: if msg.is_from_me {
                String::new()
            } else {
                msg.other.into_key()
            },
            sender_display_name: msg.name_alias.clone(),
            text: msg.text,
            attachments: Vec::new(),
            extra: {
                let mut e = BTreeMap::new();
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

/// The addresses on a PDU, each classified once from the value as written.
struct PduAddresses {
    /// The sender of a received PDU.
    sender: Option<Handle>,
    /// Every address on the PDU, the sender first, once each by key.
    participants: Vec<Handle>,
}

impl PduAddresses {
    /// A phone writes the same person as `4075550107` on one MMS and
    /// `+14075550107` on the next; both have one key, so the group they
    /// share has one chat id.
    fn of(parsed: &ParsedPdu) -> Self {
        let sender = parsed.sender.as_deref().and_then(Handle::parse);
        let mut participants: Vec<Handle> = sender.iter().cloned().collect();
        for r in parsed.recipients.iter().filter_map(|r| Handle::parse(r)) {
            if !participants.iter().any(|p| p.key() == r.key()) {
                participants.push(r);
            }
        }
        Self {
            sender,
            participants,
        }
    }
}

/// The chat a PDU message lands in.
struct PduTarget {
    chat_id: String,
    is_group: bool,
    group_title: Option<String>,
    /// The handle keys of a group's non-owner peers; empty for an
    /// individual chat.
    peers: Vec<String>,
}

/// Append one parsed PDU (binary SMS/MMS) to the conversation it belongs to.
fn add_pdu_message(
    conversations: &mut BTreeMap<String, PendingConversation>,
    parsed: ParsedPdu,
    addresses: PduAddresses,
    attachments: Vec<PendingAttachment>,
    owners: &OwnerHandleSet,
    report: &mut ExportReport,
    skips: &mut SkipDetails,
) {
    let Some(target) = pdu_target(&parsed, &addresses, owners, report, skips) else {
        return;
    };
    report.bump("pdu_messages", 1);
    if target.is_group {
        report.bump("pdu_group_messages", 1);
    }
    let pending = pdu_pending_message(parsed, addresses.sender, attachments);
    let convo = ensure_conversation(
        conversations,
        &target.chat_id,
        target.is_group,
        target.group_title,
        target.peers,
    );
    convo.messages.push(pending);
}

/// The chat the PDU belongs to, from the addresses on it that are not the
/// owner's: a group when there are two or more of them, else the one
/// other party. `None`, counted and detailed as a skip, when nobody but
/// the owner is on it.
fn pdu_target(
    parsed: &ParsedPdu,
    addresses: &PduAddresses,
    owners: &OwnerHandleSet,
    report: &mut ExportReport,
    skips: &mut SkipDetails,
) -> Option<PduTarget> {
    let others: Vec<String> = addresses
        .participants
        .iter()
        .filter(|p| !owners.is_owner(p))
        .map(|p| p.key().to_string())
        .collect();
    if others.is_empty() {
        report.bump("skipped_no_other_party", 1);
        push_skip_detail(
            &mut skips.no_party,
            &mut skips.no_party_more,
            SkippedNoPartyDetail {
                pdu_filename: pdu_basename(parsed),
                sender: parsed.sender.clone().unwrap_or_default(),
                recipients: parsed.recipients.join(";"),
                is_sent: parsed.is_sent,
            },
        );
        return None;
    }
    if others.len() >= 2 {
        let (chat_id, title) = phone::group_chat_id("chat-group-", &others);
        Some(PduTarget {
            chat_id,
            is_group: true,
            group_title: Some(title),
            peers: others,
        })
    } else {
        Some(PduTarget {
            chat_id: others[0].clone(),
            is_group: false,
            group_title: None,
            peers: Vec::new(),
        })
    }
}

/// The pending message for a PDU. Its `extra` map carries the PDU
/// diagnostics the projection reads back into the IR source fields.
fn pdu_pending_message(
    parsed: ParsedPdu,
    sender: Option<Handle>,
    attachments: Vec<PendingAttachment>,
) -> PendingMessage {
    // The projection names the owner as the sender of every outgoing message
    // itself, so only a received PDU carries its sender here.
    let sender_handle = match sender {
        Some(sender) if !parsed.is_sent => sender.into_key(),
        _ => String::new(),
    };
    let mut extra = BTreeMap::new();
    extra.insert("source_kind".into(), "pdu".to_string());
    extra.insert("android_type".into(), String::new());
    // No `date_ms`: a PDU file records whole seconds only.
    extra.insert("contact_name".into(), String::new());
    extra.insert("pdu_filename".into(), pdu_basename(&parsed));
    if !parsed.fields.is_empty() {
        extra.insert(
            "pdu_fields".into(),
            serde_json::to_string(&parsed.fields).unwrap_or_default(),
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

/// True when the path has a `.xml` extension (any case).
fn is_xml_file(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("xml"))
}

/// True for GO SMS Pro MMS files: `I_*.pdu` for a received message and
/// `S_*.pdu` for a sent one.
fn is_pdu_file(p: &Path) -> bool {
    p.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| (n.starts_with("I_") || n.starts_with("S_")) && n.ends_with(".pdu"))
}

/// GO SMS Pro deltas of the shared [`message_ir::pending_to_document`] projection.
struct GoSmsProjection {
    export: ExportMeta,
}

impl ProjectionHooks for GoSmsProjection {
    fn export(&self) -> ExportMeta {
        self.export.clone()
    }

    fn service(&self, _msg: &PendingMessage) -> IrService {
        IrService::Sms
    }

    /// Every handle is a [`Handle`] key already.
    fn normalize_handle(&self, raw: &str) -> String {
        raw.to_string()
    }

    /// The default roster, with each identity's kind read from its key: a
    /// GO SMS Pro address can be an email address or a sender name.
    fn participants(&self, chat_id: &str, convo: &PendingConversation) -> Vec<IrParticipant> {
        let mut participants = default_participants(chat_id, convo, &str::to_string);
        for p in &mut participants {
            if let Some(handle) = p.handle.as_deref().and_then(Handle::parse) {
                p.handle_type = Some(handle.kind());
            }
        }
        participants
    }

    fn attachment_to_ir(&self, att: &PendingAttachment, _msg: &PendingMessage) -> IrAttachment {
        att.to_ir()
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

    // Clean the previous run's output, or keep it when `resume` is set.
    let writer = ExportWriter::open(&output_dir, output_format, transforms, resume)?;
    let mut ingest = Ingest {
        owners: &owners,
        spool: writer.copies_attachments().then(|| writer.spool()),
        conversations: BTreeMap::new(),
        report: ExportReport::default(),
        skips: SkipDetails::default(),
    };
    for xml_path in sorted_files(input_dir, &is_xml_file)? {
        message_crate_core::check_cancel(cancel)?;
        ingest.ingest_xml(&xml_path);
    }
    for pdu_path in sorted_files(input_dir, &is_pdu_file)? {
        message_crate_core::check_cancel(cancel)?;
        ingest.ingest_pdu(&pdu_path)?;
    }
    message_crate_core::check_cancel(cancel)?;
    let Ingest {
        conversations,
        mut report,
        skips,
        ..
    } = ingest;

    let hooks = GoSmsProjection {
        export: message_crate_core::export_meta(
            EXPORT_SOURCE,
            EXPORT_TOOL,
            EXPORT_TOOL_VERSION,
            Some(owner_handle),
            None,
        ),
    };
    let mut documents = Vec::new();
    for (chat_id, mut convo) in conversations {
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
    let mut paths = message_crate_core::discover_files(dir, predicate)?;
    paths.sort();
    Ok(paths)
}

/// Parse-time state shared across every XML and PDU file in one backup.
struct Ingest<'a> {
    owners: &'a OwnerHandleSet,
    /// Where attachment payloads are written as they are parsed; `None`
    /// when the run does not copy attachments.
    spool: Option<&'a AttachmentSpool>,
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
        self.report
            .bump("skipped_unreadable_text", stats.skipped_unreadable_text);
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

    /// Add the MMS in one PDU file. A stub (the placeholder GO SMS Pro
    /// writes for an MMS it never downloaded) is counted and listed; a file
    /// that breaks the MMS rules is counted, and the first twenty are named
    /// in the report.
    ///
    /// # Errors
    ///
    /// Returns an error when an attachment cannot be written to the spool.
    fn ingest_pdu(&mut self, pdu_path: &Path) -> Result<()> {
        let parsed = match parse_pdu_file(pdu_path) {
            Ok(parsed) => parsed,
            Err(PduError::Stub) => {
                self.report.bump("skipped_empty_pdu", 1);
                push_skip_detail(
                    &mut self.skips.empty_pdu,
                    &mut self.skips.empty_pdu_more,
                    SkippedEmptyPduDetail {
                        pdu_filename: pdu_path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("")
                            .to_string(),
                    },
                );
                return Ok(());
            }
            Err(err) => {
                self.report.bump("skipped_unparseable_pdu", 1);
                if self.report.errors.len() < 20 {
                    self.report
                        .errors
                        .push(format!("{}: {err}", pdu_path.display()));
                }
                return Ok(());
            }
        };
        let addresses = PduAddresses::of(&parsed);
        let atts = queue_pdu_attachments(&parsed, self.spool)?;
        add_pdu_message(
            &mut self.conversations,
            parsed,
            addresses,
            atts,
            self.owners,
            &mut self.report,
            &mut self.skips,
        );
        Ok(())
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
    wtr.write_record(["pdu_filename", "sender", "recipients", "is_sent"])?;
    for d in details {
        wtr.write_record([
            d.pdu_filename.as_str(),
            d.sender.as_str(),
            d.recipients.as_str(),
            if d.is_sent { "1" } else { "0" },
        ])?;
    }
    if more > 0 {
        wtr.write_record(["", "", "", &format!("...and {more} more entries not shown")])?;
    }
    wtr.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A group MMS lists the owner's own number among its addresses. The
    /// group is the other people, so the owner's number changes neither the
    /// chat id nor the title.
    #[test]
    fn the_owners_number_is_not_part_of_a_group_chat_id() {
        let target = |recipients: &[&str]| {
            let owners = OwnerHandleSet::from_phones(&["+15555550100".into()]).unwrap();
            let parsed = ParsedPdu {
                path: std::path::PathBuf::from("I_1609459200_x.pdu"),
                timestamp: 1_609_459_200,
                is_sent: false,
                sender: Some("15555550122".into()),
                recipients: recipients.iter().map(|r| r.to_string()).collect(),
                body: "hi".into(),
                attachments: Vec::new(),
                fields: BTreeMap::new(),
            };
            let addresses = PduAddresses::of(&parsed);
            let target = pdu_target(
                &parsed,
                &addresses,
                &owners,
                &mut ExportReport::default(),
                &mut SkipDetails::default(),
            )
            .unwrap();
            (target.chat_id, target.group_title)
        };
        assert_eq!(
            target(&["15555550100", "15555550133"]),
            target(&["15555550133"])
        );
    }

    #[test]
    fn a_sent_pdu_to_two_people_is_a_group() {
        let owners = OwnerHandleSet::from_phones(&["+15555550100".into()]).unwrap();
        let parsed = ParsedPdu {
            path: std::path::PathBuf::from("S_1609459200_x.pdu"),
            timestamp: 1_609_459_200,
            is_sent: true,
            sender: None,
            recipients: vec!["15555550122".into(), "15555550133".into()],
            body: "hi".into(),
            attachments: Vec::new(),
            fields: BTreeMap::new(),
        };
        let mut conversations = BTreeMap::new();
        let mut report = ExportReport::default();
        let mut skips = SkipDetails::default();
        let addresses = PduAddresses::of(&parsed);
        add_pdu_message(
            &mut conversations,
            parsed,
            addresses,
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

    /// The chat ids a received PDU from `sender` to the owner lands in.
    fn chat_ids_for_received_pdu(sender: &str) -> Vec<String> {
        let owners = OwnerHandleSet::from_phones(&["+15555550100".into()]).unwrap();
        let parsed = ParsedPdu {
            path: std::path::PathBuf::from("I_1609459200_x.pdu"),
            timestamp: 1_609_459_200,
            is_sent: false,
            sender: Some(sender.into()),
            recipients: vec!["+15555550100".into()],
            body: "hi".into(),
            attachments: Vec::new(),
            fields: BTreeMap::new(),
        };
        let mut conversations = BTreeMap::new();
        let addresses = PduAddresses::of(&parsed);
        add_pdu_message(
            &mut conversations,
            parsed,
            addresses,
            Vec::new(),
            &owners,
            &mut ExportReport::default(),
            &mut SkipDetails::default(),
        );
        conversations.into_keys().collect()
    }

    #[test]
    fn a_pdu_number_with_its_country_keeps_it() {
        assert_eq!(chat_ids_for_received_pdu("+6595550100"), ["+6595550100"]);
    }

    #[test]
    fn a_pdu_from_an_email_address_is_not_a_phone_number() {
        assert_eq!(
            chat_ids_for_received_pdu("ann2020@example.com"),
            ["ann2020@example.com"]
        );
    }

    /// The messages of one conversation as the shared projection writes them.
    fn project(messages: Vec<PendingMessage>) -> (Vec<message_ir::IrMessage>, ExportReport) {
        let hooks = GoSmsProjection {
            export: message_crate_core::export_meta(
                EXPORT_SOURCE,
                EXPORT_TOOL,
                EXPORT_TOOL_VERSION,
                Some("+15555550100".into()),
                None,
            ),
        };
        let mut convo = PendingConversation::new("+15555550122", false, None, Vec::new());
        convo.messages = messages;
        let mut report = ExportReport::default();
        let doc = project_conversation("+15555550122", &mut convo, &hooks, &mut report).unwrap();
        (doc.messages, report)
    }

    /// An XML row (milliseconds, no attachments) or a PDU row (whole
    /// seconds) with `digests` as its attachments.
    fn test_msg(source_kind: &str, text: &str, digests: &[&str]) -> PendingMessage {
        let mut extra = BTreeMap::new();
        extra.insert("source_kind".into(), source_kind.to_string());
        if source_kind == "xml" {
            extra.insert("date_ms".into(), "1609459200250".to_string());
        }
        PendingMessage {
            sort_key: 1_609_459_200,
            is_from_me: true,
            sender_handle: String::new(),
            sender_display_name: None,
            text: text.into(),
            attachments: digests
                .iter()
                .map(|d| PendingAttachment {
                    rel_path: String::new(),
                    content_type: "image/jpeg".into(),
                    digest_sha256: Some(d.to_string()),
                    name_hint: None,
                })
                .collect(),
            extra,
        }
    }

    #[test]
    fn xml_and_pdu_mms_rows_collapse_keeping_attachments_and_milliseconds() {
        // The same MMS appears in the XML backup (no attachments) and as a PDU
        // file with media, in either order.
        let xml = || test_msg("xml", "hello", &[]);
        let pdu = || test_msg("pdu", "hello", &["a1"]);
        for rows in [vec![xml(), pdu()], vec![pdu(), xml()]] {
            let (msgs, report) = project(rows);
            assert_eq!(msgs.len(), 1);
            assert_eq!(report.duplicates_dropped, 1);
            assert_eq!(msgs[0].attachments.len(), 1);
            assert_eq!(
                msgs[0].source.as_ref().unwrap().fields["source_kind"],
                "pdu"
            );
            assert_eq!(msgs[0].timestamp_unix_ms, 1_609_459_200_250);
        }
    }

    #[test]
    fn distinct_mms_with_one_caption_in_one_second_are_both_kept() {
        // Two MMS sharing second, direction, and caption but with different
        // media are distinct messages: both rows survive.
        let (msgs, _) = project(vec![
            test_msg("pdu", "photo", &["a1"]),
            test_msg("pdu", "photo", &["a2"]),
        ]);
        assert_eq!(msgs.len(), 2);
        assert_ne!(msgs[0].guid, msgs[1].guid);
    }

    #[test]
    fn plain_sms_duplicates_dropped() {
        let (msgs, report) = project(vec![test_msg("xml", "hi", &[]), test_msg("xml", "hi", &[])]);
        assert_eq!(msgs.len(), 1);
        assert_eq!(report.duplicates_dropped, 1);
    }

    #[test]
    fn verify_e1_5_two_group_senders_in_one_second_are_both_kept() {
        let pdu = |sender: &str| ParsedPdu {
            path: std::path::PathBuf::from(format!("I_1609459200_{sender}.pdu")),
            timestamp: 1_609_459_200,
            is_sent: false,
            sender: Some(sender.into()),
            recipients: vec!["15555550122".into(), "15555550133".into()],
            body: "ok".into(),
            attachments: Vec::new(),
            fields: BTreeMap::new(),
        };
        let pending = |sender: &str| {
            let parsed = pdu(sender);
            let addresses = PduAddresses::of(&parsed);
            pdu_pending_message(parsed, addresses.sender, Vec::new())
        };
        let (msgs, _) = project(vec![pending("15555550122"), pending("15555550133")]);
        assert_eq!(
            msgs.len(),
            2,
            "Lee's message was dropped as a duplicate of Ana's"
        );
        assert_ne!(msgs[0].guid, msgs[1].guid);
    }
}
