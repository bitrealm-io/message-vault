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
    PendingConversation, PendingMessage, ProjectionHooks,
};
use message_staging::{AttachmentSource, ExportWriter};
use phone::sanitize_number;
use serde_json::{Map, json};
use sha2::{Digest, Sha256};
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
        ingest.ingest_file(&path);
    }
    message_crate_core::check_cancel(cancel)?;
    let (conversations, mut report) = ingest.finish();

    let export = message_crate_core::export_meta(
        EXPORT_SOURCE,
        EXPORT_TOOL,
        EXPORT_TOOL_VERSION,
        None,
        None,
    );
    let mut documents = Vec::new();
    for (chat_id, mut pending) in conversations {
        let hooks = OpenExtractProjection {
            export: &export,
            key: pending.key.as_ref(),
        };
        if let Some(doc) = project_conversation(&chat_id, &mut pending.convo, &hooks, &mut report) {
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
    conversations: BTreeMap<String, Pending>,
    /// Per-chat files that are not one-to-one. They are keyed in
    /// [`Ingest::finish`], once every file is read, because a group's key
    /// depends on whether another file starts with the same row.
    group_files: Vec<GroupFile>,
    report: ExportReport,
}

/// One conversation and its key, awaiting projection.
struct Pending {
    /// `None` for the `unknown` conversation, of sent rows that name nobody.
    key: Option<ConversationKey>,
    convo: PendingConversation,
}

/// A per-chat file that is a group, before its key is known.
struct GroupFile {
    file_name: String,
    members: Vec<IrParticipant>,
    rows: Vec<RawRow>,
}

/// The conversation a set of rows belongs to.
struct Conversation {
    /// `None` for the `unknown` conversation.
    key: Option<ConversationKey>,
    /// The other person's name, for a one-to-one conversation the source
    /// names; empty otherwise.
    contact_name: String,
    /// The `Conversation` value of a group in the all-conversations CSV.
    group_name: Option<String>,
}

impl Conversation {
    fn chat_id(&self) -> String {
        self.key
            .as_ref()
            .map_or_else(|| "unknown".to_string(), ConversationKey::chat_id)
    }

    fn is_group(&self) -> bool {
        self.key.as_ref().is_some_and(ConversationKey::is_group)
    }

    fn group(vendor_id: String, members: Vec<IrParticipant>, group_name: Option<String>) -> Self {
        Self {
            key: Some(ConversationKey::Group { vendor_id, members }),
            contact_name: String::new(),
            group_name,
        }
    }
}

impl Ingest {
    /// Parse one CSV and add its rows. A file that fails to parse is recorded
    /// in the report and skipped so one bad export does not stop the rest.
    ///
    /// A per-chat file is one conversation, whoever sent each row. In the
    /// all-conversations CSV a row's conversation is its `Conversation`
    /// value; a row with none belongs to its incoming sender.
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
        if first.source_kind == SourceKind::PerChat {
            self.ingest_per_chat_file(path, rows);
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
            .map(|(label, rows)| (label.to_string(), labelled_conversation(&rows, label)))
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

    /// A per-chat file in which one person other than the account holder
    /// wrote is one-to-one with them. Any other is a group, keyed later.
    ///
    /// A file in which nobody else wrote is a group of nobody known, so that
    /// two such files never share a conversation. It is most likely a
    /// one-to-one conversation whose recipient the source does not record;
    /// the group stands in until such conversations get a kind of their own
    /// (#1095).
    fn ingest_per_chat_file(&mut self, path: &Path, rows: Vec<RawRow>) {
        let all: Vec<&RawRow> = rows.iter().collect();
        let others = other_parties(&all);
        if let [one] = others.as_slice() {
            let conversation = one_to_one(Some(one), None);
            for row in rows {
                self.ingest_row(row, &conversation);
            }
            return;
        }
        let members = others.iter().map(|party| member(party)).collect();
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.group_files.push(GroupFile {
            file_name,
            members,
            rows,
        });
    }

    /// Key the per-chat groups and add their rows; return every conversation
    /// and the report.
    fn finish(mut self) -> (BTreeMap<String, Pending>, ExportReport) {
        let files = std::mem::take(&mut self.group_files);
        let ids = group_vendor_ids(&files);
        for (file, vendor_id) in files.into_iter().zip(ids) {
            let conversation = Conversation::group(vendor_id, file.members, None);
            for row in file.rows {
                self.ingest_row(row, &conversation);
            }
        }
        (self.conversations, self.report)
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

        let report = &mut self.report;
        let pending = self
            .conversations
            .entry(chat_id.clone())
            .or_insert_with(|| {
                if conversation
                    .key
                    .as_ref()
                    .is_some_and(ConversationKey::is_name_only)
                {
                    // Counted once per conversation, not once per row.
                    report.bump("name_only_chat", 1);
                }
                Pending {
                    key: conversation.key.clone(),
                    convo: PendingConversation::new(
                        chat_id,
                        conversation.is_group(),
                        conversation.group_name.clone(),
                        Vec::new(),
                    ),
                }
            });
        let mut extra = BTreeMap::new();
        extra.insert("contact_name".into(), conversation.contact_name.clone());
        extra.insert(
            "has_attachments".into(),
            if row.has_attachments { "true" } else { "false" }.into(),
        );
        extra.insert("source_kind".into(), row.source_kind.as_str().to_string());
        pending.convo.messages.push(PendingMessage {
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
/// names someone other than the account holder.
fn conversation_label(row: &RawRow) -> Option<&str> {
    row.conversation
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty() && !is_me(s))
}

/// The conversation of every row of the all-conversations CSV with one
/// `Conversation` value, `label`.
///
/// Two or more people other than the account holder wrote in a group, keyed
/// `group:` and the label, so it never equals a person's address. Where one
/// person did, it is one-to-one with them, and where nobody did, with the
/// person the label names: as for iMazing, only a second person makes a
/// group.
fn labelled_conversation(rows: &[&RawRow], label: &str) -> Conversation {
    match other_parties(rows).as_slice() {
        [] => one_to_one(None, Some(label)),
        [one] => one_to_one(Some(one), Some(label)),
        others => Conversation::group(
            label.to_string(),
            others.iter().map(|party| member(party)).collect(),
            Some(label.to_string()),
        ),
    }
}

/// Everyone other than the account holder who sent one of `rows`, once
/// each, in the order they first wrote. A number and the same number written
/// another way are one person.
///
/// An email address does not count as a person of its own when a number
/// wrote too: Apple Messages puts one person's number and Apple ID in the
/// same conversation.
fn other_parties<'a>(rows: &[&'a RawRow]) -> Vec<&'a str> {
    let mut seen = HashSet::new();
    let mut parties = Vec::new();
    for row in rows {
        let sender = row.sender.trim();
        if resolve_is_from_me(row) || sender.is_empty() {
            continue;
        }
        let identity = address(sender).unwrap_or_else(|| sender.to_string());
        if seen.insert(identity) {
            parties.push(sender);
        }
    }
    let is_email = |party: &&str| address(party).is_some_and(|a| a.contains('@'));
    let is_number = |party: &&str| address(party).is_some_and(|a| !a.contains('@'));
    if parties.iter().any(is_number) {
        parties.retain(|party| !is_email(party));
    }
    parties
}

/// A sender's address: an email address in lowercase, or a number in E.164
/// when that is unambiguous and as digits otherwise (never an invented
/// `+0…`). `None` for a name.
fn address(sender: &str) -> Option<String> {
    let sender = sender.trim();
    if sender.contains('@') {
        return Some(sender.to_lowercase());
    }
    sanitize_number(sender).map(|_| phone::normalize_lenient(sender))
}

/// The handle type of an [`address`].
fn handle_type_for(address: &str) -> HandleType {
    if address.contains('@') {
        HandleType::Email
    } else {
        HandleType::Phone
    }
}

/// A group member: their address, or the name the source gives in its place.
fn member(party: &str) -> IrParticipant {
    match address(party) {
        Some(address) => IrParticipant {
            handle_type: Some(handle_type_for(&address)),
            handle: Some(address),
            display_name: None,
        },
        None => IrParticipant {
            handle: None,
            display_name: Some(party.to_string()),
            handle_type: None,
        },
    }
}

/// A one-to-one conversation with the person who wrote (`sender`), or with
/// the person the `Conversation` value names (`label`).
///
/// It is keyed by an address when either gives one, the sender's first. A
/// chat labelled with a person's name whose rows carry that person's number
/// is keyed by the number the source recorded. Only when neither is an
/// address is it keyed by the name, the label's first: the exporter records
/// the name and no address, and the server resolves it against contacts on
/// import. With neither, the row goes to the `unknown` conversation.
fn one_to_one(sender: Option<&str>, label: Option<&str>) -> Conversation {
    let given = |s: &&str| !s.trim().is_empty();
    let address = [sender, label]
        .into_iter()
        .flatten()
        .filter(given)
        .find_map(address);
    let name = [label, sender]
        .into_iter()
        .flatten()
        .filter(given)
        .find(|s| self::address(s).is_none())
        .map(|s| s.trim().to_string());
    let key = match (address, &name) {
        (Some(address), _) => Some(ConversationKey::OneToOne(address)),
        (None, Some(name)) => Some(ConversationKey::NameOnly(name.clone())),
        (None, None) => None,
    };
    Conversation {
        key,
        contact_name: name.unwrap_or_default(),
        group_name: None,
    }
}

/// The vendor id of each per-chat group in `files`, in lowercase hex.
///
/// It is the digest of the file's earliest row ([`row_digests`]), so it
/// stays the same across exports while new messages arrive, when someone new
/// writes, and wherever the export is put: OpenExtract names its files by
/// number (`conversation_7.csv`), and the same number in two exports is not
/// the same conversation. Files whose earliest rows are the same take the
/// digest of all their rows, and files whose rows are all the same, such as
/// one message sent to several people who never answered, add the file name
/// as well. As for iMazing, the id changes when the oldest messages are gone
/// from the phone: the group then comes in as a second conversation, never
/// merged with another.
fn group_vendor_ids(files: &[GroupFile]) -> Vec<String> {
    let digests: Vec<Vec<[u8; 32]>> = files.iter().map(|file| row_digests(&file.rows)).collect();
    let id = |index: usize, level: u8| -> String {
        let rows = &digests[index];
        if level == 0 {
            return hex::encode(rows.first().copied().unwrap_or_default());
        }
        let mut hasher = Sha256::new();
        for digest in rows {
            hasher.update(digest);
        }
        if level >= 2 {
            hasher.update([0x1f]);
            hasher.update(files[index].file_name.as_bytes());
        }
        hex::encode(hasher.finalize())
    };
    let mut levels = vec![0_u8; files.len()];
    loop {
        let ids: Vec<String> = (0..files.len()).map(|i| id(i, levels[i])).collect();
        let mut count: HashMap<&str, usize> = HashMap::new();
        for id in &ids {
            *count.entry(id).or_default() += 1;
        }
        let mut raised = false;
        for (i, id) in ids.iter().enumerate() {
            if count[id.as_str()] > 1 && levels[i] < 2 {
                levels[i] += 1;
                raised = true;
            }
        }
        if !raised {
            return ids;
        }
    }
}

/// SHA-256 of each row's date, sender, text and direction, joined by the
/// ASCII unit separator, earliest row first. Rows of the same time are
/// ordered by their digest, so the order does not depend on the CSV's.
fn row_digests(rows: &[RawRow]) -> Vec<[u8; 32]> {
    let mut ordered: Vec<(i64, [u8; 32])> = rows
        .iter()
        .map(|row| {
            let mut hasher = Sha256::new();
            let direction = if resolve_is_from_me(row) { "1" } else { "0" };
            for (index, field) in [row.date.trim(), row.sender.trim(), &row.text, direction]
                .into_iter()
                .enumerate()
            {
                if index > 0 {
                    hasher.update([0x1f]);
                }
                hasher.update(field.as_bytes());
            }
            (
                parse_timestamp(&row.date).unwrap_or(i64::MAX),
                hasher.finalize().into(),
            )
        })
        .collect();
    ordered.sort();
    ordered.into_iter().map(|(_, digest)| digest).collect()
}

/// True for the literal `Me` OpenExtract writes for the account holder.
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
/// row's own sender. A sender the source names without an address takes the
/// address of the one-to-one conversation it is in.
fn resolve_sender(row: &RawRow, is_from_me: bool, conversation: &Conversation) -> (String, String) {
    if is_from_me {
        return (String::new(), String::new());
    }
    let sender = row.sender.trim();
    let contact_name = if conversation.is_group() {
        String::new()
    } else {
        conversation.contact_name.clone()
    };
    if let Some(address) = address(sender) {
        return (address, contact_name);
    }
    let handle = match &conversation.key {
        Some(ConversationKey::OneToOne(handle)) => handle.clone(),
        _ => String::new(),
    };
    let display = if sender.is_empty() {
        contact_name
    } else {
        sender.to_string()
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
struct OpenExtractProjection<'a> {
    export: &'a ExportMeta,
    /// The key of the conversation being projected; `None` for `unknown`.
    key: Option<&'a ConversationKey>,
}

impl ProjectionHooks for OpenExtractProjection<'_> {
    fn export(&self) -> ExportMeta {
        self.export.clone()
    }

    fn service(&self, _msg: &PendingMessage) -> IrService {
        IrService::Sms
    }

    fn source(&self, convo: &PendingConversation, msg: &PendingMessage) -> IrSource {
        let mut fields = Map::new();
        fields.insert("source_kind".into(), json!(msg.extra_str("source_kind")));
        fields.insert(
            "has_attachments".into(),
            json!(msg.extra_flag("has_attachments")),
        );
        // A group's `Conversation` value is kept as data, not as its title:
        // the export does not say whether it is a name or a list of people.
        if let Some(name) = &convo.display_name {
            fields.insert("conversation".into(), json!(name));
        }
        IrSource {
            android_type: None,
            fields,
        }
    }

    /// A group's members come from its key. A one-to-one conversation's one
    /// participant is the person it is with: their address, or for a
    /// conversation keyed by a name, the name and no address. The `unknown`
    /// conversation has no roster at all.
    fn participants(&self, _chat_id: &str, convo: &PendingConversation) -> Vec<IrParticipant> {
        match self.key {
            None => Vec::new(),
            Some(ConversationKey::Group { members, .. }) => members.clone(),
            Some(ConversationKey::OneToOne(handle)) => vec![IrParticipant {
                handle: Some(handle.clone()),
                display_name: convo.first_contact_name(),
                handle_type: Some(handle_type_for(handle)),
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
