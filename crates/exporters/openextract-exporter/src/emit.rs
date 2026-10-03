//! Convert OpenExtract rows into the shared conversation structure, then write
//! the chosen output format via [`ExportWriter`].

use crate::parse::{RawRow, SourceKind, discover_csv_files, parse_csv_file};
use anyhow::Result;
use chrono::DateTime;
use message_crate_core::{
    CancelFlag, ExportReport, ExportTransforms, OutputFormat, prepare_outputs, project_conversation,
};
use message_ir::{
    ConversationKey, ExportMeta, HandleType, IrParticipant, IrService, IrSource,
    PendingConversation, PendingMessage, ProjectionHooks, ensure_conversation,
};
use message_staging::{AttachmentSource, ExportWriter};
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
        message_crate_core::check_cancel(cancel)?;
        ingest.ingest_file(input, &path);
    }
    message_crate_core::check_cancel(cancel)?;
    let Ingest {
        conversations,
        keys,
        mut report,
    } = ingest;

    let hooks = OpenExtractProjection {
        export: message_crate_core::export_meta(
            EXPORT_SOURCE,
            EXPORT_TOOL,
            EXPORT_TOOL_VERSION,
            None,
            None,
        ),
        keys,
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
    /// Each conversation's key, by chat id. The `unknown` conversation, of
    /// sent rows that name nobody, has none.
    keys: HashMap<String, ConversationKey>,
    report: ExportReport,
}

/// The conversation a set of rows belongs to.
struct Conversation {
    /// `None` for the `unknown` conversation.
    key: Option<ConversationKey>,
    /// The other person's name, for a one-to-one conversation the source
    /// names; empty otherwise.
    contact_name: String,
}

impl Conversation {
    fn chat_id(&self) -> String {
        self.key
            .as_ref()
            .map_or_else(|| "unknown".to_string(), ConversationKey::chat_id)
    }
}

impl Ingest {
    /// Parse one CSV and add its rows. A file that fails to parse is recorded
    /// in the report and skipped so one bad export does not stop the rest.
    ///
    /// A per-chat file is one conversation, whoever sent each row. In the
    /// all-conversations CSV a row's conversation is its `Conversation`
    /// value; a row with none belongs to its incoming sender.
    fn ingest_file(&mut self, input: &Path, path: &Path) {
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
        if first.source_kind == SourceKind::PerChat {
            let all: Vec<&RawRow> = rows.iter().collect();
            let conversation = conversation_of(&all, None, || file_vendor_id(input, path));
            for row in rows {
                self.ingest_row(row, &conversation);
            }
            return;
        }
        let mut by_label: BTreeMap<&str, Vec<&RawRow>> = BTreeMap::new();
        for row in &rows {
            if let Some(label) = conversation_label(row) {
                by_label.entry(label).or_default().push(row);
            }
        }
        let labelled: HashMap<String, Conversation> = by_label
            .into_iter()
            .map(|(label, rows)| {
                let conversation = conversation_of(&rows, Some(label), || label.to_string());
                (label.to_string(), conversation)
            })
            .collect();
        for row in rows {
            match conversation_label(&row).and_then(|label| labelled.get(label)) {
                Some(conversation) => self.ingest_row(row, conversation),
                None => {
                    let sender = (!resolve_is_from_me(&row)).then_some(row.sender.as_str());
                    let conversation = one_to_one(sender, None);
                    self.ingest_row(row, &conversation);
                }
            }
        }
    }

