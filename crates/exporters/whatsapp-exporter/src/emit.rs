//! Convert wtsexporter JSON into the shared conversation structure, then write
//! the chosen output format via [`ExportWriter`].

use crate::jid::{chat_id_from_jid, is_group_jid, jid_to_e164};
use crate::parse::{
    ChatJson, MessageJson, load_chat_store, media_path, message_text, timestamp_ms, timestamp_secs,
};
use anyhow::{Context, Result};
use message_csv::{format_local_ts, json_cell};
use message_ir::{
    ExportMeta, HandleType, IrAttachment, IrParticipant, IrService, IrSource, PendingAttachment,
    PendingConversation, PendingMessage, ProjectionHooks, SortKeyUnit,
};
use message_staging::{AttachmentSource, ExportWriter};
use message_vault_io_core::{
    CancelFlag, ExportReport, ExportTransforms, OutputFormat, project_conversation,
};
use serde_json::Map;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const EXPORT_SOURCE: &str = "whatsapp";
const EXPORT_TOOL: &str = "WhatsApp Chat Exporter";
/// Pinned documented upstream version (JSON convert path; shell-out may differ).
pub(crate) const EXPORT_TOOL_VERSION: &str = "0.13.0";

/// File extension without the leading dot, e.g. `"jpg"` for `"photo.jpg"`.
fn ext_of(name: &str) -> String {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_string()
}

/// Convert a wtsexporter `result.json` into the shared conversation structure,
/// then write the chosen output format.
///
/// `media_search_roots` are directories tried when resolving relative media paths
/// (typically the wtsexporter working directory / process cwd).
///
/// When `cancel` is set, it is checked between chats (cooperative cancellation).
///
/// # Errors
///
/// Returns an error when the JSON cannot be read, a conversation cannot be
/// written, or the user cancels.
pub(crate) fn convert_json(
    json_path: &Path,
    output: &Path,
    transforms: ExportTransforms,
    media_search_roots: &[PathBuf],
    output_format: OutputFormat,
    cancel: Option<&CancelFlag>,
    resume: bool,
) -> Result<ExportReport> {
    fs::create_dir_all(output).with_context(|| format!("create {}", output.display()))?;
    // Load the chat store BEFORE cleaning the output directory. The JSON may live
    // inside the output dir (e.g. wtsexporter_result.json) and cleaning
    // deletes all *.json files.
    let store = load_chat_store(json_path)?;
    let writer = ExportWriter::open(output, output_format, transforms, resume)?;
    let copy_attachments = writer.copies_attachments();
    let mut report = ExportReport::default();
    let mut conversations: BTreeMap<String, PendingConversation> = BTreeMap::new();

    for (jid, chat) in store {
        message_vault_io_core::check_cancel(cancel)?;
        if jid.starts_with('_') {
            // Reserved / system keys if any.
            continue;
        }
        if let Some((chat_id, convo)) = ingest_chat(
            &jid,
            &chat,
            copy_attachments,
            media_search_roots,
            &mut report,
        ) {
            conversations.insert(chat_id, convo);
        }
    }

    let hooks = WhatsappProjection {
        export: message_vault_io_core::export_meta(
            EXPORT_SOURCE,
            EXPORT_TOOL,
            EXPORT_TOOL_VERSION,
            None,
            None,
        ),
    };
    let mut documents = Vec::new();
    let mut media_sources: Vec<Option<PathBuf>> = Vec::new();
    for (chat_id, mut convo) in conversations {
        message_vault_io_core::check_cancel(cancel)?;
        let Some(doc) = project_conversation(&chat_id, &mut convo, &hooks, &mut report) else {
            continue;
        };
        collect_media_sources(&convo, &mut media_sources);
        documents.push(doc);
    }

    let mut source_iter = media_sources.into_iter();
    writer.finish(
        documents,
        &mut |att| {
            let hint = att.size_bytes;
            match source_iter.next().flatten() {
                Some(path) => (AttachmentSource::Path(path), hint),
                None => (AttachmentSource::Missing, hint),
            }
        },
        cancel,
        &mut report,
    )?;

    Ok(report)
}

