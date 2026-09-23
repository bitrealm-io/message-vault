use std::collections::{HashMap, HashSet};

use super::*;
use crate::db::engine;

#[test]
fn normalize_collapses_whitespace() {
    assert_eq!(normalize_body(Some("  hi   mom \n")), "hi mom");
}

#[test]
fn group_chat_identity_is_sorted_handles() {
    let handles = vec![
        "+14075550002".to_string(),
        "+14075550001".to_string(),
        "+14075550002".to_string(),
    ];
    assert_eq!(
        chat_identity_for_content_key("chat999", Some(&handles)),
        "group:+14075550001|+14075550002"
    );
    assert_eq!(chat_identity_for_content_key("chat999", None), "chat999");
}

#[test]
fn content_key_stable_across_whitespace_and_offset_forms() {
    // The vault stores the instant in UTC with a Z; the key hashes the epoch
    // it names, so an offset spelling of the same instant hashes the same.
    let a = compute_content_key(
        "+14075551212",
        true,
        None,
        "2015-03-12T18:04:22Z",
        Some("Running late"),
        &[],
    );
    let b = compute_content_key(
        "+14075551212",
        true,
        None,
        "2015-03-12T18:04:22+00:00",
        Some("  Running   late "),
        &[],
    );
    let c = compute_content_key(
        "+14075551212",
        true,
        None,
        "2015-03-12T14:04:22-04:00",
        Some("Running late"),
        &[],
    );
    assert_eq!(a, b);
    assert_eq!(a, c);
}

#[test]
fn parallel_content_keys_match_serial() {
    let rows = vec![
        (
            1,
            10,
            "+14075551212".into(),
            "individual".into(),
            1,
            "2015-03-12T18:04:22Z".into(),
            Some("hi".into()),
            None,
        ),
        (
            2,
            11,
            "chat-group".into(),
            "group".into(),
            0,
            "2015-03-12T18:04:23Z".into(),
            Some("yo".into()),
            Some("+15555550001".into()),
        ),
    ];
    let mut groups = HashMap::new();
    groups.insert(11, vec!["+15555550001".into(), "+15555550002".into()]);
    let mut shas = HashMap::new();
    shas.insert(2, vec!["abc".into()]);
    let parallel = hash_content_keys(&rows, &groups, &shas);
    let serial: Vec<_> = rows
        .iter()
        .map(|row| content_key_for_row(row, &groups, &shas))
        .collect();
    // Both sides call `content_key_for_row`, so this alone only shows that the
    // parallel pass keeps its input order — worth having, and not enough. The
    // keys themselves are pinned below, so a change to how a key is built
    // fails here rather than passing because both sides changed together.
    assert_eq!(parallel, serial);
    assert_eq!(
        parallel,
        vec![
            (
                1,
                "dbe62b7f59674ef9f38f923fb3c387ec66c00d8a813c0d0d7edb0ceb70217bd0".to_string()
            ),
            (
                2,
                "8e9ab8eb840faf0f50a805e3a68c430ab4234ff3b8b87792fdcd70f7078cae03".to_string()
            ),
        ],
        "the content key decides which messages are duplicates; changing it          re-partitions every vault, so it is pinned deliberately"
    );
}

#[test]
fn content_key_distinguishes_group_senders() {
    let alice = compute_content_key(
        "group:+1|+2",
        false,
        Some("+15555550001"),
        "2015-03-12T18:04:22Z",
        Some("same text"),
        &[],
    );
    let bob = compute_content_key(
        "group:+1|+2",
        false,
        Some("+15555550002"),
        "2015-03-12T18:04:22Z",
        Some("same text"),
        &[],
    );
    assert_ne!(alice, bob);
}

const TEST_ACCOUNT_ID: i64 = 7;

async fn setup_db(conn: &mut AnyConnection) {
    schema::ensure_vault_schema(conn).await.unwrap();
    sqlx::query("INSERT INTO accounts (id, username) VALUES ($1, 'test')")
        .bind(TEST_ACCOUNT_ID)
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query(
        r"
        INSERT INTO handles (account_id, raw, normalized, handle_type, service)
        VALUES ($1, '+14075551212', '+14075551212', 'phone', 'phone')
        ",
    )
    .bind(TEST_ACCOUNT_ID)
    .execute(&mut *conn)
    .await
    .unwrap();
    let handle_id: i64 = sqlx::query_scalar(
        "SELECT id FROM handles WHERE account_id = $1 AND normalized = '+14075551212'",
    )
    .bind(TEST_ACCOUNT_ID)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        r"
        INSERT INTO conversations (
            account_id, chat_handle_id, conversation_type, group_title, exported_at, source_file
        )
        VALUES ($1, $2, 'individual', NULL, NULL, 't.json')
        ",
    )
    .bind(TEST_ACCOUNT_ID)
    .bind(handle_id)
    .execute(&mut *conn)
    .await
    .unwrap();
}

struct InsertMsgArgs<'a> {
    source: &'a str,
    guid: &'a str,
    /// The UTC instant, as the vault stores it.
    timestamp: &'a str,
    from_me: i64,
    body: &'a str,
    sort_order: i64,
}

