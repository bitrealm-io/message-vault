//! Stage message-ir JSONL rows into the temporary import tables.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use message_ir::{HandleService, HandleType, nonempty, trimmed};
use sqlx::AnyConnection;
use sqlx::Row;

use crate::assets::{self, AssetStats, StoredAsset};
use crate::config::validate_source_id;
use crate::db::dialect;
use crate::db::handles::{
    HandleIdCache, infer_handle_type_from_shape as infer_handle_type, upsert_handle_row_cached,
};
use crate::db::sql::{max_rows_for_bind_limit, values_tuples};
use crate::import_media;
use crate::jsonl;
use crate::models::{
    AttachmentRecord, ConversationRecord, ExportRecord, MessageRecord, TapbackRecord, clean_body,
};
use media::MediaMode;

use super::contact_name::{
    IncomingSender, ensure_contact_for_handle, resolve_incoming_sender_handle,
    resolve_name_only_participant,
};
use super::{ImportOptions, ImportStats};

struct PreparedAttachment {
    record: AttachmentRecord,
    stored: Option<StoredAsset>,
}

/// Size on disk of a stored blob, or `None` when it is not there.
fn stored_size_bytes(assets_dir: &Path, assets_path: Option<&str>) -> Option<i64> {
    let rel = assets_path?;
    let meta = std::fs::metadata(assets_dir.join(rel)).ok()?;
    Some(meta.len() as i64)
}

/// Convert/compress when requested; `None` means fall through to claimed-sha / path store.
fn try_store_converted(
    att: &mut AttachmentRecord,
    export_dir: &Path,
    assets_dir: &Path,
    asset_stats: &mut AssetStats,
    media: MediaMode,
    media_work: &Path,
) -> Result<Option<StoredAsset>> {
    if !matches!(media, MediaMode::Convert | MediaMode::Compress) {
        return Ok(None);
    }
    let Some(rel) = att.path.as_deref().and_then(trimmed) else {
        return Ok(None);
    };
    let source = crate::config::resolve_under_root(export_dir, rel)?;
    if !source.is_file() {
        return Ok(None);
    }
    let Some(resolved) =
        import_media::resolve_for_store(&source, att.mime_type.as_deref(), media, media_work)?
    else {
        return Ok(None);
    };
    // Bytes may have changed; drop any claimed SHA-256 fingerprint from the export.
    att.sha256 = None;
    att.mime_type = resolved.mime_type.or(att.mime_type.take());
    assets::hash_and_store(
        &resolved.path,
        assets_dir,
        att.mime_type.as_deref(),
        asset_stats,
    )
}

/// Store an attachment by the sha256 the export claims (reusing an existing blob) or by
/// hashing its file, counting the ones whose file is missing.
fn store_claimed_or_path(
    att: &AttachmentRecord,
    export_dir: &Path,
    assets_dir: &Path,
    asset_stats: &mut AssetStats,
) -> Result<Option<StoredAsset>> {
    if let Some(sha) = att.sha256.as_deref().and_then(trimmed) {
        if let Some(found) = assets::lookup_by_sha256(assets_dir, sha) {
            asset_stats.deduped += 1;
            return Ok(Some(StoredAsset {
                mime_type: att.mime_type.clone().or(found.mime_type),
                ..found
            }));
        }
        if let Some(rel) = att.path.as_deref().and_then(trimmed) {
            let source = crate::config::resolve_under_root(export_dir, rel)?;
            return match assets::store_verified(
                &source,
                sha,
                assets_dir,
                att.mime_type.as_deref(),
                false,
                false,
            ) {
                Ok((stored, already)) => {
                    if already {
                        asset_stats.deduped += 1;
                    } else {
                        asset_stats.copied += 1;
                    }
                    Ok(Some(stored))
                }
                Err(_) if !source.is_file() => {
                    asset_stats.missing += 1;
                    Ok(None)
                }
                Err(e) => Err(e),
            };
        }
        asset_stats.missing += 1;
        return Ok(None);
    }

    if let Some(rel) = att.path.as_deref() {
        let source = crate::config::resolve_under_root(export_dir, rel)?;
        return assets::hash_and_store(&source, assets_dir, att.mime_type.as_deref(), asset_stats);
    }
    asset_stats.missing += 1;
    Ok(None)
}

