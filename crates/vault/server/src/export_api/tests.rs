use super::*;
use crate::test_support::{SeedConversation, TestVault, seed_conversation, test_vault};

#[tokio::test]
async fn export_takes_the_search_language() {
    let (pool, _dir, f) = crate::search::tests::seeded().await;
    let mut conn = pool.acquire().await.unwrap();
    let clock = crate::search::tests::clock();
    let page = export_messages(
        &mut conn,
        ExportPageOpts {
            account_id: crate::search::tests::ACCOUNT,
            query: "from:me avocado",
            limit: 50,
            offset: 0,
            clock,

            order: crate::db::conversation_messages::DEFAULT_MESSAGE_SORT.to_vec(),
        },
    )
    .await
    .unwrap();
    let mut ids: Vec<i64> = page.items.iter().map(|m| m.id).collect();
    ids.sort_unstable();
    assert_eq!(ids, vec![f.jane_avocado_from_me, f.sam_avocado_from_me]);

    let count = export_message_count(
        &mut conn,
        ExportCountOpts {
            account_id: crate::search::tests::ACCOUNT,
            query: "source:whatsapp",
            clock,
        },
    )
    .await
    .unwrap();
    assert_eq!(count.messages, 1);

    // A word the language does not have is a 400, not a text search.
    let err = export_messages(
        &mut conn,
        ExportPageOpts {
            account_id: crate::search::tests::ACCOUNT,
            query: "sparkle:yes",
            limit: 50,
            offset: 0,
            clock,

            order: crate::db::conversation_messages::DEFAULT_MESSAGE_SORT.to_vec(),
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, ApiError::SearchQueryInvalid { .. }));
}

/// A vault with account `a1` and two individual conversations
/// (`+1555`, `+1666`), each holding one SMS message ("hello one" in the
/// first, "hello two" in the second). Returns the conversation ids the
/// seeder made, since several tests below assert on them (`in:#<id>`
/// queries, `trashed_conversations` rows).
///
/// The messages are seeded with an explicit SQL insert rather than
/// through `seed_conversation`, because `SeedMessage` has no `service`
/// field and several tests assert `message.service == Some("sms")`.
async fn seeded_export_vault() -> (TestVault, i64, i64) {
    let vault = test_vault().await;
    let account = vault.account_with_id(101, "alice").await;
    let conv1 = seed_conversation(
        &vault.state,
        &SeedConversation {
            account_id: account,
            handle: "+1555",
            conversation_type: "individual",
            group_title: None,
            source_file: "backup-a.jsonl",
            messages: &[],
        },
    )
    .await;
    let conv2 = seed_conversation(
        &vault.state,
        &SeedConversation {
            account_id: account,
            handle: "+1666",
            conversation_type: "individual",
            group_title: None,
            source_file: "backup-a.jsonl",
            messages: &[],
        },
    )
    .await;

    let mut conn = vault.conn().await;
    sqlx::query(
        "INSERT INTO messages (id, conversation_id, account_id, source, service, timestamp, is_from_me, sort_order, body)
         VALUES (1, $1, 101, 'sms', 'sms', '2020-01-01T00:00:00Z', 0, 0, 'hello one'),
                (2, $2, 101, 'sms', 'sms', '2020-01-02T00:00:00Z', 0, 0, 'hello two')",
    )
    .bind(conv1)
    .bind(conv2)
    .execute(&mut *conn)
    .await
    .unwrap();

    (vault, conv1, conv2)
}

