//! The import's staging tables (`schema/sql/staging.sql`) and the promotion
//! of their rows into production: every statement that reads or writes
//! `staging_conversations`, `staging_participants`, `staging_messages`,
//! `staging_attachments` and `staging_tapbacks`, and the two temp id maps
//! (`_promote_conv_map`, `_promote_msg_map`) the promotion joins through.
//!
//! `imports_api::staging` fills the tables and `imports_api::promote` runs
//! the promotion. Those stages sequence the statements, log them and keep
//! the counts, and hold no SQL (`docs/architecture/http-api.md`, "Code").
//! A promotion statement reads a staging table and writes a production one
//! in the same `INSERT ... SELECT`, and it lives here rather than with the
//! production table, because the staging tables have no reader but the
//! import while the production tables have many.

use std::collections::HashMap;

use anyhow::Result;
use sqlx::AnyConnection;
use sqlx::Row;

use super::dialect;
use super::sql::{SQLITE_IN_CHUNK, max_rows_for_bind_limit, values_tuples};

// ── Staging: what one import writes before promotion ─────────────────────

/// Clear one account's staging rows. Child rows go by CASCADE, and other
/// accounts are untouched.
///
/// # Errors
///
/// Returns an error when the delete fails.
pub async fn reset_for_account(conn: &mut AnyConnection, account_id: i64) -> Result<()> {
    sqlx::query("DELETE FROM staging_conversations WHERE account_id = $1")
        .bind(account_id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// One conversation header as the import stages it.
pub struct StagingConversation<'a> {
    /// Owning vault account.
    pub account_id: i64,
    /// The thread's identity handle, already written to `handles`.
    pub chat_handle_id: i64,
    /// `individual` or `group`.
    pub conversation_type: &'a str,
    /// Group label, when set.
    pub group_title: Option<&'a str>,
    /// When the export was produced, when known.
    pub exported_at: Option<&'a str>,
    /// Name of the file the thread came from.
    pub source_file: &'a str,
}

/// Insert one staged conversation and return its staging id.
///
/// # Errors
///
/// Returns an error when the insert fails, including when the account
/// already staged a conversation on the same handle.
pub async fn insert_conversation(
    conn: &mut AnyConnection,
    row: &StagingConversation<'_>,
) -> Result<i64> {
    Ok(sqlx::query_scalar(
        r"
        INSERT INTO staging_conversations (
            account_id, chat_handle_id, conversation_type, group_title, exported_at, source_file
        ) VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id
        ",
    )
    .bind(row.account_id)
    .bind(row.chat_handle_id)
    .bind(row.conversation_type)
    .bind(row.group_title)
    .bind(row.exported_at)
    .bind(row.source_file)
    .fetch_one(&mut *conn)
    .await?)
}

/// Insert one staged participant. `handle_id` is `None` for a person the
/// source named and recorded no address for; `name_alias` is what this
/// backup called them in this conversation.
///
/// # Errors
///
/// Returns an error when the insert fails.
pub async fn insert_participant(
    conn: &mut AnyConnection,
    conversation_id: i64,
    handle_id: Option<i64>,
    contact_id: Option<i64>,
    name_alias: Option<&str>,
) -> Result<()> {
    sqlx::query(
        r"
        INSERT INTO staging_participants (conversation_id, handle_id, contact_id, name_alias)
        VALUES ($1, $2, $3, $4)
        ",
    )
    .bind(conversation_id)
    .bind(handle_id)
    .bind(contact_id)
    .bind(name_alias)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// One message row as the import stages it, its sender and owner already
/// resolved to handle ids and its body cleaned.
pub struct StagingMessage<'a> {
    /// Parent staging conversation.
    pub conversation_id: i64,
    /// Owning vault account.
    pub account_id: i64,
    /// Backup family that produced the row.
    pub source: &'a str,
    /// Source-native message id, when the export had one.
    pub guid: Option<&'a str>,
    /// RFC 3339 UTC instant the message was sent.
    pub timestamp: &'a str,
    /// 1 when the account holder sent it.
    pub is_from_me: i64,
    /// Sender's handle id; `None` when unknown.
    pub sender_handle_id: Option<i64>,
    /// The account holder's own address on the message, as a handle id.
    pub owner_handle_id: Option<i64>,
    /// Per-message transport.
    pub service: Option<&'a str>,
    /// Subject line.
    pub subject: Option<&'a str>,
    /// Plain-text body.
    pub body: Option<&'a str>,
    /// 1 for a system bubble.
    pub is_announcement: i64,
    /// 1 for a threaded reply.
    pub is_reply: i64,
    /// GUID of the message a reply refers to.
    pub thread_originator_guid: Option<&'a str>,
    /// Part index within the originator for a multi-part reply.
    pub thread_originator_part: Option<i64>,
    /// Replies hanging off this message.
    pub num_replies: i64,
    /// Stable order within the conversation when timestamps collide.
    pub sort_order: i64,
    /// Import run that staged the row.
    pub import_id: Option<i64>,
}

/// One attachment row as the import stages it: the stored blob's digest,
/// path and type when the file was stored, the record's own type and
/// missing reason when it was not.
pub struct StagingAttachment {
    /// Parent staging message.
    pub message_id: i64,
    /// Path inside the export.
    pub path: Option<String>,
    /// File name from the export.
    pub original_name: Option<String>,
    /// MIME type, when known.
    pub mime_type: Option<String>,
    /// 1 for a sticker.
    pub is_sticker: i64,
    /// OCR/ASR transcription, when the exporter produced one.
    pub transcription: Option<String>,
    /// Fingerprint of the stored bytes, when stored.
    pub sha256: Option<String>,
    /// Path under the account's assets root, when stored.
    pub assets_path: Option<String>,
    /// Size in bytes, when known.
    pub size_bytes: Option<i64>,
    /// Why the file is missing, when it is.
    pub missing_reason: Option<String>,
}

/// One tapback row as the import stages it, its sender resolved to a handle.
pub struct StagingTapback {
    /// Parent staging message.
    pub message_id: i64,
    /// Attachment part the reaction applies to.
    pub part_index: i64,
    /// Reaction type, e.g. `love`.
    pub kind: String,
    /// The emoji, for a custom reaction.
    pub emoji: Option<String>,
    /// 1 when the account holder reacted.
    pub is_from_me: i64,
    /// Reactor's handle id; `None` when unknown.
    pub sender_handle_id: Option<i64>,
}

/// Bind counts, in lockstep with the `INSERT` column lists below.
const MESSAGE_BIND_COLUMNS: usize = 18;
const ATTACHMENT_BIND_COLUMNS: usize = 10;
const TAPBACK_BIND_COLUMNS: usize = 6;

/// The most message rows [`insert_messages`] takes in one statement on
/// this connection's engine.
pub fn message_chunk_rows(conn: &AnyConnection) -> usize {
    max_rows_for_bind_limit(dialect::engine_of(conn), MESSAGE_BIND_COLUMNS).max(1)
}

/// Insert one chunk of message rows in one statement, at most
/// [`message_chunk_rows`] of them. Returns the new ids by sort order; a row
/// the insert skipped (duplicate guid) has no entry, so its children are not
/// attached to another message.
///
/// # Errors
///
/// Returns an error when the insert fails.
pub async fn insert_messages(
    conn: &mut AnyConnection,
    rows: &[StagingMessage<'_>],
) -> Result<HashMap<i64, i64>> {
    let sql = format!(
        r"
        INSERT INTO staging_messages (
            conversation_id, account_id, source, guid, timestamp, is_from_me,
            sender_handle_id, owner_handle_id, service, subject, body, is_announcement, is_reply,
            thread_originator_guid, thread_originator_part, num_replies, sort_order, import_id
        ) VALUES {}
        ON CONFLICT DO NOTHING
        RETURNING id, sort_order
        ",
        values_tuples(rows.len(), MESSAGE_BIND_COLUMNS)
    );
    let mut q = sqlx::query(&sql);
    for row in rows {
        q = q
            .bind(row.conversation_id)
            .bind(row.account_id)
            .bind(row.source)
            .bind(row.guid)
            .bind(row.timestamp)
            .bind(row.is_from_me)
            .bind(row.sender_handle_id)
            .bind(row.owner_handle_id)
            .bind(row.service)
            .bind(row.subject)
            .bind(row.body)
            .bind(row.is_announcement)
            .bind(row.is_reply)
            .bind(row.thread_originator_guid)
            .bind(row.thread_originator_part)
            .bind(row.num_replies)
            .bind(row.sort_order)
            .bind(row.import_id);
    }
    let returned = q.fetch_all(&mut *conn).await?;
    let mut by_sort = HashMap::with_capacity(returned.len());
    for row in &returned {
        let id: i64 = row.try_get(0)?;
        let sort_order: i64 = row.try_get(1)?;
        by_sort.insert(sort_order, id);
    }
    Ok(by_sort)
}

/// Insert attachment rows in chunks that fit the bind limit. Returns how
/// many were inserted.
///
/// # Errors
///
/// Returns an error when an insert fails.
pub async fn insert_attachments(
    conn: &mut AnyConnection,
    rows: &[StagingAttachment],
) -> Result<u64> {
    let size = max_rows_for_bind_limit(dialect::engine_of(conn), ATTACHMENT_BIND_COLUMNS).max(1);
    let mut inserted = 0u64;
    for chunk in rows.chunks(size) {
        let sql = format!(
            r"
            INSERT INTO staging_attachments (
                message_id, path, original_name, mime_type, is_sticker, transcription,
                sha256, assets_path, size_bytes, missing_reason
            ) VALUES {}
            ",
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
        q.execute(&mut *conn).await?;
        inserted += chunk.len() as u64;
    }
    Ok(inserted)
}

/// Insert tapback rows in chunks that fit the bind limit. Returns how many
/// were inserted.
///
/// # Errors
///
/// Returns an error when an insert fails.
pub async fn insert_tapbacks(conn: &mut AnyConnection, rows: &[StagingTapback]) -> Result<u64> {
    let size = max_rows_for_bind_limit(dialect::engine_of(conn), TAPBACK_BIND_COLUMNS).max(1);
    let mut inserted = 0u64;
    for chunk in rows.chunks(size) {
        let sql = format!(
            r"
            INSERT INTO staging_tapbacks (
                message_id, part_index, kind, emoji, is_from_me, sender_handle_id
            ) VALUES {}
            ",
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
        q.execute(&mut *conn).await?;
        inserted += chunk.len() as u64;
    }
    Ok(inserted)
}

// ── Promotion: staging rows into the production tables ───────────────────
//
// Every statement joins through `_promote_conv_map` or `_promote_msg_map`,
// temp tables mapping staging ids to production ids, which the map writers
// below fill in the order `imports_api::promote` calls them.

/// Create, or empty, a temp table mapping staging ids to production ids.
/// Two statements on purpose: Postgres refuses two commands in one
/// prepared statement, and `split_ddl` only splits at line ends.
async fn reset_id_map(conn: &mut AnyConnection, table: &str) -> Result<()> {
    let create = format!(
        "CREATE TEMP TABLE IF NOT EXISTS {table} (staging_id BIGINT PRIMARY KEY, prod_id BIGINT NOT NULL)"
    );
    sqlx::query(&create).execute(&mut *conn).await?;
    let clear = format!("DELETE FROM {table}");
    sqlx::query(&clear).execute(&mut *conn).await?;
    Ok(())
}

/// How many conversations the account has staged.
///
/// # Errors
///
/// Returns an error when the count fails.
pub async fn count_staged_conversations(conn: &mut AnyConnection, account_id: i64) -> Result<i64> {
    Ok(
        sqlx::query_scalar("SELECT COUNT(*) FROM staging_conversations WHERE account_id = $1")
            .bind(account_id)
            .fetch_one(&mut *conn)
            .await?,
    )
}

/// The highest `conversations.id`, or 0 in an empty table: the watermark
/// new rows land above.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn max_conversation_id(conn: &mut AnyConnection) -> Result<i64> {
    Ok(
        sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) FROM conversations")
            .fetch_one(&mut *conn)
            .await?,
    )
}

/// Upsert the account's staged conversations into `conversations`, keyed by
/// `(account_id, chat_handle_id)`. A row already there keeps its title and
/// export time when the staged row has none.
///
/// # Errors
///
/// Returns an error when the statement fails.
pub async fn upsert_conversations(conn: &mut AnyConnection, account_id: i64) -> Result<()> {
    sqlx::query(
        r"
        INSERT INTO conversations (
            account_id, chat_handle_id, conversation_type,
            group_title, exported_at, source_file
        )
        SELECT
            account_id, chat_handle_id, conversation_type,
            group_title, exported_at, source_file
        FROM staging_conversations
        WHERE account_id = $1
        ON CONFLICT(account_id, chat_handle_id) DO UPDATE SET
            conversation_type = excluded.conversation_type,
            group_title = COALESCE(excluded.group_title, conversations.group_title),
            exported_at = COALESCE(excluded.exported_at, conversations.exported_at),
            source_file = excluded.source_file
        ",
    )
    .bind(account_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Write `_promote_conv_map`, the staging-to-production conversation id map
/// every later promotion statement joins through, from the rows
/// [`upsert_conversations`] just wrote.
///
/// # Errors
///
/// Returns an error when a statement fails.
pub async fn write_conversation_map(conn: &mut AnyConnection, account_id: i64) -> Result<()> {
    reset_id_map(conn, "_promote_conv_map").await?;
    sqlx::query(
        r"
        INSERT INTO _promote_conv_map (staging_id, prod_id)
        SELECT sc.id, c.id
        FROM staging_conversations sc
        JOIN conversations c
          ON c.account_id = sc.account_id
         AND c.chat_handle_id = sc.chat_handle_id
        WHERE sc.account_id = $1
        ",
    )
    .bind(account_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// How many mapped conversations have a production id above `max_before`:
/// the ones this promotion created.
///
/// # Errors
///
/// Returns an error when the count fails.
pub async fn count_mapped_conversations_above(
    conn: &mut AnyConnection,
    max_before: i64,
) -> Result<i64> {
    Ok(
        sqlx::query_scalar("SELECT COUNT(*) FROM _promote_conv_map WHERE prod_id > $1")
            .bind(max_before)
            .fetch_one(&mut *conn)
            .await?,
    )
}

/// How many participants the account has staged.
///
/// # Errors
///
/// Returns an error when the count fails.
pub async fn count_staged_participants(conn: &mut AnyConnection, account_id: i64) -> Result<i64> {
    Ok(sqlx::query_scalar(
        r"
        SELECT COUNT(*) FROM staging_participants
        WHERE conversation_id IN (
            SELECT id FROM staging_conversations WHERE account_id = $1
        )
        ",
    )
    .bind(account_id)
    .fetch_one(&mut *conn)
    .await?)
}

/// Insert the staged participants under their production conversations,
/// skipping any production already has. Returns how many were inserted.
///
/// # Errors
///
/// Returns an error when the statement fails.
pub async fn promote_participants(conn: &mut AnyConnection) -> Result<u64> {
    Ok(sqlx::query(
        r"
        INSERT INTO participants (conversation_id, handle_id, contact_id, name_alias)
        SELECT cm.prod_id, sp.handle_id, sp.contact_id, sp.name_alias
        FROM staging_participants sp
        JOIN _promote_conv_map cm ON cm.staging_id = sp.conversation_id
        ON CONFLICT DO NOTHING
        ",
    )
    .execute(&mut *conn)
    .await?
    .rows_affected())
}

/// How many messages the account has staged.
///
/// # Errors
///
/// Returns an error when the count fails.
pub async fn count_staged_messages(conn: &mut AnyConnection, account_id: i64) -> Result<i64> {
    Ok(sqlx::query_scalar(
        r"
        SELECT COUNT(*) FROM staging_messages
        WHERE conversation_id IN (
            SELECT id FROM staging_conversations WHERE account_id = $1
        )
        ",
    )
    .bind(account_id)
    .fetch_one(&mut *conn)
    .await?)
}

/// How many messages production holds, every account counted: what decides
/// whether the secondary indexes are cheaper to keep or to rebuild.
///
/// # Errors
///
/// Returns an error when the count fails.
pub async fn count_messages(conn: &mut AnyConnection) -> Result<i64> {
    Ok(sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(&mut *conn)
        .await?)
}

/// The highest `messages.id`, or 0 in an empty table: the watermark new
/// rows land above.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn max_message_id(conn: &mut AnyConnection) -> Result<i64> {
    Ok(
        sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) FROM messages")
            .fetch_one(&mut *conn)
            .await?,
    )
}

/// The lowest and highest staged message id under a mapped conversation,
/// or `(None, None)` when nothing is staged.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn staged_message_id_bounds(
    conn: &mut AnyConnection,
    account_id: i64,
) -> Result<(Option<i64>, Option<i64>)> {
    Ok(sqlx::query_as(
        r"
        SELECT MIN(sm.id), MAX(sm.id)
        FROM staging_messages sm
        JOIN _promote_conv_map cm ON cm.staging_id = sm.conversation_id
        WHERE sm.account_id = $1
        ",
    )
    .bind(account_id)
    .fetch_one(&mut *conn)
    .await?)
}

/// The column list and SELECT every staged-message insert shares; the caller
/// adds its WHERE tail and ordering. Rows are inserted in staging id order so
/// the production ids follow it, which the id-map zip relies on.
const INSERT_MESSAGES_FROM_STAGING: &str = r"
        INSERT INTO messages (
            conversation_id, account_id, source, guid, timestamp, is_from_me,
            sender_handle_id, owner_handle_id, service, subject, body, is_announcement, is_reply,
            thread_originator_guid, thread_originator_part, num_replies, sort_order, import_id
        )
        SELECT
            cm.prod_id, sm.account_id, sm.source, sm.guid, sm.timestamp, sm.is_from_me,
            sm.sender_handle_id, sm.owner_handle_id, sm.service, sm.subject, sm.body, sm.is_announcement, sm.is_reply,
            sm.thread_originator_guid, sm.thread_originator_part, sm.num_replies, sm.sort_order,
            sm.import_id
        FROM staging_messages sm
        JOIN _promote_conv_map cm ON cm.staging_id = sm.conversation_id
        WHERE sm.account_id = $1
";

/// The staged message ids the inserts above select, in the same order; the
/// caller adds the same WHERE tail.
const STAGED_MESSAGE_IDS: &str = r"
        SELECT sm.id
        FROM staging_messages sm
        JOIN _promote_conv_map cm ON cm.staging_id = sm.conversation_id
        WHERE sm.account_id = $1
";

const IN_ID_RANGE: &str = " AND sm.id > $2 AND sm.id <= $3 ORDER BY sm.id";
const WITH_GUID_IN_ID_RANGE: &str =
    " AND sm.guid IS NOT NULL AND sm.guid != '' AND sm.id > $2 AND sm.id <= $3 ORDER BY sm.id";
const WITHOUT_GUID: &str = " AND (sm.guid IS NULL OR sm.guid = '') ORDER BY sm.id";

/// Insert every staged message with an id in `lo..=hi` (replace mode: each
/// is new). Returns how many were inserted.
///
/// # Errors
///
/// Returns an error when the insert fails.
pub async fn promote_messages_in_range(
    conn: &mut AnyConnection,
    account_id: i64,
    lo: i64,
    hi: i64,
) -> Result<u64> {
    let sql = format!("{INSERT_MESSAGES_FROM_STAGING}{IN_ID_RANGE}");
    Ok(sqlx::query(&sql)
        .bind(account_id)
        .bind(lo)
        .bind(hi)
        .execute(&mut *conn)
        .await?
        .rows_affected())
}

/// Insert the staged messages with a guid and an id in `lo..=hi`, skipping
/// any production already holds through the partial unique index
/// `ix_messages_account_source_guid` with `ON CONFLICT DO NOTHING`.
/// (Correlated NOT EXISTS / JOIN anti-joins mis-plan onto
/// `ix_messages_source` and scan the whole source, 10s+ at 50k rows.)
/// Returns how many were inserted.
///
/// # Errors
///
/// Returns an error when the insert fails.
pub async fn promote_guid_messages_in_range(
    conn: &mut AnyConnection,
    account_id: i64,
    lo: i64,
    hi: i64,
) -> Result<u64> {
    let sql =
        format!("{INSERT_MESSAGES_FROM_STAGING}{WITH_GUID_IN_ID_RANGE} ON CONFLICT DO NOTHING");
    Ok(sqlx::query(&sql)
        .bind(account_id)
        .bind(lo)
        .bind(hi)
        .execute(&mut *conn)
        .await?
        .rows_affected())
}

/// Insert the staged messages without a guid, which are outside the guid
/// index and always new. Returns how many were inserted.
///
/// # Errors
///
/// Returns an error when the insert fails.
pub async fn promote_messages_without_guid(
    conn: &mut AnyConnection,
    account_id: i64,
) -> Result<u64> {
    let sql = format!("{INSERT_MESSAGES_FROM_STAGING}{WITHOUT_GUID}");
    Ok(sqlx::query(&sql)
        .bind(account_id)
        .execute(&mut *conn)
        .await?
        .rows_affected())
}

/// The staged message ids in `lo..=hi`, in id order: the rows
/// [`promote_messages_in_range`] inserted, in the order it inserted them.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn staged_message_ids_in_range(
    conn: &mut AnyConnection,
    account_id: i64,
    lo: i64,
    hi: i64,
) -> Result<Vec<i64>> {
    let sql = format!("{STAGED_MESSAGE_IDS}{IN_ID_RANGE}");
    Ok(sqlx::query_scalar(&sql)
        .bind(account_id)
        .bind(lo)
        .bind(hi)
        .fetch_all(&mut *conn)
        .await?)
}

/// The staged message ids without a guid, in id order: the rows
/// [`promote_messages_without_guid`] inserted, in the order it inserted them.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn staged_message_ids_without_guid(
    conn: &mut AnyConnection,
    account_id: i64,
) -> Result<Vec<i64>> {
    let sql = format!("{STAGED_MESSAGE_IDS}{WITHOUT_GUID}");
    Ok(sqlx::query_scalar(&sql)
        .bind(account_id)
        .fetch_all(&mut *conn)
        .await?)
}

/// The account's production message ids above `max_before`, in id order:
/// the rows a promotion insert just added.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn message_ids_above(
    conn: &mut AnyConnection,
    account_id: i64,
    max_before: i64,
) -> Result<Vec<i64>> {
    Ok(
        sqlx::query_scalar("SELECT id FROM messages WHERE id > $1 AND account_id = $2 ORDER BY id")
            .bind(max_before)
            .bind(account_id)
            .fetch_all(&mut *conn)
            .await?,
    )
}

