use super::*;
use message_ir::HandleType;

use crate::db::{account_profile, vault_imports};
use crate::test_support::{
    RegisteredAccount, TestVault, register_via_api, seed_one_message, test_vault,
};

/// A newest-first page — the default ordering, which is what most of these
/// tests care about. Ordering itself is covered by its own tests below.
async fn list_conversations(
    conn: &mut AnyConnection,
    account_id: &str,
    q: &str,
    limit: usize,
    offset: usize,
) -> Result<Page<ConversationSummary>, ApiError> {
    list_conversations_sorted(
        conn,
        account_id,
        q,
        &DEFAULT_CONVERSATION_SORT,
        limit,
        offset,
        crate::search::tests::clock(),
    )
    .await
}

/// A vault, a signed-in account, and one conversation holding one message.
async fn conversations_fixture() -> (TestVault, String, RegisteredAccount) {
    let vault = test_vault().await;
    let account = register_via_api(&vault.state, "alice", "hunter2hunter2").await;
    seed_one_message(&vault.state, &account.account_id).await;
    let token = account.token.clone();
    (vault, token, account)
}

#[tokio::test]
async fn conversation_list_takes_the_search_language() {
    let (vault, token, _account) = conversations_fixture().await;
    let page: serde_json::Value =
        crate::test_support::get_json(&vault.state, "/v1/conversations?q=kind:direct", &token)
            .await;
    assert!(page["total"].as_u64().unwrap() >= 1);
    let status =
        crate::test_support::get_status(&vault.state, "/v1/conversations?q=wibble:direct", &token)
            .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    let status =
        crate::test_support::get_status(&vault.state, "/v1/conversations?q=trashed:yes", &token)
            .await;
    assert_eq!(status, axum::http::StatusCode::OK);
}

/// A vault with account `00000000-0000-4000-8000-0000000000c2` and one
/// conversation (id 1) on a handle linked through the account profile,
/// with one participant and one message.
///
/// The peer handle goes through `account_profile::link_account_handle`
/// rather than `seed_conversation`, because the participant-naming query
/// reads the `account_handles` link that call creates and
/// `seed_conversation`'s bare `handles` insert does not make one; the
/// `participants` row (`name_alias`) that query also reads has no
/// counterpart in the seeder at all. So this stays as explicit SQL
/// rather than using the shared seeder.
async fn conversations_setup() -> (sqlx::AnyPool, TestVault, String) {
    let vault = test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000c2", "alice")
        .await;
    let mut conn = vault.conn().await;
    let peer = account_profile::link_account_handle(
        &mut conn,
        &account,
        "+15555550200",
        HandleType::Phone,
    )
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO conversations (
            id, account_id, chat_handle_id, conversation_type, source_file
         ) VALUES (1, $1, $2, 'individual', 'c.jsonl')",
    )
    .bind(&account)
    .bind(peer)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO participants (conversation_id, handle_id, name_alias)
         VALUES (1, $1, 'Sam')",
    )
    .bind(peer)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO messages (
            conversation_id, account_id, source, timestamp, is_from_me, sort_order, body
         ) VALUES (1, $1, 'imessage', '2024-06-01T12:00:00Z', 0, 0, 'hello')",
    )
    .bind(&account)
    .execute(&mut *conn)
    .await
    .unwrap();
    let pool = vault.state.db.clone();
    (pool, vault, account)
}

#[tokio::test]
async fn list_conversations_returns_summary() {
    let (pool, _vault, account) = conversations_setup().await;
    let mut conn = pool.acquire().await.unwrap();
    let page = list_conversations(&mut conn, &account, "", DEFAULT_LIST_LIMIT, 0)
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, 1);
    assert_eq!(page.items[0].message_count, 1);
    assert!(!page.items[0].is_group);
    assert_eq!(page.items[0].participants.len(), 1);
    assert_eq!(
        page.items[0].participants[0].handle,
        Some("+15555550200".to_string())
    );
}

#[tokio::test]
async fn list_conversations_filters_by_handle() {
    let (pool, _vault, account) = conversations_setup().await;
    let mut conn = pool.acquire().await.unwrap();
    let hit = list_conversations(
        &mut conn,
        &account,
        "handle:+15555550200",
        DEFAULT_LIST_LIMIT,
        0,
    )
    .await
    .unwrap();
    assert_eq!(hit.total, 1);
    assert_eq!(hit.items.len(), 1);
    let miss = list_conversations(
        &mut conn,
        &account,
        "handle:+19999999999",
        DEFAULT_LIST_LIMIT,
        0,
    )
    .await
    .unwrap();
    assert_eq!(miss.total, 0);
    assert!(miss.items.is_empty());
}

#[tokio::test]
async fn list_conversations_finds_a_handle_across_platforms() {
    let (pool, _vault, account) = conversations_setup().await;
    let mut conn = pool.acquire().await.unwrap();
    // conversations_setup() already has phone:+15555550200 as conversation 1.
    let wa = account_profile::link_account_handle_with_service(
        &mut conn,
        &account,
        "+15555550200",
        HandleType::Phone,
        Some("whatsapp"),
    )
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO conversations (
            id, account_id, chat_handle_id, conversation_type, source_file
         ) VALUES (10, $1, $2, 'individual', 'wa.jsonl')",
    )
    .bind(&account)
    .bind(wa)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO participants (conversation_id, handle_id, name_alias)
         VALUES (10, $1, 'Sam WA')",
    )
    .bind(wa)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO messages (
            conversation_id, account_id, source, timestamp, is_from_me, sort_order, body
         ) VALUES (10, $1, 'whatsapp', '2024-08-01T12:00:00Z', 0, 0, 'wa hello')",
    )
    .bind(&account)
    .execute(&mut *conn)
    .await
    .unwrap();

    // `handle:` matches the raw value on any platform; it does not
    // distinguish which platform a handle belongs to (there is no search
    // word for that in the current language — `service:` filters by a
    // message's own transport, imessage/sms/mms/rcs/whatsapp, which is a
    // different thing and is covered by the search module's own tests).
    let any_platform = list_conversations(
        &mut conn,
        &account,
        "handle:+15555550200",
        DEFAULT_LIST_LIMIT,
        0,
    )
    .await
    .unwrap();
    assert_eq!(any_platform.total, 2);
}

#[tokio::test]
async fn list_conversations_sorts_by_date_or_message_count() {
    let (pool, _vault, account) = conversations_setup().await;
    let mut conn = pool.acquire().await.unwrap();

    // Conversation 1 (from setup) gets two more *older* messages, so it is
    // the busiest thread but not the most recent one. Conversation 2 gets a
    // single *newer* message. Date order and count order then disagree,
    // which is what makes this test able to tell them apart.
    sqlx::query(
        "INSERT INTO messages (
            conversation_id, account_id, source, timestamp, is_from_me, sort_order, body
         ) VALUES
            (1, $1, 'imessage', '2024-05-01T12:00:00Z', 0, 1, 'older'),
            (1, $1, 'imessage', '2024-05-02T12:00:00Z', 0, 2, 'older still')",
    )
    .bind(&account)
    .execute(&mut *conn)
    .await
    .unwrap();

    let peer2 = account_profile::link_account_handle(
        &mut conn,
        &account,
        "+15555550300",
        HandleType::Phone,
    )
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO conversations (
            id, account_id, chat_handle_id, conversation_type, source_file
         ) VALUES (2, $1, $2, 'individual', 'c2.jsonl')",
    )
    .bind(&account)
    .bind(peer2)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO messages (
            conversation_id, account_id, source, timestamp, is_from_me, sort_order, body
         ) VALUES (2, $1, 'imessage', '2024-07-01T12:00:00Z', 0, 0, 'newest')",
    )
    .bind(&account)
    .execute(&mut *conn)
    .await
    .unwrap();

    async fn ids_for(
        pool: &sqlx::AnyPool,
        account: &str,
        key: ConversationSort,
        direction: crate::paging::Direction,
    ) -> Vec<i64> {
        let mut conn = pool.acquire().await.unwrap();
        list_conversations_sorted(
            &mut conn,
            account,
            "",
            &[crate::paging::SortKey { key, direction }],
            DEFAULT_LIST_LIMIT,
            0,
            crate::search::tests::clock(),
        )
        .await
        .unwrap()
        .items
        .iter()
        .map(|c| c.id)
        .collect()
    }

    // 3 messages ending 2024-06-01 (id 1) vs 1 message on 2024-07-01 (id 2).
    assert_eq!(
        ids_for(
            &pool,
            &account,
            ConversationSort::Date,
            crate::paging::Direction::Desc
        )
        .await,
        [2, 1],
        "newest activity first"
    );
    assert_eq!(
        ids_for(
            &pool,
            &account,
            ConversationSort::Date,
            crate::paging::Direction::Asc
        )
        .await,
        [1, 2],
        "oldest activity first"
    );
    assert_eq!(
        ids_for(
            &pool,
            &account,
            ConversationSort::Messages,
            crate::paging::Direction::Desc
        )
        .await,
        [1, 2],
        "busiest thread first"
    );
    assert_eq!(
        ids_for(
            &pool,
            &account,
            ConversationSort::Messages,
            crate::paging::Direction::Asc
        )
        .await,
        [2, 1],
        "quietest thread first"
    );
}

