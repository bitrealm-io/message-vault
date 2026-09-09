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
    assert_eq!(parallel, serial);
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

#[test]
fn parse_rfc3339_applies_offset() {
    assert_eq!(
        parse_rfc3339_utc_secs("2015-03-12T18:04:22Z"),
        Some(1426183462)
    );
    assert_eq!(
        parse_rfc3339_utc_secs("2015-03-12T18:04:22+00:00"),
        Some(1426183462)
    );
    assert_eq!(
        parse_rfc3339_utc_secs("2015-03-12T14:04:22-04:00"),
        Some(1426183462)
    );
}

#[test]
fn parse_rfc3339_rejects_unparseable_input() {
    assert_eq!(parse_rfc3339_utc_secs(""), None);
    assert_eq!(parse_rfc3339_utc_secs("not a timestamp"), None);
    // Missing offset is not RFC3339; compute_content_key then falls back
    // to hashing the raw string.
    assert_eq!(parse_rfc3339_utc_secs("2015-03-12T18:04:22"), None);
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
async fn dedupe_cross_source_does_not_rehash_existing_keys() {
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