/// Ingest one WhatsApp chat JSON into a pending conversation (messages + media).
fn ingest_chat(
    jid: &str,
    chat: &ChatJson,
    copy_attachments: bool,
    media_search_roots: &[PathBuf],
    report: &mut ExportReport,
) -> Option<(String, PendingConversation)> {
    let group = is_group_jid(jid);
    let chat_id = chat_id_from_jid(jid);
    let group_title = if group {
        chat.name.as_deref().and_then(message_ir::nonempty)
    } else {
        None
    };

    let mut peer_phones: BTreeSet<String> = BTreeSet::new();
    if !group && let Some(e164) = jid_to_e164(jid) {
        peer_phones.insert(e164);
    }

    let mut pending = PendingConversation::new(chat_id.clone(), group, group_title, Vec::new());
    pending.extra.insert("whatsapp_jid".into(), jid.to_string());

    let display_fallback = chat.name.clone().unwrap_or_default();

    for msg in chat.messages.values() {
        let Some(ts_raw) = msg.timestamp else {
            report.skipped_invalid_date += 1;
            continue;
        };
        let secs = timestamp_secs(ts_raw);
        if format_local_ts(secs).is_none() {
            report.skipped_invalid_date += 1;
            continue;
        }

        let is_from_me = msg.from_me;
        let (sender_handle, sender_display_name) =
            resolve_sender(msg, is_from_me, &chat_id, &display_fallback, group);
        if group
            && let Some(e164) = jid_to_e164(sender_handle.as_str())
                .or_else(|| msg.sender.as_deref().and_then(jid_to_e164))
        {
            peer_phones.insert(e164);
        }

        let text = message_text(msg);
        let (attachments, media_source) = match media_path(msg) {
            Some(src) => queue_media(
                src,
                chat.media_base.as_deref(),
                media_search_roots,
                copy_attachments,
                msg,
                report,
            ),
            None => (Vec::new(), None),
        };

        pending.messages.push(PendingMessage {
            sort_key: timestamp_ms(ts_raw),
            is_from_me,
            sender_handle,
            sender_display_name: if sender_display_name.is_empty() {
                None
            } else {
                Some(sender_display_name)
            },
            text,
            attachments,
            extra: {
                let mut e = BTreeMap::new();
                e.insert("key_id".into(), key_id_string(msg));
                e.insert("reply_json".into(), optional_json(msg.reply.as_ref()));
                e.insert("reactions_json".into(), reactions_json(&msg.reactions));
                e.insert(
                    "is_sticker".into(),
                    if msg.sticker { "true" } else { "false" }.into(),
                );
                if let Some(path) = media_source {
                    e.insert("media_source".into(), path.to_string_lossy().into_owned());
                }
                e
            },
        });
    }

    if pending.messages.is_empty() {
        return None;
    }

    pending.participant_e164s = peer_phones.into_iter().collect();
    Some((chat_id, pending))
}

/// Sender handle and display name for a WhatsApp message (empty when from me).
fn resolve_sender(
    msg: &MessageJson,
    is_from_me: bool,
    chat_id: &str,
    chat_name: &str,
    group: bool,
) -> (String, String) {
    if is_from_me {
        return (String::new(), String::new());
    }
    if group {
        // Real JID / phone sender → E.164 handle. Display-name senders (e.g. a
        // group member's name) leave the handle empty; only the display name is set.
        let sender = msg.sender.as_deref().unwrap_or_default();
        match jid_to_e164(sender) {
            Some(e164) => (e164, String::new()),
            None => (String::new(), sender.to_string()),
        }
    } else {
        let handle = if chat_id.starts_with('+') {
            chat_id.to_string()
        } else {
            msg.sender
                .as_deref()
                .and_then(jid_to_e164)
                .unwrap_or_else(|| chat_id.to_string())
        };
        (handle, chat_name.to_string())
    }
}

/// Resolve a media path during parse. Do not copy; the runner writes later.
fn queue_media(
    src: &str,
    media_base: Option<&str>,
    media_search_roots: &[PathBuf],
    copy_attachments: bool,
    msg: &MessageJson,
    report: &mut ExportReport,
) -> (Vec<PendingAttachment>, Option<PathBuf>) {
    let name = Path::new(src)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned());
    let pending = PendingAttachment {
        rel_path: String::new(),
        content_type: msg.mime.clone().unwrap_or_default(),
        extension: name.as_deref().map(ext_of).unwrap_or_default(),
        digest_sha256: None,
        name_hint: name,
    };
    if !copy_attachments {
        return (vec![pending], None);
    }
    if let Some(src_path) = resolve_media_file(src, media_base, media_search_roots) {
        (vec![pending], Some(src_path))
    } else {
        report.bump("attachments_missing", 1);
        (Vec::new(), None)
    }
}

