use super::*;
use crate::db::engine::test_pool;
use crate::test_support::{SeedConversation, TestFixture, seed_conversation, test_fixture};

const A1: i64 = 7;
const A2: i64 = 8;

async fn insert_message(conn: &mut SqliteConnection, id: i64, guid: &str, body: &str) {
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

async fn conversation_id(conn: &mut SqliteConnection, account: i64) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT id FROM conversations WHERE account_id = $1")
        .bind(account)
        .fetch_one(&mut *conn)
        .await
        .unwrap()
}

/// Column names of `table`, for contract assertions.
async fn column_names(conn: &mut SqliteConnection, table: &str) -> Vec<String> {
    table_columns(conn, table).await.unwrap()
}

async fn fts_hits(conn: &mut SqliteConnection, term: &str) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM messages_fts WHERE messages_fts MATCH $1")
        .bind(term)
        .fetch_one(&mut *conn)
        .await
        .unwrap()
}

/// A fixture with schema applied, and two accounts (`A1`/alice, `A2`/bob)
/// each holding one individual conversation on `+15555550100` from
/// `t.json`, with no messages.
async fn seeded_schema_fixture() -> (sqlx::SqlitePool, TestFixture) {
    let fixture = test_fixture().await;
    for (id, user) in [(A1, "alice"), (A2, "bob")] {
        fixture.account_with_id(id, user).await;
        seed_conversation(
            &fixture.state,
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
    let pool = fixture.state.db.clone();
    (pool, fixture)
}

#[tokio::test]
async fn promote_fts_indexing_covers_only_rows_inserted_by_this_promotion() {
    let (pool, _fixture) = seeded_schema_fixture().await;
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

    let indexed = index_messages_fts_from_promote_map(&mut conn, max_id_before_promote, 0)
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
async fn promote_fts_indexing_reindexes_an_existing_message_that_gained_an_attachment() {
    let (pool, _fixture) = seeded_schema_fixture().await;
    let mut conn = pool.acquire().await.unwrap();

    // Two messages an earlier import indexed through the insert trigger.
    insert_message(&mut conn, 10, "g-gains", "gainsbody").await;
    insert_message(&mut conn, 12, "g-keeps", "keepsbody").await;
    let max_id_before_promote: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) FROM messages")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    let max_attachment_before_promote: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) FROM attachments")
            .fetch_one(&mut *conn)
            .await
            .unwrap();

    // An append maps both, and adds an attachment to message 10 only.
    drop_messages_fts_triggers(&mut conn).await.unwrap();
    execute_batch(
        &mut conn,
        r"
        CREATE TEMP TABLE _promote_msg_map (
            staging_id INTEGER PRIMARY KEY,
            prod_id INTEGER NOT NULL
        );
        INSERT INTO _promote_msg_map (staging_id, prod_id) VALUES (1, 10), (2, 12);
        INSERT INTO attachments (message_id, original_name) VALUES (10, 'lateinvoice.pdf');
        ",
    )
    .await
    .unwrap();

    let indexed = index_messages_fts_from_promote_map(
        &mut conn,
        max_id_before_promote,
        max_attachment_before_promote,
    )
    .await
    .unwrap();
    install_messages_fts_triggers(&mut conn).await.unwrap();

    assert_eq!(
        indexed, 1,
        "only the existing message that gained an attachment is indexed again"
    );
    for (term, expected) in [("lateinvoice", 1), ("gainsbody", 1), ("keepsbody", 1)] {
        assert_eq!(
            fts_hits(&mut conn, term).await,
            expected,
            "unexpected match count for {term}"
        );
    }
}

#[tokio::test]
async fn fresh_database_has_complete_current_schema() {
    let (pool, _dir) = test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    ensure_schema(&mut conn).await.unwrap();
    assert_current_schema_contract(&mut conn).await;
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(
        version, SCHEMA_FINGERPRINT,
        "a fresh database is stamped at once"
    );
    // Ensuring again on a current database is a no-op.
    ensure_schema(&mut conn).await.unwrap();
    assert_current_schema_contract(&mut conn).await;
}

/// Assert every table, index, trigger, metadata marker, and column the
/// current schema contract lists is present.
async fn assert_current_schema_contract(conn: &mut SqliteConnection) {
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
            "disabled",
            "must_set_up_profile",
            "can_import",
            "can_export",
            "can_delete",
            "last_login_at"
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
    let (pool, _fixture) = seeded_schema_fixture().await;
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
async fn old_database_rebuilds_empty_at_current_version() {
    let (pool, _dir) = test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    // A pre-versioning database from the pre-groups era: contact_labels
    // tables, no user_version stamp.
    execute_batch(
        &mut conn,
        include_str!("../../../../../../tests/fixtures/schema/v0-schema.sql"),
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

    ensure_schema(&mut conn).await.unwrap();

    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(
        version, SCHEMA_FINGERPRINT,
        "old database must be stamped current"
    );
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
    assert_eq!(accounts, 0, "rebuild drops old database data");
    assert_eq!(contacts, 0, "rebuild drops old database data");
}

#[tokio::test]
async fn current_version_database_keeps_data_across_reensure() {
    let (pool, _fixture) = seeded_schema_fixture().await;
    let mut conn = pool.acquire().await.unwrap();
    ensure_schema(&mut conn).await.unwrap();
    let accounts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(
        accounts, 2,
        "re-ensuring a current database must not wipe data"
    );
}

/// A database stamped by a server built from different SQL — newer or older,
/// the fingerprint does not say which — is rebuilt empty at this one.
#[tokio::test]
async fn other_fingerprint_rebuilds_to_current() {
    let (pool, _fixture) = seeded_schema_fixture().await;
    let mut conn = pool.acquire().await.unwrap();
    stamp_user_version(&mut conn, SCHEMA_FINGERPRINT ^ 1)
        .await
        .unwrap();
    ensure_schema(&mut conn).await.unwrap();
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(
        version, SCHEMA_FINGERPRINT,
        "rebuilt at this server's fingerprint"
    );
    let accounts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(accounts, 0, "the rebuild drops data");
}

#[tokio::test]
async fn one_running_import_per_account() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    ensure_schema(&mut conn).await.unwrap();
    sqlx::query("INSERT INTO accounts (id, username) VALUES (7, 'alice')")
        .execute(&mut *conn)
        .await
        .unwrap();

    let insert = r"
        INSERT INTO imports (
            account_id, source, mode, status, started_at,
            message_count, attachment_count, bytes_uploaded
        ) VALUES (7, 'imessage', 'append', $1, '2026-08-30T00:00:00Z', 0, 0, 0)
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
async fn messages_fts_stays_in_sync() {
    let (pool, _fixture) = seeded_schema_fixture().await;
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
        ) VALUES ($1, $2, 'sms', 'g1', '2020-01-01T00:00:00Z', 0, 0, 'hello there', NULL)
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

    assert_eq!(fts_hits(&mut conn, "there").await, 1);
    assert_eq!(fts_hits(&mut conn, "secret").await, 1);

    sqlx::query("UPDATE messages SET body = 'goodbye' WHERE id = $1")
        .bind(message_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    assert_eq!(fts_hits(&mut conn, "there").await, 0);
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

/// How many index entries `term` has, read from the index itself. `MATCH`
/// only answers for rows the index still counts as present, so it cannot
/// show a term left behind under a deleted row.
async fn fts_term_entries(conn: &mut SqliteConnection, term: &str) -> i64 {
    sqlx::query("CREATE VIRTUAL TABLE IF NOT EXISTS temp.fts_vocab USING fts5vocab(main, messages_fts, row)")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query_scalar("SELECT COALESCE(SUM(doc), 0) FROM temp.fts_vocab WHERE term = $1")
        .bind(term)
        .fetch_one(&mut *conn)
        .await
        .unwrap()
}

/// An attachment's name and transcription leave the index when the
/// attachment is renamed, when it is removed from a message that stays,
/// and when its message is deleted with the attachment still on it (the
/// delete cascades, which is how a conversation is deleted). SQLite gives
/// the next message the id of the newest deleted one, so a term left
/// behind would make that message match a name it never carried.
#[tokio::test]
async fn messages_fts_forgets_attachment_text_that_is_gone() {
    let (pool, _fixture) = seeded_schema_fixture().await;
    let mut conn = pool.acquire().await.unwrap();
    let conversation_id: i64 =
        sqlx::query_scalar("SELECT id FROM conversations WHERE account_id = $1")
            .bind(A1)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    let insert_message = r"
        INSERT INTO messages (
            conversation_id, account_id, source, guid, timestamp,
            is_from_me, sort_order, body, subject
        ) VALUES ($1, $2, 'sms', $3, '2020-01-01T00:00:00Z', 0, 0, $4, NULL)
        RETURNING id
        ";
    let message_id: i64 = sqlx::query_scalar(insert_message)
        .bind(conversation_id)
        .bind(A1)
        .bind("g-att")
        .bind("zzbody text")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    for (name, transcription) in [("zzfirst.m4a", "zzspoken words"), ("zzsecond.pdf", "")] {
        sqlx::query(
            "INSERT INTO attachments (message_id, original_name, transcription) VALUES ($1, $2, $3)",
        )
        .bind(message_id)
        .bind(name)
        .bind(transcription)
        .execute(&mut *conn)
        .await
        .unwrap();
    }
    for term in ["zzbody", "zzfirst", "zzspoken", "zzsecond"] {
        assert_eq!(fts_hits(&mut conn, term).await, 1, "{term}");
        assert_eq!(fts_term_entries(&mut conn, term).await, 1, "{term}");
    }

    sqlx::query("UPDATE attachments SET original_name = 'zzrenamed.m4a' WHERE original_name = 'zzfirst.m4a'")
        .execute(&mut *conn)
        .await
        .unwrap();
    assert_eq!(fts_hits(&mut conn, "zzfirst").await, 0);
    assert_eq!(fts_term_entries(&mut conn, "zzfirst").await, 0);
    for term in ["zzbody", "zzrenamed", "zzspoken", "zzsecond"] {
        assert_eq!(fts_hits(&mut conn, term).await, 1, "{term}");
    }

    sqlx::query("DELETE FROM attachments WHERE original_name = 'zzrenamed.m4a'")
        .execute(&mut *conn)
        .await
        .unwrap();
    for term in ["zzrenamed", "zzspoken"] {
        assert_eq!(fts_hits(&mut conn, term).await, 0, "{term}");
        assert_eq!(fts_term_entries(&mut conn, term).await, 0, "{term}");
    }
    for term in ["zzbody", "zzsecond"] {
        assert_eq!(fts_hits(&mut conn, term).await, 1, "{term}");
    }

    sqlx::query("UPDATE messages SET body = 'zzedited text' WHERE id = $1")
        .bind(message_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    assert_eq!(fts_term_entries(&mut conn, "zzbody").await, 0);
    assert_eq!(fts_hits(&mut conn, "zzsecond").await, 1);
    assert_eq!(fts_term_entries(&mut conn, "zzsecond").await, 1);

    sqlx::query("DELETE FROM messages WHERE id = $1")
        .bind(message_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    for term in ["zzedited", "zzsecond"] {
        assert_eq!(fts_hits(&mut conn, term).await, 0, "{term}");
        assert_eq!(fts_term_entries(&mut conn, term).await, 0, "{term}");
    }

    let reused_id: i64 = sqlx::query_scalar(insert_message)
        .bind(conversation_id)
        .bind(A1)
        .bind("g-next")
        .bind("zznext text")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(
        reused_id, message_id,
        "SQLite hands the deleted id out again"
    );
    assert_eq!(fts_hits(&mut conn, "zznext").await, 1);
    assert_eq!(fts_hits(&mut conn, "zzsecond").await, 0);
}

/// The fingerprint changes whenever the schema text does, and only then:
/// a changed column, and text moved from one file to the next, each give
/// another value, and the same files give the same one. The known answers
/// are 32-bit FNV-1a with a zero byte after each file, masked to 31 bits,
/// worked out apart from this code.
#[test]
fn the_fingerprint_follows_the_schema_text() {
    let before = [
        "CREATE TABLE a (id INTEGER);",
        "CREATE TABLE b (id INTEGER);",
    ];
    let after = ["CREATE TABLE a (id INTEGER);", "CREATE TABLE b (id TEXT);"];
    assert_eq!(fingerprint_of(&before), fingerprint_of(&before));
    assert_ne!(fingerprint_of(&before), fingerprint_of(&after));
    assert_ne!(fingerprint_of(&["ab", "c"]), fingerprint_of(&["a", "bc"]));

    assert_eq!(fingerprint_of(&[]), 18_652_613);
    assert_eq!(fingerprint_of(&["a"]), 723_832_900);
    assert_eq!(fingerprint_of(&["ab", "c"]), 896_568_933);
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
