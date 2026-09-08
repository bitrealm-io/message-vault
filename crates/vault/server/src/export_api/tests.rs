use super::*;
use crate::problem::ProblemType;
use crate::test_support::{
    RegisteredAccount, SeedConversation, SeedMessage, TestVault, expect_problem, get_json, get_raw,
    get_status, post_created_json, post_json, post_raw, register_via_api, seed_conversation,
    test_vault,
};
use axum::http::StatusCode;
use serde_json::{Value, json};

/// The default sort every unit test below pages with.
fn oldest_first() -> Vec<SortKey<MessageSort>> {
    DEFAULT_MESSAGE_SORT.to_vec()
}

/// One page of `scope` for `account`, oldest first, through the same
/// function the route calls.
async fn page(
    conn: &mut AnyConnection,
    account: i64,
    scope: &ExportScope,
    limit: usize,
    offset: usize,
) -> Result<Page<Message>, ApiError> {
    export_messages(
        conn,
        ExportPageOpts {
            account_id: account,
            scope,
            limit,
            offset,
            clock: crate::search::tests::clock(),
            order: oldest_first(),
        },
    )
    .await
}

/// The ids a page holds, in page order.
fn ids(page: &Page<Message>) -> Vec<i64> {
    page.items.iter().map(|m| m.id).collect()
}

fn query(q: &str) -> ExportScope {
    ExportScope::Query { q: q.into() }
}

#[tokio::test]
async fn a_query_scope_takes_the_search_language() {
    let (pool, _dir, f) = crate::search::tests::seeded().await;
    let mut conn = pool.acquire().await.unwrap();
    let account = crate::search::tests::ACCOUNT;

    let found = page(&mut conn, account, &query("from:me avocado"), 50, 0)
        .await
        .unwrap();
    let mut got = ids(&found);
    got.sort_unstable();
    assert_eq!(got, vec![f.jane_avocado_from_me, f.sam_avocado_from_me]);

    // A word the language does not have is a 400, not a text search.
    let err = page(&mut conn, account, &query("sparkle:yes"), 50, 0)
        .await
        .unwrap_err();
    assert!(matches!(err, ApiError::SearchQueryInvalid { .. }));

    // A blank query is the `everything` form, and says so.
    let err = page(&mut conn, account, &query("   "), 50, 0)
        .await
        .unwrap_err();
    assert!(
        matches!(&err, ApiError::ValidationFailed(m) if m[0].starts_with("scope.q is blank")),
        "{err:?}"
    );
}

/// A vault with account `a1` (id 101) and two individual conversations
/// (`+1555`, `+1666`), each holding one SMS message ("hello one" in the
/// first, "hello two" in the second) with ids 1 and 2. Returns the
/// conversation ids the seeder made.
///
/// The messages are seeded with an explicit SQL insert rather than through
/// `seed_conversation`, because `SeedMessage` has no `service` field and a
/// test below asserts `message.service == Some("sms")`.
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

/// Add message `id` to `conversation` for account 101, dated on `day` of
/// January 2020.
async fn add_message(conn: &mut AnyConnection, id: i64, conversation: i64, day: u8, body: &str) {
    sqlx::query(
        "INSERT INTO messages (
            id, conversation_id, account_id, source, service, timestamp,
            is_from_me, sort_order, body
         ) VALUES ($1, $2, 101, 'sms', 'sms', $3, 0, 0, $4)",
    )
    .bind(id)
    .bind(conversation)
    .bind(format!("2020-01-{day:02}T00:00:00Z"))
    .bind(body)
    .execute(&mut *conn)
    .await
    .unwrap();
}

