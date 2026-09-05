//! Convert iMazing Messages / WhatsApp rows into the shared conversation
//! structure, then write the chosen output format via [`ExportWriter`].

use crate::attachments::{AttachmentIndex, ResolveAttachmentArgs, resolve_attachment_cell};
use crate::attachments_emit::{attachment_guid_materials, pending_attachment_to_ir};
use crate::parse::{DiscoveredCsv, RawRow, SourceKind, discover_csv_files, parse_csv_file};
use crate::parse_emit::{
    PeerInfo, TzMode, collect_peer_info, is_notification, is_outgoing, parse_message_date,
    resolve_sender, resolve_tz,
};
use anyhow::Result;
use message_ir::{
    ExportMeta, HandleType, IrAttachment, IrParticipant, IrService, IrSource, PendingAttachment,
    PendingConversation, PendingMessage, ProjectedRole, ProjectionHooks,
};
use message_ir_format::{AttachmentSource, ExportTransforms, ExportWriter, FormatSinkResult};
use message_vault_io_core::{
    CancelFlag, ExportReport, OutputFormat, prepare_outputs, project_conversation,
};
use serde_json::Map;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

const EXPORT_SOURCE: &str = "imazing";
const EXPORT_TOOL: &str = "iMazing";
const EXPORT_TOOL_VERSION: &str = "3.5.5";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TransportFamily {
    Messages,
    WhatsApp,
}

impl TransportFamily {
    /// The conversation-key prefix that keeps Messages and WhatsApp chats with the same peer apart.
    fn key_prefix(self) -> &'static str {
        match self {
            Self::Messages => "messages",
            Self::WhatsApp => "whatsapp",
        }
    }
}

/// Inputs for [`convert_export`].
pub(crate) struct ConvertExportArgs<'a> {
    pub input: &'a Path,
    pub output: &'a Path,
    pub timezone: Option<&'a str>,
    pub transforms: ExportTransforms,
    pub output_format: OutputFormat,
    pub cancel: Option<&'a CancelFlag>,
    /// Continue an interrupted export: keep previous output and skip the
    /// conversations already written.
    pub resume: bool,
}

/// Convert iMazing Messages / WhatsApp CSV(s) under `input`.
///
/// `timezone`: fixed UTC offset (e.g. `UTC-05:00`). When `None`, use the host local zone.
/// When `transforms` copies attachments, media files are copied into `output/attachments/`.
/// When `cancel` is set, cooperative cancellation is checked between CSV files.
///
/// # Errors
///
/// Returns an error when output overlaps input, a CSV cannot be parsed, or the
/// user cancels.
pub(crate) fn convert_export(
    args: ConvertExportArgs<'_>,
) -> Result<(ExportReport, FormatSinkResult)> {
    let ConvertExportArgs {
        input,
        output,
        timezone,
        transforms,
        output_format,
        cancel,
        resume,
    } = args;
    let tz = resolve_tz(timezone)?;
    let (inputs, output) = prepare_outputs(&[input.to_path_buf()], output)?;
    let input = &inputs[0];
    let writer = ExportWriter::open(&output, output_format, transforms, resume)?;
    let copy_attachments = writer.copies_attachments();

    let mut ingest = Ingest {
        tz,
        // Walk the input tree once; per-attachment lookups hit this index.
        attachment_index: copy_attachments.then(|| AttachmentIndex::build(input)),
        copy_attachments,
        conversations: BTreeMap::new(),
        seen_keys: BTreeMap::new(),
        report: ExportReport::default(),
    };
    for discovered in discover_csv_files(input)? {
        message_vault_io_core::check_cancel(cancel)?;
        ingest.ingest_file(&discovered);
    }
    let Ingest {
        conversations,
        mut report,
        ..
    } = ingest;

    let hooks = ImazingProjection {
        export: message_vault_io_core::export_meta(
            EXPORT_SOURCE,
            EXPORT_TOOL,
            EXPORT_TOOL_VERSION,
            None,
            None,
        ),
    };
    let mut documents = Vec::new();
    let mut sources = Vec::new();
    for (_key, mut convo) in conversations {
        let chat_id = convo.chat_id.clone();
        let Some(doc) = project_conversation(&chat_id, &mut convo, &hooks, &mut report) else {
            continue;
        };
        collect_attachment_sources(&convo, &mut sources);
        documents.push(doc);
    }

    let mut source_iter = sources.into_iter();
    let sink_result = writer.finish(
        documents,
        &mut |att| match source_iter.next().flatten() {
            Some(path) => {
                // iMazing's rows carry no size; stat the source so the byte
                // counters and the headroom check see it.
                let hint = att
                    .size_bytes
                    .or_else(|| std::fs::metadata(&path).ok().map(|m| m.len()));
                (AttachmentSource::Path(path), hint)
            }
            None => (AttachmentSource::Missing, att.size_bytes),
        },
        cancel,
        &mut report,
    )?;

    Ok((report, sink_result))
}

