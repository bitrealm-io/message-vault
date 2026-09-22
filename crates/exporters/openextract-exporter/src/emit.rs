//! Convert OpenExtract rows into the shared conversation structure, then write
//! the chosen output format via [`FormatSink`].

use crate::parse::{RawRow, SourceKind, discover_csv_files, parse_csv_file};
use anyhow::Result;
use chrono::DateTime;
use message_ir::{
    ExportMeta, HandleType, IrParticipant, IrService, IrSource, PendingConversation,
    PendingMessage, ProjectionHooks, ensure_conversation,
};
use message_ir_format::{AttachmentSource, ExportWriter};
use message_vault_io_core::{
    CancelFlag, ExportReport, ExportTransforms, OutputFormat, prepare_outputs, project_conversation,
};
use phone::sanitize_number;
use serde_json::{Map, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

const EXPORT_SOURCE: &str = "openextract";
const EXPORT_TOOL: &str = "OpenExtract";
const EXPORT_TOOL_VERSION: &str = "0.5.1";

/// Inputs for [`convert_export`].
pub(crate) struct ConvertExportArgs<'a> {
    pub input: &'a Path,
    pub output: &'a Path,
    pub transforms: ExportTransforms,
    pub output_format: OutputFormat,
    pub cancel: Option<&'a CancelFlag>,
    /// Continue an interrupted export: keep previous output and skip the
    /// conversations already written.
    pub resume: bool,
}

/// Convert OpenExtract CSV(s) under `input`.
///
/// When `cancel` is set, cooperative cancellation is checked between CSV files
/// and before writing. Cancelled runs return an error with message `cancelled`.
///
/// # Errors
///
/// Returns an error when output overlaps input, a CSV cannot be parsed, or the
/// user cancels.
pub(crate) fn convert_export(args: ConvertExportArgs<'_>) -> Result<ExportReport> {
    let ConvertExportArgs {
        input,
        output,
        transforms,
        output_format,
        cancel,
        resume,
    } = args;
    let (inputs, output) = prepare_outputs(&[input.to_path_buf()], output)?;
    let input = &inputs[0];
    let writer = ExportWriter::open(&output, output_format, transforms, resume)?;

    let mut ingest = Ingest::default();
    for path in discover_csv_files(input)? {
        message_vault_io_core::check_cancel(cancel)?;
        ingest.ingest_file(&path);
    }
    message_vault_io_core::check_cancel(cancel)?;
    let Ingest {
        conversations,
        mut report,
        ..
    } = ingest;

    let hooks = OpenExtractProjection {
        export: message_vault_io_core::export_meta(
            EXPORT_SOURCE,
            EXPORT_TOOL,
            EXPORT_TOOL_VERSION,
            None,
            None,
        ),
    };
    let mut documents = Vec::new();
    for (chat_id, mut convo) in conversations {
        if let Some(doc) = project_conversation(&chat_id, &mut convo, &hooks, &mut report) {
            documents.push(doc);
        }
    }

    // OpenExtract carries no attachments; every attachment source is Missing.
    writer.finish(
        documents,
        &mut |att| (AttachmentSource::Missing, att.size_bytes),
        cancel,
        &mut report,
    )?;

    Ok(report)
}

/// Parse-time state shared across every CSV file in one export.
#[derive(Default)]
struct Ingest {
    conversations: BTreeMap<String, PendingConversation>,
    /// Dedupe duplicate CSV rows: same chat + second + direction + text.
    seen_keys: HashSet<String>,
    report: ExportReport,
}

impl Ingest {
    /// Parse one CSV and add its rows. A file that fails to parse is recorded
    /// in the report and skipped so one bad export does not stop the rest.
    fn ingest_file(&mut self, path: &Path) {
        let rows = match parse_csv_file(path) {
            Ok(rows) => rows,
            Err(e) => {
                self.report
                    .errors
                    .push(format!("{}: {e:#}", path.display()));
                return;
            }
        };
        let Some(first) = rows.first() else {
            return;
        };
        // For per-chat files, infer the peer once from all rows in that file.
        let per_chat_peer =
            (first.source_kind == SourceKind::PerChat).then(|| infer_peer_label(&rows));
        let phone_by_label = phones_by_peer_label(&rows, per_chat_peer.as_deref());
        for row in rows {
            self.ingest_row(row, per_chat_peer.as_deref(), &phone_by_label);
        }
    }

