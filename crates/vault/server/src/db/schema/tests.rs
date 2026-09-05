use super::*;
use crate::db::engine::test_pool;
use crate::test_support::{SeedConversation, TestVault, seed_conversation, test_vault};

const A1: &str = "11111111-1111-1111-1111-111111111111";
const A2: &str = "22222222-2222-2222-2222-222222222222";

async fn insert_message(conn: &mut AnyConnection, id: i64, guid: &str, body: &str) {
    sqlx::query(
        r"
        INSERT INTO messages (
            id, conversation_id, account_id, source, guid,
            timestamp, is_from_me, sort_order, body
        ) VALUES ($1, 1, $2, 'imessage', $3, '2020-01-01T00:00:00Z', 0, 0, $4)
        ",
    )
    .bind(id)
    .bind(A1)
    .bind(guid)
    .bind(body)
    .execute(&mut *conn)
    .await
    .unwrap();
}

async fn conversation_id(conn: &mut AnyConnection, account: &str) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT id FROM conversations WHERE account_id = $1")
        .bind(account)
        .fetch_one(&mut *conn)
        .await
        .unwrap()
}

/// Column names of `table`, for contract assertions.
async fn column_names(conn: &mut AnyConnection, table: &str) -> Vec<String> {
    table_columns(conn, table).await.unwrap()
}

async fn fts_hits(conn: &mut AnyConnection, term: &str) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM messages_fts WHERE messages_fts MATCH $1")
        .bind(term)
        .fetch_one(&mut *conn)
        .await
        .unwrap()
}

/// Search hits via the Postgres `search_tsv` vector (`messages_fts` has no
/// Postgres twin — the tsvector lives on `messages`).
async fn pg_fts_hits(conn: &mut AnyConnection, term: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM messages WHERE search_tsv @@ plainto_tsquery('simple', $1)",
    )
    .bind(term)
    .fetch_one(&mut *conn)
    .await
    .unwrap()
}

/// A vault with schema applied, and two accounts (`A1`/alice, `A2`/bob)
/// each holding one individual conversation on `+15555550100` from
/// `t.json`, with no messages.
async fn seeded_schema_vault() -> (sqlx::AnyPool, TestVault) {
    let vault = test_vault().await;
    for (id, user) in [(A1, "alice"), (A2, "bob")] {
        vault.account_with_id(id, user).await;
        seed_conversation(
            &vault.state,
            &SeedConversation {
                account_id: id,
                handle: "+15555550100",
                conversation_type: "individual",
                group_title: None,
                source_file: "t.json",
                messages: &[],
            },
        )
        .await;
    }
    let pool = vault.state.db.clone();
    (pool, vault)
}

#[tokio::test]
async fn promote_fts_indexing_covers_only_rows_inserted_by_this_promotion() {
    if crate::test_support::on_postgres() {
        return; // SQLite-only: asserts against the FTS5 table; the Postgres twin is promote_fts_cycle_pg
    }
    let (pool, _vault) = seeded_schema_vault().await;
    let mut conn = pool.acquire().await.unwrap();

    // An earlier import already indexed this row through the insert trigger.
    insert_message(&mut conn, 10, "g-existing", "carriedover").await;
    let max_id_before_promote: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) FROM messages")
            .fetch_one(&mut *conn)
            .await
            .unwrap();

    drop_messages_fts_triggers(&mut conn).await.unwrap();
    insert_message(&mut conn, 11, "g-new", "freshbody").await;
    // Append promotion maps existing GUIDs (so child rows find their parent)
    // alongside newly inserted rows, and one production row can be the target
    // of more than one staging row.
    execute_batch(
        &mut conn,
        r"
        CREATE TEMP TABLE _promote_msg_map (
            staging_id INTEGER PRIMARY KEY,
            prod_id INTEGER NOT NULL
        );
        INSERT INTO _promote_msg_map (staging_id, prod_id) VALUES (1, 10), (2, 11), (3, 11);
        ",
    )
    .await
    .unwrap();

    let indexed = index_messages_fts_from_promote_map(&mut conn, max_id_before_promote)
        .await
        .unwrap();
    install_messages_fts_triggers(&mut conn).await.unwrap();

    assert_eq!(
        indexed, 1,
        "only rows inserted by this promotion may be indexed"
    );
    for (term, expected) in [("carriedover", 1), ("freshbody", 1)] {
        let hits: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM messages_fts WHERE messages_fts MATCH $1")
                .bind(term)
                .fetch_one(&mut *conn)
                .await
                .unwrap();
        assert_eq!(hits, expected, "unexpected match count for {term}");
    }
}