#[tokio::test]
async fn list_conversations_paginates() {
    let (pool, _vault, account) = conversations_setup().await;
    let mut conn = pool.acquire().await.unwrap();
    // Second conversation + message.
    let peer2 = account_profile::link_account_handle(
        &mut conn,
        &account,
        "+15555550300",
        HandleType::Phone,
    )
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO conversations (
            id, account_id, chat_handle_id, conversation_type, source_file
         ) VALUES (2, $1, $2, 'individual', 'c2.jsonl')",
    )
    .bind(&account)
    .bind(peer2)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO messages (
            conversation_id, account_id, source, timestamp, is_from_me, sort_order, body
         ) VALUES (2, $1, 'imessage', '2024-07-01T12:00:00Z', 0, 0, 'later')",
    )
    .bind(&account)
    .execute(&mut *conn)
    .await
    .unwrap();

    let page0 = list_conversations(&mut conn, &account, "", 1, 0)
        .await
        .unwrap();
    assert_eq!(page0.total, 2);
    assert_eq!(page0.limit, 1);
    assert_eq!(page0.offset, 0);
    assert_eq!(page0.items.len(), 1);
    assert_eq!(page0.items[0].id, 2); // newer first

    let page1 = list_conversations(&mut conn, &account, "", 1, 1)
        .await
        .unwrap();
    assert_eq!(page1.total, 2);
    assert_eq!(page1.offset, 1);
    assert_eq!(page1.items.len(), 1);
    assert_eq!(page1.items[0].id, 1);

    let by_text = list_conversations(&mut conn, &account, "5555550300", 10, 0)
        .await
        .unwrap();
    assert_eq!(by_text.total, 1);
    assert_eq!(by_text.items[0].id, 2);
}

#[tokio::test]
async fn list_queries_enforce_search_limits() {
    let (pool, _vault, account) = conversations_setup().await;
    let mut conn = pool.acquire().await.unwrap();
    let oversized = "x".repeat(2_049);
    let too_many_terms = (0..33)
        .map(|index| format!("term{index}"))
        .collect::<Vec<_>>()
        .join(" ");
    let too_many_nodes = "(".repeat(65);

    for query in [&oversized, &too_many_terms, &too_many_nodes] {
        let contact_error = crate::contacts_api::list_contacts_sorted(
            &mut conn,
            &account,
            query,
            &crate::contacts_api::DEFAULT_CONTACT_SORT,
            DEFAULT_LIST_LIMIT,
            0,
            crate::search::tests::clock(),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(contact_error, ApiError::SearchQueryInvalid { .. }),
            "contact query should be rejected: {query}"
        );

        let conversation_error =
            list_conversations(&mut conn, &account, query, DEFAULT_LIST_LIMIT, 0)
                .await
                .unwrap_err();
        assert!(
            matches!(conversation_error, ApiError::SearchQueryInvalid { .. }),
            "conversation query should be rejected: {query}"
        );
    }
}

#[tokio::test]
async fn malformed_boolean_queries_are_bad_requests_for_export() {
    let (pool, _vault, account) = conversations_setup().await;
    let mut conn = pool.acquire().await.unwrap();

    for query in ["foo OR", "(foo OR bar", "foo OR bar)"] {
        let export_error = crate::export_api::export_message_count(
            &mut conn,
            crate::export_api::ExportCountOpts {
                account_id: &account,
                query,
                clock: crate::search::tests::clock(),
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(export_error, ApiError::SearchQueryInvalid { .. }));
    }
}

#[tokio::test]
async fn list_conversations_filters_by_contact_and_type() {
    let (pool, _vault, account) = conversations_setup().await;
    let mut conn = pool.acquire().await.unwrap();
    // Link peer handle to a contact.
    sqlx::query("INSERT INTO contacts (account_id, preferred_name) VALUES ($1, 'Sam')")
        .bind(&account)
        .execute(&mut *conn)
        .await
        .unwrap();
    let contact_id: i64 = sqlx::query_scalar("SELECT id FROM contacts WHERE account_id = $1")
        .bind(&account)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    let peer_handle_id: i64 =
        sqlx::query_scalar("SELECT id FROM handles WHERE account_id = $1 AND raw = $2")
            .bind(&account)
            .bind("+15555550200")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO contact_handles (account_id, handle_id, contact_id)
         VALUES ($1, $2, $3)",
    )
    .bind(&account)
    .bind(peer_handle_id)
    .bind(contact_id)
    .execute(&mut *conn)
    .await
    .unwrap();

    // Unrelated group conversation (no link to Sam).
    let other = account_profile::link_account_handle(
        &mut conn,
        &account,
        "+15555550999",
        HandleType::Phone,
    )
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO conversations (
            id, account_id, chat_handle_id, conversation_type, group_title, source_file
         ) VALUES (9, $1, $2, 'group', 'Other', 'g.jsonl')",
    )
    .bind(&account)
    .bind(other)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO messages (
            conversation_id, account_id, source, timestamp, is_from_me, sort_order, body
         ) VALUES (9, $1, 'imessage', '2024-08-01T12:00:00Z', 0, 0, 'group')",
    )
    .bind(&account)
    .execute(&mut *conn)
    .await
    .unwrap();

    // Group that includes Sam (distinct chat handle; Sam is a participant).
    let group_chat =
        account_profile::link_account_handle(&mut conn, &account, "chat123456", HandleType::Other)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO conversations (
            id, account_id, chat_handle_id, conversation_type, group_title, source_file
         ) VALUES (3, $1, $2, 'group', 'Sam Group', 'sg.jsonl')",
    )
    .bind(&account)
    .bind(group_chat)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO participants (conversation_id, handle_id, name_alias)
         VALUES (3, $1, 'Sam')",
    )
    .bind(peer_handle_id)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO messages (
            conversation_id, account_id, source, timestamp, is_from_me, sort_order, body
         ) VALUES (3, $1, 'imessage', '2024-09-01T12:00:00Z', 0, 0, 'hi group')",
    )
    .bind(&account)
    .execute(&mut *conn)
    .await
    .unwrap();

    let all = list_conversations(
        &mut conn,
        &account,
        &format!("with:#{contact_id}"),
        DEFAULT_LIST_LIMIT,
        0,
    )
    .await
    .unwrap();
    assert_eq!(all.total, 2);
    let ids: Vec<i64> = all.items.iter().map(|c| c.id).collect();
    assert!(ids.contains(&1));
    assert!(ids.contains(&3));

    let direct = list_conversations(
        &mut conn,
        &account,
        &format!("with:#{contact_id} kind:direct"),
        DEFAULT_LIST_LIMIT,
        0,
    )
    .await
    .unwrap();
    assert_eq!(direct.total, 1);
    assert_eq!(direct.items[0].id, 1);
    assert!(!direct.items[0].is_group);

    let groups = list_conversations(
        &mut conn,
        &account,
        &format!("with:#{contact_id} kind:group"),
        DEFAULT_LIST_LIMIT,
        0,
    )
    .await
    .unwrap();
    assert_eq!(groups.total, 1);
    assert_eq!(groups.items[0].id, 3);
    assert!(groups.items[0].is_group);
}

/// A newest-first page for `conversations_setup()`'s account, with the default query and
/// paging — what each of the three participant-naming tests below needs.
async fn list_conversations_page(
    conn: &mut AnyConnection,
    account_id: &str,
) -> Page<ConversationSummary> {
    list_conversations(conn, account_id, "", DEFAULT_LIST_LIMIT, 0)
        .await
        .unwrap()
}

fn find_participant<'a>(
    page: &'a crate::paging::Page<ConversationSummary>,
    handle: &str,
) -> &'a Participant {
    page.items
        .iter()
        .flat_map(|c| c.participants.iter())
        .find(|p| p.handle.as_deref() == Some(handle))
        .expect("participant is in the page")
}