/// Parse-time state shared across every CSV file in one export.
struct Ingest {
    tz: TzMode,
    attachment_index: Option<AttachmentIndex>,
    copy_attachments: bool,
    /// Keyed by `<family>|<chat id>` so a Messages chat and a WhatsApp chat
    /// with the same peer stay separate conversations.
    conversations: BTreeMap<String, PendingConversation>,
    /// Parse-time dedupe state keyed by conversation key (the shared
    /// `PendingConversation` carries document data only).
    seen_keys: BTreeMap<String, HashSet<String>>,
    report: ExportReport,
}

impl Ingest {
    /// Parse one CSV and fold every chat session in it into the pending conversations.
    ///
    /// A file that fails to parse is recorded in the report and skipped so
    /// one bad export does not stop the rest.
    fn ingest_file(&mut self, discovered: &DiscoveredCsv) {
        match discovered.kind {
            SourceKind::Messages => self.report.bump("messages_files", 1),
            SourceKind::WhatsApp => self.report.bump("whatsapp_files", 1),
        }
        let rows = match parse_csv_file(&discovered.path, discovered.kind) {
            Ok(rows) => rows,
            Err(e) => {
                self.report
                    .errors
                    .push(format!("{}: {e:#}", discovered.path.display()));
                return;
            }
        };
        let mut by_session: BTreeMap<String, Vec<&RawRow>> = BTreeMap::new();
        for row in &rows {
            by_session
                .entry(row.chat_session.clone())
                .or_default()
                .push(row);
        }
        for (session, session_rows) in by_session {
            self.ingest_session(discovered, &session, &session_rows);
        }
    }

    /// Work out who one chat session is with, then add each of its rows.
    fn ingest_session(&mut self, discovered: &DiscoveredCsv, session: &str, rows: &[&RawRow]) {
        let peer = collect_peer_info(discovered.kind, session, rows);
        if peer.unresolved_chat {
            self.report.bump("name_only_chat", 1);
        }
        self.report.bump(
            "unresolved_group_participants",
            peer.unresolved_roster_labels,
        );
        let family = TransportFamily::from_kind(discovered.kind);
        let convo_key = format!("{}|{}", family.key_prefix(), peer.chat_id);
        self.conversations
            .entry(convo_key.clone())
            .or_insert_with(|| {
                let mut convo = PendingConversation::new(
                    peer.chat_id.clone(),
                    peer.group,
                    peer.group.then(|| session.to_string()),
                    Vec::new(),
                );
                convo
                    .extra
                    .insert("source_kind".into(), discovered.kind.as_str().to_string());
                if peer.unresolved_chat {
                    convo
                        .extra
                        .insert(message_ir::CHAT_ID_IS_NAME.into(), "1".into());
                }
                convo
            });
        for row in rows {
            if let Some(message) = self.message_from_row(discovered, row, &peer, &convo_key) {
                self.conversations
                    .get_mut(&convo_key)
                    .expect("conversation inserted above")
                    .messages
                    .push(message);
            }
        }
    }