/// Stage every attachment of one message into the asset store, converting first when the media mode asks for it.
fn prepare_attachments(
    export_dir: &Path,
    assets_dir: &Path,
    attachments: Vec<AttachmentRecord>,
    asset_stats: &mut AssetStats,
    media: MediaMode,
    media_work: &Path,
) -> Result<Vec<PreparedAttachment>> {
    if media == MediaMode::Disabled {
        return Ok(Vec::new());
    }

    let mut prepared = Vec::with_capacity(attachments.len());
    for mut att in attachments {
        let stored = match try_store_converted(
            &mut att,
            export_dir,
            assets_dir,
            asset_stats,
            media,
            media_work,
        )? {
            Some(stored) => Some(stored),
            None => store_claimed_or_path(&att, export_dir, assets_dir, asset_stats)?,
        };
        prepared.push(PreparedAttachment {
            record: att,
            stored,
        });
    }
    Ok(prepared)
}

/// Per-import insert state. Message / attachment / tapback rows flush in
/// multi-row chunks. Handle ids are remembered so the same sender is not
/// looked up on every message.
pub(super) struct StagingInserts {
    account_id: String,
    import_id: Option<i64>,
    handles: HandleIdCache,
}

const INSERT_CONVERSATION: &str = r"
INSERT INTO staging_conversations (
    account_id, chat_handle_id, conversation_type, group_title, exported_at, source_file
) VALUES ($1, $2, $3, $4, $5, $6)
RETURNING id
";

const INSERT_PARTICIPANT: &str = r"
INSERT INTO staging_participants (conversation_id, handle_id, contact_id, name_alias)
VALUES ($1, $2, $3, $4)
";

const INSERT_MESSAGE_PREFIX: &str = r"
INSERT INTO staging_messages (
    conversation_id, account_id, source, guid, timestamp, is_from_me,
    sender_handle_id, service, subject, body, is_announcement, is_reply,
    thread_originator_guid, thread_originator_part, num_replies, sort_order, import_id
) VALUES
";

/// Bind counts must stay in lockstep with the `INSERT` column lists above.
const MESSAGE_BIND_COLUMNS: usize = 17;
const ATTACHMENT_BIND_COLUMNS: usize = 10;
const TAPBACK_BIND_COLUMNS: usize = 6;

const INSERT_MESSAGE_SUFFIX: &str = r"
ON CONFLICT DO NOTHING
RETURNING id, sort_order
";

const INSERT_ATTACHMENT_PREFIX: &str = r"
INSERT INTO staging_attachments (
    message_id, path, original_name, mime_type, is_sticker, transcription,
    sha256, assets_path, size_bytes, missing_reason
) VALUES
";

const INSERT_TAPBACK_PREFIX: &str = r"
INSERT INTO staging_tapbacks (
    message_id, part_index, kind, emoji, is_from_me, sender_handle_id
) VALUES
";

impl StagingInserts {
    /// Fresh insert state for one import run.
    pub(super) fn new(account_id: &str, import_id: Option<i64>) -> Self {
        Self {
            account_id: account_id.to_string(),
            import_id,
            handles: HandleIdCache::new(),
        }
    }
}

/// One participant as the conversation header records it: handle, the name
/// this backup used for them, and the handle type when the source said.
type StagedParticipant = (Option<String>, Option<String>, Option<HandleType>);

/// The source id for a conversation: its header's `export.source` when sources come from the files, else the fixed override.
fn resolve_conversation_source(
    opts: &ImportOptions<'_>,
    path: &Path,
    chat_identifier: &str,
    export_source: Option<&str>,
) -> Result<String> {
    if opts.source_from_jsonl {
        let Some(source) = export_source.and_then(trimmed) else {
            bail!(
                "{}: conversation '{}' is missing export.source \
                 (required for CLI directory import)",
                path.display(),
                chat_identifier
            );
        };
        validate_source_id(source)?;
        Ok(source.to_string())
    } else {
        Ok(opts.source.to_string())
    }
}