#[tokio::test]
async fn list_conversations_shows_the_contact_name() {
    let (pool, _vault, account) = conversations_setup().await;
    let mut conn = pool.acquire().await.unwrap();
    let contact_id: i64 = sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name) VALUES ($1, 'Sam Preferred')
         RETURNING id",
    )
    .bind(&account)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let handle_id: i64 =
        sqlx::query_scalar("SELECT id FROM handles WHERE account_id = $1 AND raw = '+15555550200'")
            .bind(&account)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO contact_handles (account_id, handle_id, contact_id)
         VALUES ($1, $2, $3)",
    )
    .bind(&account)
    .bind(handle_id)
    .bind(contact_id)
    .execute(&mut *conn)
    .await
    .unwrap();

    let page = list_conversations_page(&mut conn, &account).await;
    let p = find_participant(&page, "+15555550200");
    assert_eq!(p.name, "Sam Preferred");
    assert_eq!(p.contact_id, Some(contact_id));
}

#[tokio::test]
async fn list_conversations_falls_back_to_the_backup_name() {
    let (pool, _vault, account) = conversations_setup().await;
    let mut conn = pool.acquire().await.unwrap();
    // conversations_setup() records the backup name 'Sam' on +15555550200 and links no
    // contact, so the backup's name is what there is to show.
    let page = list_conversations_page(&mut conn, &account).await;
    let p = find_participant(&page, "+15555550200");
    assert_eq!(p.name, "Sam");
    assert_eq!(p.contact_id, None);
}

#[tokio::test]
async fn list_conversations_falls_back_to_the_handle() {
    let (pool, _vault, account) = conversations_setup().await;
    let mut conn = pool.acquire().await.unwrap();
    sqlx::query("UPDATE participants SET name_alias = NULL")
        .execute(&mut *conn)
        .await
        .unwrap();
    let page = list_conversations_page(&mut conn, &account).await;
    let p = find_participant(&page, "+15555550200");
    assert_eq!(p.name, "+15555550200");
}

#[tokio::test]
async fn list_conversations_filters_by_participant_count() {
    let (pool, _vault, account) = conversations_setup().await;
    let mut conn = pool.acquire().await.unwrap();
    // conversations_setup() has conversation 1 with 1 participant.

    let p2 = account_profile::link_account_handle(
        &mut conn,
        &account,
        "+15555550301",
        HandleType::Phone,
    )
    .await
    .unwrap();
    let p3 = account_profile::link_account_handle(
        &mut conn,
        &account,
        "+15555550302",
        HandleType::Phone,
    )
    .await
    .unwrap();
    let group_chat =
        account_profile::link_account_handle(&mut conn, &account, "chat-big", HandleType::Other)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO conversations (
            id, account_id, chat_handle_id, conversation_type, group_title, source_file
         ) VALUES (10, $1, $2, 'group', 'Trio', 't.jsonl')",
    )
    .bind(&account)
    .bind(group_chat)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO participants (conversation_id, handle_id, name_alias) VALUES
         (10, $1, 'A'), (10, $2, 'B')",
    )
    .bind(p2)
    .bind(p3)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO messages (
            conversation_id, account_id, source, timestamp, is_from_me, sort_order, body
         ) VALUES (10, $1, 'imessage', '2024-10-01T12:00:00Z', 0, 0, 'hi')",
    )
    .bind(&account)
    .execute(&mut *conn)
    .await
    .unwrap();

    let eq2 = list_conversations(&mut conn, &account, "participants:=2", 50, 0)
        .await
        .unwrap();
    assert_eq!(eq2.total, 1);
    assert_eq!(eq2.items[0].id, 10);

    let gt1 = list_conversations(&mut conn, &account, "participants:>1", 50, 0)
        .await
        .unwrap();
    assert_eq!(gt1.total, 1);
    assert_eq!(gt1.items[0].id, 10);

    let eq1 = list_conversations(&mut conn, &account, "participants:1", 50, 0)
        .await
        .unwrap();
    assert_eq!(eq1.total, 1);
    assert_eq!(eq1.items[0].id, 1);

    let lt2 = list_conversations(&mut conn, &account, "kind:group participants:<2", 50, 0)
        .await
        .unwrap();
    assert_eq!(lt2.total, 0);
}

#[tokio::test]
async fn list_conversations_participants_eq_three_on_built_fixture() {
    let (pool, _vault, account) = conversations_setup().await;
    let mut conn = pool.acquire().await.unwrap();
    // conversations_setup() already owns conversation 1 with 1 participant, which the
    // `=3` filter below must exclude.

    let p2 = account_profile::link_account_handle(
        &mut conn,
        &account,
        "+15555550401",
        HandleType::Phone,
    )
    .await
    .unwrap();
    let p3 = account_profile::link_account_handle(
        &mut conn,
        &account,
        "+15555550402",
        HandleType::Phone,
    )
    .await
    .unwrap();
    let p4 = account_profile::link_account_handle(
        &mut conn,
        &account,
        "+15555550403",
        HandleType::Phone,
    )
    .await
    .unwrap();
    let group_chat =
        account_profile::link_account_handle(&mut conn, &account, "chat-trio", HandleType::Other)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO conversations (
            id, account_id, chat_handle_id, conversation_type, group_title, source_file
         ) VALUES (20, $1, $2, 'group', 'Trio', 't2.jsonl')",
    )
    .bind(&account)
    .bind(group_chat)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO participants (conversation_id, handle_id, name_alias) VALUES
         (20, $1, 'A'), (20, $2, 'B'), (20, $3, 'C')",
    )
    .bind(p2)
    .bind(p3)
    .bind(p4)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO messages (
            conversation_id, account_id, source, timestamp, is_from_me, sort_order, body
         ) VALUES (20, $1, 'imessage', '2024-11-01T12:00:00Z', 0, 0, 'hi trio')",
    )
    .bind(&account)
    .execute(&mut *conn)
    .await
    .unwrap();

    let page = list_conversations(&mut conn, &account, "participants:=3", 50, 0)
        .await
        .unwrap();
    assert_eq!(
        page.total, 1,
        "only the trio conversation has exactly 3 participants"
    );
    assert_eq!(page.items[0].id, 20);
    assert_eq!(page.items[0].participants.len(), 3);
}

#[tokio::test]
async fn list_conversations_filters_by_import_id() {
    // Fresh db (conversations_setup() already owns conversation 1, which this test inserts itself).
    let vault = test_vault().await;
    let pool = vault.state.db.clone();
    let account = "00000000-0000-4000-8000-0000000000c2".to_string();
    let mut conn = pool.acquire().await.unwrap();
    sqlx::query("INSERT INTO accounts (id, username) VALUES ($1, 'alice')")
        .bind(&account)
        .execute(&mut *conn)
        .await
        .unwrap();

    let import_a = vault_imports::start_import(
        &mut conn,
        &vault_imports::StartImportArgs::new(&account, "imessage-ios", "append", Some("test")),
    )
    .await
    .unwrap();
    // Only one session may be `running` per account (the partial unique
    // index); finish `import_a` so `import_b` can start.
    vault_imports::complete_import(
        &mut conn,
        &account,
        import_a,
        &vault_imports::CompleteImportArgs::succeeded(1, 0),
    )
    .await
    .unwrap();
    let import_b = vault_imports::start_import(
        &mut conn,
        &vault_imports::StartImportArgs::new(&account, "imessage-ios", "append", Some("test")),
    )
    .await
    .unwrap();

    let peer1 = account_profile::link_account_handle(
        &mut conn,
        &account,
        "+15555550200",
        HandleType::Phone,
    )
    .await
    .unwrap();
    let peer2 = account_profile::link_account_handle(
        &mut conn,
        &account,
        "+15555550300",
        HandleType::Phone,
    )
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO conversations (
            id, account_id, chat_handle_id, conversation_type, source_file
         ) VALUES (1, $1, $2, 'individual', 'c1.jsonl')",
    )
    .bind(&account)
    .bind(peer1)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO participants (conversation_id, handle_id, name_alias)
         VALUES (1, $1, 'Sam')",
    )
    .bind(peer1)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO conversations (
            id, account_id, chat_handle_id, conversation_type, source_file
         ) VALUES (2, $1, $2, 'individual', 'c2.jsonl')",
    )
    .bind(&account)
    .bind(peer2)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO participants (conversation_id, handle_id, name_alias)
         VALUES (2, $1, 'Alex')",
    )
    .bind(peer2)
    .execute(&mut *conn)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO messages (
            conversation_id, account_id, source, timestamp, is_from_me, sort_order, body,
            import_id
         ) VALUES (1, $1, 'imessage', '2024-06-01T12:00:00Z', 0, 0, 'hello', $2)",
    )
    .bind(&account)
    .bind(import_a)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO messages (
            conversation_id, account_id, source, timestamp, is_from_me, sort_order, body,
            import_id
         ) VALUES (2, $1, 'imessage', '2024-07-01T12:00:00Z', 0, 0, 'later', $2)",
    )
    .bind(&account)
    .bind(import_b)
    .execute(&mut *conn)
    .await
    .unwrap();

    let a = list_conversations(
        &mut conn,
        &account,
        &format!("import:#{import_a}"),
        DEFAULT_LIST_LIMIT,
        0,
    )
    .await
    .unwrap();
    assert_eq!(a.total, 1);
    assert_eq!(a.items[0].id, 1);

    let b = list_conversations(
        &mut conn,
        &account,
        &format!("import:#{import_b}"),
        DEFAULT_LIST_LIMIT,
        0,
    )
    .await
    .unwrap();
    assert_eq!(b.total, 1);
    assert_eq!(b.items[0].id, 2);

    let missing = list_conversations(&mut conn, &account, "import:#999999", DEFAULT_LIST_LIMIT, 0)
        .await
        .unwrap();
    assert_eq!(missing.total, 0);

    // The language refuses a value it cannot parse instead of ignoring it.
    let junk = list_conversations(
        &mut conn,
        &account,
        "import:not-a-number",
        DEFAULT_LIST_LIMIT,
        0,
    )
    .await
    .unwrap_err();
    assert!(matches!(junk, ApiError::SearchQueryInvalid { .. }));
}