    /// Add one row to its conversation, or count why it was dropped.
    fn ingest_row(
        &mut self,
        row: RawRow,
        per_chat_peer: Option<&str>,
        phone_by_label: &HashMap<String, String>,
    ) {
        let peer_label = peer_label_for(&row, per_chat_peer);
        let (chat_id, contact_name, name_only) = match resolve_chat(&peer_label) {
            (key, name, true) => match phone_by_label.get(&peer_label) {
                Some(phone) => (phone::normalize_lenient(phone), name, false),
                None => (key, name, true),
            },
            resolved => resolved,
        };
        let Some((secs, date_ms)) = parse_timestamp(&row.date) else {
            self.report.skipped_invalid_date += 1;
            return;
        };
        let is_from_me = resolve_is_from_me(&row);
        let (sender_handle, sender_display_name) =
            resolve_sender(&row, is_from_me, &chat_id, &contact_name);

        let dedupe_key = format!(
            "{}|{}|{}|{}",
            chat_id,
            secs,
            if is_from_me { "1" } else { "0" },
            row.text
        );
        if !self.seen_keys.insert(dedupe_key) {
            self.report.duplicates_dropped += 1;
            return;
        }

        let convo = ensure_conversation(&mut self.conversations, &chat_id, false, None, Vec::new());
        if name_only
            && convo
                .extra
                .insert(message_ir::CHAT_ID_IS_NAME.to_string(), "1".to_string())
                .is_none()
        {
            // Counted once per conversation, not once per row.
            self.report.bump("name_only_chat", 1);
        }
        let mut extra = BTreeMap::new();
        extra.insert("contact_name".into(), contact_name);
        extra.insert("date_ms".into(), date_ms);
        extra.insert(
            "has_attachments".into(),
            if row.has_attachments { "true" } else { "false" }.into(),
        );
        extra.insert("source_kind".into(), row.source_kind.as_str().to_string());
        convo.messages.push(PendingMessage {
            sort_key: secs,
            is_from_me,
            sender_handle,
            sender_display_name: (!sender_display_name.is_empty()).then_some(sender_display_name),
            text: row.text,
            attachments: Vec::new(),
            extra,
        });
    }
}

/// The phone number each peer label was seen with, when the source recorded one.
///
/// A chat can be labelled with a person's name while its rows still carry
/// that person's number. Prefer the address the source actually recorded;
/// only fall back to a name-only participant when the source recorded no
/// address anywhere in the chat.
fn phones_by_peer_label(rows: &[RawRow], per_chat_peer: Option<&str>) -> HashMap<String, String> {
    let mut phone_by_label: HashMap<String, String> = HashMap::new();
    for row in rows {
        if row.is_from_me || is_me(&row.sender) || sanitize_number(&row.sender).is_none() {
            continue;
        }
        phone_by_label
            .entry(peer_label_for(row, per_chat_peer))
            .or_insert_with(|| row.sender.clone());
    }
    phone_by_label
}

/// The other party's label for one row: the conversation column when it names
/// someone, else the incoming sender, else the per-file peer.
fn peer_label_for(row: &RawRow, per_chat_peer: Option<&str>) -> String {
    row.conversation
        .as_deref()
        .filter(|s| !s.is_empty() && !is_me(s))
        .map(|s| s.to_string())
        .or_else(|| {
            if !row.is_from_me && !is_me(&row.sender) {
                Some(row.sender.clone())
            } else {
                per_chat_peer.map(str::to_string)
            }
        })
        .unwrap_or_else(|| "unknown".to_string())
}