    /// Build the pending message for one CSV row, or `None` when the row has
    /// no usable date or repeats a row already seen in this conversation.
    fn message_from_row(
        &mut self,
        discovered: &DiscoveredCsv,
        row: &RawRow,
        peer: &PeerInfo,
        convo_key: &str,
    ) -> Option<PendingMessage> {
        let Some((secs, date_ms)) = parse_message_date(&row.message_date, &self.tz) else {
            self.report.skipped_invalid_date += 1;
            return None;
        };
        let is_notification = is_notification(&row.msg_type);
        let is_from_me = !is_notification && is_outgoing(&row.msg_type);
        let (sender_handle, sender_display_name) = resolve_sender(
            row,
            is_from_me,
            is_notification,
            &peer.chat_id,
            &peer.contact_name,
        );
        // sender_id distinguishes same-second same-text rows from
        // different senders in group chats.
        let dedupe_key = format!(
            "{}|{}|{}|{}|{}|{}",
            peer.chat_id,
            secs,
            if is_from_me { "1" } else { "0" },
            row.sender_id,
            row.text,
            row.attachment
        );
        if !self
            .seen_keys
            .entry(convo_key.to_string())
            .or_default()
            .insert(dedupe_key)
        {
            self.report.duplicates_dropped += 1;
            return None;
        }
        let (attachments, attachment_extra) = self.attachment_for_row(discovered, row);
        let service = if row.service.trim().is_empty() {
            match discovered.kind {
                SourceKind::WhatsApp => "WhatsApp".to_string(),
                SourceKind::Messages => "SMS".to_string(),
            }
        } else {
            row.service.clone()
        };

        let mut extra = BTreeMap::new();
        extra.insert(
            "is_notification".into(),
            if is_notification { "true" } else { "false" }.into(),
        );
        extra.insert("subject".into(), row.subject.clone());
        extra.insert("contact_name".into(), peer.contact_name.clone());
        extra.insert("date_ms".into(), date_ms);
        extra.insert("service".into(), service);
        extra.insert("imazing_status".into(), row.status.clone());
        extra.insert("imazing_type".into(), row.msg_type.clone());
        extra.insert("reactions".into(), row.reactions.clone());
        extra.insert("replying_to".into(), row.replying_to.clone());
        extra.insert("forwarded".into(), row.forwarded.clone());
        extra.insert("attachment_info".into(), row.attachment_info.clone());
        extra.insert("delivered_date".into(), row.delivered_date.clone());
        extra.insert("read_date".into(), row.read_date.clone());
        extra.insert("edited_date".into(), row.edited_date.clone());
        extra.insert("deleted_date".into(), row.deleted_date.clone());
        extra.insert("sent_date".into(), row.sent_date.clone());
        extra.extend(attachment_extra);

        Some(PendingMessage {
            sort_key: secs,
            is_from_me,
            sender_handle,
            sender_display_name: (!sender_display_name.is_empty()).then_some(sender_display_name),
            text: row.text.clone(),
            attachments,
            extra,
        })
    }

    /// The attachment a row names (iMazing rows carry at most one), plus the
    /// sticker and transcription metadata that rides on the message.
    fn attachment_for_row(
        &self,
        discovered: &DiscoveredCsv,
        row: &RawRow,
    ) -> (Vec<PendingAttachment>, BTreeMap<String, String>) {
        if row.attachment.is_empty() {
            return (Vec::new(), BTreeMap::new());
        }
        let csv_parent = discovered.path.parent().unwrap_or_else(|| Path::new("."));
        let (cell, source) = resolve_attachment_cell(ResolveAttachmentArgs {
            csv_name: &row.attachment,
            attachment_type: &row.attachment_type,
            csv_parent,
            index: self.attachment_index.as_ref(),
            copy_attachments: self.copy_attachments,
        });
        let attachment = PendingAttachment {
            rel_path: row.attachment.clone(),
            content_type: cell.meta.mime_type.clone().unwrap_or_default(),
            extension: Path::new(&row.attachment)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_string(),
            digest_sha256: None,
            name_hint: cell.meta.original_name.clone(),
        };
        let mut extra = BTreeMap::new();
        extra.insert(
            "is_sticker".into(),
            if cell.is_sticker { "true" } else { "false" }.into(),
        );
        extra.insert(
            "transcription".into(),
            cell.transcription.unwrap_or_default(),
        );
        extra.insert(
            "sticker_effect".into(),
            cell.sticker_effect.unwrap_or_default(),
        );
        if let Some(src) = source {
            extra.insert(
                "attachment_source".into(),
                src.to_string_lossy().into_owned(),
            );
        }
        (vec![attachment], extra)
    }
}

fn collect_attachment_sources(
    convo: &PendingConversation,
    out: &mut Vec<Option<std::path::PathBuf>>,
) {
    for msg in &convo.messages {
        if msg.attachments.is_empty() {
            continue;
        }
        let source = msg.extra_str("attachment_source").to_string();
        for _ in &msg.attachments {
            out.push((!source.is_empty()).then(|| std::path::PathBuf::from(&source)));
        }
    }
}