#[tokio::test]
async fn export_includes_attachment_missing_reason() {
    let (vault, conv1, _conv2) = seeded_export_vault().await;
    let mut conn = vault.conn().await;
    sqlx::query(
        "INSERT INTO attachments (
            message_id, path, original_name, mime_type, sha256, is_sticker,
            size_bytes, missing_reason
         ) VALUES (1, 'attachments/gone.bin', 'gone.bin', 'image/png', NULL, 0, 2048, 'file_missing')",
    )
    .execute(&mut *conn)
    .await
    .unwrap();

    let query = format!("in:#{conv1}");
    let res = export_messages(
        &mut conn,
        ExportPageOpts {
            account_id: 101,
            query: &query,
            limit: 100,
            offset: 0,
            clock: crate::search::tests::clock(),

            order: crate::db::conversation_messages::DEFAULT_MESSAGE_SORT.to_vec(),
        },
    )
    .await
    .unwrap();
    assert_eq!(res.items.len(), 1);
    assert_eq!(res.items[0].attachments.len(), 1);
    let att = &res.items[0].attachments[0];
    assert!(att.sha256.is_none());
    assert_eq!(att.missing_reason.as_deref(), Some("file_missing"));
    assert_eq!(att.original_name.as_deref(), Some("gone.bin"));
    assert_eq!(att.mime_type.as_deref(), Some("image/png"));
}

#[tokio::test]
async fn conversation_filter_scopes_messages() {
    let (vault, conv1, conv2) = seeded_export_vault().await;
    let mut conn = vault.conn().await;

    let query1 = format!("in:#{conv1}");
    let res = export_messages(
        &mut conn,
        ExportPageOpts {
            account_id: 101,
            query: &query1,
            limit: 100,
            offset: 0,
            clock: crate::search::tests::clock(),

            order: crate::db::conversation_messages::DEFAULT_MESSAGE_SORT.to_vec(),
        },
    )
    .await
    .unwrap();
    assert_eq!(res.items.len(), 1);
    assert_eq!(res.items[0].id, 1);
    assert_eq!(res.items[0].service.as_deref(), Some("sms"));

    let query2 = format!("in:#{conv2}");
    let res = export_messages(
        &mut conn,
        ExportPageOpts {
            account_id: 101,
            query: &query2,
            limit: 100,
            offset: 0,
            clock: crate::search::tests::clock(),

            order: crate::db::conversation_messages::DEFAULT_MESSAGE_SORT.to_vec(),
        },
    )
    .await
    .unwrap();
    assert_eq!(res.items.len(), 1);
    assert_eq!(res.items[0].id, 2);

    let res = export_messages(
        &mut conn,
        ExportPageOpts {
            account_id: 101,
            query: "",
            limit: 100,
            offset: 0,
            clock: crate::search::tests::clock(),

            order: crate::db::conversation_messages::DEFAULT_MESSAGE_SORT.to_vec(),
        },
    )
    .await
    .unwrap();
    assert_eq!(res.items.len(), 2);
}

#[tokio::test]
async fn export_message_count_supports_handle_filters() {
    let (vault, conv1, _conv2) = seeded_export_vault().await;
    let mut conn = vault.conn().await;
    let sender_id: i64 = sqlx::query_scalar(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES (101, 'alice', 'alice', 'other', 'other') RETURNING id",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    sqlx::query("UPDATE messages SET sender_handle_id = $1 WHERE id = 1")
        .bind(sender_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE handles SET raw = 'alice-chat', normalized = 'alice-chat'
         WHERE id = (SELECT chat_handle_id FROM conversations WHERE id = $1)",
    )
    .bind(conv1)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO attachments (
            message_id, path, original_name, mime_type, sha256, is_sticker, size_bytes
         ) VALUES (1, 'attachments/a.txt', 'a.txt', 'text/plain', 'abc123', 0, 12)",
    )
    .execute(&mut *conn)
    .await
    .unwrap();

    for query in ["from:alice", "in:alice-chat"] {
        let counts = export_message_count(
            &mut conn,
            ExportCountOpts {
                account_id: 101,
                query,
                clock: crate::search::tests::clock(),
            },
        )
        .await
        .unwrap();
        assert_eq!(counts.messages, 1, "query={query}");
        assert_eq!(counts.conversations, 1, "query={query}");
        assert_eq!(counts.attachments, 1, "query={query}");
    }
}

