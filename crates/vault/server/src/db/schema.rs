//! Schema management for the vault and accounts databases.
//!
//! Serve and import open their database connections through
//! [`crate::db::engine`] pools (shared pragmas for SQLite) and ensure the
//! schema with `ensure_vault_schema` / `ensure_accounts_schema`. DDL lives in
//! the SQL files embedded at compile time; the functions here apply and
//! evolve it. SQLite and Postgres each have their own DDL variants
//! (`schema/sql/*.sql` and `schema/sql/pg_*.sql`).
//!
//! Schema changes are versioned with `PRAGMA user_version` on SQLite (see
//! [`SCHEMA_VERSION`]). The rule is: any schema change requires a fresh
//! reload of data, so an out-of-date database is rebuilt empty from the
//! embedded DDL instead of being patched in place. Postgres has no
//! `user_version` pragma; its idempotent DDL (`IF NOT EXISTS`) runs once
//! behind a `schema_meta` marker gate (see [`VAULT_SCHEMA_META_KEY`]).

use anyhow::Result;
use sqlx::AnyConnection;
use sqlx::Connection;

use crate::db::dialect;
use crate::db::engine::DbEngine;

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
/// Postgres FTS twin of `FTS_VIRTUAL_DDL` + `CREATE_MESSAGES_FTS_TRIGGERS_SQL`:
/// the `search_tsv` column, GIN index, sync functions, and triggers (all
/// idempotent).
const FTS_POSTGRES_DDL: &str = include_str!("../../../../../schema/sql/fts_postgres.sql");
const DROP_MESSAGES_FTS_TRIGGERS_PG_SQL: &str =
    include_str!("../../../../../schema/sql/fts_postgres_drop.sql");

/// Current vault schema version, stamped into each SQLite database with
/// `PRAGMA user_version`. Bump this whenever any `schema/sql/*.sql` file
/// changes; a database at any other version is rebuilt empty (see
/// [`migrate_vault_schema`]).
pub const SCHEMA_VERSION: i64 = 12;

/// Bring the database to [`SCHEMA_VERSION`].
///
/// A database already at the current version is left untouched. Anything else
/// — a fresh file, a pre-versioning vault, or one stamped by a different
/// server — is rebuilt empty and stamped; the user re-imports afterwards.
///
/// The only kind of migration is a full rebuild: schema changes require a
/// fresh reload of data, never in-place column patches.
async fn migrate_vault_schema(conn: &mut AnyConnection) -> Result<()> {
    let version = user_version(conn).await?;
    if version == SCHEMA_VERSION {
        return Ok(());
    }
    if version > SCHEMA_VERSION {
        eprintln!(
            "warning: vault schema is version {version}, newer than this server's {SCHEMA_VERSION}; rebuilding empty (re-import your data)"
        );
        rebuild_vault_schema(conn).await?;
    } else {
        if has_user_tables(conn).await? {
            eprintln!(
                "warning: vault schema is version {version}; rebuilding empty at version {SCHEMA_VERSION} (re-import your data)"
            );
        }
        rebuild_vault_schema(conn).await?;
    }
    stamp_user_version(conn, SCHEMA_VERSION).await?;
    Ok(())
}

/// The `user_version` pragma value stamped by [`migrate_vault_schema`].
async fn user_version(conn: &mut AnyConnection) -> Result<i64> {
    Ok(sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *conn)
        .await?)
}

/// Record the schema version in SQLite's `user_version` pragma.
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
async fn rebuild_vault_schema(conn: &mut AnyConnection) -> Result<()> {
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
    apply_vault_ddl(conn).await?;
    Ok(())
}