/// The asset store folder for this source, created when sources come from the files.
fn assets_dir_for_source(opts: &ImportOptions<'_>, source: &str) -> Result<PathBuf> {
    if opts.source_from_jsonl {
        let paths = opts
            .paths
            .ok_or_else(|| anyhow::anyhow!("source_from_jsonl requires config paths"))?;
        let dir = paths.assets_dir_for_account(opts.account_id, source);
        fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
        Ok(dir)
    } else {
        Ok(opts.assets_dir.to_path_buf())
    }
}

/// Messages with no conversation of their own live in `orphaned.jsonl`
/// (older bundles used `orphaned.json`), so they may omit a conversation header.
pub fn is_orphaned_export(path: &Path) -> bool {
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return false;
    };
    stem.eq_ignore_ascii_case("orphaned")
}

/// Stage one JSON Lines file: its conversation header and messages, or its orphaned messages.
///
/// # Errors
///
/// Returns an error when the file cannot be read, a message precedes its
/// header, or a conversation cannot be staged.
pub(super) async fn import_file_to_staging(
    tx: &mut AnyConnection,
    stmts: &mut StagingInserts,
    opts: &ImportOptions<'_>,
    path: &Path,
    asset_stats: &mut AssetStats,
    media_work: &Path,
) -> Result<ImportStats> {
    let mut staging = FileStaging {
        tx,
        stmts,
        opts,
        source_file: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown.jsonl")
            .to_string(),
        asset_stats,
        media_work,
        stats: ImportStats::default(),
    };
    let is_orphaned = is_orphaned_export(path);
    let mut pending: Option<StagedConversation> = None;
    let mut messages: Vec<MessageRecord> = Vec::new();

    for record in jsonl::read_records(path)? {
        match record {
            ExportRecord::Conversation(c) => {
                if let Some(header) = pending.take() {
                    staging.stage(header, std::mem::take(&mut messages)).await?;
                }
                let source = resolve_conversation_source(
                    opts,
                    path,
                    &c.chat_identifier,
                    c.export_source.as_deref(),
                )?;
                pending = Some(StagedConversation::from_record(c, source));
            }
            ExportRecord::Message(m) => {
                if pending.is_none() && !is_orphaned {
                    bail!(
                        "{} is missing a conversation header (expected before messages)",
                        path.display()
                    );
                }
                messages.push(m);
            }
        }
    }

    match pending.take() {
        Some(header) => staging.stage(header, messages).await?,
        None if is_orphaned => {
            if opts.source_from_jsonl {
                bail!(
                    "{}: orphaned.jsonl without a conversation header cannot supply export.source",
                    path.display()
                );
            }
            staging
                .stage(StagedConversation::orphaned(opts.source), messages)
                .await?;
        }
        None if messages.is_empty() => bail!(
            "{} has no conversation header and no messages",
            path.display()
        ),
        None => bail!(
            "{} is missing a conversation header (expected first record)",
            path.display()
        ),
    }
    Ok(staging.stats)
}

/// A conversation header as read from the file, with the source it resolved to.
struct StagedConversation {
    chat_identifier: String,
    /// `phone` | `whatsapp` for handle rows, when the header says.
    platform_service: Option<String>,
    conversation_type: String,
    group_title: Option<String>,
    exported_at: Option<String>,
    participants: Vec<StagedParticipant>,
    source: String,
}

impl StagedConversation {
    fn from_record(record: ConversationRecord, source: String) -> Self {
        Self {
            chat_identifier: record.chat_identifier,
            platform_service: record.service,
            conversation_type: record.conversation_type,
            group_title: record.group_title,
            exported_at: record.exported_at,
            participants: record
                .participants
                .into_iter()
                .map(|p| (p.handle, p.name_alias, p.handle_type))
                .collect(),
            source,
        }
    }

    /// The header for `orphaned.jsonl`: messages with no conversation of their own.
    fn orphaned(source: &str) -> Self {
        Self {
            chat_identifier: "orphaned".to_string(),
            platform_service: None,
            conversation_type: "orphaned".to_string(),
            group_title: None,
            exported_at: None,
            participants: Vec::new(),
            source: source.to_string(),
        }
    }
}