/// `sort` reads through the shared parser: an unknown key is refused,
/// never silently the default, and two keys compose.
#[tokio::test]
async fn sort_is_parsed_against_the_lists_keys() {
    let vault = crate::test_support::test_vault().await;
    let state = vault.state.clone();
    let user = crate::test_support::register_via_api(&state, "alice", "hunter2hunter2").await;
    let (status, text) =
        crate::test_support::get_raw(&state, "/v1/conversations?sort=colour", &user.token).await;
    let problem = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::ValidationFailed,
    );
    assert_eq!(
        problem.errors.as_deref(),
        Some(&["sort: unknown key 'colour'; accepted keys are date, messages".to_string()][..])
    );
    assert_eq!(
        crate::test_support::get_status(
            &state,
            "/v1/conversations?sort=-messages,date",
            &user.token
        )
        .await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn duplicate_only_threads_sort_last_in_either_date_direction() {
    // `last_message_at` is NULL for a thread whose every message is a
    // duplicate. Those threads are only listed under an `import:` filter,
    // which is the one path where NULL ordering is observable — and the two
    // engines disagree about it unless the query says where NULLs go.
    let vault = test_vault().await;
    let pool = vault.state.db.clone();
    let account = "00000000-0000-4000-8000-0000000000c2".to_string();
    let mut conn = pool.acquire().await.unwrap();
    sqlx::query("INSERT INTO accounts (id, username) VALUES ($1, 'alice')")
        .bind(&account)
        .execute(&mut *conn)
        .await
        .unwrap();

    let import_a = vault_imports::start_import(
        &mut conn,
        &vault_imports::StartImportArgs::new(&account, "imessage-ios", "append", Some("test")),
    )
    .await
    .unwrap();

    for (id, raw) in [(3, "+15555550400"), (4, "+15555550401")] {
        let peer =
            account_profile::link_account_handle(&mut conn, &account, raw, HandleType::Phone)
                .await
                .unwrap();
        sqlx::query(
            "INSERT INTO conversations (
                id, account_id, chat_handle_id, conversation_type, source_file
             ) VALUES ($1, $2, $3, 'individual', 'c.jsonl')",
        )
        .bind(id)
        .bind(&account)
        .bind(peer)
        .execute(&mut *conn)
        .await
        .unwrap();
    }

    // Conversation 4 keeps a real message, and it belongs to the import.
    sqlx::query(
        "INSERT INTO messages (
            conversation_id, account_id, source, timestamp, is_from_me, sort_order, body,
            import_id
         ) VALUES (4, $1, 'imessage', '2024-05-01T12:00:00Z', 0, 0, 'canonical', $2)",
    )
    .bind(&account)
    .bind(import_a)
    .execute(&mut *conn)
    .await
    .unwrap();
    let winner_id: i64 = sqlx::query_scalar("SELECT id FROM messages WHERE conversation_id = 4")
        .fetch_one(&mut *conn)
        .await
        .unwrap();

    // Conversation 3's only message is a duplicate, so its last_message_at
    // is NULL even though its timestamp is the later of the two.
    sqlx::query(
        "INSERT INTO messages (
            conversation_id, account_id, source, timestamp, is_from_me, sort_order, body,
            import_id, duplicate_of
         ) VALUES (3, $1, 'imessage', '2024-06-01T12:00:00Z', 0, 0, 'dup', $2, $3)",
    )
    .bind(&account)
    .bind(import_a)
    .bind(winner_id)
    .execute(&mut *conn)
    .await
    .unwrap();

    async fn ids_for(
        pool: &sqlx::AnyPool,
        account: &str,
        q: &str,
        direction: crate::paging::Direction,
    ) -> Vec<i64> {
        let mut conn = pool.acquire().await.unwrap();
        list_conversations_sorted(
            &mut conn,
            account,
            q,
            &[crate::paging::SortKey {
                key: ConversationSort::Date,
                direction,
            }],
            DEFAULT_LIST_LIMIT,
            0,
            crate::search::tests::clock(),
        )
        .await
        .unwrap()
        .items
        .iter()
        .map(|c| c.id)
        .collect()
    }

    let q = format!("import:#{import_a}");
    assert_eq!(
        ids_for(&pool, &account, &q, crate::paging::Direction::Desc).await,
        [4, 3],
        "a thread with no surviving message sorts last, not first"
    );
    assert_eq!(
        ids_for(&pool, &account, &q, crate::paging::Direction::Asc).await,
        [4, 3],
        "and stays last when the direction flips"
    );
}

#[tokio::test]
async fn list_conversations_import_id_includes_duplicate_only_thread() {
    // Fresh db: conversations_setup() would add a second non-duplicate conversation,
    // which breaks the "all" total assertion below.
    let vault = test_vault().await;
    let pool = vault.state.db.clone();
    let account = "00000000-0000-4000-8000-0000000000c2".to_string();
    let mut conn = pool.acquire().await.unwrap();
    sqlx::query("INSERT INTO accounts (id, username) VALUES ($1, 'alice')")
        .bind(&account)
        .execute(&mut *conn)
        .await
        .unwrap();

    let import_a = vault_imports::start_import(
        &mut conn,
        &vault_imports::StartImportArgs::new(&account, "imessage-ios", "append", Some("test")),
    )
    .await
    .unwrap();

    let peer = account_profile::link_account_handle(
        &mut conn,
        &account,
        "+15555550400",
        HandleType::Phone,
    )
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO conversations (
            id, account_id, chat_handle_id, conversation_type, source_file
         ) VALUES (3, $1, $2, 'individual', 'dup-only.jsonl')",
    )
    .bind(&account)
    .bind(peer)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO participants (conversation_id, handle_id, name_alias)
         VALUES (3, $1, 'Pat')",
    )
    .bind(peer)
    .execute(&mut *conn)
    .await
    .unwrap();

    // Canonical message in another conversation (winner for dedupe).
    let peer_other = account_profile::link_account_handle(
        &mut conn,
        &account,
        "+15555550401",
        HandleType::Phone,
    )
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO conversations (
            id, account_id, chat_handle_id, conversation_type, source_file
         ) VALUES (4, $1, $2, 'individual', 'winner.jsonl')",
    )
    .bind(&account)
    .bind(peer_other)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO messages (
            conversation_id, account_id, source, timestamp, is_from_me, sort_order, body
         ) VALUES (4, $1, 'imessage', '2024-05-01T12:00:00Z', 0, 0, 'canonical')",
    )
    .bind(&account)
    .execute(&mut *conn)
    .await
    .unwrap();
    let winner_id: i64 = sqlx::query_scalar("SELECT id FROM messages WHERE conversation_id = 4")
        .fetch_one(&mut *conn)
        .await
        .unwrap();

    // Only message in conversation 3 from import A is a duplicate.
    sqlx::query(
        "INSERT INTO messages (
            conversation_id, account_id, source, timestamp, is_from_me, sort_order, body,
            import_id, duplicate_of
         ) VALUES (3, $1, 'imessage', '2024-06-01T12:00:00Z', 0, 0, 'dup', $2, $3)",
    )
    .bind(&account)
    .bind(import_a)
    .bind(winner_id)
    .execute(&mut *conn)
    .await
    .unwrap();

    let by_import = list_conversations(
        &mut conn,
        &account,
        &format!("import:#{import_a}"),
        DEFAULT_LIST_LIMIT,
        0,
    )
    .await
    .unwrap();
    assert_eq!(
        by_import.total, 1,
        "import filter should match duplicate-only thread"
    );
    assert_eq!(by_import.items[0].id, 3);

    let all = list_conversations(&mut conn, &account, "", DEFAULT_LIST_LIMIT, 0)
        .await
        .unwrap();
    assert_eq!(
        all.total, 1,
        "default list still requires a non-duplicate message"
    );
    assert_eq!(all.items[0].id, 4);
}

