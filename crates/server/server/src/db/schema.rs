//! Schema management for the database.
//!
//! Serve and import open their database connections through
//! [`crate::db::engine`] pools (shared pragmas) and ensure the
//! schema with `ensure_schema` / `ensure_accounts_schema`. DDL lives in
//! the SQL files embedded at compile time (`schema/sql/*.sql`); the
//! functions here apply and evolve it.
//!
//! The embedded SQL is fingerprinted at compile time ([`SCHEMA_FINGERPRINT`])
//! and the fingerprint is stamped into the database as `PRAGMA
//! user_version`. The rule is: any schema change requires a fresh reload of data,
//! so a database stamped with a different fingerprint is rebuilt empty from
//! the embedded DDL instead of being patched in place. Nothing is bumped by
//! hand; changing a `schema/sql/*.sql` file is the whole of a schema change.

use anyhow::Result;
use sqlx::AnyConnection;

/// Baseline DDL lives in `schema/sql/`. Every column there carries a comment;
/// `tests/schema_column_comments.rs` enforces it.
const ACCOUNTS_DDL: &str = include_str!("../../../../../schema/sql/accounts.sql");
const MESSAGE_TABLES_DDL: &str = include_str!("../../../../../schema/sql/messages.sql");
const STAGING_TABLES_DDL: &str = include_str!("../../../../../schema/sql/staging.sql");
const CONTACTS_TABLES_DDL: &str = include_str!("../../../../../schema/sql/contacts.sql");
const SAVED_SEARCHES_DDL: &str = include_str!("../../../../../schema/sql/saved_searches.sql");
const FTS_VIRTUAL_DDL: &str = include_str!("../../../../../schema/sql/fts_virtual.sql");
const DROP_MESSAGES_FTS_TRIGGERS_SQL: &str =
    include_str!("../../../../../schema/sql/fts_triggers_drop.sql");
const CREATE_MESSAGES_FTS_TRIGGERS_SQL: &str =
    include_str!("../../../../../schema/sql/fts_triggers_create.sql");

/// Fingerprint of the embedded schema: a 31-bit FNV-1a hash over every
/// `schema/sql/*.sql` file, computed at compile time. It is stamped into each
/// database as `PRAGMA user_version`; a database carrying any other value is
/// rebuilt empty (see [`migrate_schema`]).
///
/// A hash rather than a hand-kept number so that a schema change is only a
/// change to the SQL: nothing to bump, nothing to forget. 31 bits because
/// `user_version` is a signed 32-bit integer.
pub const SCHEMA_FINGERPRINT: i64 = schema_fingerprint();

// Fits `user_version`, and is not one of the small hand-kept numbers the old
// scheme stamped, so a database from that scheme is rebuilt rather than mistaken
// for current.
const _: () = assert!(SCHEMA_FINGERPRINT > 1000 && SCHEMA_FINGERPRINT <= i32::MAX as i64);

const fn schema_fingerprint() -> i64 {
    const FILES: [&str; 8] = [
        ACCOUNTS_DDL,
        MESSAGE_TABLES_DDL,
        STAGING_TABLES_DDL,
        CONTACTS_TABLES_DDL,
        SAVED_SEARCHES_DDL,
        FTS_VIRTUAL_DDL,
        DROP_MESSAGES_FTS_TRIGGERS_SQL,
        CREATE_MESSAGES_FTS_TRIGGERS_SQL,
    ];
    fingerprint_of(&FILES)
}

/// The fingerprint of `files`, in order: what [`schema_fingerprint`] computes
/// over the embedded SQL, apart so a test can hand it other text.
const fn fingerprint_of(files: &[&str]) -> i64 {
    // FNV-1a, 32-bit. Each file is followed by a zero byte so that moving
    // text across a file boundary changes the hash.
    let mut hash: u32 = 0x811c_9dc5;
    let mut f = 0;
    while f < files.len() {
        let bytes = files[f].as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            hash ^= bytes[i] as u32;
            hash = hash.wrapping_mul(0x0100_0193);
            i += 1;
        }
        hash = hash.wrapping_mul(0x0100_0193);
        f += 1;
    }
    (hash & 0x7fff_ffff) as i64
}

/// Bring the database to [`SCHEMA_FINGERPRINT`].
///
/// A database already stamped with the current fingerprint is left
/// untouched. Anything else — a fresh file, a pre-fingerprint database, or one
/// stamped by a server with different SQL — is rebuilt empty and stamped;
/// the user re-imports afterwards.
///
/// The only kind of migration is a full rebuild: schema changes require a
/// fresh reload of data, never in-place column patches.
async fn migrate_schema(conn: &mut AnyConnection) -> Result<()> {
    let stamped = user_version(conn).await?;
    if stamped == SCHEMA_FINGERPRINT {
        return Ok(());
    }
    if has_user_tables(conn).await? {
        tracing::warn!(
            stamped = %stamped,
            expected = %SCHEMA_FINGERPRINT,
            "database schema differs from this server's; rebuilding empty (re-import your data)"
        );
    }
    rebuild_schema(conn).await?;
    stamp_user_version(conn, SCHEMA_FINGERPRINT).await?;
    Ok(())
}