    /// Add one row to its conversation, or count why it was dropped.
    fn ingest_row(&mut self, row: RawRow, conversation: &Conversation) {
        let Some(secs) = parse_timestamp(&row.date) else {
            self.report.skipped_invalid_date += 1;
            return;
        };
        let chat_id = conversation.chat_id();
        let is_from_me = resolve_is_from_me(&row);
        let (sender_handle, sender_display_name) = resolve_sender(&row, is_from_me, conversation);

        if let Some(key) = &conversation.key
            && !self.keys.contains_key(&chat_id)
        {
            if key.is_name_only() {
                // Counted once per conversation, not once per row.
                self.report.bump("name_only_chat", 1);
            }
            self.keys.insert(chat_id.clone(), key.clone());
        }
        let is_group = conversation
            .key
            .as_ref()
            .is_some_and(ConversationKey::is_group);
        let convo = ensure_conversation(
            &mut self.conversations,
            &chat_id,
            is_group,
            None,
            Vec::new(),
        );
        let mut extra = BTreeMap::new();
        extra.insert("contact_name".into(), conversation.contact_name.clone());
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

/// The `Conversation` value of a row of the all-conversations CSV, when it
/// names someone other than the owner.
fn conversation_label(row: &RawRow) -> Option<&str> {
    row.conversation
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty() && !is_me(s))
}

/// The conversation of `rows`, which the source says are one conversation:
/// a per-chat file (`label` is `None`), or every row of the all-conversations
/// CSV with one `Conversation` value.
///
/// Two or more people other than the owner wrote in a group, which is keyed
/// by `group_id` and never by who wrote. Where one person did, the
/// conversation is one-to-one with them. Where nobody did, it is one-to-one
/// with the person the label names; a per-chat file has no label, and its
/// rows are a group of nobody known, keyed by the file, so that two such
/// files never share a conversation.
fn conversation_of(
    rows: &[&RawRow],
    label: Option<&str>,
    group_id: impl FnOnce() -> String,
) -> Conversation {
    let others = other_parties(rows);
    match (others.as_slice(), label) {
        ([], None) => Conversation {
            key: Some(ConversationKey::Group {
                vendor_id: group_id(),
                members: Vec::new(),
            }),
            contact_name: String::new(),
        },
        ([], Some(_)) => one_to_one(None, label),
        ([one], _) => one_to_one(Some(one), label),
        _ => Conversation {
            key: Some(ConversationKey::Group {
                vendor_id: group_id(),
                members: others.iter().map(|party| member(party)).collect(),
            }),
            contact_name: String::new(),
        },
    }
}

/// Everyone other than the owner who sent one of `rows`, once each, in the
/// order they first wrote. A number and the same number written another way
/// are one person.
fn other_parties<'a>(rows: &[&'a RawRow]) -> Vec<&'a str> {
    let mut seen = HashSet::new();
    let mut parties = Vec::new();
    for row in rows {
        let sender = row.sender.trim();
        if resolve_is_from_me(row) || sender.is_empty() {
            continue;
        }
        let identity = match sanitize_number(sender) {
            Some(_) => phone::normalize_lenient(sender),
            None => sender.to_string(),
        };
        if seen.insert(identity) {
            parties.push(sender);
        }
    }
    parties
}

/// A group member: their number, or the name the source gives in its place.
fn member(party: &str) -> IrParticipant {
    if sanitize_number(party).is_some() {
        IrParticipant {
            handle: Some(phone::normalize_lenient(party)),
            display_name: None,
            handle_type: Some(HandleType::Phone),
        }
    } else {
        IrParticipant {
            handle: None,
            display_name: Some(party.to_string()),
            handle_type: None,
        }
    }
}

/// A one-to-one conversation with the person who wrote (`sender`), or with
/// the person the `Conversation` value names (`label`).
///
/// It is keyed by a number when either gives one, the sender's first. A chat
/// labelled with a person's name whose rows carry that person's number is
/// keyed by the number the source recorded. Only when neither is a number is
/// it keyed by the name, the label's first: the exporter records the name and
/// no address, and the server resolves it against contacts on import. With
/// neither, the row goes to the `unknown` conversation.
fn one_to_one(sender: Option<&str>, label: Option<&str>) -> Conversation {
    let given = |s: &&str| !s.trim().is_empty();
    let number = [sender, label]
        .into_iter()
        .flatten()
        .filter(given)
        .find(|s| sanitize_number(s).is_some());
    let name = [label, sender]
        .into_iter()
        .flatten()
        .filter(given)
        .find(|s| sanitize_number(s).is_none())
        .map(|s| s.trim().to_string());
    let key = match (number, &name) {
        // Format as E.164 when unambiguous. Otherwise keep digits as-is. Never invent `+0…`.
        (Some(number), _) => Some(ConversationKey::OneToOne(phone::normalize_lenient(number))),
        (None, Some(name)) => Some(ConversationKey::NameOnly(name.clone())),
        (None, None) => None,
    };
    Conversation {
        key,
        contact_name: name.unwrap_or_default(),
    }
}