#[tokio::test]
async fn fresh_vault_has_complete_current_schema() {
    if crate::test_support::on_postgres() {
        return; // SQLite-only: the contract lists SQLite objects such as messages_fts
    }
    let (pool, _dir) = test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    ensure_vault_schema(&mut conn).await.unwrap();
    assert_current_schema_contract(&mut conn).await;
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION, "a fresh vault is stamped at once");
    // Ensuring again on a current vault is a no-op.
    ensure_vault_schema(&mut conn).await.unwrap();
    assert_current_schema_contract(&mut conn).await;
}

/// Assert every table, index, trigger, metadata marker, and column the
/// current schema contract lists is present.
async fn assert_current_schema_contract(conn: &mut AnyConnection) {
    let contract: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../../tests/fixtures/schema/current-schema.json"
    ))
    .unwrap();

    for table in contract["tables"].as_array().unwrap() {
        let table = table.as_str().unwrap();
        assert!(
            table_exists(conn, table).await.unwrap(),
            "missing table {table}"
        );
    }
    for index in contract["indexes"].as_array().unwrap() {
        let index = index.as_str().unwrap();
        assert!(
            index_exists(conn, index).await.unwrap(),
            "missing index {index}"
        );
    }
    for trigger in contract["triggers"].as_array().unwrap() {
        let trigger = trigger.as_str().unwrap();
        assert!(
            trigger_exists(conn, trigger).await.unwrap(),
            "missing trigger {trigger}"
        );
    }
    for key in contract["metadata"].as_array().unwrap() {
        let key = key.as_str().unwrap();
        let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM schema_meta WHERE key = $1")
            .bind(key)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
        assert!(exists > 0, "missing runtime metadata {key}");
    }

    assert_eq!(
        column_names(&mut *conn, "accounts").await,
        [
            "id",
            "username",
            "password_hash",
            "preferred_name",
            "time_zone",
            "is_admin",
            "disabled",
            "can_import",
            "can_export",
            "can_delete"
        ]
    );
    assert_eq!(
        column_names(&mut *conn, "contacts").await,
        [
            "id",
            "account_id",
            "preferred_name",
            "origin",
            "created_at",
            "last_modified"
        ]
    );
    assert_eq!(
        column_names(&mut *conn, "contact_groups").await,
        ["id", "account_id", "name", "kind"]
    );
    assert_eq!(
        column_names(&mut *conn, "contact_group_members").await,
        ["contact_id", "group_id"]
    );
    assert_eq!(
        column_names(&mut *conn, "handles").await,
        [
            "id",
            "account_id",
            "raw",
            "normalized",
            "normalized_note",
            "handle_type",
            "service",
            "origin",
            "created_at",
            "last_modified"
        ]
    );
    assert!(
        column_names(&mut *conn, "conversations")
            .await
            .iter()
            .any(|c| c == "chat_handle_id")
    );
    for column in ["account_id", "source", "content_key", "duplicate_of"] {
        assert!(
            column_names(&mut *conn, "messages")
                .await
                .iter()
                .any(|c| c == column)
        );
    }
    assert!(
        column_names(&mut *conn, "staging_messages")
            .await
            .iter()
            .any(|c| c == "account_id")
    );
    assert!(
        column_names(&mut *conn, "attachments")
            .await
            .iter()
            .any(|c| c == "size_bytes")
    );
    assert!(
        column_names(&mut *conn, "attachments")
            .await
            .iter()
            .any(|c| c == "missing_reason")
    );
    assert!(
        column_names(&mut *conn, "staging_attachments")
            .await
            .iter()
            .any(|c| c == "size_bytes")
    );
    assert!(
        column_names(&mut *conn, "staging_attachments")
            .await
            .iter()
            .any(|c| c == "missing_reason")
    );
}