/// The `user_version` pragma value stamped by [`migrate_schema`].
async fn user_version(conn: &mut AnyConnection) -> Result<i64> {
    Ok(sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *conn)
        .await?)
}

/// Record the schema fingerprint in SQLite's `user_version` pragma.
async fn stamp_user_version(conn: &mut AnyConnection, version: i64) -> Result<()> {
    sqlx::query(&format!("PRAGMA user_version = {version}"))
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Whether any user table exists. A fresh file has none, so a first run stays
/// quiet instead of warning about a rebuild.
async fn has_user_tables(conn: &mut AnyConnection) -> Result<bool> {
    let tables: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
    )
    .fetch_one(&mut *conn)
    .await?;
    Ok(tables > 0)
}

/// Drop every user table and recreate the current schema from the embedded
/// DDL. This is the only kind of migration: schema changes require a fresh
/// reload of data, never in-place column patches.
///
/// Foreign keys are turned OFF for the drop loop: SQLite's FK-aware DROP
/// processing cannot handle a schema whose remaining CREATE statements still
/// reference already-dropped tables ("no such table: main.<dropped>"). The
/// constraints themselves have `ON DELETE` actions, so the drops would
/// cascade cleanly; this is a schema-parse limitation, not a data one.
async fn rebuild_schema(conn: &mut AnyConnection) -> Result<()> {
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&mut *conn)
        .await?;
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
    )
    .fetch_all(&mut *conn)
    .await?;
    // `IF EXISTS` keeps this safe when an FTS table's shadow tables were
    // already removed with their parent.
    for table in &tables {
        sqlx::query(&format!("DROP TABLE IF EXISTS {}", quote_ident(table)))
            .execute(&mut *conn)
            .await?;
    }
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&mut *conn)
        .await?;
    apply_ddl(conn).await?;
    Ok(())
}

/// Apply the current embedded DDL: accounts, contacts, messages, staging,
/// then the FTS index and its sync triggers.
async fn apply_ddl(conn: &mut AnyConnection) -> Result<()> {
    execute_batch(conn, ACCOUNTS_DDL).await?;
    // Contacts DDL defines `handles`, the FK target of conversations, participants,
    // messages, and tapbacks (messages.sql) plus account_handles (accounts.sql).
    // Apply it before the tables that reference handles.
    execute_batch(conn, CONTACTS_TABLES_DDL).await?;
    execute_batch(conn, MESSAGE_TABLES_DDL).await?;
    execute_batch(conn, STAGING_TABLES_DDL).await?;
    execute_batch(conn, SAVED_SEARCHES_DDL).await?;
    ensure_messages_fts(conn).await?;
    Ok(())
}

/// Quote `name` as a SQL identifier: wrapped in double quotes, with any
/// double quote inside it doubled.
/// Every name that reaches a `DROP TABLE` here came out of a catalog or out
/// of the embedded DDL, and this is the one place that turns such a name
/// into statement text.
fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Create every table and index required by a current database.
///
/// The database carries the fingerprint in `PRAGMA user_version` and is
/// rebuilt when it does not match.
///
/// # Errors
///
/// Returns an error when a DDL statement fails.
pub async fn ensure_schema(conn: &mut AnyConnection) -> Result<()> {
    migrate_schema(conn).await
}

/// Marker that current full-text search (FTS) sync trigger definitions are installed.
pub const MESSAGES_FTS_TRIGGERS_META_KEY: &str = "messages_fts_triggers_v1";

/// Full-text search index over message body/subject plus attachment text:
/// a contentless FTS5 virtual table with sync triggers.
async fn ensure_messages_fts(conn: &mut AnyConnection) -> Result<()> {
    execute_batch(conn, FTS_VIRTUAL_DDL).await?;

    let triggers_ready: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM schema_meta WHERE key = $1")
        .bind(MESSAGES_FTS_TRIGGERS_META_KEY)
        .fetch_one(&mut *conn)
        .await?;
    if triggers_ready == 0 {
        install_messages_fts_triggers(conn).await?;
    }

    Ok(())
}