async fn insert_msg(conn: &mut AnyConnection, args: InsertMsgArgs<'_>) -> i64 {
    sqlx::query_scalar(
        r"
        INSERT INTO messages (
            conversation_id, account_id, source, guid, timestamp, is_from_me,
            sender_handle_id, subject, body, sort_order
        ) VALUES (1, $1, $2, $3, $4, $5, NULL, NULL, $6, $7)
        RETURNING id
        ",
    )
    .bind(TEST_ACCOUNT_ID)
    .bind(args.source)
    .bind(args.guid)
    .bind(args.timestamp)
    .bind(args.from_me)
    .bind(args.body)
    .bind(args.sort_order)
    .fetch_one(&mut *conn)
    .await
    .unwrap()
}

#[tokio::test]
async fn fill_missing_content_keys_skips_rows_that_already_have_keys() {
    let (pool, _dir) = engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    setup_db(&mut conn).await;
    insert_msg(
        &mut conn,
        InsertMsgArgs {
            source: "go-sms-pro",
            guid: "g-fill",
            timestamp: "2015-03-12T18:04:22Z",
            from_me: 1,
            body: "Need a key",
            sort_order: 0,
        },
    )
    .await;
    let first = fill_missing_content_keys(&mut conn, TEST_ACCOUNT_ID)
        .await
        .unwrap();
    assert_eq!(first, 1);
    let second = fill_missing_content_keys(&mut conn, TEST_ACCOUNT_ID)
        .await
        .unwrap();
    assert_eq!(second, 0);
    let key: Option<String> =
        sqlx::query_scalar("SELECT content_key FROM messages WHERE guid = 'g-fill'")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert!(key.as_deref().is_some_and(|k| !k.is_empty()));
}

#[tokio::test]
async fn fill_missing_content_keys_writes_multiple_rows_in_one_batch() {
    let (pool, _dir) = engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    setup_db(&mut conn).await;
    for (guid, body, sort_order) in [
        ("g-multi-a", "First", 0),
        ("g-multi-b", "Second", 1),
        ("g-multi-c", "Third", 2),
    ] {
        insert_msg(
            &mut conn,
            InsertMsgArgs {
                source: "go-sms-pro",
                guid,
                timestamp: "2015-03-12T18:04:22Z",
                from_me: 1,
                body,
                sort_order,
            },
        )
        .await;
    }
    let filled = fill_missing_content_keys(&mut conn, TEST_ACCOUNT_ID)
        .await
        .unwrap();
    assert_eq!(filled, 3);
    let keys: Vec<(String, Option<String>)> = sqlx::query_as(
        r"
        SELECT guid, content_key
        FROM messages
        WHERE guid IN ('g-multi-a', 'g-multi-b', 'g-multi-c')
        ORDER BY guid
        ",
    )
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(keys.len(), 3);
    let values: Vec<&str> = keys
        .iter()
        .map(|(_, key)| key.as_deref().expect("content_key"))
        .collect();
    assert!(values.iter().all(|key| !key.is_empty()));
    assert_eq!(values.iter().copied().collect::<HashSet<_>>().len(), 3);
}

#[tokio::test]
async fn dedupe_cross_source_does_not_rewrite_unchanged_keys() {
    let (pool, _dir) = engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    setup_db(&mut conn).await;
    insert_msg(
        &mut conn,
        InsertMsgArgs {
            source: "go-sms-pro",
            guid: "g-once",
            timestamp: "2015-03-12T18:04:22Z",
            from_me: 1,
            body: "Once",
            sort_order: 0,
        },
    )
    .await;
    let first = dedupe_cross_source(&mut conn, TEST_ACCOUNT_ID, None, 2)
        .await
        .unwrap();
    assert_eq!(first.keys_filled, 1);
    let second = dedupe_cross_source(&mut conn, TEST_ACCOUNT_ID, None, 2)
        .await
        .unwrap();
    assert_eq!(second.keys_filled, 0);
}

#[tokio::test]
async fn integration_exact_flags_cross_source() {
    let (pool, _dir) = engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    setup_db(&mut conn).await;
    let a = insert_msg(
        &mut conn,
        InsertMsgArgs {
            source: "go-sms-pro",
            guid: "g1",
            timestamp: "2015-03-12T18:04:22Z",
            from_me: 1,
            body: "Running late",
            sort_order: 0,
        },
    )
    .await;
    let b = insert_msg(
        &mut conn,
        InsertMsgArgs {
            source: "sms-backup-plus",
            guid: "g2",
            timestamp: "2015-03-12T18:04:22+00:00",
            from_me: 1,
            body: "Running late",
            sort_order: 0,
        },
    )
    .await;
    let priority = ["go-sms-pro".into(), "sms-backup-plus".into()];
    let stats = dedupe_cross_source(&mut conn, TEST_ACCOUNT_ID, Some(&priority), 2)
        .await
        .unwrap();
    assert_eq!(stats.exact_groups, 1);
    assert_eq!(stats.exact_flagged, 1);
    let dup: Option<i64> = sqlx::query_scalar("SELECT duplicate_of FROM messages WHERE id = $1")
        .bind(b)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(dup, Some(a));
    let keep: Option<i64> = sqlx::query_scalar("SELECT duplicate_of FROM messages WHERE id = $1")
        .bind(a)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(keep, None);
}