#[tokio::test]
async fn same_source_guid_allowed_across_accounts() {
    let (pool, _vault) = seeded_schema_vault().await;
    let mut conn = pool.acquire().await.unwrap();
    for (conv, account) in [
        (conversation_id(&mut conn, A1).await, A1),
        (conversation_id(&mut conn, A2).await, A2),
    ] {
        sqlx::query(
            r"
            INSERT INTO messages (
                conversation_id, account_id, source, guid, timestamp, is_from_me, sort_order
            ) VALUES ($1, $2, 'sms', 'same-guid', '2020-01-01T00:00:00Z', 0, 0)
            ",
        )
        .bind(conv)
        .bind(account)
        .execute(&mut *conn)
        .await
        .unwrap();
    }
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE guid = 'same-guid'")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn reset_staging_for_account_leaves_other_accounts() {
    let (pool, _vault) = seeded_schema_vault().await;
    let mut conn = pool.acquire().await.unwrap();
    for account in [A1, A2] {
        let conversation_id: i64 = sqlx::query_scalar(
            r"
            INSERT INTO staging_conversations (
                account_id, chat_handle_id, conversation_type,
                group_title, exported_at, source_file
            ) VALUES ($1, 1, 'individual', NULL, NULL, 't.json')
            RETURNING id
            ",
        )
        .bind(account)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
        sqlx::query(
            r"
            INSERT INTO staging_messages (
                conversation_id, account_id, source, guid, timestamp, is_from_me, sort_order
            ) VALUES ($1, $2, 'sms', 'g1', '2020-01-01T00:00:00Z', 0, 0)
            ",
        )
        .bind(conversation_id)
        .bind(account)
        .execute(&mut *conn)
        .await
        .unwrap();
    }

    reset_staging_for_account(&mut conn, A1).await.unwrap();
    let remaining: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM staging_conversations WHERE account_id = $1")
            .bind(A2)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    let messages: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM staging_messages")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(remaining, 1);
    assert_eq!(messages, 1);
}

#[tokio::test]
async fn old_vault_rebuilds_empty_at_current_version() {
    if crate::test_support::on_postgres() {
        return; // SQLite-only: builds a legacy SQLite file with its nocase collation and user_version
    }
    let (pool, _dir) = test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    // A pre-versioning vault from the pre-groups era: contact_labels
    // tables, no user_version stamp.
    execute_batch(
        &mut conn,
        include_str!("../../../../../../tests/fixtures/schema/v0-vault.sql"),
    )
    .await
    .unwrap();
    sqlx::query("INSERT INTO accounts (id, username) VALUES ($1, 'alice')")
        .bind(A1)
        .execute(&mut *conn)
        .await
        .unwrap();
    let contact_id: i64 = sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name) VALUES ($1, 'Ada') RETURNING id",
    )
    .bind(A1)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let label_id: i64 = sqlx::query_scalar(
        "INSERT INTO contact_labels (account_id, name) VALUES ($1, 'Family') RETURNING id",
    )
    .bind(A1)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    sqlx::query("INSERT INTO contact_label_members (contact_id, label_id) VALUES ($1, $2)")
        .bind(contact_id)
        .bind(label_id)
        .execute(&mut *conn)
        .await
        .unwrap();

    ensure_vault_schema(&mut conn).await.unwrap();

    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION, "old vault must be stamped current");
    assert!(!table_exists(&mut conn, "contact_labels").await.unwrap());
    assert!(
        !table_exists(&mut conn, "contact_label_members")
            .await
            .unwrap()
    );
    assert_current_schema_contract(&mut conn).await;
    let accounts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    let contacts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM contacts")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(accounts, 0, "rebuild drops old vault data");
    assert_eq!(contacts, 0, "rebuild drops old vault data");
}