/// Apply the current embedded DDL: accounts, contacts, messages, staging,
/// then the FTS index and its sync triggers.
async fn apply_vault_ddl(conn: &mut AnyConnection) -> Result<()> {
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

/// The Postgres DDL that creates the vault's own tables, in the order the
/// vault installs it — transpiled from the SQLite originals (see
/// [`crate::db::pg_ddl`]). The installer, the rebuild's drop list, and the
/// drift guard all read this one value, so a DDL file cannot reach one of
/// them and miss the others.
fn pg_vault_table_ddl() -> &'static crate::db::pg_ddl::PgDdl {
    static DDL: std::sync::OnceLock<crate::db::pg_ddl::PgDdl> = std::sync::OnceLock::new();
    DDL.get_or_init(|| {
        crate::db::pg_ddl::transpile(&[
            ACCOUNTS_DDL,
            // Contacts before messages: the messages DDL references contact tables.
            CONTACTS_TABLES_DDL,
            MESSAGE_TABLES_DDL,
            STAGING_TABLES_DDL,
            SAVED_SEARCHES_DDL,
        ])
    })
}

/// Every table name the embedded Postgres DDL creates, as the transpiler
/// collected them while producing that DDL, so the rebuild's drop list
/// cannot drift from what the vault installs.
///
/// A SQLite database file belongs to the vault alone, but a Postgres schema
/// may be shared with another application. The rebuild therefore names the
/// vault's own tables instead of sweeping `current_schema()`.
fn pg_vault_table_names() -> Vec<&'static str> {
    pg_vault_table_ddl()
        .tables
        .iter()
        .map(String::as_str)
        .collect()
}