#[tokio::test]
async fn a_selection_scope_matches_by_conversation_or_message_with_the_browse_defaults() {
    let (vault, conv1, conv2) = seeded_export_vault().await;
    let mut conn = vault.conn().await;
    add_message(&mut conn, 3, conv2, 3, "hello three").await;

    // Every message of conv1, plus message 3 alone: 1 and 3, never 2.
    let picked = ExportScope::Selection {
        conversation_ids: vec![conv1],
        message_ids: vec![3],
    };
    let found = page(&mut conn, 101, &picked, 100, 0).await.unwrap();
    assert_eq!(ids(&found), vec![1, 3]);
    assert_eq!(found.total, 2);

    // Either list alone works.
    let by_conversation = ExportScope::Selection {
        conversation_ids: vec![conv2],
        message_ids: Vec::new(),
    };
    assert_eq!(
        ids(&page(&mut conn, 101, &by_conversation, 100, 0)
            .await
            .unwrap()),
        vec![2, 3]
    );
    let by_message = ExportScope::Selection {
        conversation_ids: Vec::new(),
        message_ids: vec![2],
    };
    assert_eq!(
        ids(&page(&mut conn, 101, &by_message, 100, 0).await.unwrap()),
        vec![2]
    );

    // A picked message in a trashed conversation, or a duplicate, stays
    // hidden the way a browse hides it.
    sqlx::query("INSERT INTO trashed_conversations (account_id, conversation_id) VALUES (101, $1)")
        .bind(conv1)
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query("UPDATE messages SET duplicate_of = 2 WHERE id = 3")
        .execute(&mut *conn)
        .await
        .unwrap();
    let found = page(&mut conn, 101, &picked, 100, 0).await.unwrap();
    assert!(found.items.is_empty(), "{:?}", ids(&found));
    assert_eq!(found.total, 0);
}

#[tokio::test]
async fn a_selection_refuses_ids_the_account_does_not_hold_naming_them() {
    let (vault, conv1, _conv2) = seeded_export_vault().await;
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

    let scope = ExportScope::Selection {
        conversation_ids: vec![conv1, 99, 4242],
        message_ids: vec![99, 1],
    };
    let err = page(&mut conn, 101, &scope, 100, 0).await.unwrap_err();
    let ApiError::ValidationFailed(errors) = err else {
        panic!("expected validation-failed, got {err:?}");
    };
    assert_eq!(
        errors,
        [
            "scope.conversation_ids: 99, 4242 not found for this account",
            "scope.message_ids: 99 not found for this account",
        ]
    );

    let empty = ExportScope::Selection {
        conversation_ids: Vec::new(),
        message_ids: Vec::new(),
    };
    let err = page(&mut conn, 101, &empty, 100, 0).await.unwrap_err();
    assert!(
        matches!(&err, ApiError::ValidationFailed(m) if m[0].contains("both empty")),
        "{err:?}"
    );

    let too_many = ExportScope::Selection {
        conversation_ids: Vec::new(),
        message_ids: (1..=(MAX_SELECTION_IDS as i64 + 1)).collect(),
    };
    let err = page(&mut conn, 101, &too_many, 100, 0).await.unwrap_err();
    assert!(
        matches!(&err, ApiError::ValidationFailed(m) if m[0].contains("at most 500")),
        "{err:?}"
    );
}

#[tokio::test]
async fn export_counts_count_messages_conversations_and_distinct_attachments() {
    let (vault, conv1, _conv2) = seeded_export_vault().await;
    let mut conn = vault.conn().await;
    add_message(&mut conn, 3, conv1, 3, "third").await;
    // The same file on two messages is one attachment, counted at its
    // largest known size; a file with no fingerprint is not an attachment
    // the export can fetch, so it is not counted.
    sqlx::query(
        "INSERT INTO attachments (message_id, path, original_name, mime_type, sha256, is_sticker, size_bytes)
         VALUES (1, 'attachments/a.pdf', 'a.pdf', 'application/pdf', 'ABC123', 0, 100),
                (3, 'attachments/a.pdf', 'a.pdf', 'application/pdf', 'abc123', 0, 120),
                (2, 'attachments/b.png', 'b.png', 'image/png', 'def456', 0, 7),
                (2, 'attachments/gone.bin', 'gone.bin', 'image/png', NULL, 0, 2048)",
    )
    .execute(&mut *conn)
    .await
    .unwrap();

    let clock = crate::search::tests::clock();
    let everything = scope_filter(&mut conn, 101, &ExportScope::Everything, clock)
        .await
        .unwrap();
    assert_eq!(
        export_counts(&mut conn, &everything).await.unwrap(),
        ExportCounts {
            messages: 3,
            conversations: 2,
            attachments: 2,
            total_bytes: 127,
        }
    );

    let one = scope_filter(&mut conn, 101, &query(&format!("in:#{conv1}")), clock)
        .await
        .unwrap();
    assert_eq!(
        export_counts(&mut conn, &one).await.unwrap(),
        ExportCounts {
            messages: 2,
            conversations: 1,
            attachments: 1,
            total_bytes: 120,
        }
    );
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

    let res = page(&mut conn, 101, &query(&format!("in:#{conv1}")), 100, 0)
        .await
        .unwrap();
    assert_eq!(res.items.len(), 1);
    assert_eq!(res.items[0].service.as_deref(), Some("sms"));
    assert_eq!(res.items[0].attachments.len(), 1);
    let att = &res.items[0].attachments[0];
    assert!(att.sha256.is_none());
    assert_eq!(att.missing_reason.as_deref(), Some("file_missing"));
    assert_eq!(att.original_name.as_deref(), Some("gone.bin"));
    assert_eq!(att.mime_type.as_deref(), Some("image/png"));
}