#[test]
fn display_service_label_from_sources() {
    assert_eq!(display_service_label(&["imessage".into()]), "imessage");
    assert_eq!(
        display_service_label(&["sms-backup-restore".into()]),
        "SMS/MMS"
    );
    assert_eq!(
        display_service_label(&["imessage".into(), "sms-backup-restore".into()]),
        "SMS/MMS"
    );
    assert_eq!(display_service_label(&[]), "unknown");
    assert_eq!(display_service_label(&["whatsapp".into()]), "WhatsApp");
}

#[tokio::test]
async fn list_conversations_filters_by_tag_and_people() {
    let (pool, _vault, account) = conversations_setup().await;
    let mut conn = pool.acquire().await.unwrap();
    crate::named_membership::set_membership(
        crate::named_membership::tag_spec(),
        &mut conn,
        &account,
        &[1],
        "Holiday",
        true,
    )
    .await
    .unwrap();
    let tagged = list_conversations(&mut conn, &account, "tag:Holiday", DEFAULT_LIST_LIMIT, 0)
        .await
        .unwrap();
    assert_eq!(tagged.total, 1);
    assert_eq!(tagged.items[0].tags, vec!["Holiday".to_string()]);
    let hidden = list_conversations(&mut conn, &account, "-tag:Holiday", DEFAULT_LIST_LIMIT, 0)
        .await
        .unwrap();
    assert_eq!(hidden.total, 0);
    let untagged = list_conversations(&mut conn, &account, "tag:none", DEFAULT_LIST_LIMIT, 0)
        .await
        .unwrap();
    assert_eq!(untagged.total, 0);

    let contact_id: i64 = sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name) VALUES ($1, 'Sam') RETURNING id",
    )
    .bind(&account)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let handle_id: i64 =
        sqlx::query_scalar("SELECT chat_handle_id FROM conversations WHERE id = 1")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO contact_handles (account_id, handle_id, contact_id) VALUES ($1, $2, $3)",
    )
    .bind(&account)
    .bind(handle_id)
    .bind(contact_id)
    .execute(&mut *conn)
    .await
    .unwrap();
    crate::named_membership::set_membership(
        crate::named_membership::group_spec(),
        &mut conn,
        &account,
        &[contact_id],
        "Family",
        true,
    )
    .await
    .unwrap();
    let family = list_conversations(&mut conn, &account, "group:Family", DEFAULT_LIST_LIMIT, 0)
        .await
        .unwrap();
    assert_eq!(family.total, 1);
    let not_family =
        list_conversations(&mut conn, &account, "-group:Family", DEFAULT_LIST_LIMIT, 0)
            .await
            .unwrap();
    assert_eq!(not_family.total, 0);
}

#[tokio::test]
async fn the_conversation_list_is_a_page_with_integer_ids() {
    let vault = crate::test_support::test_vault().await;
    let state = vault.state.clone();
    let user = crate::test_support::register_via_api(&state, "alice", "hunter2hunter2").await;
    crate::test_support::seed_one_message(&state, &user.account_id).await;

    let page: serde_json::Value =
        crate::test_support::get_json(&state, "/v1/conversations?limit=10", &user.token).await;
    assert_eq!(page["total"], 1);
    assert_eq!(page["limit"], 10);
    assert_eq!(page["offset"], 0);
    assert!(
        page["items"][0]["id"].is_i64(),
        "id must be an integer: {page}"
    );
    assert!(page.get("conversations").is_none());
    assert!(page.get("ok").is_none());
}

#[tokio::test]
async fn a_limit_past_the_cap_or_an_offset_past_the_cap_is_a_422() {
    let vault = crate::test_support::test_vault().await;
    let state = vault.state.clone();
    let user = crate::test_support::register_via_api(&state, "alice", "hunter2hunter2").await;

    for path in [
        "/v1/conversations?limit=501",
        "/v1/conversations?limit=0",
        "/v1/conversations?offset=50001",
    ] {
        let status = crate::test_support::get_status(&state, path, &user.token).await;
        assert_eq!(
            status,
            axum::http::StatusCode::UNPROCESSABLE_ENTITY,
            "{path}"
        );
    }
}

#[tokio::test]
async fn conversation_detail_returns_the_owned_conversation() {
    let vault = crate::test_support::test_vault().await;
    let state = vault.state.clone();
    let user = crate::test_support::register_via_api(&state, "alice", "hunter2hunter2").await;
    crate::test_support::seed_one_message(&state, &user.account_id).await;
    let list: serde_json::Value =
        crate::test_support::get_json(&state, "/v1/conversations", &user.token).await;
    let id = list["items"][0]["id"].as_i64().unwrap();

    let body: serde_json::Value =
        crate::test_support::get_json(&state, &format!("/v1/conversations/{id}"), &user.token)
            .await;
    assert_eq!(body["id"], id);
    let participants = body["participants"].as_array().unwrap();
    assert!(!participants.is_empty());
    assert!(
        participants[0]["name"]
            .as_str()
            .is_some_and(|n| !n.is_empty()),
        "participant should carry a name: {body}"
    );
}