/// Drop full-text search sync triggers (used during bulk promote so inserts skip
/// per-row indexing).
///
/// # Errors
///
/// Returns an error when the drop statements fail.
pub(crate) async fn drop_messages_fts_triggers(conn: &mut AnyConnection) -> Result<()> {
    execute_batch(conn, DROP_MESSAGES_FTS_TRIGGERS_SQL).await?;
    sqlx::query("DELETE FROM schema_meta WHERE key = $1")
        .bind(MESSAGES_FTS_TRIGGERS_META_KEY)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Install full-text search sync triggers and mark them ready in `schema_meta`.
/// The triggers are dropped first, so installing twice is safe.
///
/// # Errors
///
/// Returns an error when the trigger SQL or metadata write fails.
pub(crate) async fn install_messages_fts_triggers(conn: &mut AnyConnection) -> Result<()> {
    execute_batch(conn, DROP_MESSAGES_FTS_TRIGGERS_SQL).await?;
    execute_batch(conn, CREATE_MESSAGES_FTS_TRIGGERS_SQL).await?;
    sqlx::query(
        "INSERT INTO schema_meta (key, value) VALUES ($1, '1')
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(MESSAGES_FTS_TRIGGERS_META_KEY)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Non-unique indexes on `messages` (kept out of bulk promote inserts, then rebuilt).
/// Unique `ix_messages_account_source_guid` stays in place for `INSERT OR IGNORE` dedup.
const MESSAGES_SECONDARY_INDEX_DDL: &[(&str, &str)] = &[
    (
        "ix_messages_conversation_timestamp",
        "CREATE INDEX IF NOT EXISTS ix_messages_conversation_timestamp ON messages (conversation_id, timestamp)",
    ),
    (
        "ix_messages_conversation_source_timestamp",
        "CREATE INDEX IF NOT EXISTS ix_messages_conversation_source_timestamp ON messages (conversation_id, source, timestamp)",
    ),
    (
        "ix_messages_account_id",
        "CREATE INDEX IF NOT EXISTS ix_messages_account_id ON messages (account_id)",
    ),
    (
        "ix_messages_content_key",
        "CREATE INDEX IF NOT EXISTS ix_messages_content_key ON messages (content_key) WHERE content_key IS NOT NULL AND content_key != ''",
    ),
    (
        "ix_messages_duplicate_of",
        "CREATE INDEX IF NOT EXISTS ix_messages_duplicate_of ON messages (duplicate_of) WHERE duplicate_of IS NOT NULL",
    ),
    (
        "ix_messages_import_id",
        "CREATE INDEX IF NOT EXISTS ix_messages_import_id ON messages (import_id) WHERE import_id IS NOT NULL",
    ),
    (
        "ix_messages_source",
        "CREATE INDEX IF NOT EXISTS ix_messages_source ON messages (source)",
    ),
];

/// Drop secondary `messages` indexes during bulk promote (same transaction as
/// the promote inserts).
pub(crate) async fn drop_messages_secondary_indexes(conn: &mut AnyConnection) -> Result<()> {
    for (name, _) in MESSAGES_SECONDARY_INDEX_DDL {
        sqlx::query(&format!("DROP INDEX IF EXISTS {name}"))
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

/// Recreate secondary `messages` indexes after bulk promote inserts.
pub(crate) async fn create_messages_secondary_indexes(conn: &mut AnyConnection) -> Result<()> {
    for (_, ddl) in MESSAGES_SECONDARY_INDEX_DDL {
        sqlx::query(ddl).execute(&mut *conn).await?;
    }
    Ok(())
}

/// Bulk-index promoted messages (joined via temp `_promote_msg_map`).
/// Call after attachment rows exist so `attachment_text` is complete.
/// Inserts into the contentless `messages_fts` table.
///
/// `_promote_msg_map` also targets messages that already existed before this
/// promotion (so attachments and tapbacks can attach to them), and several
/// staging rows can point at one production row. Those rows are already
/// indexed, so `min_new_message_id`, the highest `messages.id` that existed
/// before this promotion inserted anything, keeps them out: only distinct
/// production ids above it are indexed here.
pub(crate) async fn index_messages_fts_from_promote_map(
    conn: &mut AnyConnection,
    min_new_message_id: i64,
) -> Result<u64> {
    let n = sqlx::query(
        r"
        INSERT INTO messages_fts(rowid, body, subject, attachment_text)
        SELECT
            m.id,
            coalesce(m.body, ''),
            coalesce(m.subject, ''),
            coalesce((
                SELECT group_concat(
                    trim(coalesce(a.original_name, '') || ' ' || coalesce(a.transcription, '')),
                    ' '
                )
                FROM attachments a
                WHERE a.message_id = m.id
            ), '')
        FROM (
            SELECT DISTINCT prod_id FROM _promote_msg_map WHERE prod_id > $1
        ) mm
        JOIN messages m ON m.id = mm.prod_id
        ",
    )
    .bind(min_new_message_id)
    .execute(&mut *conn)
    .await?;
    Ok(n.rows_affected())
}

/// Message ids for one source within one account, bound as `$1` = source, `$2` = account.
const MESSAGE_IDS_FOR_SOURCE: &str = "SELECT m.id FROM messages m \
     JOIN conversations c ON c.id = m.conversation_id \
     WHERE m.source = $1 AND c.account_id = $2";

/// Delete all production messages (and cascaded rows) for one import source within one account.
///
/// # Errors
///
/// Returns an error when a delete or update statement fails.
pub async fn delete_messages_for_source(
    conn: &mut AnyConnection,
    account_id: i64,
    source: &str,
) -> Result<u64> {
    sqlx::query(&format!(
        "DELETE FROM attachments WHERE message_id IN ({MESSAGE_IDS_FOR_SOURCE})"
    ))
    .bind(source)
    .bind(account_id)
    .execute(&mut *conn)
    .await?;
    sqlx::query(&format!(
        "DELETE FROM tapbacks WHERE message_id IN ({MESSAGE_IDS_FOR_SOURCE})"
    ))
    .bind(source)
    .bind(account_id)
    .execute(&mut *conn)
    .await?;
    sqlx::query(&format!(
        "UPDATE messages SET duplicate_of = NULL
         WHERE duplicate_of IN ({MESSAGE_IDS_FOR_SOURCE})"
    ))
    .bind(source)
    .bind(account_id)
    .execute(&mut *conn)
    .await?;
    let n = sqlx::query(
        r"
        DELETE FROM messages
        WHERE source = $1
          AND conversation_id IN (
              SELECT id FROM conversations WHERE account_id = $2
          )
        ",
    )
    .bind(source)
    .bind(account_id)
    .execute(&mut *conn)
    .await?;
    Ok(n.rows_affected())
}

/// Create current account and server metadata tables.
///
/// Account tables live in the same database file as everything else, so
/// the one `user_version` stamp covers them. A stamped database needs
/// nothing; anything else gets the full schema (with the rebuild that
/// implies).
///
/// # Errors
///
/// Returns an error when a DDL statement fails.
pub async fn ensure_accounts_schema(conn: &mut AnyConnection) -> Result<()> {
    if user_version(conn).await? != SCHEMA_FINGERPRINT {
        ensure_schema(conn).await?;
    }
    Ok(())
}

/// True when `table` exists. Used by [`crate::process_assets::run`] to skip
/// the account sweep on a database that has no schema yet.
pub async fn table_exists(conn: &mut AnyConnection, name: &str) -> Result<bool> {
    let found: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = $1")
            .bind(name)
            .fetch_one(&mut *conn)
            .await?;
    Ok(found > 0)
}

/// Column names of `table` in ordinal order.
#[cfg(test)]
async fn table_columns(conn: &mut AnyConnection, table: &str) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar("SELECT name FROM pragma_table_info($1)")
        .bind(table)
        .fetch_all(&mut *conn)
        .await?)
}

/// True when an index named `name` exists.
#[cfg(test)]
async fn index_exists(conn: &mut AnyConnection, name: &str) -> Result<bool> {
    let found: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = $1")
            .bind(name)
            .fetch_one(&mut *conn)
            .await?;
    Ok(found > 0)
}

/// True when a trigger named `name` exists.
#[cfg(test)]
async fn trigger_exists(conn: &mut AnyConnection, name: &str) -> Result<bool> {
    let found: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND name = $1",
    )
    .bind(name)
    .fetch_one(&mut *conn)
    .await?;
    Ok(found > 0)
}

/// Split a multi-statement DDL batch into individual statements.
///
/// The schema files follow a fixed format: comments are whole `--` lines,
/// ordinary statements end with `;` at end of line, and the only multi-line
/// statements are trigger bodies, which end in a line ending with `END;`.
pub fn split_ddl(batch: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut in_trigger = false;
    for line in batch.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("--") {
            continue;
        }
        if trimmed.starts_with("CREATE TRIGGER") {
            in_trigger = true;
        }
        current.push_str(line);
        current.push('\n');
        if in_trigger {
            if trimmed.ends_with("END;") {
                statements.push(current.trim_end().to_string());
                current.clear();
                in_trigger = false;
            }
        } else if trimmed.ends_with(';') {
            statements.push(current.trim_end().to_string());
            current.clear();
        }
    }
    debug_assert!(
        current.trim().is_empty(),
        "unterminated DDL statement: {current}"
    );
    statements
}

/// Run every statement in a DDL batch against one connection.
async fn execute_batch(conn: &mut AnyConnection, batch: &str) -> Result<()> {
    for stmt in split_ddl(batch) {
        sqlx::query(&stmt).execute(&mut *conn).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