#[tokio::test]
async fn integration_near_flags_within_window() {
    let (pool, _dir) = engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    setup_db(&mut conn).await;
    let a = insert_msg(
        &mut conn,
        InsertMsgArgs {
            source: "go-sms-pro",
            guid: "g1",
            timestamp: "2015-03-12T18:04:22Z",
            from_me: 0,
            body: "On my way",
            sort_order: 0,
        },
    )
    .await;
    let b = insert_msg(
        &mut conn,
        InsertMsgArgs {
            source: "sms-backup-plus",
            guid: "g2",
            timestamp: "2015-03-12T18:04:24Z",
            from_me: 0,
            body: "On my way",
            sort_order: 1,
        },
    )
    .await;
    let priority = ["go-sms-pro".into(), "sms-backup-plus".into()];
    let stats = dedupe_cross_source(&mut conn, TEST_ACCOUNT_ID, Some(&priority), 2)
        .await
        .unwrap();
    assert_eq!(stats.exact_flagged, 0);
    assert_eq!(stats.near_flagged, 1);
    let dup: Option<i64> = sqlx::query_scalar("SELECT duplicate_of FROM messages WHERE id = $1")
        .bind(b)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(dup, Some(a));
}

#[tokio::test]
async fn integration_negative_far_apart_not_flagged() {
    let (pool, _dir) = engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    setup_db(&mut conn).await;
    insert_msg(
        &mut conn,
        InsertMsgArgs {
            source: "go-sms-pro",
            guid: "g1",
            timestamp: "2015-03-12T18:04:22Z",
            from_me: 0,
            body: "On my way",
            sort_order: 0,
        },
    )
    .await;
    insert_msg(
        &mut conn,
        InsertMsgArgs {
            source: "sms-backup-plus",
            guid: "g2",
            timestamp: "2015-03-12T18:05:22Z",
            from_me: 0,
            body: "On my way",
            sort_order: 1,
        },
    )
    .await;
    let priority = ["go-sms-pro".into(), "sms-backup-plus".into()];
    let stats = dedupe_cross_source(&mut conn, TEST_ACCOUNT_ID, Some(&priority), 2)
        .await
        .unwrap();
    assert_eq!(stats.exact_flagged, 0);
    assert_eq!(stats.near_flagged, 0);
    let hidden: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE duplicate_of IS NOT NULL")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(hidden, 0);
}

#[tokio::test]
async fn integration_priority_prefers_first_imported_source() {
    let (pool, _dir) = engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    setup_db(&mut conn).await;
    // First row wins when priority is derived from min(message id) per source.
    let first_imported = insert_msg(
        &mut conn,
        InsertMsgArgs {
            source: "sms-backup-plus",
            guid: "g1",
            timestamp: "2015-03-12T18:04:22Z",
            from_me: 1,
            body: "Hello",
            sort_order: 0,
        },
    )
    .await;
    let second_imported = insert_msg(
        &mut conn,
        InsertMsgArgs {
            source: "go-sms-pro",
            guid: "g2",
            timestamp: "2015-03-12T18:04:22Z",
            from_me: 1,
            body: "Hello",
            sort_order: 1,
        },
    )
    .await;
    dedupe_cross_source(&mut conn, TEST_ACCOUNT_ID, None, 2)
        .await
        .unwrap();
    let dup_first: Option<i64> =
        sqlx::query_scalar("SELECT duplicate_of FROM messages WHERE id = $1")
            .bind(first_imported)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    let dup_second: Option<i64> =
        sqlx::query_scalar("SELECT duplicate_of FROM messages WHERE id = $1")
            .bind(second_imported)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(dup_first, None);
    assert_eq!(dup_second, Some(first_imported));
}

/// The near-time window, exactly at its edge and one second past it.
///
/// The two integration tests around this use two seconds apart with a window
/// of two, and sixty apart with a window of two — comfortably inside and
/// comfortably outside. The comparison itself is `row.secs - first.secs <=
/// window_secs`, and nothing tested the `<=`: turning it into `<` narrows the
/// window by a second for every import, and turning it into `>=` collapses
/// unrelated messages into one.
///
/// Which second is the boundary matters in practice. Two exporters reading the
/// same SMS routinely disagree by exactly the window, because one records when
/// the message arrived and the other when it was written to the phone's
/// database.
#[tokio::test]
async fn a_twin_exactly_at_the_window_edge_is_flagged_and_one_past_it_is_not() {
    for (gap_secs, second_timestamp, expect_flagged) in [
        (2, "2015-03-12T18:04:24Z", true),
        (3, "2015-03-12T18:04:25Z", false),
    ] {
        let (pool, _dir) = engine::test_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        setup_db(&mut conn).await;

        let first = insert_msg(
            &mut conn,
            InsertMsgArgs {
                source: "go-sms-pro",
                guid: "g1",
                timestamp: "2015-03-12T18:04:22Z",
                from_me: 0,
                body: "On my way",
                sort_order: 0,
            },
        )
        .await;
        let second = insert_msg(
            &mut conn,
            InsertMsgArgs {
                source: "sms-backup-plus",
                guid: "g2",
                timestamp: second_timestamp,
                from_me: 0,
                body: "On my way",
                sort_order: 1,
            },
        )
        .await;

        // The window is two seconds in every case; only the gap moves.
        let priority = ["go-sms-pro".into(), "sms-backup-plus".into()];
        let stats = dedupe_cross_source(&mut conn, TEST_ACCOUNT_ID, Some(&priority), 2)
            .await
            .unwrap();

        let flagged: Option<i64> =
            sqlx::query_scalar("SELECT duplicate_of FROM messages WHERE id = $1")
                .bind(second)
                .fetch_one(&mut *conn)
                .await
                .unwrap();

        if expect_flagged {
            assert_eq!(
                stats.near_flagged, 1,
                "a gap of {gap_secs}s equals the window, so it is a duplicate"
            );
            assert_eq!(flagged, Some(first));
        } else {
            assert_eq!(
                stats.near_flagged, 0,
                "a gap of {gap_secs}s is past the window, so it is not"
            );
            assert_eq!(flagged, None, "and the message is left alone");
        }
    }
}