#[tokio::test]
async fn free_text_matches_message_body_via_fts() {
    let (vault, _conv1, _conv2) = seeded_export_vault().await;
    let mut conn = vault.conn().await;
    let res = export_messages(
        &mut conn,
        ExportPageOpts {
            account_id: 101,
            query: "one",
            limit: 100,
            offset: 0,
            clock: crate::search::tests::clock(),

            order: crate::db::conversation_messages::DEFAULT_MESSAGE_SORT.to_vec(),
        },
    )
    .await
    .unwrap();
    assert_eq!(res.items.len(), 1);
    assert_eq!(res.items[0].id, 1);
    assert!(res.items[0].text.as_deref().unwrap_or("").contains("one"));
}

#[tokio::test]
async fn export_boolean_query_preserves_or() {
    let (vault, conv1, _conv2) = seeded_export_vault().await;
    let mut conn = vault.conn().await;
    sqlx::query("UPDATE messages SET body = 'foo' WHERE id = 1")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query("UPDATE messages SET body = 'bar' WHERE id = 2")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO messages (
            id, conversation_id, account_id, source, service, timestamp,
            is_from_me, sort_order, body
         ) VALUES (
            3, $1, 101, 'sms', 'sms', '2020-01-03T00:00:00Z',
            0, 0, 'foo bar'
         )",
    )
    .bind(conv1)
    .execute(&mut *conn)
    .await
    .unwrap();

    let result = export_messages(
        &mut conn,
        ExportPageOpts {
            account_id: 101,
            query: "foo OR bar",
            limit: 100,
            offset: 0,
            clock: crate::search::tests::clock(),

            order: crate::db::conversation_messages::DEFAULT_MESSAGE_SORT.to_vec(),
        },
    )
    .await
    .unwrap();
    let ids: Vec<i64> = result.items.iter().map(|message| message.id).collect();
    assert_eq!(ids, vec![1, 2, 3]);
}

#[tokio::test]
async fn export_boolean_query_preserves_and_and_not() {
    let (vault, conv1, _conv2) = seeded_export_vault().await;
    let pool = vault.state.db.clone();
    let mut conn = vault.conn().await;
    sqlx::query("UPDATE messages SET body = 'foo' WHERE id = 1")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query("UPDATE messages SET body = 'bar' WHERE id = 2")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO messages (
            id, conversation_id, account_id, source, service, timestamp,
            is_from_me, sort_order, body
         ) VALUES (
            3, $1, 101, 'sms', 'sms', '2020-01-03T00:00:00Z',
            0, 0, 'foo bar'
         )",
    )
    .bind(conv1)
    .execute(&mut *conn)
    .await
    .unwrap();

    // All call sites pass string literals, so `'static` sidesteps the
    // closure-returning-future lifetime puzzle (the future would otherwise
    // borrow a caller-owned reference the closure cannot name).
    let matching_ids = |query: &'static str| {
        let pool = pool.clone();
        async move {
            let mut conn = pool.acquire().await.unwrap();
            export_messages(
                &mut conn,
                ExportPageOpts {
                    account_id: 101,
                    query,
                    limit: 100,
                    offset: 0,
                    clock: crate::search::tests::clock(),

                    order: crate::db::conversation_messages::DEFAULT_MESSAGE_SORT.to_vec(),
                },
            )
            .await
            .unwrap()
            .items
            .into_iter()
            .map(|message| message.id)
            .collect::<Vec<_>>()
        }
    };
    assert_eq!(matching_ids("foo AND bar").await, vec![3]);
    assert_eq!(matching_ids("foo AND NOT bar").await, vec![1]);
}