/// Write `_promote_msg_map`, the staging-to-production message id map the
/// child-row statements join through: `pairs` first, then every guid row by
/// joining production on `(account, source, guid)`, which also maps the
/// append-mode rows that were skipped as duplicates.
///
/// # Errors
///
/// Returns an error when a statement fails.
pub async fn write_message_map(
    conn: &mut AnyConnection,
    account_id: i64,
    pairs: &HashMap<i64, i64>,
) -> Result<()> {
    reset_id_map(conn, "_promote_msg_map").await?;
    let pairs: Vec<(i64, i64)> = pairs.iter().map(|(&s, &p)| (s, p)).collect();
    for chunk in pairs.chunks(SQLITE_IN_CHUNK) {
        let sql = format!(
            "INSERT INTO _promote_msg_map (staging_id, prod_id) VALUES {}",
            values_tuples(chunk.len(), 2)
        );
        let mut q = sqlx::query(&sql);
        for &(staging_id, prod_id) in chunk {
            q = q.bind(staging_id).bind(prod_id);
        }
        q.execute(&mut *conn).await?;
    }
    sqlx::query(
        r"
        INSERT INTO _promote_msg_map (staging_id, prod_id)
        SELECT sm.id, m.id
        FROM staging_messages sm
        JOIN messages m
          ON m.account_id = sm.account_id
         AND m.source = sm.source
         AND m.guid = sm.guid
        JOIN _promote_conv_map cm ON cm.staging_id = sm.conversation_id
        WHERE sm.account_id = $1
          AND sm.guid IS NOT NULL
          AND sm.guid != ''
        ON CONFLICT(staging_id) DO UPDATE SET prod_id = excluded.prod_id
        ",
    )
    .bind(account_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// What [`promote_attachments`] did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PromotedAttachments {
    /// Production rows stored without a file that took the file from a
    /// staged row.
    pub filled: u64,
    /// New rows inserted.
    pub inserted: u64,
}

/// Insert the staged attachments under their production messages.
///
/// An attachment with a path is the same attachment when its message, path,
/// original name and sticker flag match; one without a path is the same only
/// when every field matches. A production row stored without its file takes
/// the file from a staged row that has one, so importing again after the
/// file turns up fills the row in instead of adding a second one.
///
/// # Errors
///
/// Returns an error when a statement fails.
pub async fn promote_attachments(conn: &mut AnyConnection) -> Result<PromotedAttachments> {
    let filled = sqlx::query(
        r"
        UPDATE attachments AS a
        SET sha256 = f.sha256,
            assets_path = f.assets_path,
            size_bytes = f.size_bytes,
            mime_type = COALESCE(f.mime_type, a.mime_type),
            missing_reason = NULL
        FROM (
            SELECT
                mm.prod_id AS message_id, sa.path, sa.original_name, sa.is_sticker,
                sa.sha256, sa.assets_path, sa.size_bytes, sa.mime_type
            FROM staging_attachments sa
            JOIN _promote_msg_map mm ON mm.staging_id = sa.message_id
            WHERE sa.path IS NOT NULL
              AND sa.sha256 IS NOT NULL
        ) AS f
        WHERE a.message_id = f.message_id
          AND a.path = f.path
          AND a.original_name IS NOT DISTINCT FROM f.original_name
          AND a.is_sticker = f.is_sticker
          AND a.sha256 IS NULL
        ",
    )
    .execute(&mut *conn)
    .await?
    .rows_affected();
    let inserted = sqlx::query(
        r"
        INSERT INTO attachments (
            message_id, path, original_name, mime_type, is_sticker, transcription,
            sha256, assets_path, size_bytes, missing_reason
        )
        SELECT
            mm.prod_id, sa.path, sa.original_name, sa.mime_type, sa.is_sticker, sa.transcription,
            sa.sha256, sa.assets_path, sa.size_bytes, sa.missing_reason
        FROM staging_attachments sa
        JOIN _promote_msg_map mm ON mm.staging_id = sa.message_id
        WHERE NOT EXISTS (
            SELECT 1
            FROM attachments a
            WHERE a.message_id = mm.prod_id
              AND a.path IS NOT DISTINCT FROM sa.path
              AND a.original_name IS NOT DISTINCT FROM sa.original_name
              AND a.is_sticker = sa.is_sticker
              AND (
                  sa.path IS NOT NULL
                  OR (
                      a.mime_type IS NOT DISTINCT FROM sa.mime_type
                      AND a.transcription IS NOT DISTINCT FROM sa.transcription
                      AND a.sha256 IS NOT DISTINCT FROM sa.sha256
                      AND a.assets_path IS NOT DISTINCT FROM sa.assets_path
                      AND a.size_bytes IS NOT DISTINCT FROM sa.size_bytes
                      AND a.missing_reason IS NOT DISTINCT FROM sa.missing_reason
                  )
              )
        )
        ",
    )
    .execute(&mut *conn)
    .await?
    .rows_affected();
    Ok(PromotedAttachments { filled, inserted })
}

/// Insert the staged tapbacks under their production messages, skipping any
/// row production already has field for field. Returns how many were
/// inserted.
///
/// # Errors
///
/// Returns an error when the statement fails.
pub async fn promote_tapbacks(conn: &mut AnyConnection) -> Result<u64> {
    Ok(sqlx::query(
        r"
        INSERT INTO tapbacks (
            message_id, part_index, kind, emoji, is_from_me, sender_handle_id
        )
        SELECT
            mm.prod_id, st.part_index, st.kind, st.emoji, st.is_from_me, st.sender_handle_id
        FROM staging_tapbacks st
        JOIN _promote_msg_map mm ON mm.staging_id = st.message_id
        WHERE NOT EXISTS (
            SELECT 1
            FROM tapbacks t
            WHERE t.message_id = mm.prod_id
              AND t.part_index = st.part_index
              AND t.kind = st.kind
              AND t.emoji IS NOT DISTINCT FROM st.emoji
              AND t.is_from_me = st.is_from_me
              AND t.sender_handle_id IS NOT DISTINCT FROM st.sender_handle_id
        )
        ",
    )
    .execute(&mut *conn)
    .await?
    .rows_affected())
}

#[cfg(test)]
mod tests;