#[tokio::test]
async fn current_version_vault_keeps_data_across_reensure() {
    let (pool, _vault) = seeded_schema_vault().await;
    let mut conn = pool.acquire().await.unwrap();
    ensure_vault_schema(&mut conn).await.unwrap();
    let accounts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(
        accounts, 2,
        "re-ensuring a current vault must not wipe data"
    );
}

#[tokio::test]
async fn newer_version_vault_rebuilds_to_current() {
    if crate::test_support::on_postgres() {
        return; // SQLite-only: reads PRAGMA user_version; stale_postgres_marker_rebuilds_vault_schema_empty is the twin
    }
    let (pool, _vault) = seeded_schema_vault().await;
    let mut conn = pool.acquire().await.unwrap();
    stamp_user_version(&mut conn, SCHEMA_VERSION + 1)
        .await
        .unwrap();
    ensure_vault_schema(&mut conn).await.unwrap();
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION, "downgrade rebuilds at current");
    let accounts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(accounts, 0, "downgrade rebuild drops data");
}

#[tokio::test]
async fn one_running_import_per_account() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    ensure_vault_schema(&mut conn).await.unwrap();
    sqlx::query("INSERT INTO accounts (id, username) VALUES ('acct', 'alice')")
        .execute(&mut *conn)
        .await
        .unwrap();

    let insert = r"
        INSERT INTO vault_imports (
            account_id, source, mode, status, started_at,
            message_count, attachment_count, bytes_uploaded
        ) VALUES ('acct', 'imessage', 'append', $1, '2026-08-30T00:00:00Z', 0, 0, 0)
    ";

    sqlx::query(insert)
        .bind("running")
        .execute(&mut *conn)
        .await
        .expect("first running session inserts");

    let second = sqlx::query(insert)
        .bind("running")
        .execute(&mut *conn)
        .await;
    assert!(second.is_err(), "a second running session must be rejected");

    // A finished session does not occupy the slot.
    sqlx::query(insert)
        .bind("completed")
        .execute(&mut *conn)
        .await
        .expect("a completed session is not covered by the partial index");
}

#[tokio::test]
async fn vault_imports_carries_the_session_columns() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    ensure_vault_schema(&mut conn).await.unwrap();
    sqlx::query(
        "SELECT stage, staging_dir, device_id, form_json, source_fingerprint
         FROM vault_imports WHERE 1 = 0",
    )
    .fetch_optional(&mut *conn)
    .await
    .expect("session columns exist");
}

#[tokio::test]
async fn fresh_accounts_default_to_full_permissions() {
    let (pool, _dir) = test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    ensure_accounts_schema(&mut conn).await.unwrap();
    sqlx::query("INSERT INTO accounts (id, username) VALUES ($1, 'fresh')")
        .bind(A1)
        .execute(&mut *conn)
        .await
        .unwrap();
    let row: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT is_admin, can_import, can_export, can_delete FROM accounts WHERE id = $1",
    )
    .bind(A1)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(row, (0, 1, 1, 1));
}