#[tokio::test]
async fn export_boolean_query_combines_body_phrases_prefixes_and_nesting() {
    let (vault, _conv1, conv2) = seeded_export_vault().await;
    let pool = vault.state.db.clone();
    let mut conn = vault.conn().await;
    sqlx::query("UPDATE messages SET body = 'alpha phrase at sunrise' WHERE id = 1")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query("UPDATE messages SET body = 'unrelated' WHERE id = 2")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO attachments (
            message_id, path, original_name, mime_type, sha256, is_sticker
         ) VALUES (
            2, 'attachments/report-final.pdf', 'report-final.pdf',
            'application/pdf', 'report-digest', 0
         )",
    )
    .execute(&mut *conn)
    .await
    .unwrap();

    // All call sites pass string literals, so `'static` sidesteps the
    // closure-returning-future lifetime puzzle (the future would otherwise
    // borrow a caller-owned reference the closure cannot name).
    let matching_ids = |query: &'static str| {
        let pool = pool.clone();
        async move {
            let mut conn = pool.acquire().await.unwrap();
            export_messages(
                &mut conn,
                ExportPageOpts {
                    account_id: 101,
                    query,
                    limit: 100,
                    offset: 0,
                    clock: crate::search::tests::clock(),

                    order: crate::db::conversation_messages::DEFAULT_MESSAGE_SORT.to_vec(),
                },
            )
            .await
            .unwrap()
            .items
            .into_iter()
            .map(|message| message.id)
            .collect::<Vec<_>>()
        }
    };

    assert_eq!(
        matching_ids(r#""alpha phrase" OR report*"#).await,
        vec![1, 2]
    );
    assert_eq!(
        matching_ids(r#"sunrise AND ("alpha phrase" OR report*)"#).await,
        vec![1]
    );
    assert_eq!(matching_ids("NOT NOT report*").await, vec![2]);

    sqlx::query(
        "INSERT INTO trashed_conversations (account_id, conversation_id)
         VALUES (101, $1)",
    )
    .bind(conv2)
    .execute(&mut *conn)
    .await
    .unwrap();
    assert_eq!(matching_ids(r#""alpha phrase" OR report*"#).await, vec![1]);
}

#[tokio::test]
async fn rejects_an_oversized_query() {
    let (vault, _conv1, _conv2) = seeded_export_vault().await;
    let mut conn = vault.conn().await;
    let huge = "x".repeat(crate::search::lex::MAX_QUERY_BYTES + 1);
    let err = export_messages(
        &mut conn,
        ExportPageOpts {
            account_id: 101,
            query: &huge,
            limit: 10,
            offset: 0,
            clock: crate::search::tests::clock(),

            order: crate::db::conversation_messages::DEFAULT_MESSAGE_SORT.to_vec(),
        },
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&err, ApiError::SearchQueryInvalid { detail, .. } if detail.contains("longer than")),
        "{err:?}"
    );
}

#[tokio::test]
async fn export_does_not_leak_other_account_messages() {
    let (vault, _conv1, _conv2) = seeded_export_vault().await;
    vault.account_with_id(102, "bob").await;
    let mut conn = vault.conn().await;
    let bob_handle: i64 = sqlx::query_scalar(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES (102, '+1777', '+1777', 'phone', 'phone') RETURNING id",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO conversations (id, account_id, chat_handle_id, conversation_type, source_file)
         VALUES (99, 102, $1, 'individual', 'bob.jsonl')",
    )
    .bind(bob_handle)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO messages (id, conversation_id, account_id, source, service, timestamp, is_from_me, sort_order, body)
         VALUES (99, 99, 102, 'sms', 'sms', '2020-02-01T00:00:00Z', 0, 0, 'bob secret')",
    )
    .execute(&mut *conn)
    .await
    .unwrap();

    let alice = export_messages(
        &mut conn,
        ExportPageOpts {
            account_id: 101,
            query: "secret",
            limit: 100,
            offset: 0,
            clock: crate::search::tests::clock(),

            order: crate::db::conversation_messages::DEFAULT_MESSAGE_SORT.to_vec(),
        },
    )
    .await
    .unwrap();
    assert!(alice.items.is_empty(), "alice must not see bob's FTS hits");

    let alice_all = export_messages(
        &mut conn,
        ExportPageOpts {
            account_id: 101,
            query: "",
            limit: 100,
            offset: 0,
            clock: crate::search::tests::clock(),

            order: crate::db::conversation_messages::DEFAULT_MESSAGE_SORT.to_vec(),
        },
    )
    .await
    .unwrap();
    assert!(alice_all.items.iter().all(|m| m.id != 99));
}

#[tokio::test]
async fn export_pages_by_offset_and_reports_the_total() {
    let (vault, conv1, _conv2) = seeded_export_vault().await;
    let pool = vault.state.db.clone();
    let mut conn = vault.conn().await;
    sqlx::query(
        "INSERT INTO messages (
            id, conversation_id, account_id, source, service, timestamp,
            is_from_me, sort_order, body
         ) VALUES (
            3, $1, 101, 'sms', 'sms', '2020-01-03T00:00:00Z',
            0, 0, 'third'
         )",
    )
    .bind(conv1)
    .execute(&mut *conn)
    .await
    .unwrap();

    let page = |limit: usize, offset: usize| {
        let pool = pool.clone();
        async move {
            let mut conn = pool.acquire().await.unwrap();
            export_messages(
                &mut conn,
                ExportPageOpts {
                    account_id: 101,
                    query: "",
                    limit,
                    offset,
                    clock: crate::search::tests::clock(),

                    order: crate::db::conversation_messages::DEFAULT_MESSAGE_SORT.to_vec(),
                },
            )
            .await
            .unwrap()
        }
    };

    let first = page(2, 0).await;
    assert_eq!(first.items.len(), 2);
    assert_eq!((first.total, first.limit, first.offset), (3, 2, 0));

    let second = page(2, 2).await;
    assert_eq!(second.items.len(), 1);
    assert_eq!(second.items[0].id, 3);
    assert_eq!(second.total, 3);

    // Past the end is an empty page with the true total, not an error.
    let past = page(2, 10).await;
    assert!(past.items.is_empty());
    assert_eq!(past.total, 3);
}