/// True for the literal `Me` OpenExtract writes for the owner.
fn is_me(s: &str) -> bool {
    s.trim().eq_ignore_ascii_case("me")
}

/// The other party for a per-chat file: the first phone number seen, else the first name.
fn infer_peer_label(rows: &[RawRow]) -> String {
    let mut phone_peer = None;
    let mut name_peer = None;
    for row in rows {
        if row.is_from_me || is_me(&row.sender) {
            continue;
        }
        if sanitize_number(&row.sender).is_some() {
            phone_peer.get_or_insert_with(|| row.sender.clone());
        } else if name_peer.is_none() {
            name_peer = Some(row.sender.clone());
        }
    }
    phone_peer
        .or(name_peer)
        .unwrap_or_else(|| "unknown".to_string())
}

/// Returns `(chat_key, contact_name, name_only)`.
///
/// OpenExtract CSVs identify the other party by phone number or by name. When
/// it is a name, the chat is keyed by a stem of that name and `name_only` is
/// set: the exporter records the name and no address, and the vault resolves
/// it against contacts on import. No address is invented here.
fn resolve_chat(peer: &str) -> (String, String, bool) {
    let peer = peer.trim();
    if peer.is_empty() || peer.eq_ignore_ascii_case("unknown") {
        return ("unknown".to_string(), String::new(), false);
    }
    if sanitize_number(peer).is_some() {
        // Format as E.164 when unambiguous. Otherwise keep digits as-is. Never invent `+0…`.
        return (phone::normalize_lenient(peer), String::new(), false);
    }
    (
        message_vault_io_core::name_stem(peer),
        peer.to_string(),
        true,
    )
}

/// Whether the row is outgoing, from its direction column or its sender.
fn resolve_is_from_me(row: &RawRow) -> bool {
    if let Some(dir) = row.direction.as_deref() {
        let d = dir.trim().to_ascii_lowercase();
        if d == "sent" || d == "outgoing" {
            return true;
        }
        if d == "received" || d == "incoming" {
            return false;
        }
    }
    row.is_from_me
}

/// The sender handle and display name for a row: empty for outgoing, else the peer.
fn resolve_sender(
    row: &RawRow,
    is_from_me: bool,
    chat_id: &str,
    contact_name: &str,
) -> (String, String) {
    if is_from_me {
        return (String::new(), String::new());
    }
    // Prefer phone on chat_id when it looks like E.164.
    let handle = if chat_id.starts_with('+') || sanitize_number(chat_id).is_some() {
        if chat_id.starts_with('+') {
            // Only unambiguous +-prefixed values pass through. A fabricated
            // `+0…` stays digits-as-is so the vault can flag it.
            phone::normalize_lenient(chat_id)
        } else {
            phone::normalize_digits_us(chat_id).unwrap_or_default()
        }
    } else {
        phone::normalize_digits_us(&row.sender).unwrap_or_default()
    };

    let display = if !contact_name.is_empty() {
        contact_name.to_string()
    } else if sanitize_number(&row.sender).is_some() {
        String::new()
    } else if !is_me(&row.sender) {
        row.sender.clone()
    } else {
        String::new()
    };

    (handle, display)
}

/// Unix seconds and the original string for a row's date, in RFC 3339 or OpenExtract's local formats.
fn parse_timestamp(raw: &str) -> Option<(i64, String)> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    // RFC3339 / ISO-8601 with offset (OpenExtract style).
    if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        let secs = dt.timestamp();
        return Some((secs, (secs * 1000).to_string()));
    }
    // Fallback without fractional seconds.
    if let Ok(dt) = DateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S%z") {
        let secs = dt.timestamp();
        return Some((secs, (secs * 1000).to_string()));
    }
    None
}

/// OpenExtract deltas of the shared [`message_ir::pending_to_document`] projection.
struct OpenExtractProjection {
    export: ExportMeta,
}

impl ProjectionHooks for OpenExtractProjection {
    fn export(&self) -> ExportMeta {
        self.export.clone()
    }

    fn service(&self, _msg: &PendingMessage) -> IrService {
        IrService::Sms
    }