#[tokio::test]
async fn conversation_detail_404s_for_an_id_this_account_does_not_own() {
    let vault = crate::test_support::test_vault().await;
    let state = vault.state.clone();
    let user = crate::test_support::register_via_api(&state, "alice", "hunter2hunter2").await;

    let status =
        crate::test_support::get_status(&state, "/v1/conversations/999999", &user.token).await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn conversation_detail_404s_for_another_accounts_conversation() {
    let vault = crate::test_support::test_vault().await;
    let state = vault.state.clone();
    let alice = crate::test_support::register_via_api(&state, "alice", "hunter2hunter2").await;
    crate::test_support::seed_one_message(&state, &alice.account_id).await;
    let alice_list: serde_json::Value =
        crate::test_support::get_json(&state, "/v1/conversations", &alice.token).await;
    let alice_conversation_id = alice_list["items"][0]["id"].as_i64().unwrap();

    let bob = crate::test_support::register_via_api(&state, "bob", "hunter2hunter2").await;
    crate::test_support::seed_one_message(&state, &bob.account_id).await;

    // Bob asking for Alice's conversation id must 404, not 403 — a 403
    // would confirm the id exists in someone else's vault, and it must
    // not come back as Bob's own conversation either.
    let status = crate::test_support::get_status(
        &state,
        &format!("/v1/conversations/{alice_conversation_id}"),
        &bob.token,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn conversation_detail_reads_a_trashed_conversation() {
    let vault = crate::test_support::test_vault().await;
    let state = vault.state.clone();
    let user = crate::test_support::register_via_api(&state, "alice", "hunter2hunter2").await;
    crate::test_support::seed_one_message(&state, &user.account_id).await;
    let list: serde_json::Value =
        crate::test_support::get_json(&state, "/v1/conversations", &user.token).await;
    let id = list["items"][0]["id"].as_i64().unwrap();

    let mut conn = state.db.acquire().await.unwrap();
    sqlx::query("INSERT INTO trashed_conversations (account_id, conversation_id) VALUES ($1, $2)")
        .bind(&user.account_id)
        .bind(id)
        .execute(&mut *conn)
        .await
        .unwrap();
    drop(conn);

    // Trashed for the list, which no longer applies here — trash is a
    // property the list applies, not a gate on reading.
    let list_after: serde_json::Value =
        crate::test_support::get_json(&state, "/v1/conversations", &user.token).await;
    assert_eq!(
        list_after["total"], 0,
        "trashed conversation leaves the inbox list"
    );

    let status =
        crate::test_support::get_status(&state, &format!("/v1/conversations/{id}"), &user.token)
            .await;
    assert_eq!(status, axum::http::StatusCode::OK);
}

async fn trashed_conversation_row_count(
    conn: &mut AnyConnection,
    account_id: &str,
    id: i64,
) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM trashed_conversations
         WHERE account_id = $1 AND conversation_id = $2",
    )
    .bind(account_id)
    .bind(id)
    .fetch_one(&mut *conn)
    .await
    .unwrap()
}

#[tokio::test]
async fn conversation_trash_drops_it_from_the_list() {
    let vault = crate::test_support::test_vault().await;
    let state = vault.state.clone();
    let user = crate::test_support::register_via_api(&state, "alice", "hunter2hunter2").await;
    crate::test_support::seed_one_message(&state, &user.account_id).await;
    let list: serde_json::Value =
        crate::test_support::get_json(&state, "/v1/conversations", &user.token).await;
    let id = list["items"][0]["id"].as_i64().unwrap();

    let status = crate::test_support::post_status(
        &state,
        &format!("/v1/conversations/{id}/trash"),
        &user.token,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NO_CONTENT);

    let list_after: serde_json::Value =
        crate::test_support::get_json(&state, "/v1/conversations", &user.token).await;
    assert_eq!(
        list_after["total"], 0,
        "a trashed conversation must leave the conversations list"
    );
}

#[tokio::test]
async fn conversation_trash_twice_is_204_with_no_second_marker() {
    let vault = crate::test_support::test_vault().await;
    let state = vault.state.clone();
    let user = crate::test_support::register_via_api(&state, "alice", "hunter2hunter2").await;
    crate::test_support::seed_one_message(&state, &user.account_id).await;
    let list: serde_json::Value =
        crate::test_support::get_json(&state, "/v1/conversations", &user.token).await;
    let id = list["items"][0]["id"].as_i64().unwrap();
    let path = format!("/v1/conversations/{id}/trash");

    for _ in 0..2 {
        let status =
            crate::test_support::post_status(&state, &path, &user.token, serde_json::json!({}))
                .await;
        assert_eq!(status, axum::http::StatusCode::NO_CONTENT);
    }

    let mut conn = state.db.acquire().await.unwrap();
    assert_eq!(
        trashed_conversation_row_count(&mut conn, &user.account_id, id).await,
        1,
        "trashing twice must not create a second marker row"
    );
}

#[tokio::test]
async fn conversation_restore_brings_it_back_to_the_list() {
    let vault = crate::test_support::test_vault().await;
    let state = vault.state.clone();
    let user = crate::test_support::register_via_api(&state, "alice", "hunter2hunter2").await;
    crate::test_support::seed_one_message(&state, &user.account_id).await;
    let list: serde_json::Value =
        crate::test_support::get_json(&state, "/v1/conversations", &user.token).await;
    let id = list["items"][0]["id"].as_i64().unwrap();
    crate::test_support::post_status(
        &state,
        &format!("/v1/conversations/{id}/trash"),
        &user.token,
        serde_json::json!({}),
    )
    .await;

    let status = crate::test_support::post_status(
        &state,
        &format!("/v1/conversations/{id}/restore"),
        &user.token,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NO_CONTENT);

    let list_after: serde_json::Value =
        crate::test_support::get_json(&state, "/v1/conversations", &user.token).await;
    assert_eq!(
        list_after["total"], 1,
        "a restored conversation must come back to the conversations list"
    );
}

#[tokio::test]
async fn conversation_restore_twice_is_204_with_marker_gone() {
    let vault = crate::test_support::test_vault().await;
    let state = vault.state.clone();
    let user = crate::test_support::register_via_api(&state, "alice", "hunter2hunter2").await;
    crate::test_support::seed_one_message(&state, &user.account_id).await;
    let list: serde_json::Value =
        crate::test_support::get_json(&state, "/v1/conversations", &user.token).await;
    let id = list["items"][0]["id"].as_i64().unwrap();
    crate::test_support::post_status(
        &state,
        &format!("/v1/conversations/{id}/trash"),
        &user.token,
        serde_json::json!({}),
    )
    .await;
    let path = format!("/v1/conversations/{id}/restore");

    for _ in 0..2 {
        let status =
            crate::test_support::post_status(&state, &path, &user.token, serde_json::json!({}))
                .await;
        assert_eq!(status, axum::http::StatusCode::NO_CONTENT);
    }

    let mut conn = state.db.acquire().await.unwrap();
    assert_eq!(
        trashed_conversation_row_count(&mut conn, &user.account_id, id).await,
        0,
        "restoring twice must leave no marker row"
    );
}

#[tokio::test]
async fn conversation_trash_404s_for_an_unknown_id() {
    let vault = crate::test_support::test_vault().await;
    let state = vault.state.clone();
    let user = crate::test_support::register_via_api(&state, "alice", "hunter2hunter2").await;

    let status = crate::test_support::post_status(
        &state,
        "/v1/conversations/999999/trash",
        &user.token,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn conversation_restore_404s_for_an_unknown_id() {
    let vault = crate::test_support::test_vault().await;
    let state = vault.state.clone();
    let user = crate::test_support::register_via_api(&state, "alice", "hunter2hunter2").await;

    let status = crate::test_support::post_status(
        &state,
        "/v1/conversations/999999/restore",
        &user.token,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn conversation_trash_404s_for_another_accounts_conversation() {
    let vault = crate::test_support::test_vault().await;
    let state = vault.state.clone();
    let alice = crate::test_support::register_via_api(&state, "alice", "hunter2hunter2").await;
    crate::test_support::seed_one_message(&state, &alice.account_id).await;
    let alice_list: serde_json::Value =
        crate::test_support::get_json(&state, "/v1/conversations", &alice.token).await;
    let alice_conversation_id = alice_list["items"][0]["id"].as_i64().unwrap();

    let bob = crate::test_support::register_via_api(&state, "bob", "hunter2hunter2").await;
    crate::test_support::seed_one_message(&state, &bob.account_id).await;

    // Bob trashing Alice's conversation id must 404, not 403 — a 403
    // would confirm the id exists in someone else's vault.
    let status = crate::test_support::post_status(
        &state,
        &format!("/v1/conversations/{alice_conversation_id}/trash"),
        &bob.token,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);

    let mut conn = state.db.acquire().await.unwrap();
    assert_eq!(
        trashed_conversation_row_count(&mut conn, &alice.account_id, alice_conversation_id).await,
        0,
        "Bob's request must not trash Alice's conversation"
    );
}

#[tokio::test]
async fn conversation_restore_404s_for_another_accounts_conversation() {
    let vault = crate::test_support::test_vault().await;
    let state = vault.state.clone();
    let alice = crate::test_support::register_via_api(&state, "alice", "hunter2hunter2").await;
    crate::test_support::seed_one_message(&state, &alice.account_id).await;
    let alice_list: serde_json::Value =
        crate::test_support::get_json(&state, "/v1/conversations", &alice.token).await;
    let alice_conversation_id = alice_list["items"][0]["id"].as_i64().unwrap();
    let mut conn = state.db.acquire().await.unwrap();
    sqlx::query("INSERT INTO trashed_conversations (account_id, conversation_id) VALUES ($1, $2)")
        .bind(&alice.account_id)
        .bind(alice_conversation_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    drop(conn);

    let bob = crate::test_support::register_via_api(&state, "bob", "hunter2hunter2").await;
    crate::test_support::seed_one_message(&state, &bob.account_id).await;

    let status = crate::test_support::post_status(
        &state,
        &format!("/v1/conversations/{alice_conversation_id}/restore"),
        &bob.token,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);

    let mut conn = state.db.acquire().await.unwrap();
    assert_eq!(
        trashed_conversation_row_count(&mut conn, &alice.account_id, alice_conversation_id).await,
        1,
        "Bob's request must not restore Alice's conversation"
    );
}

#[tokio::test]
async fn conversation_trash_requires_auth() {
    let vault = crate::test_support::test_vault().await;
    let state = vault.state.clone();
    let user = crate::test_support::register_via_api(&state, "alice", "hunter2hunter2").await;
    crate::test_support::seed_one_message(&state, &user.account_id).await;
    let list: serde_json::Value =
        crate::test_support::get_json(&state, "/v1/conversations", &user.token).await;
    let id = list["items"][0]["id"].as_i64().unwrap();

    let status = crate::test_support::post_status(
        &state,
        &format!("/v1/conversations/{id}/trash"),
        "not-a-token",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
}

/// A signed-in account with one conversation already in the trash,
/// returning the account and the conversation's id.
async fn trashed_conversation_fixture() -> (TestVault, RegisteredAccount, i64) {
    let vault = crate::test_support::test_vault().await;
    let user = crate::test_support::register_via_api(&vault.state, "alice", "hunter2hunter2").await;
    crate::test_support::seed_one_message(&vault.state, &user.account_id).await;
    let list: serde_json::Value =
        crate::test_support::get_json(&vault.state, "/v1/conversations", &user.token).await;
    let id = list["items"][0]["id"].as_i64().unwrap();
    let status = crate::test_support::post_status(
        &vault.state,
        &format!("/v1/conversations/{id}/trash"),
        &user.token,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NO_CONTENT);
    (vault, user, id)
}

async fn conversation_row_count(conn: &mut AnyConnection, id: i64) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM conversations WHERE id = $1")
        .bind(id)
        .fetch_one(&mut *conn)
        .await
        .unwrap()
}

#[tokio::test]
async fn conversation_delete_removes_a_trashed_conversation_for_good() {
    let (vault, user, id) = trashed_conversation_fixture().await;

    let status = crate::test_support::delete_status(
        &vault.state,
        &format!("/v1/conversations/{id}"),
        &user.token,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NO_CONTENT);

    let detail = crate::test_support::get_status(
        &vault.state,
        &format!("/v1/conversations/{id}"),
        &user.token,
    )
    .await;
    assert_eq!(detail, axum::http::StatusCode::NOT_FOUND);
    let trashed: serde_json::Value =
        crate::test_support::get_json(&vault.state, "/v1/conversations?q=trashed:any", &user.token)
            .await;
    assert_eq!(trashed["total"], 0, "gone from every list, trash included");
    let mut conn = vault.conn().await;
    assert_eq!(
        trashed_conversation_row_count(&mut conn, &user.account_id, id).await,
        0,
        "the trash marker goes with the row"
    );
    let messages: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE account_id = $1")
        .bind(&user.account_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(messages, 0, "its messages are deleted with it");
}

#[tokio::test]
async fn conversation_delete_removes_files_only_the_deleted_conversation_used() {
    let (vault, user, doomed) = trashed_conversation_fixture().await;
    let kept = crate::test_support::seed_conversation(
        &vault.state,
        &crate::test_support::SeedConversation {
            account_id: &user.account_id,
            handle: "+15550002",
            conversation_type: "individual",
            group_title: None,
            source_file: "seed.jsonl",
            messages: &[crate::test_support::SeedMessage {
                source: "imessage",
                timestamp: "2020-01-02T00:00:00Z",
                is_from_me: false,
                body: "hi",
            }],
        },
    )
    .await;
    let shared = crate::test_support::fake_sha256('a');
    let unshared = crate::test_support::fake_sha256('b');
    let shared_file =
        crate::test_support::attach_stored_file(&vault.state, &user.account_id, doomed, &shared)
            .await;
    crate::test_support::attach_stored_file(&vault.state, &user.account_id, kept, &shared).await;
    let unshared_file =
        crate::test_support::attach_stored_file(&vault.state, &user.account_id, doomed, &unshared)
            .await;

    let status = crate::test_support::delete_status(
        &vault.state,
        &format!("/v1/conversations/{doomed}"),
        &user.token,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NO_CONTENT);

    assert!(
        !unshared_file.exists(),
        "the file only this conversation used is removed"
    );
    assert!(
        shared_file.exists(),
        "the file the kept conversation shares stays"
    );
}

#[tokio::test]
async fn conversation_delete_refuses_a_conversation_that_is_not_in_the_trash() {
    let vault = crate::test_support::test_vault().await;
    let user = crate::test_support::register_via_api(&vault.state, "alice", "hunter2hunter2").await;
    crate::test_support::seed_one_message(&vault.state, &user.account_id).await;
    let list: serde_json::Value =
        crate::test_support::get_json(&vault.state, "/v1/conversations", &user.token).await;
    let id = list["items"][0]["id"].as_i64().unwrap();

    let (status, body) = crate::test_support::delete_raw(
        &vault.state,
        &format!("/v1/conversations/{id}"),
        &user.token,
    )
    .await;
    assert_eq!(
        status,
        axum::http::StatusCode::CONFLICT,
        "trash is the only door to deletion: {body}"
    );
    assert!(body.contains("not in the trash"), "{body}");
    let mut conn = vault.conn().await;
    assert_eq!(conversation_row_count(&mut conn, id).await, 1);
}

#[tokio::test]
async fn conversation_delete_404s_for_an_unknown_id_and_for_another_accounts() {
    let (vault, alice, alices) = trashed_conversation_fixture().await;
    let bob = crate::test_support::register_via_api(&vault.state, "bob", "hunter2hunter2").await;

    let status =
        crate::test_support::delete_status(&vault.state, "/v1/conversations/999999", &alice.token)
            .await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);

    // Bob deleting Alice's trashed conversation must 404, not 403 — a 403
    // would confirm the id exists — and must not delete it.
    let status = crate::test_support::delete_status(
        &vault.state,
        &format!("/v1/conversations/{alices}"),
        &bob.token,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
    let mut conn = vault.conn().await;
    assert_eq!(conversation_row_count(&mut conn, alices).await, 1);
}

#[tokio::test]
async fn conversation_delete_needs_the_delete_permission() {
    let (vault, user, id) = trashed_conversation_fixture().await;
    {
        let mut conn = vault.conn().await;
        sqlx::query("UPDATE accounts SET can_delete = 0 WHERE id = $1")
            .bind(&user.account_id)
            .execute(&mut *conn)
            .await
            .unwrap();
    }

    let status = crate::test_support::delete_status(
        &vault.state,
        &format!("/v1/conversations/{id}"),
        &user.token,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
    let mut conn = vault.conn().await;
    assert_eq!(
        conversation_row_count(&mut conn, id).await,
        1,
        "an account that may not delete keeps its trashed conversation"
    );
}

#[tokio::test]
async fn conversation_delete_requires_auth() {
    let (vault, _user, id) = trashed_conversation_fixture().await;
    let status = crate::test_support::delete_status(
        &vault.state,
        &format!("/v1/conversations/{id}"),
        "not-a-token",
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn conversation_restore_requires_auth() {
    let vault = crate::test_support::test_vault().await;
    let state = vault.state.clone();
    let user = crate::test_support::register_via_api(&state, "alice", "hunter2hunter2").await;
    crate::test_support::seed_one_message(&state, &user.account_id).await;
    let list: serde_json::Value =
        crate::test_support::get_json(&state, "/v1/conversations", &user.token).await;
    let id = list["items"][0]["id"].as_i64().unwrap();

    let status = crate::test_support::post_status(
        &state,
        &format!("/v1/conversations/{id}/restore"),
        "not-a-token",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
}

/// A signed-in account and one conversation with no messages yet, for
/// tests that seed their own message rows with specific timestamps and
/// `sort_order`.
async fn conversation_messages_fixture() -> (TestVault, RegisteredAccount, i64) {
    let vault = crate::test_support::test_vault().await;
    let state = vault.state.clone();
    let user = crate::test_support::register_via_api(&state, "alice", "hunter2hunter2").await;
    let mut conn = state.db.acquire().await.unwrap();
    let handle_id: i64 = sqlx::query_scalar(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES ($1, $2, $2, 'phone', 'phone') RETURNING id",
    )
    .bind(&user.account_id)
    .bind(format!("+1555{}", user.account_id))
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let conversation_id: i64 = sqlx::query_scalar(
        "INSERT INTO conversations (account_id, chat_handle_id, conversation_type, source_file)
         VALUES ($1, $2, 'individual', 'seed.jsonl') RETURNING id",
    )
    .bind(&user.account_id)
    .bind(handle_id)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    (vault, user, conversation_id)
}

/// Insert one message row with an explicit `timestamp` and `sort_order`,
/// the control the JSON-import path does not give.
async fn insert_message(
    conn: &mut AnyConnection,
    conversation_id: i64,
    account_id: &str,
    timestamp: &str,
    sort_order: i64,
    body: &str,
) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO messages (
            conversation_id, account_id, source, timestamp, is_from_me, sort_order, body
         ) VALUES ($1, $2, 'imessage', $3, 1, $4, $5) RETURNING id",
    )
    .bind(conversation_id)
    .bind(account_id)
    .bind(timestamp)
    .bind(sort_order)
    .bind(body)
    .fetch_one(&mut *conn)
    .await
    .unwrap()
}

#[tokio::test]
async fn conversation_messages_are_ascending_by_timestamp_then_sort_order() {
    let (vault, user, conversation_id) = conversation_messages_fixture().await;
    let mut conn = vault.state.db.acquire().await.unwrap();
    // Deliberately inserted out of order: the third-in-time message
    // first, and two same-timestamp messages ordered only by sort_order.
    insert_message(
        &mut conn,
        conversation_id,
        &user.account_id,
        "2024-01-03T00:00:00Z",
        0,
        "third",
    )
    .await;
    insert_message(
        &mut conn,
        conversation_id,
        &user.account_id,
        "2024-01-01T00:00:00Z",
        5,
        "second",
    )
    .await;
    insert_message(
        &mut conn,
        conversation_id,
        &user.account_id,
        "2024-01-01T00:00:00Z",
        1,
        "first",
    )
    .await;
    drop(conn);

    let page: serde_json::Value = crate::test_support::get_json(
        &vault.state,
        &format!("/v1/conversations/{conversation_id}/messages"),
        &user.token,
    )
    .await;
    let texts: Vec<&str> = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["text"].as_str().unwrap())
        .collect();
    assert_eq!(texts, vec!["first", "second", "third"]);
}

#[tokio::test]
async fn conversation_messages_page_and_total_is_the_whole_count() {
    let (vault, user, conversation_id) = conversation_messages_fixture().await;
    let mut conn = vault.state.db.acquire().await.unwrap();
    for day in 1..=5 {
        insert_message(
            &mut conn,
            conversation_id,
            &user.account_id,
            &format!("2024-01-0{day}T00:00:00Z"),
            0,
            &format!("msg{day}"),
        )
        .await;
    }
    drop(conn);

    let page: serde_json::Value = crate::test_support::get_json(
        &vault.state,
        &format!("/v1/conversations/{conversation_id}/messages?limit=2&offset=1"),
        &user.token,
    )
    .await;
    assert_eq!(
        page["total"], 5,
        "total is the whole count, not the page's length: {page}"
    );
    assert_eq!(page["items"].as_array().unwrap().len(), 2);
    assert_eq!(page["limit"], 2);
    assert_eq!(page["offset"], 1);
    let texts: Vec<&str> = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["text"].as_str().unwrap())
        .collect();
    assert_eq!(texts, vec!["msg2", "msg3"]);
}

#[tokio::test]
async fn conversation_messages_year_narrows_and_total_is_the_years_count() {
    let (vault, user, conversation_id) = conversation_messages_fixture().await;
    let mut conn = vault.state.db.acquire().await.unwrap();
    for day in 1..=2 {
        insert_message(
            &mut conn,
            conversation_id,
            &user.account_id,
            &format!("2023-06-0{day}T00:00:00Z"),
            0,
            "in 2023",
        )
        .await;
    }
    for day in 1..=3 {
        insert_message(
            &mut conn,
            conversation_id,
            &user.account_id,
            &format!("2024-06-0{day}T00:00:00Z"),
            0,
            "in 2024",
        )
        .await;
    }
    drop(conn);

    let page: serde_json::Value = crate::test_support::get_json(
        &vault.state,
        &format!("/v1/conversations/{conversation_id}/messages?year=2024"),
        &user.token,
    )
    .await;
    assert_eq!(page["total"], 3, "total is the year's count: {page}");
    assert!(
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["text"] == "in 2024"),
        "only 2024 messages: {page}"
    );

    let whole: serde_json::Value = crate::test_support::get_json(
        &vault.state,
        &format!("/v1/conversations/{conversation_id}/messages"),
        &user.token,
    )
    .await;
    assert_eq!(whole["total"], 5, "no year= is the whole conversation");
}

#[tokio::test]
async fn a_message_at_31_december_2359_local_is_in_that_year_not_the_next() {
    let (vault, user, conversation_id) = conversation_messages_fixture().await;
    let mut conn = vault.state.db.acquire().await.unwrap();
    // The account lives in New York. A message at 2024-12-31 23:59 there is
    // the instant 2025-01-01T04:59:00Z, which is what the vault stores. The
    // year's edges are computed in the account's zone, the same rule
    // `date:2024` uses, so this message is in 2024 and not in 2025. A
    // boundary computed in UTC would file it under 2025.
    crate::db::account_profile::set_time_zone(
        &mut conn,
        &user.account_id,
        chrono_tz::America::New_York,
    )
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO messages (
            conversation_id, account_id, source, timestamp,
            is_from_me, sort_order, body
         ) VALUES ($1, $2, 'imessage', $3, 1, 0, 'new year''s eve')",
    )
    .bind(conversation_id)
    .bind(&user.account_id)
    .bind("2025-01-01T04:59:00Z")
    .execute(&mut *conn)
    .await
    .unwrap();
    drop(conn);

    let this_year: serde_json::Value = crate::test_support::get_json(
        &vault.state,
        &format!("/v1/conversations/{conversation_id}/messages?year=2024"),
        &user.token,
    )
    .await;
    assert_eq!(
        this_year["total"], 1,
        "31 Dec 23:59 local belongs to its own year: {this_year}"
    );

    let next_year: serde_json::Value = crate::test_support::get_json(
        &vault.state,
        &format!("/v1/conversations/{conversation_id}/messages?year=2025"),
        &user.token,
    )
    .await;
    assert_eq!(
        next_year["total"], 0,
        "31 Dec 23:59 local does not leak into the next year: {next_year}"
    );
}

/// A backup that recorded the thread's address and nothing about who was
/// in it leaves no `participants` rows. The conversation list has always
/// named that person from the conversation's chat handle; while that
/// fallback lived in this file rather than in `db::participant_names`,
/// the message page for the same thread answered with an empty
/// participant list. Both read the person's name through one function
/// now, so both name them.
#[tokio::test]
async fn conversation_messages_name_the_person_a_thread_has_no_participants_row_for() {
    let (vault, user, conversation_id) = conversation_messages_fixture().await;
    let mut conn = vault.state.db.acquire().await.unwrap();
    insert_message(
        &mut conn,
        conversation_id,
        &user.account_id,
        "2024-01-01T00:00:00Z",
        0,
        "hello",
    )
    .await;
    let participants: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM participants WHERE conversation_id = $1")
            .bind(conversation_id)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(participants, 0, "the fixture leaves no participants rows");
    drop(conn);

    let summary: serde_json::Value = crate::test_support::get_json(
        &vault.state,
        &format!("/v1/conversations/{conversation_id}"),
        &user.token,
    )
    .await;
    let page: serde_json::Value = crate::test_support::get_json(
        &vault.state,
        &format!("/v1/conversations/{conversation_id}/messages"),
        &user.token,
    )
    .await;
    let from_messages = &page["items"][0]["conversation"]["participants"];
    assert_eq!(
        from_messages, &summary["participants"],
        "the message page names the same people the conversation does: {page}"
    );
    assert_eq!(
        from_messages[0]["handle"].as_str().unwrap(),
        format!("+1555{}", user.account_id),
        "and names them by the thread's own address: {page}"
    );
}

#[tokio::test]
async fn conversation_messages_404s_for_an_unknown_id() {
    let (vault, user, _conversation_id) = conversation_messages_fixture().await;
    let status = crate::test_support::get_status(
        &vault.state,
        "/v1/conversations/999999/messages",
        &user.token,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn conversation_messages_404s_for_another_accounts_conversation() {
    let (vault, _alice, alice_conversation_id) = conversation_messages_fixture().await;
    let bob = crate::test_support::register_via_api(&vault.state, "bob", "hunter2hunter2").await;
    crate::test_support::seed_one_message(&vault.state, &bob.account_id).await;

    let status = crate::test_support::get_status(
        &vault.state,
        &format!("/v1/conversations/{alice_conversation_id}/messages"),
        &bob.token,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn conversation_messages_reads_a_trashed_conversations_messages() {
    let (vault, user, conversation_id) = conversation_messages_fixture().await;
    let mut conn = vault.state.db.acquire().await.unwrap();
    insert_message(
        &mut conn,
        conversation_id,
        &user.account_id,
        "2024-01-01T00:00:00Z",
        0,
        "still here",
    )
    .await;
    sqlx::query("INSERT INTO trashed_conversations (account_id, conversation_id) VALUES ($1, $2)")
        .bind(&user.account_id)
        .bind(conversation_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    drop(conn);

    let page: serde_json::Value = crate::test_support::get_json(
        &vault.state,
        &format!("/v1/conversations/{conversation_id}/messages"),
        &user.token,
    )
    .await;
    assert_eq!(
        page["total"], 1,
        "a trashed conversation's messages are readable: {page}"
    );
}

#[tokio::test]
async fn conversation_messages_bad_limit_is_refused_like_other_paged_routes() {
    let (vault, user, conversation_id) = conversation_messages_fixture().await;

    for path in [
        format!("/v1/conversations/{conversation_id}/messages?limit=501"),
        format!("/v1/conversations/{conversation_id}/messages?limit=0"),
        format!("/v1/conversations/{conversation_id}/messages?offset=50001"),
    ] {
        let status = crate::test_support::get_status(&vault.state, &path, &user.token).await;
        assert_eq!(
            status,
            axum::http::StatusCode::UNPROCESSABLE_ENTITY,
            "{path}"
        );
    }
}