/// iMazing identifiers are E.164 phones, emails, or (rarely) name stems;
/// infer the type from the handle shape.
fn handle_type_for(handle: &str) -> HandleType {
    if handle.contains('@') {
        HandleType::Email
    } else {
        HandleType::Phone
    }
}

/// Peer handles for a chat: the comma-separated list for groups, else the chat id itself.
fn imazing_peers(is_group: bool, chat_id: &str) -> Vec<String> {
    if is_group {
        chat_id
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        Vec::new()
    }
}

/// `__whatsapp` for WhatsApp chats so their files do not collide with Messages files for the same peer.
fn imazing_packaging_stem_suffix(source_kind: &str) -> Option<String> {
    if source_kind == "whatsapp" {
        Some("__whatsapp".into())
    } else {
        None
    }
}

/// iMazing deltas of the shared [`message_ir::pending_to_document`] projection.
struct ImazingProjection {
    export: ExportMeta,
}

impl ProjectionHooks for ImazingProjection {
    fn export(&self) -> ExportMeta {
        self.export.clone()
    }

    fn service(&self, msg: &PendingMessage) -> IrService {
        IrService::parse(msg.extra_str("service"))
    }

    fn role(&self, msg: &PendingMessage) -> ProjectedRole {
        if msg.extra_flag("is_notification") {
            ProjectedRole::Notification
        } else if msg.is_from_me {
            ProjectedRole::Outgoing
        } else {
            ProjectedRole::Incoming
        }
    }

    fn subject(&self, msg: &PendingMessage) -> Option<String> {
        msg.extra_opt("subject")
    }

    fn guid_materials(&self, msg: &PendingMessage) -> Vec<String> {
        attachment_guid_materials(&msg.attachments)
    }

    fn attachment_to_ir(&self, att: &PendingAttachment, msg: &PendingMessage) -> IrAttachment {
        pending_attachment_to_ir(att, msg)
    }

    fn participants(&self, chat_id: &str, convo: &PendingConversation) -> Vec<IrParticipant> {
        let peers = imazing_peers(convo.is_group, chat_id);
        let mut participants: Vec<IrParticipant> = peers
            .iter()
            .map(|h| IrParticipant {
                handle: Some(h.clone()),
                display_name: None,
                handle_type: Some(handle_type_for(h)),
            })
            .collect();
        if participants.is_empty() && !convo.is_group && !chat_id.is_empty() {
            if convo.extra.contains_key(message_ir::CHAT_ID_IS_NAME) {
                // The source named this person and recorded no address.
                participants.push(IrParticipant {
                    handle: None,
                    display_name: convo.first_contact_name(),
                    handle_type: None,
                });
            } else {
                participants.push(IrParticipant {
                    handle: Some(chat_id.to_string()),
                    display_name: convo.first_contact_name(),
                    handle_type: Some(handle_type_for(chat_id)),
                });
            }
        }
        participants
    }

    fn packaging_stem_suffix(&self, convo: &PendingConversation) -> Option<String> {
        imazing_packaging_stem_suffix(convo.extra_str("source_kind"))
    }

    fn source(&self, convo: &PendingConversation, msg: &PendingMessage) -> IrSource {
        let mut fields = Map::new();
        // Session string is not a real group title: stored as data only
        // (the document's `group_title` stays `None`, matching the previous
        // CSV/mail stem).
        let session_title = convo.display_name.as_deref().unwrap_or("");
        if !session_title.is_empty() {
            fields.insert(
                "group_title".into(),
                serde_json::Value::String(session_title.to_string()),
            );
        }
        for key in [
            "imazing_status",
            "imazing_type",
            "reactions",
            "replying_to",
            "forwarded",
            "attachment_info",
            "delivered_date",
            "read_date",
            "edited_date",
            "deleted_date",
            "sent_date",
        ] {
            let val = msg.extra_str(key);
            if !val.is_empty() {
                fields.insert(key.into(), serde_json::Value::String(val.to_string()));
            }
        }
        IrSource {
            android_type: None,
            fields,
        }
    }
}

#[cfg(test)]
mod tests;