/// One JSON Lines file being staged: the connection and prepared statements,
/// the options, the counters its conversations add to, and the file's name
/// for the conversation rows.
struct FileStaging<'a> {
    tx: &'a mut AnyConnection,
    stmts: &'a mut StagingInserts,
    opts: &'a ImportOptions<'a>,
    source_file: String,
    asset_stats: &'a mut AssetStats,
    media_work: &'a Path,
    stats: ImportStats,
}

impl FileStaging<'_> {
    /// Stage one conversation: its media on disk, then its handle, conversation
    /// row, participants, and message rows in the staging tables.
    ///
    /// # Errors
    ///
    /// Returns an error when a media file cannot be stored or a row cannot be written.
    async fn stage(
        &mut self,
        conversation: StagedConversation,
        messages: Vec<MessageRecord>,
    ) -> Result<()> {
        let assets_dir = assets_dir_for_source(self.opts, &conversation.source)?;
        let mut stats = ImportStats::default();
        let platform = platform_for(
            conversation.platform_service.as_deref(),
            &conversation.source,
        );

        // Copy or convert media first: it needs no database rows, and a failure
        // here leaves nothing half-written.
        let prepared_messages = prepare_message_attachments(
            self.opts,
            &assets_dir,
            messages,
            self.asset_stats,
            self.media_work,
        )?;

        // Conversation identity: the chat handle, typed from its shape (Phone for
        // SMS/iMessage/WhatsApp numbers, Email for `@`, Other for group ids).
        let (chat_handle_id, flagged, cached) = upsert_handle_row_cached(
            self.tx,
            &mut self.stmts.handles,
            &self.stmts.account_id,
            &conversation.chat_identifier,
            infer_handle_type(&conversation.chat_identifier),
            Some(platform.as_str()),
        )
        .await?;
        if flagged {
            stats.phones_needing_review += 1;
        }
        if !cached {
            let _ = ensure_contact_for_handle(
                self.tx,
                &self.stmts.account_id,
                chat_handle_id,
                None,
                &mut stats,
            )
            .await?;
        }
        let conversation_id: i64 = sqlx::query_scalar(INSERT_CONVERSATION)
            .bind(&self.stmts.account_id)
            .bind(chat_handle_id)
            .bind(conversation.conversation_type)
            .bind(conversation.group_title)
            .bind(conversation.exported_at)
            .bind(&self.source_file)
            .fetch_one(&mut *self.tx)
            .await?;
        stats.conversations = 1;

        for participant in conversation.participants {
            insert_participant(
                self.tx,
                self.stmts,
                conversation_id,
                participant,
                platform,
                &mut stats,
            )
            .await?;
        }

        let pending_rows =
            resolve_message_rows(self.tx, self.stmts, prepared_messages, platform, &mut stats)
                .await?;
        let engine = dialect::engine_of(self.tx);
        let msg_chunk = max_rows_for_bind_limit(engine, MESSAGE_BIND_COLUMNS).max(1);
        for chunk in pending_rows.chunks(msg_chunk) {
            flush_staging_message_chunk(
                self.tx,
                self.stmts,
                &mut stats,
                conversation_id,
                &conversation.source,
                &assets_dir,
                chunk,
            )
            .await?;
        }
        self.stats.merge_file(&stats);
        Ok(())
    }
}

/// Platform for chat and participant handles: the conversation's own hint,
/// else WhatsApp for a WhatsApp export, else phone.
fn platform_for(platform_service: Option<&str>, source: &str) -> HandleService {
    platform_service
        .map(HandleService::parse)
        .unwrap_or_else(|| {
            if source.eq_ignore_ascii_case("whatsapp") {
                HandleService::Whatsapp
            } else {
                HandleService::Phone
            }
        })
}

/// Stage every message's attachments on disk, pairing each message with what
/// was kept.
///
/// # Errors
///
/// Returns an error when a media file cannot be copied or converted.
fn prepare_message_attachments(
    opts: &ImportOptions<'_>,
    assets_dir: &Path,
    messages: Vec<MessageRecord>,
    asset_stats: &mut AssetStats,
    media_work: &Path,
) -> Result<Vec<(MessageRecord, Vec<PreparedAttachment>)>> {
    let mut prepared = Vec::with_capacity(messages.len());
    for mut msg in messages {
        let attachments = prepare_attachments(
            opts.asset_root,
            assets_dir,
            std::mem::take(&mut msg.attachments),
            asset_stats,
            opts.media,
            media_work,
        )?;
        prepared.push((msg, attachments));
    }
    Ok(prepared)
}