#[tokio::test]
async fn the_export_route_answers_a_page_and_refuses_a_bad_limit() {
    let vault = crate::test_support::test_vault().await;
    let state = vault.state.clone();
    let user = crate::test_support::register_via_api(&state, "alice", "hunter2hunter2").await;
    crate::test_support::seed_one_message(&state, user.account_id).await;

    let page: serde_json::Value =
        crate::test_support::get_json(&state, "/v1/export/messages?q=&limit=10", &user.token).await;
    assert_eq!(page["total"], 1);
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    for gone in ["ok", "query", "messages", "next_cursor", "truncated"] {
        assert!(page.get(gone).is_none(), "{gone} must be gone: {page}");
    }

    let status =
        crate::test_support::get_status(&state, "/v1/export/messages?q=&limit=501", &user.token)
            .await;
    assert_eq!(status, axum::http::StatusCode::UNPROCESSABLE_ENTITY);

    let count: serde_json::Value =
        crate::test_support::get_json(&state, "/v1/export/messages/count?q=", &user.token).await;
    assert_eq!(count["messages"], 1);
    assert!(count.get("ok").is_none());
    assert!(count.get("query").is_none());
}

/// End-to-end placeholder discipline: the assembled export query's `$N`
/// placeholders must be exactly `1..=params.len()` in bind order.
#[tokio::test]
async fn export_sql_placeholders_match_params_order() {
    let (vault, _conv1, _conv2) = seeded_export_vault().await;
    let conn = vault.conn().await;
    let filter = message_filter(
        engine_of(&conn),
        101,
        r#"from:alice to:bo subject:hello tag:work ("alpha phrase" OR report*)"#,
        crate::search::tests::clock(),
    )
    .unwrap();
    let sql = format!(
        "SELECT m.id {messages_from_sql} WHERE {where_sql}",
        messages_from_sql = messages_from_sql(),
        where_sql = filter.where_sql(),
    );
    let renumbered = renumber_placeholders(&sql);
    assert!(
        !renumbered.contains('?'),
        "no `?` may survive: {renumbered}"
    );
    assert_eq!(renumbered.matches('$').count(), filter.params().len());
    for n in 1..=filter.params().len() {
        assert!(
            renumbered.contains(&format!("${n}")),
            "missing ${n}: {renumbered}"
        );
    }
}