#[tokio::test]
async fn messages_fts_stays_in_sync() {
    if crate::test_support::on_postgres() {
        return; // SQLite-only: queries the FTS5 table with MATCH; messages_fts_stays_in_sync_pg is the twin
    }
    let (pool, _vault) = seeded_schema_vault().await;
    let mut conn = pool.acquire().await.unwrap();
    let conversation_id: i64 =
        sqlx::query_scalar("SELECT id FROM conversations WHERE account_id = $1")
            .bind(A1)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    let message_id: i64 = sqlx::query_scalar(
        r"
        INSERT INTO messages (
            conversation_id, account_id, source, guid, timestamp,
            is_from_me, sort_order, body, subject
        ) VALUES ($1, $2, 'sms', 'g1', '2020-01-01T00:00:00Z', 0, 0, 'hello vault', NULL)
        RETURNING id
        ",
    )
    .bind(conversation_id)
    .bind(A1)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO attachments (message_id, original_name, transcription) VALUES ($1, 'voice.m4a', 'secret phrase')",
    )
    .bind(message_id)
    .execute(&mut *conn)
    .await
    .unwrap();

    assert_eq!(fts_hits(&mut conn, "vault").await, 1);
    assert_eq!(fts_hits(&mut conn, "secret").await, 1);

    sqlx::query("UPDATE messages SET body = 'goodbye' WHERE id = $1")
        .bind(message_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    assert_eq!(fts_hits(&mut conn, "vault").await, 0);
    assert_eq!(fts_hits(&mut conn, "goodbye").await, 1);

    sqlx::query("DELETE FROM attachments WHERE message_id = $1")
        .bind(message_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query("DELETE FROM messages WHERE id = $1")
        .bind(message_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    assert_eq!(fts_hits(&mut conn, "goodbye").await, 0);
}

/// The `messages_fts_stays_in_sync` twin for Postgres: the sync triggers
/// keep `search_tsv` in step with message and attachment edits. Skips
/// unless `MV_TEST_POSTGRES_URL` is set.
#[tokio::test]
async fn messages_fts_stays_in_sync_pg() {
    let Some(url) = crate::pg_test_url() else {
        return;
    };
    let pool = crate::db::engine::pg_test_schema_pool(&url).await;
    let mut conn = pool.acquire().await.unwrap();
    ensure_vault_schema(&mut conn).await.unwrap();

    // One account + conversation, mirroring the SQLite test's setup.
    sqlx::query("INSERT INTO accounts (id, username) VALUES ($1, 'alice')")
        .bind(A1)
        .execute(&mut *conn)
        .await
        .unwrap();
    let handle_id: i64 = sqlx::query_scalar(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES ($1, '+15555550100', '+15555550100', 'phone', 'phone')
         RETURNING id",
    )
    .bind(A1)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let conversation_id: i64 = sqlx::query_scalar(
        r"
        INSERT INTO conversations (
            account_id, chat_handle_id, conversation_type,
            group_title, exported_at, source_file
        ) VALUES ($1, $2, 'individual', NULL, NULL, 't.json')
        RETURNING id
        ",
    )
    .bind(A1)
    .bind(handle_id)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let message_id: i64 = sqlx::query_scalar(
        r"
        INSERT INTO messages (
            conversation_id, account_id, source, guid, timestamp,
            is_from_me, sort_order, body, subject
        ) VALUES ($1, $2, 'sms', 'g1', '2020-01-01T00:00:00Z', 0, 0, 'hello vault', NULL)
        RETURNING id
        ",
    )
    .bind(conversation_id)
    .bind(A1)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO attachments (message_id, original_name, transcription) VALUES ($1, 'voice.m4a', 'secret phrase')",
    )
    .bind(message_id)
    .execute(&mut *conn)
    .await
    .unwrap();

    assert_eq!(pg_fts_hits(&mut conn, "vault").await, 1);
    assert_eq!(pg_fts_hits(&mut conn, "secret").await, 1);

    sqlx::query("UPDATE messages SET body = 'goodbye' WHERE id = $1")
        .bind(message_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    assert_eq!(pg_fts_hits(&mut conn, "vault").await, 0);
    assert_eq!(pg_fts_hits(&mut conn, "goodbye").await, 1);

    sqlx::query("DELETE FROM attachments WHERE message_id = $1")
        .bind(message_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query("DELETE FROM messages WHERE id = $1")
        .bind(message_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    assert_eq!(pg_fts_hits(&mut conn, "goodbye").await, 0);
}

/// The `old_vault_rebuilds_empty_at_current_version` twin for Postgres: a
/// vault stamped with a stale [`VAULT_SCHEMA_META_KEY`] is rebuilt empty
/// by [`drop_pg_user_tables`] rather than patched in place, so the new
/// session columns land on an already-installed vault too. Skips unless
/// `MV_TEST_POSTGRES_URL` is set.
#[tokio::test]
async fn stale_postgres_marker_rebuilds_vault_schema_empty() {
    let Some(url) = crate::pg_test_url() else {
        return;
    };
    let pool = crate::db::engine::pg_test_schema_pool(&url).await;
    let mut conn = pool.acquire().await.unwrap();
    ensure_vault_schema(&mut conn).await.unwrap();
    sqlx::query("INSERT INTO accounts (id, username) VALUES ($1, 'alice')")
        .bind(A1)
        .execute(&mut *conn)
        .await
        .unwrap();

    // Roll the marker back to what a vault installed before this schema
    // change would carry, simulating the upgrade scenario the rebuild
    // path exists for.
    sqlx::query("DELETE FROM schema_meta WHERE key = $1")
        .bind(VAULT_SCHEMA_META_KEY)
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO schema_meta (key, value) VALUES ('vault_schema_v1', '1')
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .execute(&mut *conn)
    .await
    .unwrap();

    ensure_vault_schema(&mut conn).await.unwrap();

    let accounts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(accounts, 0, "a stale marker rebuilds the vault empty");

    let ready: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM schema_meta WHERE key = $1")
        .bind(VAULT_SCHEMA_META_KEY)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(ready, 1, "the rebuild stamps the current marker");

    sqlx::query(
        "SELECT stage, staging_dir, device_id, form_json, source_fingerprint
         FROM vault_imports WHERE 1 = 0",
    )
    .fetch_optional(&mut *conn)
    .await
    .expect("the rebuilt Postgres vault carries the session columns");
}

/// A vault sharing its Postgres schema with another application rebuilds
/// its own tables and leaves the neighbour's alone. Skips unless
/// `MV_TEST_POSTGRES_URL` is set.
#[tokio::test]
async fn postgres_rebuild_spares_tables_the_vault_does_not_own() {
    let Some(url) = crate::pg_test_url() else {
        return;
    };
    let pool = crate::db::engine::pg_test_schema_pool(&url).await;
    let mut conn = pool.acquire().await.unwrap();
    ensure_vault_schema(&mut conn).await.unwrap();

    // A co-tenant application's table, sitting in the same schema.
    sqlx::query("CREATE TABLE mv_test_neighbour (id BIGINT PRIMARY KEY, note TEXT)")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query("INSERT INTO mv_test_neighbour (id, note) VALUES (1, 'keep me')")
        .execute(&mut *conn)
        .await
        .unwrap();

    // Roll the marker back so the next ensure takes the rebuild path.
    sqlx::query("DELETE FROM schema_meta WHERE key = $1")
        .bind(VAULT_SCHEMA_META_KEY)
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO schema_meta (key, value) VALUES ('vault_schema_v1', '1')
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .execute(&mut *conn)
    .await
    .unwrap();

    ensure_vault_schema(&mut conn).await.unwrap();

    let note: Option<String> =
        sqlx::query_scalar("SELECT note FROM mv_test_neighbour WHERE id = 1")
            .fetch_optional(&mut *conn)
            .await
            .expect("a table the vault does not own survives the rebuild")
            .flatten();
    assert_eq!(
        note.as_deref(),
        Some("keep me"),
        "the neighbour's rows survive the rebuild too"
    );

    // The vault's own tables were still rebuilt.
    let ready: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM schema_meta WHERE key = $1")
        .bind(VAULT_SCHEMA_META_KEY)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(ready, 1, "the rebuild stamps the current marker");
}

/// The drop list is read out of the embedded DDL, so it covers every
/// table the vault installs and nothing else.
#[test]
fn pg_vault_table_names_match_the_embedded_ddl() {
    let names = pg_vault_table_names();
    for expected in [
        "accounts",
        "account_api_tokens",
        "schema_meta",
        "vault_imports",
        "vault_import_issues",
        "contacts",
        "handles",
        "trashed_conversations",
        "conversations",
        "messages",
        "attachments",
        "message_tags",
        "staging_messages",
    ] {
        assert!(
            names.contains(&expected),
            "{expected} missing from {names:?}"
        );
    }
    let declared = pg_vault_table_ddl()
        .files
        .iter()
        .flat_map(|ddl| ddl.lines())
        .filter(|line| line.trim_start().starts_with("CREATE TABLE"))
        .count();
    assert_eq!(
        names.len(),
        declared,
        "every CREATE TABLE in the Postgres DDL is on the drop list"
    );
}

#[test]
fn split_ddl_keeps_trigger_bodies_intact() {
    let create = include_str!("../../../../../../schema/sql/fts_triggers_create.sql");
    let drop = include_str!("../../../../../../schema/sql/fts_triggers_drop.sql");
    let fts = include_str!("../../../../../../schema/sql/fts_virtual.sql");
    assert_eq!(split_ddl(create).len(), 6, "six sync triggers");
    assert_eq!(split_ddl(drop).len(), 6);
    assert_eq!(split_ddl(fts).len(), 1);
    for stmt in split_ddl(create) {
        assert!(
            stmt.starts_with("CREATE TRIGGER"),
            "unexpected split: {stmt}"
        );
    }
    // A statement is never empty and never ends mid-line.
    for stmt in split_ddl(include_str!("../../../../../../schema/sql/messages.sql")) {
        assert!(stmt.ends_with(';'), "statement must end with ;: {stmt}");
        assert!(stmt.starts_with("CREATE"), "unexpected split: {stmt}");
    }
}

#[test]
fn split_ddl_skips_comments_and_blanks() {
    let out = split_ddl("-- header\nCREATE TABLE a (x INTEGER);\n\nCREATE TABLE b (y INTEGER);\n");
    assert_eq!(
        out,
        vec!["CREATE TABLE a (x INTEGER);", "CREATE TABLE b (y INTEGER);"]
    );
}

#[test]
fn split_ddl_keeps_do_blocks_intact() {
    let fks = &pg_vault_table_ddl().deferred_fks;
    let stmts = split_ddl(fks);
    assert_eq!(stmts.len(), 1, "the deferred FKs must be one DO block");
    assert!(
        stmts[0].starts_with("DO $$"),
        "unexpected split: {}",
        stmts[0]
    );
    assert!(stmts[0].ends_with("$$;"), "DO block must end in $$;");
}

#[test]
fn split_ddl_keeps_pg_function_bodies_intact() {
    let ddl = include_str!("../../../../../../schema/sql/fts_postgres.sql");
    let stmts = split_ddl(ddl);
    // Column + GIN index + two sync functions + six one-line triggers.
    assert_eq!(stmts.len(), 10, "unexpected split of fts_postgres.sql");
    let mut functions = 0;
    let mut triggers = 0;
    for stmt in &stmts {
        if stmt.starts_with("CREATE OR REPLACE FUNCTION") {
            functions += 1;
            assert!(
                stmt.ends_with("$$ LANGUAGE plpgsql;"),
                "function must end in $$ LANGUAGE plpgsql;: {stmt}"
            );
        } else if stmt.starts_with("CREATE TRIGGER") {
            triggers += 1;
            assert!(
                stmt.ends_with("EXECUTE FUNCTION messages_fts_sync();")
                    || stmt.ends_with("EXECUTE FUNCTION attachments_fts_sync();"),
                "unexpected split: {stmt}"
            );
        } else {
            assert!(stmt.ends_with(';'), "statement must end with ;: {stmt}");
        }
    }
    assert_eq!(functions, 2);
    assert_eq!(triggers, 6);
    let drop = split_ddl(include_str!(
        "../../../../../../schema/sql/fts_postgres_drop.sql"
    ));
    assert_eq!(drop.len(), 6, "six sync triggers to drop");
    for stmt in drop {
        assert!(
            stmt.starts_with("DROP TRIGGER IF EXISTS"),
            "unexpected split: {stmt}"
        );
    }
}