/// Insert one participant row, bound to a handle when the source recorded an
/// address and to a name-only contact when it did not.
///
/// # Errors
///
/// Returns an error when a handle or contact row cannot be written.
async fn insert_participant(
    tx: &mut AnyConnection,
    stmts: &mut StagingInserts,
    conversation_id: i64,
    (handle, name_alias, handle_type): StagedParticipant,
    platform: HandleService,
    stats: &mut ImportStats,
) -> Result<()> {
    let Some(handle) = handle else {
        // The source named this person and recorded no address for them.
        // Nothing but a contact can hold a name with no identity, so the
        // participant is bound to one and carries no handle.
        let (contact_id, name_alias) =
            resolve_name_only_participant(tx, &stmts.account_id, name_alias.as_deref()).await?;
        // `resolve_name_only_participant` returns `(None, None)` when
        // there is nothing to create and nothing to show; honor that here
        // instead of inserting a row that names no one.
        let (Some(contact_id), Some(name_alias)) = (contact_id, name_alias) else {
            return Ok(());
        };
        sqlx::query(INSERT_PARTICIPANT)
            .bind(conversation_id)
            .bind(Option::<i64>::None)
            .bind(Some(contact_id))
            .bind(Some(name_alias))
            .execute(&mut *tx)
            .await?;
        stats.participants += 1;
        return Ok(());
    };
    // Prefer the source-provided type; fall back to shape inference.
    let handle_type = handle_type.unwrap_or_else(|| infer_handle_type(&handle));
    let (handle_id, flagged, _cached) = upsert_handle_row_cached(
        tx,
        &mut stmts.handles,
        &stmts.account_id,
        &handle,
        handle_type,
        Some(platform.as_str()),
    )
    .await?;
    if flagged {
        stats.phones_needing_review += 1;
    }
    let backup_name = name_alias.as_deref().and_then(nonempty);
    let contact_id = ensure_contact_for_handle(
        tx,
        &stmts.account_id,
        handle_id,
        backup_name.as_deref(),
        stats,
    )
    .await?;
    // `participants.name_alias` keeps what this backup called them in this
    // conversation. It is the second clause of the naming rule, never the
    // first.
    sqlx::query(INSERT_PARTICIPANT)
        .bind(conversation_id)
        .bind(handle_id)
        .bind(Some(contact_id))
        .bind(backup_name)
        .execute(&mut *tx)
        .await?;
    stats.participants += 1;
    Ok(())
}

/// Resolve each message's body text and sender handle into a row ready for
/// the bulk staging insert.
///
/// # Errors
///
/// Returns an error when a sender handle cannot be written.
async fn resolve_message_rows(
    tx: &mut AnyConnection,
    stmts: &mut StagingInserts,
    prepared: Vec<(MessageRecord, Vec<PreparedAttachment>)>,
    platform: HandleService,
    stats: &mut ImportStats,
) -> Result<Vec<PendingStagingMessage>> {
    let mut rows = Vec::with_capacity(prepared.len());
    for (sort_order, (msg, attachments)) in prepared.into_iter().enumerate() {
        let body = if msg.is_announcement {
            clean_body(msg.announcement.as_deref()).or_else(|| clean_body(msg.text.as_deref()))
        } else {
            clean_body(msg.text.as_deref())
        };
        let sender_platform = msg
            .service
            .as_deref()
            .map(HandleService::parse)
            .unwrap_or(platform);
        let sender_handle_id = resolve_incoming_sender_handle(
            tx,
            &mut stmts.handles,
            &stmts.account_id,
            IncomingSender {
                is_from_me: msg.is_from_me,
                address: msg.sender.as_deref(),
                handle_type: msg.sender_handle_type,
                platform: sender_platform.as_str(),
            },
            stats,
        )
        .await?;
        rows.push(PendingStagingMessage {
            msg,
            attachments,
            sender_handle_id,
            sender_platform: sender_platform.as_str().to_string(),
            body,
            sort_order: sort_order as i64,
        });
    }
    Ok(rows)
}