/// Two messages a second apart from the *same* source are not duplicates,
/// however alike they are. Someone sending "ok" twice is sending two messages,
/// and collapsing them loses one of them for good.
#[tokio::test]
async fn two_near_messages_from_one_source_are_both_kept() {
    let (pool, _dir) = engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    setup_db(&mut conn).await;

    insert_msg(
        &mut conn,
        InsertMsgArgs {
            source: "go-sms-pro",
            guid: "g1",
            timestamp: "2015-03-12T18:04:22Z",
            from_me: 1,
            body: "ok",
            sort_order: 0,
        },
    )
    .await;
    let second = insert_msg(
        &mut conn,
        InsertMsgArgs {
            source: "go-sms-pro",
            guid: "g2",
            timestamp: "2015-03-12T18:04:23Z",
            from_me: 1,
            body: "ok",
            sort_order: 1,
        },
    )
    .await;

    let priority = ["go-sms-pro".into(), "sms-backup-plus".into()];
    let stats = dedupe_cross_source(&mut conn, TEST_ACCOUNT_ID, Some(&priority), 2)
        .await
        .unwrap();

    assert_eq!(
        stats.near_flagged, 0,
        "a cluster needs two sources; one person sending the same word twice is two messages"
    );
    let flagged: Option<i64> =
        sqlx::query_scalar("SELECT duplicate_of FROM messages WHERE id = $1")
            .bind(second)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(flagged, None);
}

// ---------------------------------------------------------------------------
// Several conversations, handles, senders and attachments.
//
// The tests above use one 1:1 conversation and two messages. The ones below
// build what a real vault holds: 1:1 and group conversations, the same group
// under two chat identifiers (two exporters naming one group differently),
// incoming group messages with a sender, and attachments.
// ---------------------------------------------------------------------------

/// Insert (or find) a handle of the test account and return its id.
async fn handle(conn: &mut AnyConnection, normalized: &str) -> i64 {
    if let Some(id) = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM handles WHERE account_id = $1 AND normalized = $2",
    )
    .bind(TEST_ACCOUNT_ID)
    .bind(normalized)
    .fetch_optional(&mut *conn)
    .await
    .unwrap()
    {
        return id;
    }
    sqlx::query_scalar(
        r"
        INSERT INTO handles (account_id, raw, normalized, handle_type, service)
        VALUES ($1, $2, $2, 'phone', 'phone')
        RETURNING id
        ",
    )
    .bind(TEST_ACCOUNT_ID)
    .bind(normalized)
    .fetch_one(&mut *conn)
    .await
    .unwrap()
}

/// A conversation of the test account whose chat handle is `chat`.
async fn conversation(conn: &mut AnyConnection, chat: &str, kind: &str) -> i64 {
    let chat_handle = handle(conn, chat).await;
    sqlx::query_scalar(
        r"
        INSERT INTO conversations (
            account_id, chat_handle_id, conversation_type, group_title, exported_at, source_file
        )
        VALUES ($1, $2, $3, NULL, NULL, 't.json')
        RETURNING id
        ",
    )
    .bind(TEST_ACCOUNT_ID)
    .bind(chat_handle)
    .bind(kind)
    .fetch_one(&mut *conn)
    .await
    .unwrap()
}

async fn add_participant(conn: &mut AnyConnection, conversation_id: i64, normalized: &str) {
    let handle_id = handle(conn, normalized).await;
    sqlx::query("INSERT INTO participants (conversation_id, handle_id) VALUES ($1, $2)")
        .bind(conversation_id)
        .bind(handle_id)
        .execute(&mut *conn)
        .await
        .unwrap();
}

async fn add_attachment(conn: &mut AnyConnection, message_id: i64, sha: &str) {
    sqlx::query("INSERT INTO attachments (message_id, sha256) VALUES ($1, $2)")
        .bind(message_id)
        .bind(sha)
        .execute(&mut *conn)
        .await
        .unwrap();
}

struct Msg<'a> {
    conversation_id: i64,
    source: &'a str,
    guid: &'a str,
    timestamp: &'a str,
    from_me: bool,
    /// The sender's handle id; `None` for an outgoing message.
    sender: Option<i64>,
    body: &'a str,
}

async fn message(conn: &mut AnyConnection, m: Msg<'_>) -> i64 {
    sqlx::query_scalar(
        r"
        INSERT INTO messages (
            conversation_id, account_id, source, guid, timestamp, is_from_me,
            sender_handle_id, subject, body, sort_order
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, NULL, $8, 0)
        RETURNING id
        ",
    )
    .bind(m.conversation_id)
    .bind(TEST_ACCOUNT_ID)
    .bind(m.source)
    .bind(m.guid)
    .bind(m.timestamp)
    .bind(i64::from(m.from_me))
    .bind(m.sender)
    .bind(m.body)
    .fetch_one(&mut *conn)
    .await
    .unwrap()
}