/// The export route runs the search language, not a metadata subset. This
/// goes over HTTP rather than through the query builder, so a change to the
/// route's wiring is caught as well as a change to the compiler.
#[tokio::test]
async fn the_export_route_runs_the_search_language() {
    let vault = crate::test_support::test_vault().await;
    let user = crate::test_support::register_via_api(&vault.state, "alice", "hunter2hunter2").await;
    crate::test_support::seed_conversation(
        &vault.state,
        &crate::test_support::SeedConversation {
            account_id: user.account_id,
            handle: "+15555550100",
            conversation_type: "individual",
            group_title: None,
            source_file: "backup-a.jsonl",
            messages: &[
                crate::test_support::SeedMessage {
                    source: "imessage",
                    timestamp: "2020-01-01T00:00:00Z",
                    is_from_me: true,
                    body: "pizza tonight",
                },
                crate::test_support::SeedMessage {
                    source: "imessage",
                    timestamp: "2020-01-02T00:00:00Z",
                    is_from_me: false,
                    body: "salad tomorrow",
                },
            ],
        },
    )
    .await;

    let page: serde_json::Value = crate::test_support::get_json(
        &vault.state,
        "/v1/export/messages?q=pizza&limit=10",
        &user.token,
    )
    .await;
    assert_eq!(page["total"], 1, "free text must match one message: {page}");
    assert_eq!(page["items"][0]["text"], "pizza tonight");

    let negated: serde_json::Value = crate::test_support::get_json(
        &vault.state,
        "/v1/export/messages?q=NOT%20pizza&limit=10",
        &user.token,
    )
    .await;
    assert_eq!(negated["total"], 1, "NOT must be honoured: {negated}");
    assert_eq!(negated["items"][0]["text"], "salad tomorrow");
}

/// An unknown field is a 400 with a sentence, not an empty page.
#[tokio::test]
async fn the_export_route_refuses_a_word_the_language_does_not_have() {
    let vault = crate::test_support::test_vault().await;
    let user = crate::test_support::register_via_api(&vault.state, "alice", "hunter2hunter2").await;

    let (status, text) = crate::test_support::get_raw(
        &vault.state,
        "/v1/export/messages?q=wibble:yes",
        &user.token,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST, "{text}");
    let body: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(body["detail"].is_string(), "{body}");
}

/// Export must never reach another account's messages, whatever the query.
#[tokio::test]
async fn the_export_route_does_not_leak_another_account() {
    let vault = crate::test_support::test_vault().await;
    let alice =
        crate::test_support::register_via_api(&vault.state, "alice", "hunter2hunter2").await;
    let bob = crate::test_support::register_via_api(&vault.state, "bob", "hunter2hunter2").await;
    crate::test_support::seed_one_message(&vault.state, alice.account_id).await;

    let page: serde_json::Value =
        crate::test_support::get_json(&vault.state, "/v1/export/messages?q=&limit=50", &bob.token)
            .await;
    assert_eq!(page["total"], 0, "bob must see nothing of alice's: {page}");
    assert_eq!(page["items"].as_array().unwrap().len(), 0);

    let mine: serde_json::Value = crate::test_support::get_json(
        &vault.state,
        "/v1/export/messages?q=&limit=50",
        &alice.token,
    )
    .await;
    assert_eq!(mine["total"], 1, "alice must see her own message: {mine}");
}