/// Collect source paths in the same order attachments will appear on documents.
fn collect_media_sources(convo: &PendingConversation, out: &mut Vec<Option<PathBuf>>) {
    for msg in &convo.messages {
        if msg.attachments.is_empty() {
            continue;
        }
        let source = msg.extra_str("media_source").to_string();
        for _ in &msg.attachments {
            out.push((!source.is_empty()).then(|| PathBuf::from(&source)));
        }
    }
}

/// Resolve a wtsexporter media path against `media_base` and search roots.
///
/// Only paths that resolve inside an allowed root (search roots, or an
/// absolute `media_base`) are accepted. Absolute hints and `..` segments
/// cannot escape those roots.
fn resolve_media_file(
    src: &str,
    media_base: Option<&str>,
    media_search_roots: &[PathBuf],
) -> Option<PathBuf> {
    let allowed = allowed_media_roots(media_base, media_search_roots);
    if allowed.is_empty() {
        return None;
    }

    let hint = Path::new(src);
    let mut candidates: Vec<PathBuf> = Vec::new();
    if hint.is_absolute() {
        candidates.push(hint.to_path_buf());
    } else if let Some(base) = media_base.and_then(message_ir::trimmed) {
        let base_path = Path::new(base);
        if base_path.is_absolute() {
            candidates.push(base_path.join(hint));
        }
        for root in media_search_roots {
            candidates.push(root.join(base_path).join(hint));
        }
        for root in media_search_roots {
            candidates.push(root.join(hint));
        }
    } else {
        for root in media_search_roots {
            candidates.push(root.join(hint));
        }
    }

    candidates
        .into_iter()
        .find(|p| p.is_file() && path_within_any(p, &allowed))
}

/// Allowed roots for media path checks (search roots plus an absolute `media_base`).
fn allowed_media_roots(media_base: Option<&str>, media_search_roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = media_search_roots.to_vec();
    if let Some(base) = media_base.and_then(message_ir::trimmed) {
        let base_path = Path::new(base);
        if base_path.is_absolute() {
            roots.push(base_path.to_path_buf());
        }
    }
    roots
}

/// True when `path` resolves to a location under any of `roots`.
fn path_within_any(path: &Path, roots: &[PathBuf]) -> bool {
    let Ok(canon) = fs::canonicalize(path) else {
        return false;
    };
    roots.iter().any(|root| {
        fs::canonicalize(root)
            .ok()
            .is_some_and(|root_canon| canon.starts_with(root_canon))
    })
}

/// WhatsApp `key_id` as a string (empty when missing).
fn key_id_string(msg: &MessageJson) -> String {
    match &msg.key_id {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
        None => String::new(),
    }
}

/// Compact JSON cell, or empty when `None` / null.
fn optional_json(v: Option<&serde_json::Value>) -> String {
    match v {
        Some(val) if !val.is_null() => json_cell(val),
        _ => String::new(),
    }
}

/// Compact JSON for reactions, or empty when null / empty object.
fn reactions_json(v: &serde_json::Value) -> String {
    if v.is_null() || (v.is_object() && v.as_object().is_some_and(|o| o.is_empty())) {
        String::new()
    } else {
        json_cell(v)
    }
}

/// WhatsApp deltas of the shared [`message_ir::pending_to_document`] projection.
struct WhatsappProjection {
    export: ExportMeta,
}

impl ProjectionHooks for WhatsappProjection {
    fn export(&self) -> ExportMeta {
        self.export.clone()
    }

    fn service(&self, _msg: &PendingMessage) -> IrService {
        IrService::Whatsapp
    }

    fn sort_key_unit(&self) -> SortKeyUnit {
        SortKeyUnit::Milliseconds
    }

    /// Same-millisecond rows fall back to `key_id` so output order is stable.
    fn message_order(&self, a: &PendingMessage, b: &PendingMessage) -> std::cmp::Ordering {
        a.sort_key
            .cmp(&b.sort_key)
            .then_with(|| a.extra_str("key_id").cmp(b.extra_str("key_id")))
    }

    fn guid_materials(&self, msg: &PendingMessage) -> Vec<String> {
        let mut digests: Vec<String> = msg
            .attachments
            .iter()
            .map(|a| a.digest_sha256.clone().unwrap_or_default())
            .collect();
        // key_id uniquely identifies a WhatsApp message; feed it into the GUID
        // so same-second messages with identical text get distinct GUIDs.
        digests.push(msg.extra_str("key_id").to_string());
        digests
    }