async fn setup_account(conn: &mut AnyConnection) {
    schema::ensure_vault_schema(conn).await.unwrap();
    sqlx::query("INSERT INTO accounts (id, username) VALUES ($1, 'test')")
        .bind(TEST_ACCOUNT_ID)
        .execute(&mut *conn)
        .await
        .unwrap();
}

async fn duplicate_of(conn: &mut AnyConnection, id: i64) -> Option<i64> {
    sqlx::query_scalar("SELECT duplicate_of FROM messages WHERE id = $1")
        .bind(id)
        .fetch_one(&mut *conn)
        .await
        .unwrap()
}

/// A message imported before its attachment reached the vault still pairs
/// with its twin once the attachment arrives.
///
/// An append import of the same source maps a message it already holds onto
/// the existing row and adds the attachments that row lacks (see
/// `promote_attachments`). The content key covers the attachment digests, so
/// the key has to follow. The two copies sit in different conversations (the
/// same group under two chat identifiers), so only the content key can pair
/// them: the near-time pass looks inside one conversation.
#[tokio::test]
async fn an_attachment_added_after_the_first_dedupe_changes_the_content_key() {
    let (pool, _dir) = engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    setup_account(&mut conn).await;
    let x = conversation(&mut conn, "chat-x", "group").await;
    let y = conversation(&mut conn, "chat-y", "group").await;
    for conv in [x, y] {
        add_participant(&mut conn, conv, "+15555550001").await;
        add_participant(&mut conn, conv, "+15555550002").await;
    }

    let first = message(
        &mut conn,
        Msg {
            conversation_id: x,
            source: "imessage",
            guid: "a-1",
            timestamp: "2015-03-12T18:04:22Z",
            from_me: true,
            sender: None,
            body: "look",
        },
    )
    .await;
    dedupe_cross_source(&mut conn, TEST_ACCOUNT_ID, None, 2)
        .await
        .unwrap();

    // The re-import brings the photo; the other exporter had it all along.
    add_attachment(&mut conn, first, "sha-photo").await;
    let twin = message(
        &mut conn,
        Msg {
            conversation_id: y,
            source: "sms-backup-plus",
            guid: "b-1",
            timestamp: "2015-03-12T18:04:22Z",
            from_me: true,
            sender: None,
            body: "look",
        },
    )
    .await;
    add_attachment(&mut conn, twin, "sha-photo").await;

    let stats = dedupe_cross_source(&mut conn, TEST_ACCOUNT_ID, None, 2)
        .await
        .unwrap();

    assert_eq!(
        stats.exact_flagged, 1,
        "the same photo and text at the same second from two sources is one message"
    );
    assert_eq!(duplicate_of(&mut conn, twin).await, Some(first));
    assert_eq!(duplicate_of(&mut conn, first).await, None);
}

/// A group whose participant list grows after the first dedupe pairs with the
/// same group from another exporter once both list the same people.
///
/// The content key of a group message names the group by its sorted
/// participants, so the key has to follow the list: an append import adds
/// the participants a conversation lacks (see `promote_participants`).
#[tokio::test]
async fn a_participant_added_after_the_first_dedupe_changes_the_group_content_key() {
    let (pool, _dir) = engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    setup_account(&mut conn).await;
    let x = conversation(&mut conn, "chat-x", "group").await;
    let y = conversation(&mut conn, "chat-y", "group").await;
    // The first export of chat-x named only one of the two people.
    add_participant(&mut conn, x, "+15555550001").await;
    let first = message(
        &mut conn,
        Msg {
            conversation_id: x,
            source: "imessage",
            guid: "a-1",
            timestamp: "2015-03-12T18:04:22Z",
            from_me: true,
            sender: None,
            body: "dinner at 7?",
        },
    )
    .await;
    dedupe_cross_source(&mut conn, TEST_ACCOUNT_ID, None, 2)
        .await
        .unwrap();

    add_participant(&mut conn, x, "+15555550002").await;
    add_participant(&mut conn, y, "+15555550001").await;
    add_participant(&mut conn, y, "+15555550002").await;
    let twin = message(
        &mut conn,
        Msg {
            conversation_id: y,
            source: "sms-backup-plus",
            guid: "b-1",
            timestamp: "2015-03-12T18:04:22Z",
            from_me: true,
            sender: None,
            body: "dinner at 7?",
        },
    )
    .await;

    dedupe_cross_source(&mut conn, TEST_ACCOUNT_ID, None, 2)
        .await
        .unwrap();

    assert_eq!(duplicate_of(&mut conn, twin).await, Some(first));
    assert_eq!(duplicate_of(&mut conn, first).await, None);
}