#[tokio::test]
async fn export_boolean_queries_preserve_or_and_and_not() {
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
    add_message(&mut conn, 3, conv1, 3, "foo bar").await;

    for (q, expected) in [
        ("foo OR bar", vec![1, 2, 3]),
        ("foo AND bar", vec![3]),
        ("foo AND NOT bar", vec![1]),
        ("NOT NOT bar", vec![2, 3]),
    ] {
        let found = page(&mut conn, 101, &query(q), 100, 0).await.unwrap();
        assert_eq!(ids(&found), expected, "query={q}");
    }
}

#[tokio::test]
async fn rejects_an_oversized_query() {
    let (vault, _conv1, _conv2) = seeded_export_vault().await;
    let mut conn = vault.conn().await;
    let huge = "x".repeat(crate::search::lex::MAX_QUERY_BYTES + 1);
    let err = page(&mut conn, 101, &query(&huge), 10, 0)
        .await
        .unwrap_err();
    assert!(
        matches!(&err, ApiError::SearchQueryInvalid { detail, .. } if detail.contains("longer than")),
        "{err:?}"
    );
}

#[tokio::test]
async fn export_pages_by_offset_and_reports_the_total() {
    let (vault, conv1, _conv2) = seeded_export_vault().await;
    let mut conn = vault.conn().await;
    add_message(&mut conn, 3, conv1, 3, "third").await;

    let first = page(&mut conn, 101, &ExportScope::Everything, 2, 0)
        .await
        .unwrap();
    assert_eq!(ids(&first), vec![1, 2]);
    assert_eq!((first.total, first.limit, first.offset), (3, 2, 0));

    let second = page(&mut conn, 101, &ExportScope::Everything, 2, 2)
        .await
        .unwrap();
    assert_eq!(ids(&second), vec![3]);
    assert_eq!(second.total, 3);

    // Past the end is an empty page with the true total, not an error.
    let past = page(&mut conn, 101, &ExportScope::Everything, 2, 10)
        .await
        .unwrap();
    assert!(past.items.is_empty());
    assert_eq!(past.total, 3);
}