    fn source(&self, _convo: &PendingConversation, msg: &PendingMessage) -> IrSource {
        let mut fields = Map::new();
        fields.insert("source_kind".into(), json!(msg.extra_str("source_kind")));
        fields.insert(
            "has_attachments".into(),
            json!(msg.extra_flag("has_attachments")),
        );
        IrSource {
            android_type: None,
            fields,
        }
    }

    /// The roster is the single peer named by the chat id; an unresolved
    /// `unknown` chat has no roster at all.
    fn participants(&self, chat_id: &str, convo: &PendingConversation) -> Vec<IrParticipant> {
        if chat_id.is_empty() || chat_id.eq_ignore_ascii_case("unknown") {
            return Vec::new();
        }
        if convo.extra.contains_key(message_ir::CHAT_ID_IS_NAME) {
            // The source named this person and recorded no address for them.
            return vec![IrParticipant {
                handle: None,
                display_name: convo.first_contact_name(),
                handle_type: None,
            }];
        }
        vec![IrParticipant {
            handle: Some(chat_id.to_string()),
            display_name: convo.first_contact_name(),
            handle_type: Some(HandleType::Phone),
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;
    use std::path::PathBuf;

    fn write(dir: &tempfile::TempDir, name: &str, body: &str) -> PathBuf {
        let path = dir.path().join(name);
        let mut f = File::create(&path).unwrap();
        write!(f, "{body}").unwrap();
        path
    }

    fn convert(input: &std::path::Path, output: &std::path::Path) -> Result<ExportReport> {
        convert_export(ConvertExportArgs {
            input,
            output,
            transforms: ExportTransforms::none(),
            output_format: OutputFormat::Csv,
            cancel: None,
            resume: false,
        })
    }

    #[test]
    fn phone_peer_keeps_its_number_as_the_identity() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir,
            "conversation_1.csv",
            "Date,Sender,Text,Is From Me,Has Attachments\n\
2020-01-01T12:00:00+00:00,+15555550122,Hello,False,False\n\
2020-01-01T12:01:00+00:00,me,Hi,True,False\n",
        );
        let out = dir.path().join("out");
        let report = convert(dir.path(), &out).unwrap();
        assert_eq!(report.conversations, 1);
        assert_eq!(report.extra("name_only_chat"), 0);
        let body = fs::read_to_string(out.join("+15555550122.csv")).unwrap();
        assert!(body.contains("openextract"));
    }

    #[test]
    fn name_peer_becomes_a_participant_with_no_identity() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir,
            "conversation_2.csv",
            "Date,Sender,Text,Is From Me,Has Attachments\n\
2020-01-01T12:00:00+00:00,Cathy Arp,Hi,False,False\n\
2020-01-01T12:01:00+00:00,me,Hello,True,False\n",
        );
        let out = dir.path().join("out");
        let report = convert(dir.path(), &out).unwrap();
        assert_eq!(report.extra("name_only_chat"), 1);
        assert_eq!(report.conversations, 1);
        let csv_path = out.join("Cathy_Arp.csv");
        assert!(csv_path.is_file(), "missing {}", csv_path.display());
        let body = fs::read_to_string(&csv_path).unwrap();
        assert!(
            body.contains("Cathy Arp"),
            "the name the source gave must survive: {body}"
        );
    }

    #[test]
    fn duplicate_rows_are_dropped() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir,
            "conversation_1.csv",
            "Date,Sender,Text,Is From Me,Has Attachments\n\
2020-01-01T12:00:00+00:00,+15555550122,Hello,False,False\n\
2020-01-01T12:00:00+00:00,+15555550122,Hello,False,False\n\
2020-01-01T12:01:00+00:00,me,Hi,True,False\n",
        );
        let out = dir.path().join("out");
        let report = convert(dir.path(), &out).unwrap();
        assert_eq!(report.duplicates_dropped, 1);
        assert_eq!(report.messages, 2);
        assert_eq!(report.conversations, 1);
    }
}