/// A dedupe that fails part way leaves the duplicates it found last time
/// hidden.
///
/// Dedupe clears every flag and sets them again. If the clearing is committed
/// on its own and a later pass then fails, every duplicate in the account
/// shows twice until the next successful run. The failure is forced by
/// creating, on the same connection, a temp table under the name a pass
/// writes its flags to but with the wrong columns; temp tables belong to one
/// connection on both engines, so this works on SQLite and Postgres alike.
#[tokio::test]
async fn a_failed_dedupe_keeps_the_previous_duplicates_hidden() {
    for broken_table in ["_pass_a_flags", "_pass_b_flags"] {
        let (pool, _dir) = engine::test_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        setup_account(&mut conn).await;
        let peer = conversation(&mut conn, "+14075551212", "individual").await;
        let mut ids = Vec::new();
        // An exact twin (same second) and a near-time twin (one second apart).
        for (source, guid, timestamp, body) in [
            ("go-sms-pro", "a-1", "2015-03-12T18:04:22Z", "Running late"),
            (
                "sms-backup-plus",
                "b-1",
                "2015-03-12T18:04:22Z",
                "Running late",
            ),
            ("go-sms-pro", "a-2", "2015-03-12T18:10:00Z", "On my way"),
            (
                "sms-backup-plus",
                "b-2",
                "2015-03-12T18:10:01Z",
                "On my way",
            ),
        ] {
            ids.push(
                message(
                    &mut conn,
                    Msg {
                        conversation_id: peer,
                        source,
                        guid,
                        timestamp,
                        from_me: true,
                        sender: None,
                        body,
                    },
                )
                .await,
            );
        }
        let stats = dedupe_cross_source(&mut conn, TEST_ACCOUNT_ID, None, 2)
            .await
            .unwrap();
        assert_eq!((stats.exact_flagged, stats.near_flagged), (1, 1));

        sqlx::query(&format!("CREATE TEMP TABLE {broken_table} (id BIGINT)"))
            .execute(&mut *conn)
            .await
            .unwrap();
        let result = dedupe_cross_source(&mut conn, TEST_ACCOUNT_ID, None, 2).await;
        assert!(result.is_err(), "{broken_table}: the pass should fail");
        sqlx::query(&format!("DROP TABLE {broken_table}"))
            .execute(&mut *conn)
            .await
            .unwrap();

        assert_eq!(
            duplicate_of(&mut conn, ids[1]).await,
            Some(ids[0]),
            "{broken_table}: the exact twin stays hidden"
        );
        assert_eq!(
            duplicate_of(&mut conn, ids[3]).await,
            Some(ids[2]),
            "{broken_table}: the near-time twin stays hidden"
        );
    }
}

// ---------------------------------------------------------------------------
// Invariants over generated vaults.
// ---------------------------------------------------------------------------

/// A linear congruential generator: the same seed builds the same vault on
/// every machine, with no dependency.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % n as u64).unwrap()
    }

    fn one_in(&mut self, n: u64) -> bool {
        self.next().is_multiple_of(n)
    }
}

const GEN_WINDOW_SECS: i64 = 2;
const GEN_PEOPLE: [&str; 4] = [
    "+15555550001",
    "+15555550002",
    "+15555550003",
    "+15555550004",
];
const GEN_SOURCES: [&str; 3] = ["imessage", "sms-backup-plus", "go-sms-pro"];
const GEN_BODIES: [&str; 5] = ["ok", "on my way", "see you at 6", "lol", ""];
const GEN_SHAS: [&str; 3] = ["sha-a", "sha-b", "sha-c"];

/// One chat as the generator sees it: the conversations a copy of a message
/// may land in (two for a group two exporters name differently) and the
/// people who can send in it.
struct GenChat {
    conversations: Vec<i64>,
    senders: Vec<i64>,
}

/// Build a vault from `seed`: two 1:1 chats, one group under two chat
/// identifiers, one smaller group. Each logical message is written by one
/// to three sources (a source may write it twice), and each copy is an exact
/// twin, a near-time twin (inside the window or one second past it), or a
/// near-miss (the body or one attachment differs). Messages are a few seconds
/// apart and share a small vocabulary, so unrelated messages collide too.
async fn generate_vault(conn: &mut AnyConnection, seed: u64) -> Vec<i64> {
    let mut rng = Lcg(seed);
    setup_account(conn).await;
    let mut people = Vec::new();
    for p in GEN_PEOPLE {
        people.push(handle(conn, p).await);
    }
    let mut chats = Vec::new();
    for (i, peer) in GEN_PEOPLE.iter().take(2).enumerate() {
        chats.push(GenChat {
            conversations: vec![conversation(conn, peer, "individual").await],
            senders: vec![people[i]],
        });
    }
    let x = conversation(conn, "chat-x", "group").await;
    let y = conversation(conn, "chat-y", "group").await;
    let z = conversation(conn, "chat-z", "group").await;
    for p in &GEN_PEOPLE[..3] {
        add_participant(conn, x, p).await;
        add_participant(conn, y, p).await;
    }
    for p in &GEN_PEOPLE[..2] {
        add_participant(conn, z, p).await;
    }
    chats.push(GenChat {
        conversations: vec![x, y],
        senders: people[..3].to_vec(),
    });
    chats.push(GenChat {
        conversations: vec![z],
        senders: people[..2].to_vec(),
    });

    let mut ids = Vec::new();
    let mut secs = 1_426_183_200; // 2015-03-12T18:00:00Z
    let mut guid = 0;
    for _ in 0..12 + rng.below(12) {
        secs += rng.below(6) as i64;
        let chat = &chats[rng.below(chats.len())];
        let from_me = rng.one_in(2);
        let sender = if from_me {
            None
        } else {
            Some(chat.senders[rng.below(chat.senders.len())])
        };
        let body = GEN_BODIES[rng.below(GEN_BODIES.len())];
        let shas: Vec<&str> = (0..rng.below(3))
            .map(|_| GEN_SHAS[rng.below(GEN_SHAS.len())])
            .collect();
        for _ in 0..1 + rng.below(3) {
            let mut copy_secs = secs;
            let mut copy_body = body.to_string();
            let mut copy_shas = shas.clone();
            match rng.below(6) {
                0 => copy_secs += 1 + rng.below(GEN_WINDOW_SECS as usize + 1) as i64,
                1 => copy_body.push('!'),
                2 => {
                    copy_shas.pop();
                }
                _ => {}
            }
            guid += 1;
            let timestamp = chrono::DateTime::from_timestamp(copy_secs, 0)
                .unwrap()
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
            let conversation_id = chat.conversations[rng.below(chat.conversations.len())];
            let source = GEN_SOURCES[rng.below(GEN_SOURCES.len())];
            let id = message(
                conn,
                Msg {
                    conversation_id,
                    source,
                    guid: &format!("g-{guid}"),
                    timestamp: &timestamp,
                    from_me,
                    sender,
                    body: &copy_body,
                },
            )
            .await;
            for sha in copy_shas {
                add_attachment(conn, id, sha).await;
            }
            ids.push(id);
        }
    }
    ids
}