/// End-to-end placeholder discipline: the assembled query's `$N`
/// placeholders must be exactly `1..=params.len()` in bind order, with the
/// selection's own `?` list renumbered after the query's.
#[tokio::test]
async fn export_sql_placeholders_match_params_order() {
    let (vault, conv1, conv2) = seeded_export_vault().await;
    let mut conn = vault.conn().await;
    let scope = ExportScope::Selection {
        conversation_ids: vec![conv1, conv2],
        message_ids: vec![1],
    };
    let filter = scope_filter(&mut conn, 101, &scope, crate::search::tests::clock())
        .await
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

// ── The routes ───────────────────────────────────────────────────────────────

/// An account with two conversations over HTTP: `+15555550100` holding
/// "pizza tonight" and "salad tomorrow", and `+15555550101` holding "the
/// menu"; the menu message carries one 13-byte attachment. Returns the vault,
/// the account, and the two conversation ids.
async fn vault_with_two_conversations() -> (TestVault, RegisteredAccount, i64, i64) {
    let vault = test_vault().await;
    let alice = register_via_api(&vault.state, "alice", "hunter2hunter2").await;
    let dinner = seed_conversation(
        &vault.state,
        &SeedConversation {
            account_id: alice.account_id,
            handle: "+15555550100",
            conversation_type: "individual",
            group_title: None,
            source_file: "backup-a.jsonl",
            messages: &[
                SeedMessage {
                    source: "imessage",
                    timestamp: "2020-01-01T00:00:00Z",
                    is_from_me: true,
                    body: "pizza tonight",
                },
                SeedMessage {
                    source: "imessage",
                    timestamp: "2020-01-02T00:00:00Z",
                    is_from_me: false,
                    body: "salad tomorrow",
                },
            ],
        },
    )
    .await;
    let menu = seed_conversation(
        &vault.state,
        &SeedConversation {
            account_id: alice.account_id,
            handle: "+15555550101",
            conversation_type: "individual",
            group_title: None,
            source_file: "backup-a.jsonl",
            messages: &[SeedMessage {
                source: "imessage",
                timestamp: "2020-01-03T00:00:00Z",
                is_from_me: false,
                body: "the menu",
            }],
        },
    )
    .await;
    let mut conn = vault.conn().await;
    sqlx::query(
        "INSERT INTO attachments (message_id, path, original_name, mime_type, sha256, is_sticker, size_bytes)
         SELECT id, 'attachments/menu.pdf', 'menu.pdf', 'application/pdf', 'abc123', 0, 13
         FROM messages WHERE conversation_id = $1",
    )
    .bind(menu)
    .execute(&mut *conn)
    .await
    .unwrap();
    (vault, alice, dinner, menu)
}

/// The message ids of `conversation`, oldest first.
async fn message_ids(vault: &TestVault, conversation: i64) -> Vec<i64> {
    let mut conn = vault.conn().await;
    sqlx::query_scalar("SELECT id FROM messages WHERE conversation_id = $1 ORDER BY sort_order, id")
        .bind(conversation)
        .fetch_all(&mut *conn)
        .await
        .unwrap()
}

/// `POST /v1/exports` for `scope`, asserting `201 Created` and that the
/// `Location` names the run the body carries.
async fn create_run(vault: &TestVault, token: &str, scope: Value) -> Value {
    let (location, run): (String, Value) = post_created_json(
        &vault.state,
        "/v1/exports",
        token,
        json!({ "scope": scope, "tool": "  tests  " }),
    )
    .await;
    assert_eq!(location, format!("/v1/exports/{}", run["id"]));
    run
}

/// An API token for `user` with the export scope on or off.
async fn api_token(vault: &TestVault, user: &RegisteredAccount, can_export: bool) -> String {
    let (_location, created): (String, Value) = post_created_json(
        &vault.state,
        &format!("/v1/accounts/{}/api-tokens", user.account_id),
        &user.token,
        json!({ "label": "pull", "can_import": false, "can_export": can_export }),
    )
    .await;
    created["token"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn creating_a_run_records_the_scope_the_tool_and_the_counts() {
    let (vault, alice, dinner, menu) = vault_with_two_conversations().await;
    let menu_message = message_ids(&vault, menu).await[0];

    let everything = create_run(&vault, &alice.token, json!({ "kind": "everything" })).await;
    assert_eq!(everything["scope"], json!({ "kind": "everything" }));
    assert_eq!(everything["tool"], "tests");
    assert_eq!(everything["status"], "running");
    assert!(everything["started_at"].is_string());
    assert!(everything["finished_at"].is_null());
    assert_eq!(everything["message_count"], 3);
    assert_eq!(everything["conversation_count"], 2);
    assert_eq!(everything["attachment_count"], 1);
    assert_eq!(everything["total_bytes"], 13);
    assert_eq!(everything["messages_delivered"], 0);

    let by_query = create_run(
        &vault,
        &alice.token,
        json!({ "kind": "query", "q": "pizza" }),
    )
    .await;
    assert_eq!(by_query["scope"], json!({ "kind": "query", "q": "pizza" }));
    assert_eq!(by_query["message_count"], 1);
    assert_eq!(by_query["conversation_count"], 1);
    assert_eq!(by_query["attachment_count"], 0);

    let picked = create_run(
        &vault,
        &alice.token,
        json!({ "kind": "selection", "conversation_ids": [dinner], "message_ids": [menu_message] }),
    )
    .await;
    assert_eq!(
        picked["scope"],
        json!({ "kind": "selection", "conversation_ids": [dinner], "message_ids": [menu_message] })
    );
    assert_eq!(picked["message_count"], 3);
    assert_eq!(picked["conversation_count"], 2);
    assert_eq!(picked["attachment_count"], 1);
    assert_eq!(picked["total_bytes"], 13);

    // A selection with one list left out stores the other as given and the
    // missing one as empty.
    let one_list = create_run(
        &vault,
        &alice.token,
        json!({ "kind": "selection", "message_ids": [menu_message] }),
    )
    .await;
    assert_eq!(
        one_list["scope"],
        json!({ "kind": "selection", "conversation_ids": [], "message_ids": [menu_message] })
    );
    assert_eq!(one_list["message_count"], 1);

    // A run with no tool stores null, not an empty string.
    let (_location, untooled): (String, Value) = post_created_json(
        &vault.state,
        "/v1/exports",
        &alice.token,
        json!({ "scope": { "kind": "everything" } }),
    )
    .await;
    assert!(untooled["tool"].is_null());
}

#[tokio::test]
async fn a_scope_the_vault_cannot_honour_is_refused_and_no_run_is_recorded() {
    let (vault, alice, _dinner, _menu) = vault_with_two_conversations().await;
    let bob = register_via_api(&vault.state, "bob", "hunter2hunter2").await;
    crate::test_support::seed_one_message(&vault.state, bob.account_id).await;
    let mut conn = vault.conn().await;
    let bobs_conversation: i64 =
        sqlx::query_scalar("SELECT id FROM conversations WHERE account_id = $1")
            .bind(bob.account_id)
            .fetch_one(&mut *conn)
            .await
            .unwrap();

    let (status, text) = post_raw(
        &vault.state,
        "/v1/exports",
        &alice.token,
        "application/json",
        json!({ "scope": { "kind": "selection", "conversation_ids": [bobs_conversation, 4242] } })
            .to_string(),
    )
    .await;
    let problem = expect_problem(status, &text, ProblemType::ValidationFailed);
    assert_eq!(
        problem.errors.unwrap(),
        [format!(
            "scope.conversation_ids: {bobs_conversation}, 4242 not found for this account"
        )]
    );

    let (status, text) = post_raw(
        &vault.state,
        "/v1/exports",
        &alice.token,
        "application/json",
        json!({ "scope": { "kind": "selection" } }).to_string(),
    )
    .await;
    expect_problem(status, &text, ProblemType::ValidationFailed);

    let (status, text) = post_raw(
        &vault.state,
        "/v1/exports",
        &alice.token,
        "application/json",
        json!({ "scope": { "kind": "query", "q": " " } }).to_string(),
    )
    .await;
    expect_problem(status, &text, ProblemType::ValidationFailed);

    let (status, text) = post_raw(
        &vault.state,
        "/v1/exports",
        &alice.token,
        "application/json",
        json!({ "scope": { "kind": "query", "q": "wibble:yes" } }).to_string(),
    )
    .await;
    expect_problem(status, &text, ProblemType::SearchQueryInvalid);

    // A body that parsed as JSON and then named a kind the vault does not
    // have broke a rule, which is 422 like every other field.
    let (status, text) = post_raw(
        &vault.state,
        "/v1/exports",
        &alice.token,
        "application/json",
        json!({ "scope": { "kind": "backup" } }).to_string(),
    )
    .await;
    let problem = expect_problem(status, &text, ProblemType::ValidationFailed);
    assert!(
        problem.errors.unwrap()[0].contains("unknown variant `backup`"),
        "{text}"
    );

    let list: Value = get_json(&vault.state, "/v1/exports", &alice.token).await;
    assert_eq!(list["total"], 0, "a refused scope records nothing: {list}");
}

#[tokio::test]
async fn the_list_is_newest_first_filters_by_status_and_refuses_unknown_values() {
    let (vault, alice, _dinner, _menu) = vault_with_two_conversations().await;
    let first = create_run(&vault, &alice.token, json!({ "kind": "everything" })).await;
    let second = create_run(
        &vault,
        &alice.token,
        json!({ "kind": "query", "q": "pizza" }),
    )
    .await;
    let cancelled: Value = post_json(
        &vault.state,
        &format!("/v1/exports/{}/cancel", first["id"]),
        &alice.token,
        json!({}),
    )
    .await;
    assert_eq!(cancelled["status"], "cancelled");

    let list: Value = get_json(&vault.state, "/v1/exports", &alice.token).await;
    assert_eq!(list["total"], 2);
    assert_eq!(list["limit"], 40);
    assert_eq!(list["offset"], 0);
    let listed: Vec<&Value> = list["items"].as_array().unwrap().iter().collect();
    assert_eq!(listed[0]["id"], second["id"], "newest first: {list}");
    assert_eq!(listed[1]["id"], first["id"]);
    assert_eq!(listed[1]["status"], "cancelled");

    let oldest_first: Value =
        get_json(&vault.state, "/v1/exports?sort=started_at", &alice.token).await;
    assert_eq!(oldest_first["items"][0]["id"], first["id"]);

    let running: Value = get_json(&vault.state, "/v1/exports?status=running", &alice.token).await;
    assert_eq!(running["total"], 1);
    assert_eq!(running["items"][0]["id"], second["id"]);

    let (status, text) = get_raw(&vault.state, "/v1/exports?status=bogus", &alice.token).await;
    let problem = expect_problem(status, &text, ProblemType::ValidationFailed);
    assert_eq!(
        problem.errors.unwrap(),
        [
            "status: unknown value 'bogus'; accepted values are running, completed, failed, cancelled"
        ]
    );
    let (status, text) = get_raw(&vault.state, "/v1/exports?sort=colour", &alice.token).await;
    expect_problem(status, &text, ProblemType::ValidationFailed);
}

#[tokio::test]
async fn a_run_belongs_to_its_account() {
    let (vault, alice, _dinner, _menu) = vault_with_two_conversations().await;
    let bob = register_via_api(&vault.state, "bob", "hunter2hunter2").await;
    let run = create_run(&vault, &alice.token, json!({ "kind": "everything" })).await;
    let id = run["id"].as_i64().unwrap();

    let mine: Value = get_json(&vault.state, &format!("/v1/exports/{id}"), &alice.token).await;
    assert_eq!(mine, run);

    for path in [
        format!("/v1/exports/{id}"),
        format!("/v1/exports/{id}/messages"),
    ] {
        let (status, text) = get_raw(&vault.state, &path, &bob.token).await;
        expect_problem(status, &text, ProblemType::NotFound);
    }
    for action in ["complete", "cancel"] {
        let (status, text) = post_raw(
            &vault.state,
            &format!("/v1/exports/{id}/{action}"),
            &bob.token,
            "application/json",
            "{}",
        )
        .await;
        expect_problem(status, &text, ProblemType::NotFound);
    }
    let bobs: Value = get_json(&vault.state, "/v1/exports", &bob.token).await;
    assert_eq!(bobs["total"], 0);
}

#[tokio::test]
async fn paging_a_run_raises_messages_delivered_to_the_rows_handed_over() {
    let (vault, alice, _dinner, _menu) = vault_with_two_conversations().await;
    let run = create_run(&vault, &alice.token, json!({ "kind": "everything" })).await;
    let id = run["id"].as_i64().unwrap();
    let delivered = |vault: &TestVault| {
        let token = alice.token.clone();
        let state = vault.state.clone();
        async move {
            let run: Value = get_json(&state, &format!("/v1/exports/{id}"), &token).await;
            run["messages_delivered"].as_i64().unwrap()
        }
    };

    let first: Value = get_json(
        &vault.state,
        &format!("/v1/exports/{id}/messages?limit=2"),
        &alice.token,
    )
    .await;
    assert_eq!(first["total"], 3);
    assert_eq!(first["limit"], 2);
    assert_eq!(first["offset"], 0);
    assert_eq!(
        first["items"][0]["text"], "pizza tonight",
        "oldest first: {first}"
    );
    assert_eq!(first["items"][1]["text"], "salad tomorrow");
    assert_eq!(delivered(&vault).await, 2);

    let second: Value = get_json(
        &vault.state,
        &format!("/v1/exports/{id}/messages?limit=2&offset=2"),
        &alice.token,
    )
    .await;
    assert_eq!(second["items"].as_array().unwrap().len(), 1);
    assert_eq!(second["items"][0]["text"], "the menu");
    assert_eq!(second["items"][0]["attachments"][0]["sha256"], "abc123");
    assert_eq!(delivered(&vault).await, 3);

    // Reading a page again does not count it twice, and a page past the end
    // is empty rather than an error: export has no offset cap.
    let _again: Value = get_json(
        &vault.state,
        &format!("/v1/exports/{id}/messages?limit=2"),
        &alice.token,
    )
    .await;
    assert_eq!(delivered(&vault).await, 3);
    let past: Value = get_json(
        &vault.state,
        &format!("/v1/exports/{id}/messages?offset=60000"),
        &alice.token,
    )
    .await;
    assert_eq!(past["total"], 3);
    assert!(past["items"].as_array().unwrap().is_empty());
    assert_eq!(delivered(&vault).await, 3);

    let newest_first: Value = get_json(
        &vault.state,
        &format!("/v1/exports/{id}/messages?sort=-date"),
        &alice.token,
    )
    .await;
    assert_eq!(newest_first["items"][0]["text"], "the menu");
    assert_eq!(newest_first["limit"], DEFAULT_EXPORT_LIMIT);

    let (status, text) = get_raw(
        &vault.state,
        &format!("/v1/exports/{id}/messages?limit=501"),
        &alice.token,
    )
    .await;
    expect_problem(status, &text, ProblemType::ValidationFailed);
}

#[tokio::test]
async fn a_finished_run_refuses_pages_and_a_second_close() {
    let (vault, alice, _dinner, _menu) = vault_with_two_conversations().await;
    let run = create_run(&vault, &alice.token, json!({ "kind": "everything" })).await;
    let id = run["id"].as_i64().unwrap();

    let completed: Value = post_json(
        &vault.state,
        &format!("/v1/exports/{id}/complete"),
        &alice.token,
        json!({}),
    )
    .await;
    assert_eq!(completed["id"], id);
    assert_eq!(completed["status"], "completed");
    assert!(completed["finished_at"].is_string(), "{completed}");
    assert_eq!(completed["message_count"], 3);

    let (status, text) = get_raw(
        &vault.state,
        &format!("/v1/exports/{id}/messages"),
        &alice.token,
    )
    .await;
    let problem = expect_problem(status, &text, ProblemType::StateConflict);
    assert_eq!(
        problem.detail.as_deref(),
        Some(format!("export {id} is not running (status=completed)").as_str())
    );
    for action in ["complete", "cancel"] {
        let (status, text) = post_raw(
            &vault.state,
            &format!("/v1/exports/{id}/{action}"),
            &alice.token,
            "application/json",
            "{}",
        )
        .await;
        expect_problem(status, &text, ProblemType::StateConflict);
    }

    let other = create_run(&vault, &alice.token, json!({ "kind": "everything" })).await;
    let other_id = other["id"].as_i64().unwrap();
    let cancelled: Value = post_json(
        &vault.state,
        &format!("/v1/exports/{other_id}/cancel"),
        &alice.token,
        json!({}),
    )
    .await;
    assert_eq!(cancelled["status"], "cancelled");
    assert!(cancelled["finished_at"].is_string());
    let (status, text) = post_raw(
        &vault.state,
        &format!("/v1/exports/{other_id}/complete"),
        &alice.token,
        "application/json",
        "{}",
    )
    .await;
    expect_problem(status, &text, ProblemType::StateConflict);
}

#[tokio::test]
async fn an_export_token_reads_messages_only_through_a_run() {
    let (vault, alice, _dinner, _menu) = vault_with_two_conversations().await;
    let token = api_token(&vault, &alice, true).await;

    let run = create_run(&vault, &token, json!({ "kind": "query", "q": "pizza" })).await;
    let id = run["id"].as_i64().unwrap();
    let page: Value = get_json(&vault.state, &format!("/v1/exports/{id}/messages"), &token).await;
    assert_eq!(page["total"], 1);
    assert_eq!(page["items"][0]["text"], "pizza tonight");
    let closed: Value = post_json(
        &vault.state,
        &format!("/v1/exports/{id}/complete"),
        &token,
        json!({}),
    )
    .await;
    assert_eq!(closed["status"], "completed");
    assert_eq!(closed["messages_delivered"], 1);

    // The same token is refused from every browse route: outside a run,
    // reading messages needs a session.
    for path in [
        "/v1/messages",
        "/v1/conversations",
        "/v1/conversations/1/messages",
    ] {
        let (status, text) = get_raw(&vault.state, path, &token).await;
        expect_problem(status, &text, ProblemType::InsufficientScope);
    }

    let without_export = api_token(&vault, &alice, false).await;
    let (status, text) = post_raw(
        &vault.state,
        "/v1/exports",
        &without_export,
        "application/json",
        json!({ "scope": { "kind": "everything" } }).to_string(),
    )
    .await;
    expect_problem(status, &text, ProblemType::InsufficientScope);
    assert_eq!(
        get_status(&vault.state, "/v1/exports", &without_export).await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn the_old_export_routes_are_gone() {
    let (vault, alice, _dinner, _menu) = vault_with_two_conversations().await;
    for path in ["/v1/export/messages?q=", "/v1/export/messages/count?q="] {
        let (status, text) = get_raw(&vault.state, path, &alice.token).await;
        expect_problem(status, &text, ProblemType::NotFound);
    }
}