/// Quote `name` as a SQL identifier: wrapped in double quotes, with any
/// double quote inside it doubled. SQLite and Postgres both read this form.
/// Every name that reaches a `DROP TABLE` here came out of a catalog or out
/// of the embedded DDL, and this is the one place that turns such a name
/// into statement text.
fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Drop the vault's own tables in the current schema. Postgres twin of
/// [`rebuild_vault_schema`]: a vault stamped with an older marker is
/// rebuilt empty rather than patched in place.
///
/// Only the tables [`pg_vault_table_names`] lists are dropped, and each is
/// schema-qualified, so a vault sharing its schema with another application
/// rebuilds its own data without touching the neighbour's.
///
/// `CASCADE` takes the FTS triggers and foreign keys down with their
/// tables; the sync functions are recreated with `CREATE OR REPLACE`.
async fn drop_pg_user_tables(conn: &mut AnyConnection) -> Result<()> {
    // `::text` because the Any driver has no mapping for Postgres's `name`.
    let schema: String = sqlx::query_scalar("SELECT current_schema()::text")
        .fetch_one(&mut *conn)
        .await?;
    let schema = quote_ident(&schema);
    for table in pg_vault_table_names() {
        sqlx::query(&format!(
            "DROP TABLE IF EXISTS {schema}.{} CASCADE",
            quote_ident(table)
        ))
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Apply the Postgres DDL variants. The DDL is idempotent (`IF NOT EXISTS`),
/// so applying it again is a no-op.
async fn apply_postgres_vault_ddl(conn: &mut AnyConnection) -> Result<()> {
    // Installed vaults skip straight past this (one marker lookup per
    // request instead of re-running the DDL batch).
    if pg_vault_schema_ready(&mut *conn).await? {
        return Ok(());
    }
    // One-time install: the advisory lock serializes concurrent
    // first-touches (the trigger drop/create pair is not race-safe), and
    // the re-check under the lock turns a waiter into a no-op.
    let mut tx = conn.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(VAULT_SCHEMA_LOCK_ID)
        .execute(&mut *tx)
        .await?;
    if !pg_vault_schema_ready(&mut tx).await? {
        // A vault carrying an older marker (or none, with tables present)
        // is rebuilt empty — the same contract SQLite's user_version
        // gives. Re-importing is the migration.
        if table_exists(&mut tx, "vault_imports").await? {
            eprintln!(
                "warning: vault schema predates {VAULT_SCHEMA_META_KEY}; rebuilding empty (re-import your data)"
            );
            drop_pg_user_tables(&mut tx).await?;
        }
        // Same ordering as the SQLite variant: contacts before messages.
        for ddl in &pg_vault_table_ddl().files {
            execute_batch(&mut tx, ddl).await?;
        }
        // Post-hoc FKs last: they reference tables created across the DDL
        // sequence (see `pg_ddl` rule 4).
        execute_batch(&mut tx, &pg_vault_table_ddl().deferred_fks).await?;
        // FTS last, same as the SQLite variant: the tsvector column, GIN
        // index, and sync triggers all target tables created above.
        ensure_messages_fts(&mut tx).await?;
        sqlx::query(
            "INSERT INTO schema_meta (key, value) VALUES ($1, '1')
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(VAULT_SCHEMA_META_KEY)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// True when the one-time Postgres DDL marker is present. Also false when
/// `schema_meta` itself does not exist yet (pre-install).
async fn pg_vault_schema_ready(conn: &mut AnyConnection) -> Result<bool> {
    if !table_exists(&mut *conn, "schema_meta").await? {
        return Ok(false);
    }
    let ready: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM schema_meta WHERE key = $1")
        .bind(VAULT_SCHEMA_META_KEY)
        .fetch_one(&mut *conn)
        .await?;
    Ok(ready > 0)
}

/// Create every table and index required by a current vault.
///
/// SQLite is versioned with `PRAGMA user_version` and rebuilt when the stamp
/// does not match; Postgres gates its one-time idempotent install behind a
/// `schema_meta` marker (see [`VAULT_SCHEMA_META_KEY`]) so repeated ensures
/// cost one marker lookup instead of re-running the DDL.
///
/// # Errors
///
/// Returns an error when a DDL statement fails.
pub async fn ensure_vault_schema(conn: &mut AnyConnection) -> Result<()> {
    if dialect::engine_of(conn) == DbEngine::Postgres {
        return apply_postgres_vault_ddl(conn).await;
    }
    migrate_vault_schema(conn).await
}

/// Marker that current full-text search (FTS) sync trigger definitions are installed.
pub const MESSAGES_FTS_TRIGGERS_META_KEY: &str = "messages_fts_triggers_v1";

/// Marker that the one-time Postgres vault DDL install has completed.
/// Bumped with the schema: a vault holding an older marker is rebuilt
/// empty, matching SQLite's `user_version` behaviour.
pub const VAULT_SCHEMA_META_KEY: &str = "vault_schema_v4";

/// Advisory lock id serializing the one-time Postgres DDL install so two
/// concurrent first-touches cannot interleave the trigger drop/create pair
/// (arbitrary but unique within this database).
const VAULT_SCHEMA_LOCK_ID: i64 = 0x4D56_0001;

/// Full-text search index over message body/subject plus attachment text:
/// contentless FTS5 virtual table with sync triggers on SQLite, a `search_tsv`
/// tsvector column with GIN index and sync triggers on Postgres.
async fn ensure_messages_fts(conn: &mut AnyConnection) -> Result<()> {
    if dialect::engine_of(conn) == DbEngine::Postgres {
        // Postgres has no `CREATE TRIGGER IF NOT EXISTS`, so installing means
        // dropping the six sync triggers and recreating them. That may only
        // run when the marker says they are missing: every schema ensure
        // (each import's reset_staging_for_account) would otherwise drop and
        // recreate the triggers behind a concurrent writer, a silent desync
        // window for rows written in between. install_messages_fts_triggers
        // writes the marker, drop_messages_fts_triggers deletes it.
        let triggers_ready: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM schema_meta WHERE key = $1")
                .bind(MESSAGES_FTS_TRIGGERS_META_KEY)
                .fetch_one(&mut *conn)
                .await?;
        if triggers_ready == 0 {
            install_messages_fts_triggers(conn).await?;
        }
        return Ok(());
    }
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
/// per-row indexing). On Postgres this is the drop half of the trigger install
/// (the promote path disables triggers instead — see
/// [`disable_fts_triggers_pg`]).
///
/// # Errors
///
/// Returns an error when the drop statements fail.
pub(crate) async fn drop_messages_fts_triggers(conn: &mut AnyConnection) -> Result<()> {
    if dialect::engine_of(conn) == DbEngine::Postgres {
        execute_batch(conn, DROP_MESSAGES_FTS_TRIGGERS_PG_SQL).await?;
    } else {
        execute_batch(conn, DROP_MESSAGES_FTS_TRIGGERS_SQL).await?;
    }
    sqlx::query("DELETE FROM schema_meta WHERE key = $1")
        .bind(MESSAGES_FTS_TRIGGERS_META_KEY)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Install full-text search sync triggers and mark them ready in `schema_meta`.
/// On Postgres the trigger statements are made idempotent by dropping first,
/// exactly like the SQLite path.
///
/// # Errors
///
/// Returns an error when the trigger SQL or metadata write fails.
pub(crate) async fn install_messages_fts_triggers(conn: &mut AnyConnection) -> Result<()> {
    if dialect::engine_of(conn) == DbEngine::Postgres {
        execute_batch(conn, DROP_MESSAGES_FTS_TRIGGERS_PG_SQL).await?;
        execute_batch(conn, FTS_POSTGRES_DDL).await?;
    } else {
        execute_batch(conn, DROP_MESSAGES_FTS_TRIGGERS_SQL).await?;
        execute_batch(conn, CREATE_MESSAGES_FTS_TRIGGERS_SQL).await?;
    }
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

/// Disable the six Postgres FTS sync triggers by name during bulk promote, so
/// per-row FTS sync work is skipped (Postgres has no per-statement "don't run
/// triggers" mode; SQLite drops its FTS triggers instead — see
/// [`drop_messages_fts_triggers`]). Only the FTS triggers are touched: FK
/// constraint triggers stay enabled, so a staging row that violates a foreign
/// key still fails loudly, and the statements need only table ownership (no
/// superuser). The bulk vector fill runs afterwards via
/// [`index_messages_fts_from_promote_map`], then
/// [`enable_fts_triggers_pg`] restores the triggers. Disabling and re-enabling
/// are transactional, so a failed promote rolls the disable back.
///
/// # Errors
///
/// Returns an error when a disable statement fails.
pub(crate) async fn disable_fts_triggers_pg(conn: &mut AnyConnection) -> Result<()> {
    sqlx::query("ALTER TABLE messages DISABLE TRIGGER messages_fts_ai")
        .execute(&mut *conn)
        .await?;
    sqlx::query("ALTER TABLE messages DISABLE TRIGGER messages_fts_au")
        .execute(&mut *conn)
        .await?;
    sqlx::query("ALTER TABLE messages DISABLE TRIGGER messages_fts_ad")
        .execute(&mut *conn)
        .await?;
    sqlx::query("ALTER TABLE attachments DISABLE TRIGGER attachments_fts_ai")
        .execute(&mut *conn)
        .await?;
    sqlx::query("ALTER TABLE attachments DISABLE TRIGGER attachments_fts_ad")
        .execute(&mut *conn)
        .await?;
    sqlx::query("ALTER TABLE attachments DISABLE TRIGGER attachments_fts_au")
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Re-enable the six Postgres FTS sync triggers disabled by
/// [`disable_fts_triggers_pg`], by the same names.
///
/// # Errors
///
/// Returns an error when an enable statement fails.
pub(crate) async fn enable_fts_triggers_pg(conn: &mut AnyConnection) -> Result<()> {
    sqlx::query("ALTER TABLE messages ENABLE TRIGGER messages_fts_ai")
        .execute(&mut *conn)
        .await?;
    sqlx::query("ALTER TABLE messages ENABLE TRIGGER messages_fts_au")
        .execute(&mut *conn)
        .await?;
    sqlx::query("ALTER TABLE messages ENABLE TRIGGER messages_fts_ad")
        .execute(&mut *conn)
        .await?;
    sqlx::query("ALTER TABLE attachments ENABLE TRIGGER attachments_fts_ai")
        .execute(&mut *conn)
        .await?;
    sqlx::query("ALTER TABLE attachments ENABLE TRIGGER attachments_fts_ad")
        .execute(&mut *conn)
        .await?;
    sqlx::query("ALTER TABLE attachments ENABLE TRIGGER attachments_fts_au")
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Bulk-index promoted messages (joined via temp `_promote_msg_map`).
/// Call after attachment rows exist so `attachment_text` is complete.
/// SQLite inserts into the contentless `messages_fts` table; Postgres fills
/// the `messages.search_tsv` tsvector instead.
///
/// `_promote_msg_map` also targets messages that already existed before this
/// promotion (so attachments and tapbacks can attach to them), and several
/// staging rows can point at one production row. `messages_fts` stores no
/// copy of the message text, so re-indexing an already indexed row writes
/// extra index entries that a later delete does not fully retract.
/// `min_new_message_id` is the highest `messages.id` that existed before this
/// promotion inserted anything; only distinct production ids above it are
/// indexed here.
pub(crate) async fn index_messages_fts_from_promote_map(
    conn: &mut AnyConnection,
    min_new_message_id: i64,
) -> Result<u64> {
    if dialect::engine_of(conn) == DbEngine::Postgres {
        let n = sqlx::query(
            r"
            UPDATE messages SET search_tsv = fts.vec
            FROM (
                SELECT mm.prod_id,
                       to_tsvector('simple',
                           coalesce(m.body, '') || ' ' || coalesce(m.subject, '') || ' ' || coalesce(a.attachment_text, '')) AS vec
                FROM (SELECT DISTINCT prod_id FROM _promote_msg_map WHERE prod_id > $1) mm
                JOIN messages m ON m.id = mm.prod_id
                LEFT JOIN (
                    SELECT message_id,
                           string_agg(trim(coalesce(original_name, '') || ' ' || coalesce(transcription, '')), ' ') AS attachment_text
                    FROM attachments
                    GROUP BY message_id
                ) a ON a.message_id = mm.prod_id
            ) fts
            WHERE messages.id = fts.prod_id
            ",
        )
        .bind(min_new_message_id)
        .execute(&mut *conn)
        .await?;
        return Ok(n.rows_affected());
    }
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
    account_id: &str,
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

/// Clear one account's staging rows (the temporary import area). Child rows
/// are removed by CASCADE. Other accounts are untouched.
///
/// # Errors
///
/// Returns an error when schema setup or the delete fails.
pub async fn reset_staging_for_account(conn: &mut AnyConnection, account_id: &str) -> Result<()> {
    ensure_vault_schema(conn).await?;
    sqlx::query("DELETE FROM staging_conversations WHERE account_id = $1")
        .bind(account_id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Create current account and vault metadata tables.
///
/// Account tables live in the same database file as the rest of the vault, so
/// the one `user_version` stamp covers them on SQLite. A stamped database
/// needs nothing; anything else gets the full vault schema (with the rebuild
/// that implies). On Postgres the one-time DDL install is gated by the
/// [`VAULT_SCHEMA_META_KEY`] marker.
///
/// # Errors
///
/// Returns an error when a DDL statement fails.
pub async fn ensure_accounts_schema(conn: &mut AnyConnection) -> Result<()> {
    if dialect::engine_of(conn) == DbEngine::Postgres {
        return ensure_vault_schema(conn).await;
    }
    if user_version(conn).await? != SCHEMA_VERSION {
        ensure_vault_schema(conn).await?;
    }
    Ok(())
}

/// True when `table` exists on this engine.
///
/// Branches on the engine: `pg_catalog.pg_tables` for Postgres, `sqlite_master`
/// for SQLite. Used by [`crate::process_assets::run`] to skip the account
/// sweep on a database that has no vault schema yet.
///
/// The Postgres lookup is restricted to `current_schema()` — the schema the
/// vault reads, writes, and rebuilds — so a same-named table in another
/// schema of the same database never stands in for the vault's own.
pub async fn table_exists(conn: &mut AnyConnection, name: &str) -> Result<bool> {
    let found: i64 = if dialect::engine_of(conn) == DbEngine::Postgres {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM pg_catalog.pg_tables
             WHERE tablename = $1 AND schemaname = current_schema()",
        )
        .bind(name)
        .fetch_one(&mut *conn)
        .await?
    } else {
        sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = $1")
            .bind(name)
            .fetch_one(&mut *conn)
            .await?
    };
    Ok(found > 0)
}

/// Column names of `table` in ordinal order.
#[cfg(test)]
async fn table_columns(conn: &mut AnyConnection, table: &str) -> Result<Vec<String>> {
    if dialect::engine_of(conn) == DbEngine::Postgres {
        return Ok(sqlx::query_scalar(
            "SELECT column_name FROM information_schema.columns
             WHERE table_name = $1 ORDER BY ordinal_position",
        )
        .bind(table)
        .fetch_all(&mut *conn)
        .await?);
    }
    Ok(sqlx::query_scalar("SELECT name FROM pragma_table_info($1)")
        .bind(table)
        .fetch_all(&mut *conn)
        .await?)
}

/// True when an index named `name` exists on this engine.
#[cfg(test)]
async fn index_exists(conn: &mut AnyConnection, name: &str) -> Result<bool> {
    let found: i64 = if dialect::engine_of(conn) == DbEngine::Postgres {
        sqlx::query_scalar("SELECT COUNT(*) FROM pg_catalog.pg_indexes WHERE indexname = $1")
            .bind(name)
            .fetch_one(&mut *conn)
            .await?
    } else {
        sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = $1")
            .bind(name)
            .fetch_one(&mut *conn)
            .await?
    };
    Ok(found > 0)
}

/// True when a trigger named `name` exists on this engine.
#[cfg(test)]
async fn trigger_exists(conn: &mut AnyConnection, name: &str) -> Result<bool> {
    let found: i64 = if dialect::engine_of(conn) == DbEngine::Postgres {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM information_schema.triggers WHERE trigger_name = $1",
        )
        .bind(name)
        .fetch_one(&mut *conn)
        .await?
    } else {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND name = $1",
        )
        .bind(name)
        .fetch_one(&mut *conn)
        .await?
    };
    Ok(found > 0)
}

/// Split a multi-statement DDL batch into individual statements.
///
/// The schema files follow a fixed format: comments are whole `--` lines,
/// ordinary statements end with `;` at end of line, and the only multi-line
/// statements are trigger bodies (ending in a line ending with `END;`, or
/// ending on the same line they start), Postgres `DO $$` blocks (ending in a
/// line ending with `$$;`), and Postgres `CREATE OR REPLACE FUNCTION … AS $$`
/// blocks (ending in a line that starts with `$$`).
pub fn split_ddl(batch: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut in_trigger = false;
    let mut in_do_block = false;
    let mut in_function = false;
    for line in batch.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("--") {
            continue;
        }
        let starts_trigger = trimmed.starts_with("CREATE TRIGGER");
        let starts_do_block = trimmed.starts_with("DO $$");
        let starts_function = trimmed.starts_with("CREATE OR REPLACE FUNCTION");
        if starts_trigger {
            in_trigger = true;
        }
        if starts_do_block {
            in_do_block = true;
        }
        if starts_function {
            in_function = true;
        }
        current.push_str(line);
        current.push('\n');
        if in_trigger {
            // Multi-line trigger bodies end with `END;`; a one-line trigger
            // (e.g. `EXECUTE FUNCTION`) ends with `;` on its own line.
            if trimmed.ends_with("END;") || (starts_trigger && trimmed.ends_with(';')) {
                statements.push(current.trim_end().to_string());
                current.clear();
                in_trigger = false;
            }
        } else if in_do_block {
            if trimmed.ends_with("$$;") {
                statements.push(current.trim_end().to_string());
                current.clear();
                in_do_block = false;
            }
        } else if in_function {
            // The body's closing delimiter is its own `$$` line, e.g.
            // `$$ LANGUAGE plpgsql;`.
            if trimmed.starts_with("$$") && trimmed.ends_with(';') {
                statements.push(current.trim_end().to_string());
                current.clear();
                in_function = false;
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