/// One message as the invariants read it.
#[derive(Debug)]
struct GenRow {
    conversation_id: i64,
    source: String,
    is_from_me: i64,
    sender: String,
    secs: i64,
    body: String,
    /// The attachment digests in order, as the near-time pass compares them.
    attachments: String,
    content_key: Option<String>,
    duplicate_of: Option<i64>,
}

impl GenRow {
    /// The near-time twin rule, written out again: same conversation,
    /// direction and sender, another source, and the same body or the same
    /// attachments. Time is checked by the caller.
    fn is_near_twin_of(&self, other: &GenRow) -> bool {
        self.conversation_id == other.conversation_id
            && self.is_from_me == other.is_from_me
            && self.sender == other.sender
            && self.source != other.source
            && ((!self.body.is_empty() && self.body == other.body)
                || (!self.attachments.is_empty() && self.attachments == other.attachments))
    }
}

async fn load_gen_rows(conn: &mut AnyConnection) -> HashMap<i64, GenRow> {
    type Raw = (
        i64,
        i64,
        String,
        i64,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<i64>,
    );
    let raw: Vec<Raw> = sqlx::query_as(
        r"
        SELECT m.id, m.conversation_id, m.source, m.is_from_me, COALESCE(h.normalized, ''),
               m.timestamp, m.body, m.content_key, m.duplicate_of
        FROM messages m
        LEFT JOIN handles h ON h.id = m.sender_handle_id
        WHERE m.account_id = $1
        ",
    )
    .bind(TEST_ACCOUNT_ID)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    let atts: Vec<(i64, String)> =
        sqlx::query_as("SELECT message_id, sha256 FROM attachments ORDER BY message_id, sha256")
            .fetch_all(&mut *conn)
            .await
            .unwrap();
    let mut shas: HashMap<i64, Vec<String>> = HashMap::new();
    for (id, sha) in atts {
        shas.entry(id).or_default().push(sha);
    }
    raw.into_iter()
        .map(
            |(id, conversation_id, source, is_from_me, sender, ts, body, key, dup)| {
                let row = GenRow {
                    conversation_id,
                    source,
                    is_from_me,
                    sender: if is_from_me != 0 {
                        String::new()
                    } else {
                        sender
                    },
                    secs: parse_rfc3339_utc_secs(&ts).unwrap(),
                    body: normalize_body(body.as_deref()),
                    attachments: shas.remove(&id).unwrap_or_default().join(","),
                    content_key: key,
                    duplicate_of: dup,
                };
                (id, row)
            },
        )
        .collect()
}

/// Follow `duplicate_of` from `id` to the message shown in its place.
/// Panics on a chain that does not end.
fn survivor(rows: &HashMap<i64, GenRow>, id: i64, ctx: &str) -> i64 {
    let mut at = id;
    for _ in 0..=rows.len() {
        match rows[&at].duplicate_of {
            None => return at,
            Some(next) => {
                assert!(
                    rows.contains_key(&next),
                    "{ctx}: {at} is a duplicate of {next}, which is not a message of the account"
                );
                at = next;
            }
        }
    }
    panic!("{ctx}: the duplicate_of chain from {id} never ends");
}