struct PendingStagingMessage {
    msg: MessageRecord,
    attachments: Vec<PreparedAttachment>,
    sender_handle_id: Option<i64>,
    sender_platform: String,
    body: Option<String>,
    sort_order: i64,
}

struct PendingAttachmentRow {
    message_id: i64,
    path: Option<String>,
    original_name: Option<String>,
    mime_type: Option<String>,
    is_sticker: i64,
    transcription: Option<String>,
    sha256: Option<String>,
    assets_path: Option<String>,
    size_bytes: Option<i64>,
    missing_reason: Option<String>,
}

struct PendingTapbackRow {
    message_id: i64,
    part_index: i64,
    kind: String,
    emoji: Option<String>,
    is_from_me: i64,
    sender_handle_id: Option<i64>,
}

/// Bulk-insert one chunk of message rows, then their attachments and tapbacks keyed by the ids returned.
async fn flush_staging_message_chunk(
    tx: &mut AnyConnection,
    stmts: &mut StagingInserts,
    stats: &mut ImportStats,
    conversation_id: i64,
    source: &str,
    assets_dir: &Path,
    chunk: &[PendingStagingMessage],
) -> Result<()> {
    if chunk.is_empty() {
        return Ok(());
    }
    let mut by_sort = insert_message_rows(tx, stmts, conversation_id, source, chunk).await?;

    let mut att_rows = Vec::new();
    let mut tap_rows = Vec::new();
    for row in chunk {
        // Consume the RETURNING id so a conflicted row (duplicate guid) is
        // skipped instead of attaching children to another message.
        let Some(message_id) = by_sort.remove(&row.sort_order) else {
            stats.messages_deduped += 1;
            continue;
        };
        stats.messages += 1;
        att_rows.extend(
            row.attachments
                .iter()
                .map(|prepared| PendingAttachmentRow::new(message_id, prepared, assets_dir)),
        );
        for tap in &row.msg.tapbacks {
            tap_rows.push(tapback_row(tx, stmts, stats, message_id, row, tap).await?);
        }
    }

    flush_attachment_chunks(tx, &att_rows, stats).await?;
    flush_tapback_chunks(tx, &tap_rows, stats).await?;
    Ok(())
}

/// Insert the chunk's message rows in one statement. Returns the new ids by
/// sort order; a row the insert skipped (duplicate guid) has no entry.
async fn insert_message_rows(
    tx: &mut AnyConnection,
    stmts: &StagingInserts,
    conversation_id: i64,
    source: &str,
    chunk: &[PendingStagingMessage],
) -> Result<HashMap<i64, i64>> {
    let sql = format!(
        "{INSERT_MESSAGE_PREFIX} {} {INSERT_MESSAGE_SUFFIX}",
        values_tuples(chunk.len(), MESSAGE_BIND_COLUMNS)
    );
    let mut q = sqlx::query(&sql);
    for row in chunk {
        q = q
            .bind(conversation_id)
            .bind(&stmts.account_id)
            .bind(source)
            .bind(row.msg.guid.as_deref())
            .bind(&row.msg.timestamp)
            .bind(row.msg.is_from_me as i64)
            .bind(row.sender_handle_id)
            .bind(row.msg.service.as_deref())
            .bind(row.msg.subject.as_deref())
            .bind(row.body.as_deref())
            .bind(row.msg.is_announcement as i64)
            .bind(row.msg.is_reply as i64)
            .bind(row.msg.thread_originator_guid.as_deref())
            .bind(row.msg.thread_originator_part)
            .bind(row.msg.num_replies)
            .bind(row.sort_order)
            .bind(stmts.import_id);
    }
    let returned = q.fetch_all(&mut *tx).await?;
    let mut by_sort = HashMap::with_capacity(returned.len());
    for row in &returned {
        let id: i64 = row.try_get(0)?;
        let sort_order: i64 = row.try_get(1)?;
        by_sort.insert(sort_order, id);
    }
    Ok(by_sort)
}