    fn attachment_to_ir(&self, att: &PendingAttachment, msg: &PendingMessage) -> IrAttachment {
        IrAttachment {
            path: (!att.rel_path.is_empty()).then(|| att.rel_path.clone()),
            original_name: att.name_hint.clone(),
            mime_type: att.mime_type(),
            digest_sha256: att.digest_sha256.clone(),
            is_sticker: msg.extra_flag("is_sticker"),
            transcription: None,
            sticker_effect: None,
            size_bytes: None,
            missing_reason: None,
            bytes: None,
        }
    }

    /// The raw E.164 roster, without display names and without the
    /// single-peer chat-id fallback: WhatsApp chat ids are not always phone
    /// handles, and peer names live on the messages instead.
    fn participants(&self, _chat_id: &str, convo: &PendingConversation) -> Vec<IrParticipant> {
        convo
            .participant_e164s
            .iter()
            .filter(|h| !h.is_empty())
            .map(|h| IrParticipant {
                handle: Some(h.clone()),
                display_name: None,
                handle_type: Some(HandleType::Phone),
            })
            .collect()
    }

    fn group_title(&self, convo: &PendingConversation) -> Option<String> {
        convo.display_name.clone()
    }

    fn packaging_stem_suffix(&self, _convo: &PendingConversation) -> Option<String> {
        Some("__whatsapp".into())
    }

    fn source(&self, convo: &PendingConversation, msg: &PendingMessage) -> IrSource {
        let mut fields = Map::new();
        let whatsapp_jid = convo.extra_str("whatsapp_jid");
        if !whatsapp_jid.is_empty() {
            fields.insert(
                "jid".into(),
                serde_json::Value::String(whatsapp_jid.to_string()),
            );
        }
        let key_id = msg.extra_str("key_id");
        if !key_id.is_empty() {
            fields.insert(
                "key_id".into(),
                serde_json::Value::String(key_id.to_string()),
            );
        }
        let reply_json = msg.extra_str("reply_json");
        if !reply_json.is_empty() {
            fields.insert(
                "reply".into(),
                serde_json::from_str(reply_json)
                    .unwrap_or_else(|_| serde_json::Value::String(reply_json.to_string())),
            );
        }
        let reactions_json = msg.extra_str("reactions_json");
        if !reactions_json.is_empty() {
            fields.insert(
                "reactions".into(),
                serde_json::from_str(reactions_json)
                    .unwrap_or_else(|_| serde_json::Value::String(reactions_json.to_string())),
            );
        }
        IrSource {
            android_type: None,
            fields,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_media_rejects_absolute_paths_outside_roots() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let secret = outside.path().join("secret.bin");
        fs::write(&secret, b"secret").unwrap();
        assert!(
            resolve_media_file(secret.to_str().unwrap(), None, &[root.path().to_path_buf()],)
                .is_none()
        );
    }

    #[test]
    fn resolve_media_rejects_file_only_under_cwd_like_path() {
        // Media roots passed to convert must be explicit (input / JSON parent /
        // work dir). A path that only exists under a separate "CWD-like" tree
        // must not resolve when that tree is omitted from the allowlist.
        let allowed = tempfile::tempdir().unwrap();
        let cwd_like = tempfile::tempdir().unwrap();
        let secret = cwd_like.path().join("media.jpg");
        fs::write(&secret, b"jpeg").unwrap();
        let roots = [allowed.path().to_path_buf()];
        assert!(!path_within_any(&secret, &roots));
        assert!(
            resolve_media_file(secret.to_str().unwrap(), None, &roots).is_none(),
            "file under a non-allowed tree must be rejected"
        );
    }

    #[test]
    fn resolve_media_rejects_dotdot_escape() {
        let root = tempfile::tempdir().unwrap();
        let sibling = root.path().parent().unwrap().join("escape_probe.bin");
        fs::write(&sibling, b"x").unwrap();
        let hint = "../escape_probe.bin";
        assert!(
            resolve_media_file(hint, None, &[root.path().to_path_buf()]).is_none(),
            "relative .. must not escape search roots"
        );
        let _ = fs::remove_file(&sibling);
    }

    #[test]
    fn resolve_media_accepts_path_under_search_root() {
        let root = tempfile::tempdir().unwrap();
        let media_base = "AppDomainGroup-group.net.whatsapp.WhatsApp.shared";
        let rel = "Message/Media/chat/a/b/photo.jpg";
        let src = root.path().join(media_base).join(rel);
        fs::create_dir_all(src.parent().unwrap()).unwrap();
        fs::write(&src, b"jpeg").unwrap();
        let found = resolve_media_file(rel, Some(media_base), &[root.path().to_path_buf()]);
        assert_eq!(found.as_deref(), Some(src.as_path()));
    }
}