/// The rules every dedupe result must satisfy, whatever the messages.
///
/// - Every content key equals the key computed from the message's current
///   chat identity, attachments, and fields.
/// - Every hidden message's `duplicate_of` chain ends at a shown message.
/// - Each link is one the passes can make: an exact link joins two messages
///   with the same content key (the scope is the account: the same group
///   named twice is two conversations with one key); a near-time link joins
///   two messages of one conversation that are each the near-time twin of one
///   earlier-or-equal message within the window, or are that message. The
///   near-time pass clusters around a first message, so the two ends of a
///   link need not share a body or attachments with each other.
/// - A content key shared by two or more sources ends in one shown message:
///   at most one of its messages is shown, and all of them lead to the same
///   one. That one may be outside the group, when the near-time pass hid the
///   group's own survivor.
async fn assert_dedupe_invariants(conn: &mut AnyConnection, ctx: &str) {
    let rows = load_gen_rows(conn).await;

    let expected: HashMap<i64, String> = ContentKeyInputs::load(conn, TEST_ACCOUNT_ID, false)
        .await
        .unwrap()
        .expect("the vault has messages")
        .hash()
        .into_iter()
        .collect();
    for (id, row) in &rows {
        assert_eq!(
            row.content_key.as_deref(),
            Some(expected[id].as_str()),
            "{ctx}: message {id} has a stale content key ({row:?})"
        );
    }

    for (&id, row) in &rows {
        let Some(target) = row.duplicate_of else {
            continue;
        };
        survivor(&rows, id, ctx);
        let other = &rows[&target];
        let exact = row.content_key == other.content_key;
        let near = rows.iter().any(|(&first_id, first)| {
            let latest = row.secs.max(other.secs);
            first.conversation_id == row.conversation_id
                && first.secs <= row.secs.min(other.secs)
                && latest - first.secs <= GEN_WINDOW_SECS
                && (first_id == id || first.is_near_twin_of(row))
                && (first_id == target || first.is_near_twin_of(other))
        });
        assert!(
            exact || near,
            "{ctx}: {id} is hidden as a duplicate of {target}, which is neither its exact nor \
             its near-time twin\n  {id}: {row:?}\n  {target}: {other:?}"
        );
    }

    let mut by_key: HashMap<&str, Vec<i64>> = HashMap::new();
    for (&id, row) in &rows {
        by_key
            .entry(row.content_key.as_deref().unwrap_or(""))
            .or_default()
            .push(id);
    }
    for (key, ids) in by_key {
        let sources: HashSet<&str> = ids.iter().map(|id| rows[id].source.as_str()).collect();
        if sources.len() < 2 {
            continue;
        }
        let shown: Vec<i64> = ids
            .iter()
            .copied()
            .filter(|id| rows[id].duplicate_of.is_none())
            .collect();
        assert!(
            shown.len() <= 1,
            "{ctx}: content key {key} is shown {} times: {shown:?}",
            shown.len()
        );
        let survivors: HashSet<i64> = ids.iter().map(|&id| survivor(&rows, id, ctx)).collect();
        assert_eq!(
            survivors.len(),
            1,
            "{ctx}: the messages of content key {key} ({ids:?}) lead to {survivors:?}"
        );
    }
}

/// `(id, content key, duplicate_of)` for every message, in id order.
async fn dedupe_snapshot(conn: &mut AnyConnection) -> Vec<(i64, Option<String>, Option<i64>)> {
    let mut v: Vec<_> = load_gen_rows(conn)
        .await
        .into_iter()
        .map(|(id, r)| (id, r.content_key, r.duplicate_of))
        .collect();
    v.sort();
    v
}

/// Run dedupe, check the invariants, then run it again and check it changed
/// nothing. Returns the first run's counts.
async fn dedupe_and_check(conn: &mut AnyConnection, ctx: &str) -> DedupeStats {
    let stats = dedupe_cross_source(conn, TEST_ACCOUNT_ID, None, GEN_WINDOW_SECS)
        .await
        .unwrap();
    assert_dedupe_invariants(conn, ctx).await;

    let before = dedupe_snapshot(conn).await;
    let again = dedupe_cross_source(conn, TEST_ACCOUNT_ID, None, GEN_WINDOW_SECS)
        .await
        .unwrap();
    assert_eq!(
        again.keys_filled, 0,
        "{ctx}: a second run rewrote content keys"
    );
    assert_eq!(
        before,
        dedupe_snapshot(conn).await,
        "{ctx}: a second run changed the result"
    );
    stats
}

/// Dedupe over generated vaults, before and after a later import adds
/// attachments to messages already in the vault and a participant to one
/// name of the shared group. The seed is in every failure message; add it
/// to the list to keep a case that once failed.
#[tokio::test]
async fn dedupe_invariants_hold_over_generated_vaults() {
    let (mut exact, mut near, mut rewritten) = (0, 0, 0);
    for seed in [1_u64, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 233, 377, 610] {
        let (pool, _dir) = engine::test_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        let ids = generate_vault(&mut conn, seed).await;
        let stats = dedupe_and_check(&mut conn, &format!("seed {seed}, first import")).await;
        exact += stats.exact_flagged;
        near += stats.near_flagged;

        let mut rng = Lcg(seed ^ 0x5eed);
        for &id in &ids {
            if rng.one_in(4) {
                add_attachment(&mut conn, id, "sha-later").await;
            }
        }
        let x = handle(&mut conn, "chat-x").await;
        let x: i64 = sqlx::query_scalar("SELECT id FROM conversations WHERE chat_handle_id = $1")
            .bind(x)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
        add_participant(&mut conn, x, GEN_PEOPLE[3]).await;
        let stats =
            dedupe_and_check(&mut conn, &format!("seed {seed}, after a later import")).await;
        rewritten += stats.keys_filled;
    }
    // The generator has to exercise both passes and the key refresh, or the
    // invariants hold for want of anything to check.
    assert!(
        exact > 0 && near > 0 && rewritten > 0,
        "exact={exact} near={near} rewritten={rewritten}"
    );
}