impl PendingAttachmentRow {
    /// The row for one of a staged message's attachments: the stored blob's
    /// digest, path, and type when the file was stored, the record's own
    /// type and missing reason when it was not.
    fn new(message_id: i64, prepared: &PreparedAttachment, assets_dir: &Path) -> Self {
        let att = &prepared.record;
        let (sha256, assets_path, mime_type) = match &prepared.stored {
            Some(stored) => (
                Some(stored.sha256.clone()),
                Some(stored.assets_path.clone()),
                stored.mime_type.clone().or_else(|| att.mime_type.clone()),
            ),
            None => (None, None, att.mime_type.clone()),
        };
        let size_bytes = stored_size_bytes(assets_dir, assets_path.as_deref())
            .or_else(|| att.size_bytes.map(|n| n as i64));
        let missing_reason = if sha256.is_none() {
            att.missing_reason.clone()
        } else {
            None
        };
        Self {
            message_id,
            path: att.path.clone(),
            original_name: att.original_name.clone(),
            mime_type,
            is_sticker: att.is_sticker as i64,
            transcription: att.transcription.clone(),
            sha256,
            assets_path,
            size_bytes,
            missing_reason,
        }
    }
}

/// The row for one tapback on a staged message, its sender resolved to a
/// handle the way an incoming message's sender is.
async fn tapback_row(
    tx: &mut AnyConnection,
    stmts: &mut StagingInserts,
    stats: &mut ImportStats,
    message_id: i64,
    row: &PendingStagingMessage,
    tap: &TapbackRecord,
) -> Result<PendingTapbackRow> {
    let sender_handle_id = resolve_incoming_sender_handle(
        tx,
        &mut stmts.handles,
        &stmts.account_id,
        IncomingSender {
            is_from_me: tap.is_from_me,
            address: tap.sender.as_deref(),
            handle_type: None,
            platform: &row.sender_platform,
        },
        stats,
    )
    .await?;
    Ok(PendingTapbackRow {
        message_id,
        part_index: tap.part_index,
        kind: tap.kind.clone(),
        emoji: tap.emoji.clone(),
        is_from_me: tap.is_from_me as i64,
        sender_handle_id,
    })
}

/// Bulk-insert attachment rows in chunks that fit the bind limit.
async fn flush_attachment_chunks(
    tx: &mut AnyConnection,
    rows: &[PendingAttachmentRow],
    stats: &mut ImportStats,
) -> Result<()> {
    let size = max_rows_for_bind_limit(dialect::engine_of(tx), ATTACHMENT_BIND_COLUMNS).max(1);
    for chunk in rows.chunks(size) {
        if chunk.is_empty() {
            continue;
        }
        let sql = format!(
            "{INSERT_ATTACHMENT_PREFIX} {}",
            values_tuples(chunk.len(), ATTACHMENT_BIND_COLUMNS)
        );
        let mut q = sqlx::query(&sql);
        for row in chunk {
            q = q
                .bind(row.message_id)
                .bind(row.path.as_deref())
                .bind(row.original_name.as_deref())
                .bind(row.mime_type.as_deref())
                .bind(row.is_sticker)
                .bind(row.transcription.as_deref())
                .bind(row.sha256.as_deref())
                .bind(row.assets_path.as_deref())
                .bind(row.size_bytes)
                .bind(row.missing_reason.as_deref());
        }
        q.execute(&mut *tx).await?;
        stats.attachments += chunk.len() as u64;
    }
    Ok(())
}

/// Bulk-insert tapback rows in chunks that fit the bind limit.
async fn flush_tapback_chunks(
    tx: &mut AnyConnection,
    rows: &[PendingTapbackRow],
    stats: &mut ImportStats,
) -> Result<()> {
    let size = max_rows_for_bind_limit(dialect::engine_of(tx), TAPBACK_BIND_COLUMNS).max(1);
    for chunk in rows.chunks(size) {
        if chunk.is_empty() {
            continue;
        }
        let sql = format!(
            "{INSERT_TAPBACK_PREFIX} {}",
            values_tuples(chunk.len(), TAPBACK_BIND_COLUMNS)
        );
        let mut q = sqlx::query(&sql);
        for row in chunk {
            q = q
                .bind(row.message_id)
                .bind(row.part_index)
                .bind(&row.kind)
                .bind(row.emoji.as_deref())
                .bind(row.is_from_me)
                .bind(row.sender_handle_id);
        }
        q.execute(&mut *tx).await?;
        stats.tapbacks += chunk.len() as u64;
    }
    Ok(())
}