/// A per-chat file's id within the export: its path under the input folder,
/// or its file name when the input is the file itself.
fn file_vendor_id(input: &Path, path: &Path) -> String {
    path.strip_prefix(input)
        .ok()
        .filter(|relative| !relative.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new(path.file_name().unwrap_or(path.as_os_str())))
        .to_string_lossy()
        .replace('\\', "/")
}

/// True for the literal `Me` OpenExtract writes for the owner.
fn is_me(s: &str) -> bool {
    s.trim().eq_ignore_ascii_case("me")
}

/// Whether the row is outgoing, from its direction column, else its
/// "Is From Me" column, else a sender of `Me`.
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
    row.is_from_me || is_me(&row.sender)
}

/// The sender handle and display name for a row: empty for outgoing, else the
/// row's own sender. A sender the source names without a number takes the
/// number of the one-to-one conversation it is in.
fn resolve_sender(row: &RawRow, is_from_me: bool, conversation: &Conversation) -> (String, String) {
    if is_from_me {
        return (String::new(), String::new());
    }
    let sender = row.sender.trim();
    let is_group = conversation
        .key
        .as_ref()
        .is_some_and(ConversationKey::is_group);
    if sanitize_number(sender).is_some() {
        let display = if is_group {
            String::new()
        } else {
            conversation.contact_name.clone()
        };
        return (phone::normalize_lenient(sender), display);
    }
    let handle = match &conversation.key {
        Some(ConversationKey::OneToOne(handle)) => handle.clone(),
        _ => String::new(),
    };
    let display = if !sender.is_empty() {
        sender.to_string()
    } else if !is_group {
        conversation.contact_name.clone()
    } else {
        String::new()
    };
    (handle, display)
}

/// Unix seconds, and the same instant in milliseconds as a string, for a row's
/// date in RFC 3339 or OpenExtract's local formats.
fn parse_timestamp(raw: &str) -> Option<i64> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    // RFC3339 / ISO-8601 with offset (OpenExtract style).
    if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        return Some(dt.timestamp());
    }
    // Fallback without fractional seconds.
    if let Ok(dt) = DateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S%z") {
        return Some(dt.timestamp());
    }
    None
}

/// OpenExtract deltas of the shared [`message_ir::pending_to_document`] projection.
struct OpenExtractProjection {
    export: ExportMeta,
    keys: HashMap<String, ConversationKey>,
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

    /// A group's members come from its key. A one-to-one conversation's one
    /// participant is the person it is with: their number, or for a
    /// conversation keyed by a name, the name and no address. The `unknown`
    /// conversation has no roster at all.
    fn participants(&self, chat_id: &str, convo: &PendingConversation) -> Vec<IrParticipant> {
        match self.keys.get(chat_id) {
            None => Vec::new(),
            Some(ConversationKey::Group { members, .. }) => members.clone(),
            Some(ConversationKey::OneToOne(handle)) => vec![IrParticipant {
                handle: Some(handle.clone()),
                display_name: convo.first_contact_name(),
                handle_type: Some(HandleType::Phone),
            }],
            Some(ConversationKey::NameOnly(_)) => vec![IrParticipant {
                handle: None,
                display_name: convo.first_contact_name(),
                handle_type: None,
            }],
        }
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
