use super::*;
use crate::db::contacts::read::DEFAULT_CONTACT_SORT;
use crate::paging::{DEFAULT_LIST_LIMIT, parse_sort};
use edit::ContactEditError;
use message_ir::HandleType;

use crate::db::account_profile;
use crate::test_support::{
    RegisteredAccount, TestFixture, fixture_with_account, post_json, post_status, register_via_api,
    test_fixture,
};
use axum::http::StatusCode;

/// A fixture, a logged-in account, and `handles` linked as contacts (one
/// contact per phone, named `Contact 0`, `Contact 1`, ...).
async fn contacts_fixture_with_handles(handles: &[&str]) -> (TestFixture, RegisteredAccount) {
    let (fixture, account) = fixture_with_account().await;
    if !handles.is_empty() {
        let mut conn = fixture.state.db.acquire().await.unwrap();
        for (i, handle) in handles.iter().enumerate() {
            insert_contact_with_handle(
                &mut conn,
                account.account_id,
                &format!("Contact {i}"),
                handle,
            )
            .await;
        }
    }
    (fixture, account)
}

/// A fixture, a logged-in account, and one contact linked to `handle` that
/// is then trashed.
async fn contacts_fixture_with_trashed_handle(handle: &str) -> (TestFixture, RegisteredAccount) {
    let (fixture, account) = fixture_with_account().await;
    let mut conn = fixture.state.db.acquire().await.unwrap();
    let contact_id =
        insert_contact_with_handle(&mut conn, account.account_id, "Trashed", handle).await;
    sqlx::query("INSERT INTO trashed_contacts (account_id, contact_id) VALUES ($1, $2)")
        .bind(account.account_id)
        .bind(contact_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    (fixture, account)
}

/// A second logged-in account in the same server, with `handle` linked to
/// one of its contacts. Used to prove `/v1/contacts/unmatched-identities` is scoped to
/// the calling account rather than the whole database.
async fn account_with_handle(fixture: &TestFixture, handle: &str) -> RegisteredAccount {
    let account = register_via_api(&fixture.state, "bob", "hunter2hunter2").await;
    let mut conn = fixture.state.db.acquire().await.unwrap();
    insert_contact_with_handle(&mut conn, account.account_id, "Other", handle).await;
    account
}

#[tokio::test]
async fn contact_match_reports_only_the_identifiers_the_database_does_not_have() {
    let (fixture, account) = contacts_fixture_with_handles(&["+15550100"]).await;
    let body = serde_json::json!({ "identifiers": ["+15550100", "+15550999"] });
    let response = post_json::<serde_json::Value>(
        &fixture.state,
        "/v1/contacts/unmatched-identities",
        &account.token,
        body,
    )
    .await;
    assert_eq!(response["items"], serde_json::json!(["+15550999"]));
}

#[tokio::test]
async fn contact_match_ignores_blank_identifiers_and_de_duplicates() {
    let (fixture, account) = contacts_fixture_with_handles(&[]).await;
    let body = serde_json::json!({ "identifiers": ["+15550999", "  ", "+15550999", ""] });
    let response = post_json::<serde_json::Value>(
        &fixture.state,
        "/v1/contacts/unmatched-identities",
        &account.token,
        body,
    )
    .await;
    assert_eq!(response["items"], serde_json::json!(["+15550999"]));
}

#[tokio::test]
async fn contact_match_collapses_duplicates_by_normalized_form() {
    // Two spellings of the same phone number must read as one new
    // person, not two — otherwise Gate 1's "N new" count
    // double-counts a single human written two ways.
    let (fixture, account) = contacts_fixture_with_handles(&[]).await;
    let body = serde_json::json!({ "identifiers": ["+1 (555) 010-0100", "+15550100100"] });
    let response = post_json::<serde_json::Value>(
        &fixture.state,
        "/v1/contacts/unmatched-identities",
        &account.token,
        body,
    )
    .await;
    assert_eq!(
        response["items"],
        serde_json::json!(["+1 (555) 010-0100"]),
        "both spellings normalize to the same value, so only the \
         first-seen spelling should come back once"
    );
}

#[tokio::test]
async fn contact_match_matches_a_differently_spelled_identifier_against_the_stored_normalized_value()
 {
    // Guards against a regression to matching on `h.raw`: the fixture
    // stores the E.164 form through the normal handle-linking path; the
    // request asks about a spaced-out spelling of the same number.
    let (fixture, account) = contacts_fixture_with_handles(&["+15550100"]).await;
    let body = serde_json::json!({ "identifiers": ["+1 555 0100"] });
    let response = post_json::<serde_json::Value>(
        &fixture.state,
        "/v1/contacts/unmatched-identities",
        &account.token,
        body,
    )
    .await;
    assert_eq!(
        response["items"],
        serde_json::json!([]),
        "the differently-spelled identifier normalizes to the stored value, so it is known"
    );
}

#[tokio::test]
async fn contact_match_preserves_order_across_multiple_unknowns() {
    let (fixture, account) = contacts_fixture_with_handles(&["+15550100"]).await;
    let body = serde_json::json!({ "identifiers": ["+15550100", "+15550200", "+15550300"] });
    let response = post_json::<serde_json::Value>(
        &fixture.state,
        "/v1/contacts/unmatched-identities",
        &account.token,
        body,
    )
    .await;
    assert_eq!(
        response["items"],
        serde_json::json!(["+15550200", "+15550300"])
    );
}

#[tokio::test]
async fn contact_match_counts_a_trashed_contact_as_new() {
    // An import that meets this handle discards the trashed contact and
    // makes a fresh one from the backup (ADR-0013, `imports_api::contact_name`),
    // so the person is about to see a new contact, and the count says so.
    let (fixture, account) = contacts_fixture_with_trashed_handle("+15550100").await;
    let body = serde_json::json!({ "identifiers": ["+15550100"] });
    let response = post_json::<serde_json::Value>(
        &fixture.state,
        "/v1/contacts/unmatched-identities",
        &account.token,
        body,
    )
    .await;
    assert_eq!(response["items"], serde_json::json!(["+15550100"]));
}

#[tokio::test]
async fn contact_match_is_scoped_to_the_calling_account() {
    let (fixture, mine) = contacts_fixture_with_handles(&[]).await;
    let _other = account_with_handle(&fixture, "+15550100").await;
    let body = serde_json::json!({ "identifiers": ["+15550100"] });
    let response = post_json::<serde_json::Value>(
        &fixture.state,
        "/v1/contacts/unmatched-identities",
        &mine.token,
        body,
    )
    .await;
    assert_eq!(response["items"], serde_json::json!(["+15550100"]));
}

/// A refusal reaches the person as a 422 Unprocessable Entity carrying the
/// sentence written for them, and a request that names no edit at all is
/// refused the same way. Both went through a downcast on the error's type before
/// `ContactEditError` existed.
#[tokio::test]
async fn a_refused_contact_edit_answers_422_with_the_persons_sentence() {
    let (fixture, account) = contacts_fixture_with_handles(&[]).await;
    let mut conn = fixture.state.db.acquire().await.unwrap();
    let first =
        insert_contact_with_handle(&mut conn, account.account_id, "Ada", "+15555550100").await;
    insert_contact_with_handle(&mut conn, account.account_id, "Grace", "+15555550200").await;
    drop(conn);

    // Taking an identity that is already another contact’s.
    let (status, sentence) = crate::test_support::patch_failure(
        &fixture.state,
        &format!("/v1/contacts/{first}"),
        &account.token,
        serde_json::json!({ "add_identity": { "address": "+15555550200" } }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(sentence, "identity already linked to another contact");

    // No edit named at all.
    let status = crate::test_support::patch_status(
        &fixture.state,
        &format!("/v1/contacts/{first}"),
        &account.token,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

/// A contact deleted after the edit found it: the rename updated no row and
/// answered `500` with "contact missing after mutate". The edit is one write
/// transaction, so it finds the contact gone and answers `404`.
#[tokio::test]
async fn renaming_a_contact_deleted_meanwhile_answers_not_found() {
    let (fixture, account) = contacts_fixture_with_handles(&[]).await;
    let mut conn = fixture.state.db.acquire().await.unwrap();
    let ada =
        insert_contact_with_handle(&mut conn, account.account_id, "Ada", "+15555550100").await;

    let mut other = crate::db::begin_write(&mut conn).await.unwrap();
    sqlx::query("DELETE FROM contacts WHERE id = $1")
        .bind(ada)
        .execute(&mut *other)
        .await
        .unwrap();
    let (status, _) = crate::db::write_tx::commit_during(
        other,
        crate::test_support::patch_failure(
            &fixture.state,
            &format!("/v1/contacts/{ada}"),
            &account.token,
            serde_json::json!({ "name": "Ada Lovelace" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn replacing_an_identity_with_an_empty_address_is_refused_and_keeps_the_old_one() {
    let (fixture, account) = contacts_fixture_with_handles(&[]).await;
    let mut conn = fixture.state.db.acquire().await.unwrap();
    let ada =
        insert_contact_with_handle(&mut conn, account.account_id, "Ada", "+15555550100").await;
    drop(conn);

    let (status, sentence) = crate::test_support::patch_failure(
        &fixture.state,
        &format!("/v1/contacts/{ada}"),
        &account.token,
        serde_json::json!({
            "update_identity": { "previous_address": "+15555550100", "address": "  " }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(sentence, "previous_address and address must not be empty");

    let detail: serde_json::Value = crate::test_support::get_json(
        &fixture.state,
        &format!("/v1/contacts/{ada}"),
        &account.token,
    )
    .await;
    let addresses: Vec<&str> = detail["identities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|identity| identity["address"].as_str().unwrap())
        .collect();
    assert_eq!(addresses, ["+15555550100"]);
}

#[tokio::test]
async fn contact_match_rejects_an_oversized_batch() {
    let (fixture, account) = contacts_fixture_with_handles(&[]).await;
    let identifiers: Vec<String> = (0..MAX_MATCH_IDENTIFIERS + 1)
        .map(|i| format!("+1555{i:06}"))
        .collect();
    let status = post_status(
        &fixture.state,
        "/v1/contacts/unmatched-identities",
        &account.token,
        serde_json::json!({ "identifiers": identifiers }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn list_contacts_uses_preferred_name_and_handle_ids() {
    let fixture = test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    let contact_id: i64 = sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name) VALUES ($1, 'Pat') RETURNING id",
    )
    .bind(account)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let handle_id =
        account_profile::link_account_handle(&mut conn, account, "+15555550100", HandleType::Phone)
            .await
            .unwrap();
    // link_account_handle puts it on account_handles; also link as contact handle.
    sqlx::query(
        "INSERT INTO contact_handles (account_id, handle_id, contact_id)
         VALUES ($1, $2, $3)",
    )
    .bind(account)
    .bind(handle_id)
    .bind(contact_id)
    .execute(&mut *conn)
    .await
    .unwrap();

    let page = list_contacts_sorted(
        &mut conn,
        account,
        "",
        &DEFAULT_CONTACT_SORT,
        DEFAULT_LIST_LIMIT,
        0,
        crate::search::tests::clock(),
    )
    .await
    .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].name, "Pat");
    assert_eq!(page.items[0].identity_count, 1);
    assert!(
        page.items[0]
            .addresses
            .iter()
            .any(|h| h.contains("5555550100") || h.contains("+15555550100")),
        "addresses={:?}",
        page.items[0].addresses
    );
}

#[tokio::test]
async fn list_contacts_filters_and_paginates() {
    let fixture = test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    for (name, phone) in [
        ("Pat", "+15555550100"),
        ("Sam", "+15555550200"),
        ("Alex", "+15555550300"),
    ] {
        let contact_id: i64 = sqlx::query_scalar(
            "INSERT INTO contacts (account_id, preferred_name) VALUES ($1, $2) RETURNING id",
        )
        .bind(account)
        .bind(name)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
        let handle_id =
            account_profile::link_account_handle(&mut conn, account, phone, HandleType::Phone)
                .await
                .unwrap();
        sqlx::query(
            "INSERT INTO contact_handles (account_id, handle_id, contact_id)
             VALUES ($1, $2, $3)",
        )
        .bind(account)
        .bind(handle_id)
        .bind(contact_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    }

    let by_name = list_contacts_sorted(
        &mut conn,
        account,
        "sam",
        &DEFAULT_CONTACT_SORT,
        DEFAULT_LIST_LIMIT,
        0,
        crate::search::tests::clock(),
    )
    .await
    .unwrap();
    assert_eq!(by_name.total, 1);
    assert_eq!(by_name.items[0].name, "Sam");

    let by_handle = list_contacts_sorted(
        &mut conn,
        account,
        "identity:5555550200",
        &DEFAULT_CONTACT_SORT,
        DEFAULT_LIST_LIMIT,
        0,
        crate::search::tests::clock(),
    )
    .await
    .unwrap();
    assert_eq!(by_handle.total, 1);
    assert_eq!(by_handle.items[0].name, "Sam");

    let page0 = list_contacts_sorted(
        &mut conn,
        account,
        "",
        &DEFAULT_CONTACT_SORT,
        2,
        0,
        crate::search::tests::clock(),
    )
    .await
    .unwrap();
    assert_eq!(page0.total, 3);
    assert_eq!(page0.limit, 2);
    assert_eq!(page0.offset, 0);
    assert_eq!(page0.items.len(), 2);
    let page1 = list_contacts_sorted(
        &mut conn,
        account,
        "",
        &DEFAULT_CONTACT_SORT,
        2,
        2,
        crate::search::tests::clock(),
    )
    .await
    .unwrap();
    assert_eq!(page1.total, 3);
    assert_eq!(page1.offset, 2);
    assert_eq!(page1.items.len(), 1);
}

#[tokio::test]
async fn get_contact_detail_counts_direct_group_and_messages() {
    let fixture = test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    let contact_id: i64 = sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name) VALUES ($1, 'Sam') RETURNING id",
    )
    .bind(account)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let peer =
        account_profile::link_account_handle(&mut conn, account, "+15555550200", HandleType::Phone)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO contact_handles (account_id, handle_id, contact_id)
         VALUES ($1, $2, $3)",
    )
    .bind(account)
    .bind(peer)
    .bind(contact_id)
    .execute(&mut *conn)
    .await
    .unwrap();

    // Direct conversation with 2 messages.
    sqlx::query(
        "INSERT INTO conversations (
            id, account_id, chat_handle_id, conversation_type, source_file
         ) VALUES (1, $1, $2, 'individual', 'd.jsonl')",
    )
    .bind(account)
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
    for (body, ts) in [
        ("hi", "2024-06-01T12:00:00Z"),
        ("there", "2024-06-01T13:00:00Z"),
    ] {
        sqlx::query(
            "INSERT INTO messages (
                conversation_id, account_id, source, timestamp, is_from_me,
                sender_handle_id, sort_order, body
             ) VALUES (1, $1, 'imessage', $2, 0, $3, 0, $4)",
        )
        .bind(account)
        .bind(ts)
        .bind(peer)
        .bind(body)
        .execute(&mut *conn)
        .await
        .unwrap();
    }
    // A reply of yours: not a message Sam sent, so not in `total_messages`.
    sqlx::query(
        "INSERT INTO messages (
            conversation_id, account_id, source, timestamp, is_from_me, sort_order, body
         ) VALUES (1, $1, 'imessage', '2024-06-01T14:00:00Z', 1, 0, 'back at you')",
    )
    .bind(account)
    .execute(&mut *conn)
    .await
    .unwrap();

    // Group conversation that includes Sam, with 1 message.
    let group_chat = account_profile::link_account_handle(
        &mut conn,
        account,
        "chat-sam-group",
        HandleType::Other,
    )
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO conversations (
            id, account_id, chat_handle_id, conversation_type, group_title, source_file
         ) VALUES (2, $1, $2, 'group', 'Sam Group', 'g.jsonl')",
    )
    .bind(account)
    .bind(group_chat)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO participants (conversation_id, handle_id, name_alias)
         VALUES (2, $1, 'Sam')",
    )
    .bind(peer)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO messages (
            conversation_id, account_id, source, timestamp, is_from_me,
            sender_handle_id, sort_order, body
         ) VALUES (2, $1, 'imessage', '2024-07-01T12:00:00Z', 0, $2, 0, 'group hi')",
    )
    .bind(account)
    .bind(peer)
    .execute(&mut *conn)
    .await
    .unwrap();

    // Unrelated conversation should not be counted.
    let other =
        account_profile::link_account_handle(&mut conn, account, "+15555550999", HandleType::Phone)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO conversations (
            id, account_id, chat_handle_id, conversation_type, source_file
         ) VALUES (9, $1, $2, 'individual', 'other.jsonl')",
    )
    .bind(account)
    .bind(other)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO messages (
            conversation_id, account_id, source, timestamp, is_from_me, sort_order, body
         ) VALUES (9, $1, 'imessage', '2024-08-01T12:00:00Z', 0, 0, 'nope')",
    )
    .bind(account)
    .execute(&mut *conn)
    .await
    .unwrap();

    let detail = get_contact_detail(&mut conn, account, contact_id)
        .await
        .unwrap()
        .expect("contact exists");
    assert_eq!(detail.name, "Sam");
    assert_eq!(detail.direct_conversations, 1);
    assert_eq!(detail.group_conversations, 1);
    assert_eq!(detail.total_messages, 3);
    assert_eq!(detail.identities.len(), 1);
    assert!(
        detail.identities[0].address.contains("5555550200")
            || detail.identities[0].address.contains("+15555550200"),
        "handle={:?}",
        detail.identities[0].address
    );
    assert_eq!(detail.identities[0].conversations, 2);
    // The identity table counts what Sam sent from the identity, as
    // `total_messages` does: your reply is not Sam's (#913).
    assert_eq!(detail.identities[0].direct_messages, 2);
    assert_eq!(detail.identities[0].group_messages, 1);
    assert_eq!(
        detail.identities[0].start_date.as_deref(),
        Some("2024-06-01T12:00:00Z")
    );
    assert_eq!(
        detail.identities[0].end_date.as_deref(),
        Some("2024-07-01T12:00:00Z")
    );
}

#[tokio::test]
async fn get_contact_summaries_counts_two_contacts_in_one_query() {
    let fixture = test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;

    let sam_id: i64 = sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name) VALUES ($1, 'Sam') RETURNING id",
    )
    .bind(account)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let sam_handle =
        account_profile::link_account_handle(&mut conn, account, "+15555550200", HandleType::Phone)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO contact_handles (account_id, handle_id, contact_id)
         VALUES ($1, $2, $3)",
    )
    .bind(account)
    .bind(sam_handle)
    .bind(sam_id)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO conversations (
            id, account_id, chat_handle_id, conversation_type, source_file
         ) VALUES (1, $1, $2, 'individual', 'd.jsonl')",
    )
    .bind(account)
    .bind(sam_handle)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO participants (conversation_id, handle_id, name_alias)
         VALUES (1, $1, 'Sam')",
    )
    .bind(sam_handle)
    .execute(&mut *conn)
    .await
    .unwrap();
    for (body, ts) in [
        ("hi", "2024-06-01T12:00:00Z"),
        ("there", "2024-06-01T13:00:00Z"),
    ] {
        sqlx::query(
            "INSERT INTO messages (
                conversation_id, account_id, source, timestamp, is_from_me,
                sender_handle_id, sort_order, body
             ) VALUES (1, $1, 'imessage', $2, 0, $3, 0, $4)",
        )
        .bind(account)
        .bind(ts)
        .bind(sam_handle)
        .bind(body)
        .execute(&mut *conn)
        .await
        .unwrap();
    }
    let group_chat = account_profile::link_account_handle(
        &mut conn,
        account,
        "chat-sam-group",
        HandleType::Other,
    )
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO conversations (
            id, account_id, chat_handle_id, conversation_type, group_title, source_file
         ) VALUES (2, $1, $2, 'group', 'Sam Group', 'g.jsonl')",
    )
    .bind(account)
    .bind(group_chat)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO participants (conversation_id, handle_id, name_alias)
         VALUES (2, $1, 'Sam')",
    )
    .bind(sam_handle)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO messages (
            conversation_id, account_id, source, timestamp, is_from_me,
            sender_handle_id, sort_order, body
         ) VALUES (2, $1, 'imessage', '2024-07-01T12:00:00Z', 0, $2, 0, 'group hi')",
    )
    .bind(account)
    .bind(sam_handle)
    .execute(&mut *conn)
    .await
    .unwrap();

    let pat_id: i64 = sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name) VALUES ($1, 'Pat') RETURNING id",
    )
    .bind(account)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let pat_handle =
        account_profile::link_account_handle(&mut conn, account, "+15555550100", HandleType::Phone)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO contact_handles (account_id, handle_id, contact_id)
         VALUES ($1, $2, $3)",
    )
    .bind(account)
    .bind(pat_handle)
    .bind(pat_id)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO conversations (
            id, account_id, chat_handle_id, conversation_type, source_file
         ) VALUES (3, $1, $2, 'individual', 'pat.jsonl')",
    )
    .bind(account)
    .bind(pat_handle)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO participants (conversation_id, handle_id, name_alias)
         VALUES (3, $1, 'Pat')",
    )
    .bind(pat_handle)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO messages (
            conversation_id, account_id, source, timestamp, is_from_me,
            sender_handle_id, sort_order, body
         ) VALUES (3, $1, 'imessage', '2024-05-01T09:00:00Z', 0, $2, 0, 'hey')",
    )
    .bind(account)
    .bind(pat_handle)
    .execute(&mut *conn)
    .await
    .unwrap();

    let summaries = get_contact_summaries(&mut conn, account, &[sam_id, pat_id, 99_999])
        .await
        .unwrap();
    assert_eq!(summaries.len(), 2);

    assert_eq!(summaries[0].id, sam_id);
    assert_eq!(summaries[0].name, "Sam");
    assert_eq!(summaries[0].individual_conversations, 1);
    assert_eq!(summaries[0].group_conversations, 1);
    assert_eq!(summaries[0].individual_message_count, 2);
    assert_eq!(summaries[0].group_message_count, 1);
    assert_eq!(
        summaries[0].start_date.as_deref(),
        Some("2024-06-01T12:00:00Z")
    );
    assert_eq!(
        summaries[0].end_date.as_deref(),
        Some("2024-07-01T12:00:00Z")
    );

    assert_eq!(summaries[1].id, pat_id);
    assert_eq!(summaries[1].name, "Pat");
    assert_eq!(summaries[1].individual_conversations, 1);
    assert_eq!(summaries[1].group_conversations, 0);
    assert_eq!(summaries[1].individual_message_count, 1);
    assert_eq!(summaries[1].group_message_count, 0);
    assert_eq!(
        summaries[1].start_date.as_deref(),
        Some("2024-05-01T09:00:00Z")
    );
    assert_eq!(
        summaries[1].end_date.as_deref(),
        Some("2024-05-01T09:00:00Z")
    );
}

/// A conversation holding two of one contact's identities is one of the
/// contact's conversations, not two, both on the contact and in its summary
/// (#1248). Each identity still counts it once for itself.
#[tokio::test]
async fn a_conversation_with_two_of_a_contacts_identities_counts_once() {
    let fixture = test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    let contact_id: i64 = sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name) VALUES ($1, 'Sam') RETURNING id",
    )
    .bind(account)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let mut handles = Vec::new();
    for (raw, handle_type) in [
        ("+15555550200", HandleType::Phone),
        ("sam@example.com", HandleType::Email),
    ] {
        let (handle, _) =
            crate::db::handles::upsert_handle_row(&mut conn, account, raw, handle_type, None)
                .await
                .unwrap();
        sqlx::query(
            "INSERT INTO contact_handles (account_id, handle_id, contact_id)
             VALUES ($1, $2, $3)",
        )
        .bind(account)
        .bind(handle)
        .bind(contact_id)
        .execute(&mut *conn)
        .await
        .unwrap();
        handles.push(handle);
    }
    let (group_chat, _) = crate::db::handles::upsert_handle_row(
        &mut conn,
        account,
        "chat-sam-group",
        HandleType::Other,
        None,
    )
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO conversations (
            id, account_id, chat_handle_id, conversation_type, group_title, source_file
         ) VALUES (1, $1, $2, 'group', 'Sam Group', 'g.jsonl')",
    )
    .bind(account)
    .bind(group_chat)
    .execute(&mut *conn)
    .await
    .unwrap();
    for handle in &handles {
        sqlx::query(
            "INSERT INTO participants (conversation_id, handle_id, name_alias)
             VALUES (1, $1, 'Sam')",
        )
        .bind(handle)
        .execute(&mut *conn)
        .await
        .unwrap();
    }

    let detail = get_contact_detail(&mut conn, account, contact_id)
        .await
        .unwrap()
        .expect("contact exists");
    assert_eq!(detail.identities.len(), 2);
    for identity in &detail.identities {
        assert_eq!(identity.conversations, 1, "{}", identity.address);
    }
    assert_eq!(detail.direct_conversations, 0);
    assert_eq!(detail.group_conversations, 1);

    let summaries = get_contact_summaries(&mut conn, account, &[contact_id])
        .await
        .unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].individual_conversations, 0);
    assert_eq!(summaries[0].group_conversations, 1);
}

/// A conversation of `kind` with `participants`, in the trash when `trashed`.
async fn insert_conversation(
    conn: &mut SqliteConnection,
    account: i64,
    id: i64,
    kind: &str,
    chat_handle: i64,
    participants: &[i64],
    trashed: bool,
) {
    sqlx::query(
        "INSERT INTO conversations (
            id, account_id, chat_handle_id, conversation_type, source_file
         ) VALUES ($1, $2, $3, $4, 'c.jsonl')",
    )
    .bind(id)
    .bind(account)
    .bind(chat_handle)
    .bind(kind)
    .execute(&mut *conn)
    .await
    .unwrap();
    for handle in participants {
        sqlx::query(
            "INSERT INTO participants (conversation_id, handle_id, name_alias)
             VALUES ($1, $2, NULL)",
        )
        .bind(id)
        .bind(handle)
        .execute(&mut *conn)
        .await
        .unwrap();
    }
    if trashed {
        sqlx::query(
            "INSERT INTO trashed_conversations (account_id, conversation_id) VALUES ($1, $2)",
        )
        .bind(account)
        .bind(id)
        .execute(&mut *conn)
        .await
        .unwrap();
    }
}

/// One message in `conversation` at minute `minute` of `day`, held at the
/// account's `owner` identity: sent by the account holder when `sender` is
/// `None`, else received from `sender`.
async fn insert_held_message(
    conn: &mut SqliteConnection,
    account: i64,
    conversation: i64,
    day: &str,
    minute: usize,
    owner: i64,
    sender: Option<i64>,
) {
    let ts = format!("{day}T{:02}:{:02}:00Z", minute / 60, minute % 60);
    sqlx::query(
        "INSERT INTO messages (
            conversation_id, account_id, source, timestamp, is_from_me,
            owner_handle_id, sender_handle_id, sort_order, body
         ) VALUES ($1, $2, 'imessage', $3, $4, $5, $6, $7, 'm')",
    )
    .bind(conversation)
    .bind(account)
    .bind(ts)
    .bind(i64::from(sender.is_none()))
    .bind(owner)
    .bind(sender)
    .bind(minute as i64)
    .execute(&mut *conn)
    .await
    .unwrap();
}

/// Jane sent 40 of the 100 messages in her direct conversation with the
/// account holder and 10 of the 60 in a group with Bob. Her identity row and
/// her selection summary both read 40 direct and 10 group, with her own
/// first and last message as the dates: the holder's replies and Bob's
/// messages are not hers, and a conversation in the trash is left out, as
/// `messages:` leaves it out (#913). The account's own identity still counts
/// every message held at it (ADR-0015).
#[tokio::test]
async fn a_contacts_identity_and_summary_count_the_messages_it_sent() {
    let fixture = test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;

    let mine =
        account_profile::link_account_handle(&mut conn, account, "+15555550001", HandleType::Phone)
            .await
            .unwrap();
    let jane = insert_contact_with_handle(&mut conn, account, "Jane", "+15555550100").await;
    let jane_phone: i64 =
        sqlx::query_scalar("SELECT handle_id FROM contact_handles WHERE contact_id = $1")
            .bind(jane)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    // Jane's second identity takes part in the group and sent nothing: its
    // row must not borrow the messages her phone sent.
    let (jane_email, _) = handles::upsert_handle_row(
        &mut conn,
        account,
        "jane@example.com",
        HandleType::Email,
        Some("email"),
    )
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO contact_handles (account_id, handle_id, contact_id) VALUES ($1, $2, $3)",
    )
    .bind(account)
    .bind(jane_email)
    .bind(jane)
    .execute(&mut *conn)
    .await
    .unwrap();
    let bob = insert_contact_with_handle(&mut conn, account, "Bob", "+15555550200").await;
    let bob_phone: i64 =
        sqlx::query_scalar("SELECT handle_id FROM contact_handles WHERE contact_id = $1")
            .bind(bob)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    let (group_chat, _) =
        handles::upsert_handle_row(&mut conn, account, "chat-jane-bob", HandleType::Other, None)
            .await
            .unwrap();

    // Direct: Jane sends when minute % 5 is 1 or 2 (40 of 100); the holder
    // writes the first and the last message.
    insert_conversation(
        &mut conn,
        account,
        1,
        "individual",
        jane_phone,
        &[jane_phone],
        false,
    )
    .await;
    for minute in 0..100 {
        let sender = matches!(minute % 5, 1 | 2).then_some(jane_phone);
        insert_held_message(&mut conn, account, 1, "2024-01-01", minute, mine, sender).await;
    }
    // Group: Jane sends when minute % 6 is 1 (10 of 60), Bob when it is 2,
    // 3 or 4, the holder otherwise, the last message included.
    insert_conversation(
        &mut conn,
        account,
        2,
        "group",
        group_chat,
        &[jane_phone, jane_email, bob_phone],
        false,
    )
    .await;
    for minute in 0..60 {
        let sender = match minute % 6 {
            1 => Some(jane_phone),
            2..=4 => Some(bob_phone),
            _ => None,
        };
        insert_held_message(&mut conn, account, 2, "2024-02-01", minute, mine, sender).await;
    }
    // A later group in the trash, where Jane sent 5 more.
    let (trashed_chat, _) =
        handles::upsert_handle_row(&mut conn, account, "chat-trashed", HandleType::Other, None)
            .await
            .unwrap();
    insert_conversation(
        &mut conn,
        account,
        3,
        "group",
        trashed_chat,
        &[jane_phone],
        true,
    )
    .await;
    for minute in 0..5 {
        insert_held_message(
            &mut conn,
            account,
            3,
            "2024-03-01",
            minute,
            mine,
            Some(jane_phone),
        )
        .await;
    }

    let detail = get_contact_detail(&mut conn, account, jane)
        .await
        .unwrap()
        .expect("Jane exists");
    assert_eq!(detail.total_messages, 50);
    let [phone, email] = detail.identities.as_slice() else {
        panic!("two identities: {:?}", detail.identities);
    };
    assert_eq!(phone.address, "+15555550100");
    assert_eq!(phone.conversations, 2, "the trashed group is left out");
    assert_eq!(phone.direct_messages, 40);
    assert_eq!(phone.group_messages, 10);
    assert_eq!(phone.start_date.as_deref(), Some("2024-01-01T00:01:00Z"));
    assert_eq!(phone.end_date.as_deref(), Some("2024-02-01T00:55:00Z"));
    assert_eq!(email.address, "jane@example.com");
    assert_eq!(email.conversations, 1, "it takes part in the group");
    assert_eq!(email.direct_messages, 0);
    assert_eq!(email.group_messages, 0);
    assert_eq!(email.start_date, None);
    assert_eq!(email.end_date, None);

    let summaries = get_contact_summaries(&mut conn, account, &[jane, bob])
        .await
        .unwrap();
    let [jane_summary, bob_summary] = summaries.as_slice() else {
        panic!("two summaries: {summaries:?}");
    };
    assert_eq!(jane_summary.id, jane);
    assert_eq!(jane_summary.individual_conversations, 1);
    assert_eq!(jane_summary.group_conversations, 1);
    assert_eq!(jane_summary.individual_message_count, 40);
    assert_eq!(jane_summary.group_message_count, 10);
    assert_eq!(
        jane_summary.start_date.as_deref(),
        Some("2024-01-01T00:01:00Z")
    );
    assert_eq!(
        jane_summary.end_date.as_deref(),
        Some("2024-02-01T00:55:00Z")
    );
    assert_eq!(bob_summary.id, bob);
    assert_eq!(bob_summary.individual_message_count, 0);
    assert_eq!(bob_summary.group_message_count, 30);

    // The holder's identity counts every message held at it, sent or
    // received, in both kept conversations.
    let own = handles::identities(&mut conn, IdentitiesOf::Account(account))
        .await
        .unwrap();
    let held = own
        .iter()
        .find(|identity| identity.address == "+15555550001")
        .expect("the holder's identity");
    assert_eq!(held.conversations, 2);
    assert_eq!(held.direct_messages, 100);
    assert_eq!(held.group_messages, 60);
    assert_eq!(held.start_date.as_deref(), Some("2024-01-01T00:00:00Z"));
    assert_eq!(held.end_date.as_deref(), Some("2024-02-01T00:59:00Z"));
}

#[tokio::test]
async fn mutate_contact_add_update_remove_handle_and_rename() {
    let fixture = test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    let contact_id: i64 = sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name) VALUES ($1, 'Sam') RETURNING id",
    )
    .bind(account)
    .fetch_one(&mut *conn)
    .await
    .unwrap();

    assert!(
        mutate_committed(
            &mut conn,
            account,
            contact_id,
            &UpdateContactRequest {
                name: None,
                add_identity: Some(AddContactIdentityRequest {
                    address: "+15555550200".into(),
                    service: Some("phone".into()),
                }),
                update_identity: None,
                remove_identity: None,
            },
        )
        .await
        .unwrap()
    );

    let detail = get_contact_detail(&mut conn, account, contact_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(detail.identities.len(), 1);
    assert!(detail.identities[0].address.contains("5555550200"));

    assert!(
        mutate_committed(
            &mut conn,
            account,
            contact_id,
            &UpdateContactRequest {
                name: Some("Samantha".into()),
                add_identity: None,
                update_identity: None,
                remove_identity: None,
            },
        )
        .await
        .unwrap()
    );
    let renamed = get_contact_detail(&mut conn, account, contact_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(renamed.name, "Samantha");

    assert!(
        mutate_committed(
            &mut conn,
            account,
            contact_id,
            &UpdateContactRequest {
                name: None,
                add_identity: None,
                update_identity: Some(UpdateContactIdentityRequest {
                    previous_address: detail.identities[0].address.clone(),
                    address: "sam@example.com".into(),
                    service: Some("email".into()),
                }),
                remove_identity: None,
            },
        )
        .await
        .unwrap()
    );
    let updated = get_contact_detail(&mut conn, account, contact_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.identities.len(), 1);
    assert_eq!(updated.identities[0].address, "sam@example.com");

    assert!(
        mutate_committed(
            &mut conn,
            account,
            contact_id,
            &UpdateContactRequest {
                name: None,
                add_identity: None,
                update_identity: None,
                remove_identity: Some(RemoveContactIdentityRequest {
                    address: "sam@example.com".into(),
                    service: Some("phone".into()),
                }),
            },
        )
        .await
        .unwrap()
    );
    let empty = get_contact_detail(&mut conn, account, contact_id)
        .await
        .unwrap()
        .unwrap();
    assert!(empty.identities.is_empty());
}

/// Add `raw` to `contact_id` with `service`, through the contact edit.
async fn add_identity(
    conn: &mut SqliteConnection,
    account: i64,
    contact_id: i64,
    raw: &str,
    service: Option<&str>,
) {
    assert!(
        mutate_committed(
            conn,
            account,
            contact_id,
            &UpdateContactRequest {
                name: None,
                add_identity: Some(AddContactIdentityRequest {
                    address: raw.into(),
                    service: service.map(Into::into),
                }),
                update_identity: None,
                remove_identity: None,
            },
        )
        .await
        .unwrap()
    );
}

/// The stored `(handle_type, service)` of the handle added as `raw`.
async fn handle_type_and_service(
    conn: &mut SqliteConnection,
    account: i64,
    raw: &str,
) -> (String, Option<String>) {
    sqlx::query_as("SELECT handle_type, service FROM handles WHERE account_id = $1 AND raw = $2")
        .bind(account)
        .bind(raw)
        .fetch_one(&mut *conn)
        .await
        .unwrap()
}

/// A number added under a messaging service is a phone, so it matches the
/// same number from any other source; an address is an email; a bare
/// username with no service stays Other rather than passing for a phone.
#[tokio::test]
async fn a_handle_takes_its_type_from_the_service_it_is_added_under() {
    let fixture = test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    let contact_id = insert_contact_with_handle(&mut conn, account, "Sam", "+15555550100").await;

    for (raw, service, expected) in [
        ("+15555550201", Some("sms"), "phone"),
        ("+15555550202", Some("imessage"), "phone"),
        ("+15555550203", Some("whatsapp"), "phone"),
        ("+15555550204", Some("phone"), "phone"),
        ("+15555550205", None, "phone"),
        ("sam@example.com", Some("email"), "email"),
        ("sam", None, "other"),
        ("sam#1234", Some("discord"), "other"),
    ] {
        add_identity(&mut conn, account, contact_id, raw, service).await;
        assert_eq!(
            handle_type_and_service(&mut conn, account, raw).await.0,
            expected,
            "{raw} under {service:?}"
        );
    }
}

/// Naming a linked handle again under another transport of the same
/// platform (`iMessage` for a number added under `sms`) changes nothing: a
/// handle's service is its platform, `phone` or `whatsapp`, never the
/// transport. The number stays one row, so the next import or edit that
/// names it under any phone transport finds that row rather than adding a
/// second one.
#[tokio::test]
async fn naming_a_handle_again_under_another_transport_keeps_one_row() {
    let fixture = test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    let contact_id = insert_contact_with_handle(&mut conn, account, "Sam", "+15555550100").await;
    add_identity(&mut conn, account, contact_id, "+15555550300", Some("sms")).await;

    assert!(
        mutate_committed(
            &mut conn,
            account,
            contact_id,
            &UpdateContactRequest {
                name: None,
                add_identity: None,
                update_identity: Some(UpdateContactIdentityRequest {
                    previous_address: "+15555550300".into(),
                    address: "+15555550300".into(),
                    service: Some("iMessage".into()),
                }),
                remove_identity: None,
            },
        )
        .await
        .unwrap()
    );
    add_identity(&mut conn, account, contact_id, "+15555550300", Some("sms")).await;

    let rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT handle_type, service FROM handles WHERE account_id = $1 AND raw = $2",
    )
    .bind(account)
    .bind("+15555550300")
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(rows, [("phone".to_string(), Some("phone".to_string()))]);
}

/// A contact holding only `+15555550100` on WhatsApp.
async fn contact_on_whatsapp(conn: &mut SqliteConnection, account: i64) -> i64 {
    let contact_id: i64 = sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name) VALUES ($1, 'Sam') RETURNING id",
    )
    .bind(account)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    add_identity(conn, account, contact_id, "+15555550100", Some("whatsapp")).await;
    contact_id
}

/// Replace the contact's `+15555550100` with `+15555550101`, under `service`.
async fn replace_identity(
    conn: &mut SqliteConnection,
    account: i64,
    contact_id: i64,
    service: Option<&str>,
) {
    assert!(
        mutate_committed(
            conn,
            account,
            contact_id,
            &UpdateContactRequest {
                name: None,
                add_identity: None,
                update_identity: Some(UpdateContactIdentityRequest {
                    previous_address: "+15555550100".into(),
                    address: "+15555550101".into(),
                    service: service.map(Into::into),
                }),
                remove_identity: None,
            },
        )
        .await
        .unwrap()
    );
}

/// The `(raw, service)` of every identity on the contact.
async fn contact_identities(
    conn: &mut SqliteConnection,
    account: i64,
    contact_id: i64,
) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT h.raw, h.service FROM contact_handles ch
         JOIN handles h ON h.id = ch.handle_id
         WHERE ch.account_id = $1 AND ch.contact_id = $2
         ORDER BY h.raw",
    )
    .bind(account)
    .bind(contact_id)
    .fetch_all(&mut *conn)
    .await
    .unwrap()
}

/// With no service in the request, the new identity takes the service of the
/// one it replaces: a WhatsApp contact stays on WhatsApp rather than moving
/// onto the phone service and leaving its WhatsApp conversations Unknown.
#[tokio::test]
async fn replacing_an_identity_with_no_service_keeps_its_service() {
    let fixture = test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    let contact_id = contact_on_whatsapp(&mut conn, account).await;

    replace_identity(&mut conn, account, contact_id, None).await;

    assert_eq!(
        contact_identities(&mut conn, account, contact_id).await,
        [("+15555550101".to_string(), "whatsapp".to_string())]
    );
}

/// A request that names a service replaces the identity on that service and
/// puts the new one there, even when the contact holds the same number on
/// the phone service, which a request with no service would pick first.
#[tokio::test]
async fn replacing_an_identity_under_a_service_uses_that_service() {
    let fixture = test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    let contact_id = contact_on_whatsapp(&mut conn, account).await;
    add_identity(
        &mut conn,
        account,
        contact_id,
        "+15555550100",
        Some("phone"),
    )
    .await;

    replace_identity(&mut conn, account, contact_id, Some("whatsapp")).await;

    assert_eq!(
        contact_identities(&mut conn, account, contact_id).await,
        [
            ("+15555550100".to_string(), "phone".to_string()),
            ("+15555550101".to_string(), "whatsapp".to_string()),
        ]
    );
}

#[tokio::test]
async fn mutate_contact_rejects_trashed_contact() {
    let fixture = test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    let contact_id =
        insert_contact_with_handle(&mut conn, account, "Trashed", "+15555550100").await;
    sqlx::query("INSERT INTO trashed_contacts (account_id, contact_id) VALUES ($1, $2)")
        .bind(account)
        .bind(contact_id)
        .execute(&mut *conn)
        .await
        .unwrap();

    let changed = mutate_committed(
        &mut conn,
        account,
        contact_id,
        &UpdateContactRequest {
            name: Some("Changed".into()),
            add_identity: None,
            update_identity: None,
            remove_identity: None,
        },
    )
    .await
    .unwrap();

    assert!(!changed);
    let name: String =
        sqlx::query_scalar("SELECT preferred_name FROM contacts WHERE id = $1 AND account_id = $2")
            .bind(contact_id)
            .bind(account)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(name, "Trashed");
}

async fn contact_last_modified(
    conn: &mut SqliteConnection,
    account: i64,
    contact_id: i64,
) -> String {
    sqlx::query_scalar("SELECT last_modified FROM contacts WHERE id = $1 AND account_id = $2")
        .bind(contact_id)
        .bind(account)
        .fetch_one(&mut *conn)
        .await
        .unwrap()
}

async fn set_contact_last_modified(
    conn: &mut SqliteConnection,
    account: i64,
    contact_id: i64,
    value: &str,
) {
    sqlx::query("UPDATE contacts SET last_modified = $1 WHERE id = $2 AND account_id = $3")
        .bind(value)
        .bind(contact_id)
        .bind(account)
        .execute(&mut *conn)
        .await
        .unwrap();
}

#[tokio::test]
async fn mutate_contact_bumps_last_modified_on_shape_changes() {
    let fixture = test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    let contact_id: i64 = sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name) VALUES ($1, 'Sam') RETURNING id",
    )
    .bind(account)
    .fetch_one(&mut *conn)
    .await
    .unwrap();

    let detail = get_contact_detail(&mut conn, account, contact_id)
        .await
        .unwrap()
        .unwrap();
    assert!(!detail.last_modified.is_empty());
    let page = list_contacts_sorted(
        &mut conn,
        account,
        "",
        &DEFAULT_CONTACT_SORT,
        DEFAULT_LIST_LIMIT,
        0,
        crate::search::tests::clock(),
    )
    .await
    .unwrap();
    assert_eq!(page.items[0].last_modified, detail.last_modified);

    const OLD: &str = "2000-01-01 00:00:00";
    set_contact_last_modified(&mut conn, account, contact_id, OLD).await;
    assert!(
        mutate_committed(
            &mut conn,
            account,
            contact_id,
            &UpdateContactRequest {
                name: Some("Samantha".into()),
                add_identity: None,
                update_identity: None,
                remove_identity: None,
            },
        )
        .await
        .unwrap()
    );
    let after_rename = contact_last_modified(&mut conn, account, contact_id).await;
    assert_ne!(after_rename, OLD);

    set_contact_last_modified(&mut conn, account, contact_id, OLD).await;
    assert!(
        mutate_committed(
            &mut conn,
            account,
            contact_id,
            &UpdateContactRequest {
                name: None,
                add_identity: Some(AddContactIdentityRequest {
                    address: "+15555550200".into(),
                    service: Some("phone".into()),
                }),
                update_identity: None,
                remove_identity: None,
            },
        )
        .await
        .unwrap()
    );
    let after_add = contact_last_modified(&mut conn, account, contact_id).await;
    assert_ne!(after_add, OLD);

    // Re-adding the same handle is a no-op and must not bump.
    set_contact_last_modified(&mut conn, account, contact_id, OLD).await;
    assert!(
        mutate_committed(
            &mut conn,
            account,
            contact_id,
            &UpdateContactRequest {
                name: None,
                add_identity: Some(AddContactIdentityRequest {
                    address: "+15555550200".into(),
                    service: Some("phone".into()),
                }),
                update_identity: None,
                remove_identity: None,
            },
        )
        .await
        .unwrap()
    );
    assert_eq!(
        contact_last_modified(&mut conn, account, contact_id).await,
        OLD
    );

    set_contact_last_modified(&mut conn, account, contact_id, OLD).await;
    assert!(
        mutate_committed(
            &mut conn,
            account,
            contact_id,
            &UpdateContactRequest {
                name: None,
                add_identity: None,
                update_identity: None,
                remove_identity: Some(RemoveContactIdentityRequest {
                    address: "+15555550200".into(),
                    service: Some("phone".into()),
                }),
            },
        )
        .await
        .unwrap()
    );
    assert_ne!(
        contact_last_modified(&mut conn, account, contact_id).await,
        OLD
    );
}

async fn insert_contact_with_handle(
    conn: &mut SqliteConnection,
    account: i64,
    name: &str,
    phone: &str,
) -> i64 {
    // Schema requires preferred_name NOT NULL; empty string = no display name.
    let contact_id: i64 = sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name) VALUES ($1, $2) RETURNING id",
    )
    .bind(account)
    .bind(name)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let handle_id = account_profile::link_account_handle(conn, account, phone, HandleType::Phone)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO contact_handles (account_id, handle_id, contact_id)
         VALUES ($1, $2, $3)",
    )
    .bind(account)
    .bind(handle_id)
    .bind(contact_id)
    .execute(&mut *conn)
    .await
    .unwrap();
    contact_id
}

async fn insert_direct_conversation(
    conn: &mut SqliteConnection,
    account: i64,
    conversation_id: i64,
    phone: &str,
    service: &str,
    timestamps: &[&str],
) {
    let handle_id: i64 = match sqlx::query_scalar::<_, i64>(
        "SELECT id FROM handles WHERE account_id = $1 AND (raw = $2 OR normalized = $2) LIMIT 1",
    )
    .bind(account)
    .bind(phone)
    .fetch_optional(&mut *conn)
    .await
    .unwrap()
    {
        Some(id) => id,
        None => account_profile::link_account_handle(conn, account, phone, HandleType::Phone)
            .await
            .unwrap(),
    };
    sqlx::query(
        "INSERT INTO conversations (
            id, account_id, chat_handle_id, conversation_type, source_file
         ) VALUES ($1, $2, $3, 'individual', 't.jsonl')",
    )
    .bind(conversation_id)
    .bind(account)
    .bind(handle_id)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO participants (conversation_id, handle_id, name_alias)
         VALUES ($1, $2, NULL)",
    )
    .bind(conversation_id)
    .bind(handle_id)
    .execute(&mut *conn)
    .await
    .unwrap();
    // Received messages, so the peer is their sender, as an import records.
    for (i, ts) in timestamps.iter().enumerate() {
        sqlx::query(
            "INSERT INTO messages (
                conversation_id, account_id, source, service, timestamp, is_from_me,
                sender_handle_id, sort_order, body
             ) VALUES ($1, $2, $3, $3, $4, 0, $5, $6, 'hi')",
        )
        .bind(conversation_id)
        .bind(account)
        .bind(service)
        .bind(ts)
        .bind(handle_id)
        .bind(i as i64)
        .execute(&mut *conn)
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn list_contacts_filters_has_messages_and_never_messaged() {
    let fixture = test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    insert_contact_with_handle(&mut conn, account, "Messaged", "+15555550100").await;
    insert_contact_with_handle(&mut conn, account, "Silent", "+15555550200").await;
    insert_direct_conversation(
        &mut conn,
        account,
        1,
        "+15555550100",
        "imessage",
        &["2024-06-01T12:00:00Z"],
    )
    .await;

    let with_msg = list_contacts_sorted(
        &mut conn,
        account,
        "messages:>0",
        &DEFAULT_CONTACT_SORT,
        DEFAULT_LIST_LIMIT,
        0,
        crate::search::tests::clock(),
    )
    .await
    .unwrap();
    assert_eq!(with_msg.total, 1);
    assert_eq!(with_msg.items[0].name, "Messaged");

    let never = list_contacts_sorted(
        &mut conn,
        account,
        "messages:0",
        &DEFAULT_CONTACT_SORT,
        DEFAULT_LIST_LIMIT,
        0,
        crate::search::tests::clock(),
    )
    .await
    .unwrap();
    assert_eq!(never.total, 1);
    assert_eq!(never.items[0].name, "Silent");
}

/// One message in `conversation_id` at `ts`, with `phone`'s handle as its
/// sender handle; `is_from_me` marks it as the owner's own.
async fn insert_message_from(
    conn: &mut SqliteConnection,
    account: i64,
    conversation_id: i64,
    phone: &str,
    ts: &str,
    is_from_me: bool,
) {
    let handle_id: i64 = sqlx::query_scalar(
        "SELECT id FROM handles WHERE account_id = $1 AND (raw = $2 OR normalized = $2) LIMIT 1",
    )
    .bind(account)
    .bind(phone)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO messages (
            conversation_id, account_id, source, service, timestamp, is_from_me,
            sender_handle_id, sort_order, body
         ) VALUES ($1, $2, 'imessage', 'imessage', $3, $4, $5, 0, 'hi')",
    )
    .bind(conversation_id)
    .bind(account)
    .bind(ts)
    .bind(i64::from(is_from_me))
    .bind(handle_id)
    .execute(&mut *conn)
    .await
    .unwrap();
}

/// `last_heard_at` is the newest message the contact's own handles sent.
/// A message the owner sent in the contact's thread does not count, and a
/// contact whose handles never sent anything has no value and sorts last in
/// either direction.
#[tokio::test]
async fn list_contacts_sorts_by_last_heard_with_silent_contacts_last() {
    let fixture = test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    insert_contact_with_handle(&mut conn, account, "Recent", "+15555550100").await;
    insert_contact_with_handle(&mut conn, account, "Older", "+15555550200").await;
    insert_contact_with_handle(&mut conn, account, "Silent", "+15555550300").await;
    // A thread per contact; the owner's messages ride on the owner's handle.
    account_profile::link_account_handle(&mut conn, account, "+15555550001", HandleType::Phone)
        .await
        .unwrap();
    insert_direct_conversation(&mut conn, account, 1, "+15555550100", "imessage", &[]).await;
    insert_direct_conversation(&mut conn, account, 2, "+15555550200", "imessage", &[]).await;
    insert_direct_conversation(&mut conn, account, 3, "+15555550300", "imessage", &[]).await;
    insert_message_from(
        &mut conn,
        account,
        1,
        "+15555550100",
        "2024-01-01T00:00:00Z",
        false,
    )
    .await;
    insert_message_from(
        &mut conn,
        account,
        1,
        "+15555550100",
        "2024-06-01T00:00:00Z",
        false,
    )
    .await;
    insert_message_from(
        &mut conn,
        account,
        2,
        "+15555550200",
        "2024-03-01T00:00:00Z",
        false,
    )
    .await;
    // The owner wrote to Silent last year and to Older yesterday: neither
    // is hearing from them.
    insert_message_from(
        &mut conn,
        account,
        3,
        "+15555550001",
        "2023-12-01T00:00:00Z",
        true,
    )
    .await;
    insert_message_from(
        &mut conn,
        account,
        2,
        "+15555550001",
        "2025-01-01T00:00:00Z",
        true,
    )
    .await;
    // Older wrote again this year, in a group chat now in the trash: the
    // column leaves the trash out, as `last-message:` does (#725).
    let binned_chat =
        account_profile::link_account_handle(&mut conn, account, "chat-binned", HandleType::Other)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO conversations (
            id, account_id, chat_handle_id, conversation_type, group_title, source_file
         ) VALUES (4, $1, $2, 'group', 'Binned', 'b.jsonl')",
    )
    .bind(account)
    .bind(binned_chat)
    .execute(&mut *conn)
    .await
    .unwrap();
    insert_message_from(
        &mut conn,
        account,
        4,
        "+15555550200",
        "2025-02-01T00:00:00Z",
        false,
    )
    .await;
    sqlx::query("INSERT INTO trashed_conversations (account_id, conversation_id) VALUES ($1, 4)")
        .bind(account)
        .execute(&mut *conn)
        .await
        .unwrap();

    let newest_first = names_and_last_heard(&mut conn, account, "-last_heard").await;
    assert_eq!(
        newest_first,
        [
            (
                "Recent".to_string(),
                Some("2024-06-01T00:00:00Z".to_string())
            ),
            (
                "Older".to_string(),
                Some("2024-03-01T00:00:00Z".to_string())
            ),
            ("Silent".to_string(), None),
        ]
    );
    let oldest_first = names_and_last_heard(&mut conn, account, "last_heard").await;
    assert_eq!(
        oldest_first
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>(),
        ["Older", "Recent", "Silent"]
    );
}

/// The whole contact list under `sort`, as (name, last_heard_at) pairs.
async fn names_and_last_heard(
    conn: &mut SqliteConnection,
    account: i64,
    sort: &str,
) -> Vec<(String, Option<String>)> {
    let page = list_contacts_sorted(
        conn,
        account,
        "",
        &parse_sort(Some(sort), &CONTACT_SORT_KEYS, &DEFAULT_CONTACT_SORT).unwrap(),
        DEFAULT_LIST_LIMIT,
        0,
        crate::search::tests::clock(),
    )
    .await
    .unwrap();
    page.items
        .into_iter()
        .map(|c| (c.name, c.last_heard_at))
        .collect()
}

#[tokio::test]
async fn contacts_route_accepts_last_heard_and_refuses_other_keys() {
    let (fixture, account) = contacts_fixture_with_handles(&["+15555550100"]).await;
    let page: serde_json::Value = crate::test_support::get_json(
        &fixture.state,
        "/v1/contacts?sort=-last_heard",
        &account.token,
    )
    .await;
    assert_eq!(page["items"][0]["name"], "Contact 0");
    assert!(page["items"][0]["last_heard_at"].is_null());

    let (status, body) = crate::test_support::get_raw(
        &fixture.state,
        "/v1/contacts?sort=last_seen",
        &account.token,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body.contains("name, last_heard"), "{body}");
}

#[tokio::test]
async fn list_contacts_filters_no_handle() {
    let fixture = test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    insert_contact_with_handle(&mut conn, account, "WithHandle", "+15555550100").await;
    sqlx::query("INSERT INTO contacts (account_id, preferred_name) VALUES ($1, $2)")
        .bind(account)
        .bind("Orphan")
        .execute(&mut *conn)
        .await
        .unwrap();

    let page = list_contacts_sorted(
        &mut conn,
        account,
        "identity:none",
        &DEFAULT_CONTACT_SORT,
        DEFAULT_LIST_LIMIT,
        0,
        crate::search::tests::clock(),
    )
    .await
    .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].name, "Orphan");
    assert_eq!(page.items[0].identity_count, 0);
}

#[tokio::test]
async fn list_contacts_filters_service_or() {
    let fixture = test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    insert_contact_with_handle(&mut conn, account, "IMsg", "+15555550100").await;
    insert_contact_with_handle(&mut conn, account, "Sms", "+15555550200").await;
    insert_contact_with_handle(&mut conn, account, "Wa", "+15555550300").await;
    insert_direct_conversation(
        &mut conn,
        account,
        1,
        "+15555550100",
        "iMessage",
        &["2024-06-01T12:00:00Z"],
    )
    .await;
    insert_direct_conversation(
        &mut conn,
        account,
        2,
        "+15555550200",
        "sms",
        &["2024-06-01T12:00:00Z"],
    )
    .await;
    insert_direct_conversation(
        &mut conn,
        account,
        3,
        "+15555550300",
        "whatsapp",
        &["2024-06-01T12:00:00Z"],
    )
    .await;

    let page = list_contacts_sorted(
        &mut conn,
        account,
        "service:imessage,sms",
        &DEFAULT_CONTACT_SORT,
        DEFAULT_LIST_LIMIT,
        0,
        crate::search::tests::clock(),
    )
    .await
    .unwrap();
    assert_eq!(page.total, 2);
    let names: Vec<_> = page.items.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"IMsg"));
    assert!(names.contains(&"Sms"));
}

/// The header every address book starts with.
const ADDRESS_BOOK_HEADER: &str = "contact_id,display_name,groups,service,identity_type,identity";

/// `POST /v1/contacts` with a `text/csv` body.
async fn load_address_book(
    fixture: &TestFixture,
    account: &RegisteredAccount,
    query: &str,
    body: impl Into<reqwest::Body>,
) -> (StatusCode, String) {
    crate::test_support::post_raw(
        &fixture.state,
        &format!("/v1/contacts{query}"),
        &account.token,
        "text/csv",
        body,
    )
    .await
}

/// `POST /v1/contacts/address-book`: the status, the two headers that make
/// the answer a file, and the body.
async fn export_address_book(
    fixture: &TestFixture,
    account: &RegisteredAccount,
    body: serde_json::Value,
    accept: Option<&str>,
) -> (StatusCode, String, String, String) {
    let server = crate::test_support::serve(&fixture.state).await;
    let mut request = reqwest::Client::new()
        .post(format!("{}/v1/contacts/address-book", server.base()))
        .bearer_auth(&account.token)
        .json(&body);
    if let Some(accept) = accept {
        request = request.header(reqwest::header::ACCEPT, accept);
    }
    let response = request.send().await.unwrap();
    let status = response.status();
    let header = |name: reqwest::header::HeaderName| {
        response
            .headers()
            .get(name)
            .map(|v| v.to_str().unwrap().to_string())
            .unwrap_or_default()
    };
    let content_type = header(reqwest::header::CONTENT_TYPE);
    let disposition = header(reqwest::header::CONTENT_DISPOSITION);
    // Read the body before `server` drops and aborts the task.
    let text = response.text().await.unwrap();
    (status, content_type, disposition, text)
}

/// A good file loads and answers the seven counts; a file that breaks a rule
/// is the caller's to fix, so it answers `422` with one sentence for each bad
/// row and stores nothing.
#[tokio::test]
async fn an_address_book_loads_and_a_bad_one_is_a_422_naming_each_row() {
    let (fixture, account) = fixture_with_account().await;
    let bad = format!(
        "{ADDRESS_BOOK_HEADER}\n\
         a,Dana,,phone,phone,+15555550100\n\
         b,Eli,,carrier pigeon,phone,+15555550101\n\
         c,Flo,,phone,email,flo.example.com\n"
    );
    let (status, text) = load_address_book(&fixture, &account, "", bad).await;
    let problem = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::ValidationFailed,
    );
    let errors = problem.errors.unwrap();
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(errors[0].starts_with("row 3: service"), "{errors:?}");
    assert!(errors[1].starts_with("row 4: "), "{errors:?}");
    let page: serde_json::Value =
        crate::test_support::get_json(&fixture.state, "/v1/contacts", &account.token).await;
    assert_eq!(page["total"], 0, "the good row did not go in");

    let good = format!(
        "{ADDRESS_BOOK_HEADER}\n\
         a,Dana,Family,phone,phone,+15555550100\n\
         a,Dana,Family,phone,email,dana@example.com\n"
    );
    let (status, text) = load_address_book(&fixture, &account, "", good).await;
    assert_eq!(status, StatusCode::OK, "{text}");
    let body: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        body,
        serde_json::json!({
            "contacts_created": 1,
            "contacts_updated": 0,
            "contacts_deleted": 0,
            "identities_added": 2,
            "identities_moved": 0,
            "identities_removed": 0,
            "groups_created": 1,
            "notes": [],
        })
    );
}

/// `mode` is how the file is applied. Absent is `append`, which removes
/// nothing; `edit` takes off what the rows do not list; any other word is
/// refused before the file is read.
#[tokio::test]
async fn the_mode_parameter_picks_append_or_edit() {
    let (fixture, account) = contacts_fixture_with_handles(&["+15555550100"]).await;
    let page: serde_json::Value =
        crate::test_support::get_json(&fixture.state, "/v1/contacts", &account.token).await;
    let id = page["items"][0]["id"].as_i64().unwrap();
    // The contact stays in the file with a different identity.
    let file = format!("{ADDRESS_BOOK_HEADER}\n{id},Contact 0,,phone,email,zero@example.com\n");

    for query in ["", "?mode=append"] {
        let (status, text) = load_address_book(&fixture, &account, query, file.clone()).await;
        assert_eq!(status, StatusCode::OK, "{text}");
        let body: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(body["identities_removed"], 0, "{query}: {text}");
    }
    let (status, text) = load_address_book(&fixture, &account, "?mode=edit", file.clone()).await;
    assert_eq!(status, StatusCode::OK, "{text}");
    let body: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(body["identities_removed"], 1, "{text}");
    assert_eq!(body["contacts_updated"], 1, "{text}");

    let (status, text) = load_address_book(&fixture, &account, "?mode=replace", file).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{text}");
}

/// The file is the body and `text/csv` is its only format. A vCard, which
/// the route once read, is `415` like any other type.
#[tokio::test]
async fn an_address_book_that_is_not_text_csv_is_a_415() {
    let (fixture, account) = fixture_with_account().await;
    for content_type in ["text/vcard", "application/json", "text/plain"] {
        let (status, text) = crate::test_support::post_raw(
            &fixture.state,
            "/v1/contacts",
            &account.token,
            content_type,
            "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Dana\r\nTEL:+15555550100\r\nEND:VCARD\r\n",
        )
        .await;
        crate::test_support::expect_problem(
            status,
            &text,
            crate::problem::ProblemType::UnsupportedMediaType,
        );
    }
    let (status, text) = crate::test_support::post_raw(
        &fixture.state,
        "/v1/contacts",
        &account.token,
        "text/csv; charset=utf-8",
        format!("{ADDRESS_BOOK_HEADER}\n,Dana,,phone,phone,+15555550100\n"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a charset parameter is still text/csv: {text}"
    );
}

/// An address book of exactly `len` bytes: one contact whose name is padded
/// to make up the size.
fn address_book_of(len: usize) -> String {
    let head = format!("{ADDRESS_BOOK_HEADER}\n,");
    let tail = ",,phone,phone,+15555550100\n";
    let padding = len
        .checked_sub(head.len() + tail.len())
        .expect("len holds the fixed part of the file");
    let book = format!("{head}{}{tail}", "a".repeat(padding));
    assert_eq!(book.len(), len);
    book
}

/// The size cap is on the file as sent: a book of exactly
/// `MAX_ADDRESS_BOOK_BYTES` loads, and one byte more answers `413` with
/// `read_body_limited`'s sentence, which proves the route's own cap answered
/// and not the app-wide body limit layer (whose sentence is "the request
/// body is too large") or Axum's 2 MiB extractor default.
#[tokio::test]
async fn an_address_book_at_the_size_cap_loads_and_one_byte_over_is_a_413() {
    use address_book::MAX_ADDRESS_BOOK_BYTES;
    let (fixture, account) = fixture_with_account().await;

    let (status, text) = load_address_book(
        &fixture,
        &account,
        "",
        address_book_of(MAX_ADDRESS_BOOK_BYTES),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{text}");
    let body: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(body["contacts_created"], 1, "{text}");

    let (status, text) = load_address_book(
        &fixture,
        &account,
        "",
        address_book_of(MAX_ADDRESS_BOOK_BYTES + 1),
    )
    .await;
    let problem = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::PayloadTooLarge,
    );
    assert_eq!(
        problem.detail.as_deref(),
        Some("request body too large"),
        "the sentence must be the route's own: {text}"
    );
}

/// The export answers a file, not JSON: `text/csv`, a `Content-Disposition`
/// naming it, and one row per identity. It is let past the JSON `Accept`
/// check, so a client that asks for `text/csv` is answered, not refused
/// with `406 Not Acceptable`.
#[tokio::test]
async fn the_address_book_export_answers_a_csv_attachment() {
    let (fixture, account) = contacts_fixture_with_handles(&["+15555550100", "+15555550101"]).await;
    for accept in [None, Some("text/csv"), Some("application/json")] {
        let (status, content_type, disposition, text) =
            export_address_book(&fixture, &account, serde_json::json!({}), accept).await;
        assert_eq!(status, StatusCode::OK, "{accept:?}: {text}");
        assert_eq!(content_type, "text/csv; charset=utf-8", "{accept:?}");
        assert_eq!(
            disposition, "attachment; filename=\"address-book.csv\"",
            "{accept:?}"
        );
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], ADDRESS_BOOK_HEADER);
        assert_eq!(lines.len(), 3, "{text}");
        assert!(
            lines[1].ends_with(",Contact 0,,phone,phone,'+15555550100"),
            "{text}"
        );
        assert!(
            lines[2].ends_with(",Contact 1,,phone,phone,'+15555550101"),
            "{text}"
        );
    }
}

/// `q` is the Contacts list's search and `ids` its checked rows. Either
/// narrows the file, both together keep the checked rows the search matches,
/// and a search the list would refuse is refused here the same way.
#[tokio::test]
async fn the_address_book_export_holds_the_contacts_the_search_and_the_checked_rows_pick() {
    let (fixture, account) =
        contacts_fixture_with_handles(&["+15555550100", "+15555550101", "+15555550102"]).await;
    let page: serde_json::Value =
        crate::test_support::get_json(&fixture.state, "/v1/contacts", &account.token).await;
    let id_of = |name: &str| {
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == name)
            .unwrap()["id"]
            .as_i64()
            .unwrap()
    };
    let names = |text: &str| -> Vec<String> {
        text.lines()
            .skip(1)
            .map(|line| line.split(',').nth(1).unwrap().to_string())
            .collect()
    };

    let (_, _, _, text) = export_address_book(
        &fixture,
        &account,
        serde_json::json!({ "q": "+15555550101" }),
        None,
    )
    .await;
    assert_eq!(names(&text), ["Contact 1"]);

    let (_, _, _, text) = export_address_book(
        &fixture,
        &account,
        serde_json::json!({ "ids": [id_of("Contact 0"), id_of("Contact 2")] }),
        None,
    )
    .await;
    assert_eq!(names(&text), ["Contact 0", "Contact 2"]);

    let (_, _, _, text) = export_address_book(
        &fixture,
        &account,
        serde_json::json!({
            "q": "+15555550101",
            "ids": [id_of("Contact 0"), id_of("Contact 1")],
        }),
        None,
    )
    .await;
    assert_eq!(names(&text), ["Contact 1"]);

    let (status, _, _, text) = export_address_book(
        &fixture,
        &account,
        serde_json::json!({ "q": "nosuchword:1" }),
        None,
    )
    .await;
    crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::SearchQueryInvalid,
    );
}

/// Another account's contact id selects nothing: the file holds only the
/// caller's contacts.
#[tokio::test]
async fn the_address_book_export_never_holds_another_accounts_contact() {
    let (fixture, account) = contacts_fixture_with_handles(&["+15555550100"]).await;
    let other = account_with_handle(&fixture, "+15555550199").await;
    let theirs: serde_json::Value =
        crate::test_support::get_json(&fixture.state, "/v1/contacts", &other.token).await;
    let (status, _, _, text) = export_address_book(
        &fixture,
        &account,
        serde_json::json!({ "ids": [theirs["items"][0]["id"]] }),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(text.trim_end(), ADDRESS_BOOK_HEADER);
}

/// The file the export route answers goes back through the load route
/// unchanged, in both modes, with every count zero and no note.
#[tokio::test]
async fn the_exported_file_loads_back_through_the_route_and_changes_nothing() {
    let (fixture, account) = contacts_fixture_with_handles(&["+15555550100", "+15555550101"]).await;
    let (_, _, _, file) =
        export_address_book(&fixture, &account, serde_json::json!({}), None).await;
    for query in ["?mode=append", "?mode=edit"] {
        let (status, text) = load_address_book(&fixture, &account, query, file.clone()).await;
        assert_eq!(status, StatusCode::OK, "{text}");
        let mut body: serde_json::Value = serde_json::from_str(&text).unwrap();
        let body = body.as_object_mut().unwrap();
        assert_eq!(body.remove("notes"), Some(serde_json::json!([])), "{query}");
        for (count, value) in body.iter() {
            assert_eq!(value, 0, "{query}: {count}");
        }
        let (_, _, _, again) =
            export_address_book(&fixture, &account, serde_json::json!({}), None).await;
        assert_eq!(again, file, "{query}");
    }
}

#[tokio::test]
async fn unknown_group_collects_contacts_missing_a_name_or_an_identity() {
    let fixture = test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;

    // Knows who and how to reach them: not Unknown.
    insert_contact_with_handle(&mut conn, account, "Ada", "+15555550100").await;
    // Has an identity, no preferred name: Unknown by the second clause.
    insert_contact_with_handle(&mut conn, account, "", "+15555550200").await;
    // Has a name, no identity at all: Unknown by the first clause.
    crate::db::contacts::create_contact(
        &mut conn,
        account,
        "Sarah",
        crate::db::contacts::Origin::Import,
    )
    .await
    .unwrap();

    let unknown = list_contacts_sorted(
        &mut conn,
        account,
        "group:unknown",
        &DEFAULT_CONTACT_SORT,
        DEFAULT_LIST_LIMIT,
        0,
        crate::search::tests::clock(),
    )
    .await
    .unwrap();
    assert_eq!(unknown.total, 2);
    let mut names: Vec<String> = unknown.items.iter().map(|c| c.name.clone()).collect();
    names.sort();
    // A nameless contact has an empty name; the client shows its identity.
    assert_eq!(names, vec![String::new(), "Sarah".to_string()]);
    assert!(
        unknown.items.iter().all(|c| c.unknown),
        "every row of group:unknown says it is Unknown"
    );

    // Naming the nameless one takes it out of Unknown, because membership
    // is computed rather than stored.
    sqlx::query("UPDATE contacts SET preferred_name = 'Ben' WHERE account_id = $1 AND trim(preferred_name) = ''")
        .bind(account)
        .execute(&mut *conn)
        .await
        .unwrap();
    let after = list_contacts_sorted(
        &mut conn,
        account,
        "group:unknown",
        &DEFAULT_CONTACT_SORT,
        DEFAULT_LIST_LIMIT,
        0,
        crate::search::tests::clock(),
    )
    .await
    .unwrap();
    assert_eq!(after.total, 1);
    assert_eq!(after.items[0].name, "Sarah");
}

#[tokio::test]
async fn list_contacts_filters_by_group_and_no_group() {
    let fixture = test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    let family = insert_contact_with_handle(&mut conn, account, "Ada", "+15555550100").await;
    insert_contact_with_handle(&mut conn, account, "Ben", "+15555550200").await;
    crate::db::named_membership::set_membership(
        crate::db::named_membership::group_spec(),
        &mut conn,
        account,
        &[family],
        "Family",
        true,
    )
    .await
    .unwrap();

    let grouped = list_contacts_sorted(
        &mut conn,
        account,
        "group:Family",
        &DEFAULT_CONTACT_SORT,
        DEFAULT_LIST_LIMIT,
        0,
        crate::search::tests::clock(),
    )
    .await
    .unwrap();
    assert_eq!(grouped.total, 1);
    assert_eq!(grouped.items[0].name, "Ada");
    assert_eq!(grouped.items[0].groups, vec!["Family".to_string()]);

    let quoted = list_contacts_sorted(
        &mut conn,
        account,
        r#"group:"Family""#,
        &DEFAULT_CONTACT_SORT,
        DEFAULT_LIST_LIMIT,
        0,
        crate::search::tests::clock(),
    )
    .await
    .unwrap();
    assert_eq!(quoted.total, 1);

    let none = list_contacts_sorted(
        &mut conn,
        account,
        "group:none",
        &DEFAULT_CONTACT_SORT,
        DEFAULT_LIST_LIMIT,
        0,
        crate::search::tests::clock(),
    )
    .await
    .unwrap();
    assert_eq!(none.total, 1);
    assert_eq!(none.items[0].name, "Ben");
    assert!(none.items[0].groups.is_empty());
}

#[tokio::test]
async fn contact_list_takes_the_search_language() {
    let (fixture, account) = contacts_fixture_with_handles(&["+15550100", "+15550101"]).await;
    {
        let mut conn = fixture.state.db.acquire().await.unwrap();
        let group_id: i64 = sqlx::query_scalar(
            "INSERT INTO contact_groups (account_id, name) VALUES ($1, 'Family') RETURNING id",
        )
        .bind(account.account_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
        let first: i64 = sqlx::query_scalar("SELECT MIN(id) FROM contacts WHERE account_id = $1")
            .bind(account.account_id)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
        sqlx::query("INSERT INTO contact_group_members (contact_id, group_id) VALUES ($1, $2)")
            .bind(first)
            .bind(group_id)
            .execute(&mut *conn)
            .await
            .unwrap();
    }
    let page: serde_json::Value = crate::test_support::get_json(
        &fixture.state,
        "/v1/contacts?q=group:Family",
        &account.token,
    )
    .await;
    assert_eq!(page["total"], 1);
    assert_eq!(page["items"][0]["name"], "Contact 0");
    let page: serde_json::Value =
        crate::test_support::get_json(&fixture.state, "/v1/contacts?q=group:none", &account.token)
            .await;
    assert_eq!(page["total"], 1);
    assert_eq!(page["items"][0]["name"], "Contact 1");
}

#[tokio::test]
async fn contact_list_refuses_a_word_from_another_list() {
    let (fixture, account) = contacts_fixture_with_handles(&["+15550100"]).await;
    let status =
        crate::test_support::get_status(&fixture.state, "/v1/contacts?q=from:me", &account.token)
            .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[test]
fn a_refusal_is_the_persons_sentence_and_anything_else_is_internal() {
    match ApiError::from(ContactEditError::Refused(
        "handle already linked to another contact".into(),
    )) {
        ApiError::ValidationFailed(m) => {
            assert_eq!(m, ["handle already linked to another contact"])
        }
        other => panic!("expected ValidationFailed, got {other:?}"),
    }
    // A database error reaches this type through `?`, so it is a failure
    // by construction rather than by inspection of its message.
    let failed: ContactEditError = anyhow::Error::from(sqlx::Error::PoolClosed)
        .context("update contact")
        .into();
    match ApiError::from(failed) {
        ApiError::Internal(_) => {}
        other => panic!("expected Internal, got {other:?}"),
    }
}

#[tokio::test]
async fn the_contact_list_is_a_page_and_summaries_are_items() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let state = fixture.state.clone();

    let page: serde_json::Value =
        crate::test_support::get_json(&state, "/v1/contacts?limit=5", &user.token).await;
    assert_eq!(page["total"], 0);
    assert_eq!(page["limit"], 5);
    assert!(page["items"].is_array());
    assert!(page.get("contacts").is_none());

    let status =
        crate::test_support::get_status(&state, "/v1/contacts?limit=501", &user.token).await;
    assert_eq!(status, axum::http::StatusCode::UNPROCESSABLE_ENTITY);

    let summaries: serde_json::Value = crate::test_support::post_json(
        &state,
        "/v1/contacts/summaries",
        &user.token,
        serde_json::json!({ "ids": [] }),
    )
    .await;
    assert!(summaries["items"].is_array());
    assert!(summaries.get("contacts").is_none());
}

async fn trashed_contact_row_count(conn: &mut SqliteConnection, account_id: i64, id: i64) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM trashed_contacts WHERE account_id = $1 AND contact_id = $2",
    )
    .bind(account_id)
    .bind(id)
    .fetch_one(&mut *conn)
    .await
    .unwrap()
}

/// A logged-in account with one named contact on `+15550100`, in one
/// conversation (id 1) holding two messages, and already in the trash.
/// Returns the account and the contact's id.
async fn trashed_contact_fixture() -> (TestFixture, RegisteredAccount, i64) {
    let (fixture, account) = contacts_fixture_with_handles(&["+15550100"]).await;
    let mut conn = fixture.conn().await;
    insert_direct_conversation(
        &mut conn,
        account.account_id,
        1,
        "+15550100",
        "imessage",
        &["2020-01-01T00:00:00Z", "2020-01-02T00:00:00Z"],
    )
    .await;
    let id: i64 = sqlx::query_scalar("SELECT id FROM contacts WHERE account_id = $1")
        .bind(account.account_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    let status = crate::test_support::post_status(
        &fixture.state,
        &format!("/v1/contacts/{id}/trash"),
        &account.token,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    (fixture, account, id)
}

async fn contact_name_and_origin(conn: &mut SqliteConnection, id: i64) -> (String, String) {
    sqlx::query_as("SELECT preferred_name, origin FROM contacts WHERE id = $1")
        .bind(id)
        .fetch_one(&mut *conn)
        .await
        .unwrap()
}

#[tokio::test]
async fn contact_delete_makes_it_unknown_and_leaves_its_conversations_alone() {
    let (fixture, account, id) = trashed_contact_fixture().await;

    let status = crate::test_support::delete_status(
        &fixture.state,
        &format!("/v1/contacts/{id}"),
        &account.token,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let mut conn = fixture.conn().await;
    assert_eq!(
        contact_name_and_origin(&mut conn, id).await,
        (String::new(), "import".into()),
        "the name goes and the row is an import's again"
    );
    assert_eq!(
        trashed_contact_row_count(&mut conn, account.account_id, id).await,
        0,
        "it leaves the trash"
    );
    // Out of the trash and nameless, it opens again — as Unknown — and its
    // conversation counts are what they were.
    let detail: serde_json::Value = crate::test_support::get_json(
        &fixture.state,
        &format!("/v1/contacts/{id}"),
        &account.token,
    )
    .await;
    assert_eq!(detail["name"], "", "{detail}");
    assert_eq!(detail["unknown"], true, "{detail}");
    assert_eq!(detail["direct_conversations"], 1, "{detail}");
    assert_eq!(detail["total_messages"], 2, "{detail}");
    let conversations: serde_json::Value = crate::test_support::get_json(
        &fixture.state,
        "/v1/conversations?q=trashed:any",
        &account.token,
    )
    .await;
    assert_eq!(
        conversations["total"], 1,
        "no conversation is deleted with a contact"
    );
    assert_eq!(
        conversations["items"][0]["participants"][0]["name"], "+15550100",
        "the conversation now shows the handle: {conversations}"
    );
}

#[tokio::test]
async fn contact_delete_refuses_a_contact_that_is_not_in_the_trash() {
    let (fixture, account) = contacts_fixture_with_handles(&["+15550100"]).await;
    let list: serde_json::Value =
        crate::test_support::get_json(&fixture.state, "/v1/contacts", &account.token).await;
    let id = list["items"][0]["id"].as_i64().unwrap();

    let (status, body) = crate::test_support::delete_raw(
        &fixture.state,
        &format!("/v1/contacts/{id}"),
        &account.token,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body.contains("not in the trash"), "{body}");
    let mut conn = fixture.conn().await;
    assert_eq!(
        contact_name_and_origin(&mut conn, id).await.0,
        "Contact 0",
        "the name stays"
    );
    drop(account);
}

#[tokio::test]
async fn contact_delete_404s_for_an_unknown_id_and_for_another_accounts() {
    let (fixture, alice, alices) = trashed_contact_fixture().await;
    let bob = register_via_api(&fixture.state, "bob", "hunter2hunter2").await;

    let status =
        crate::test_support::delete_status(&fixture.state, "/v1/contacts/999999", &alice.token)
            .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let status = crate::test_support::delete_status(
        &fixture.state,
        &format!("/v1/contacts/{alices}"),
        &bob.token,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "Bob must not learn the id exists"
    );
    let mut conn = fixture.conn().await;
    assert_eq!(
        contact_name_and_origin(&mut conn, alices).await.0,
        "Contact 0",
        "Bob's request must not touch Alice's contact"
    );
}

#[tokio::test]
async fn contact_delete_needs_the_delete_permission() {
    let (fixture, account, id) = trashed_contact_fixture().await;
    fixture.turn_off_delete(account.account_id).await;

    let status = crate::test_support::delete_status(
        &fixture.state,
        &format!("/v1/contacts/{id}"),
        &account.token,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let mut conn = fixture.conn().await;
    assert_eq!(contact_name_and_origin(&mut conn, id).await.0, "Contact 0");
}

#[tokio::test]
async fn contact_trash_drops_it_from_the_list() {
    let (fixture, account) = contacts_fixture_with_handles(&["+15550100"]).await;
    let list: serde_json::Value =
        crate::test_support::get_json(&fixture.state, "/v1/contacts", &account.token).await;
    let id = list["items"][0]["id"].as_i64().unwrap();

    let status = crate::test_support::post_status(
        &fixture.state,
        &format!("/v1/contacts/{id}/trash"),
        &account.token,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NO_CONTENT);

    let list_after: serde_json::Value =
        crate::test_support::get_json(&fixture.state, "/v1/contacts", &account.token).await;
    assert_eq!(
        list_after["total"], 0,
        "a trashed contact must leave the contacts list"
    );
}

#[tokio::test]
async fn contact_trash_twice_is_204_with_no_second_marker() {
    let (fixture, account) = contacts_fixture_with_handles(&["+15550100"]).await;
    let list: serde_json::Value =
        crate::test_support::get_json(&fixture.state, "/v1/contacts", &account.token).await;
    let id = list["items"][0]["id"].as_i64().unwrap();
    let path = format!("/v1/contacts/{id}/trash");

    for _ in 0..2 {
        let status = crate::test_support::post_status(
            &fixture.state,
            &path,
            &account.token,
            serde_json::json!({}),
        )
        .await;
        assert_eq!(status, axum::http::StatusCode::NO_CONTENT);
    }

    let mut conn = fixture.state.db.acquire().await.unwrap();
    assert_eq!(
        trashed_contact_row_count(&mut conn, account.account_id, id).await,
        1,
        "trashing twice must not create a second marker row"
    );
}

#[tokio::test]
async fn contact_restore_brings_it_back_to_the_list() {
    let (fixture, account) = contacts_fixture_with_handles(&["+15550100"]).await;
    let list: serde_json::Value =
        crate::test_support::get_json(&fixture.state, "/v1/contacts", &account.token).await;
    let id = list["items"][0]["id"].as_i64().unwrap();
    crate::test_support::post_status(
        &fixture.state,
        &format!("/v1/contacts/{id}/trash"),
        &account.token,
        serde_json::json!({}),
    )
    .await;

    let status = crate::test_support::post_status(
        &fixture.state,
        &format!("/v1/contacts/{id}/restore"),
        &account.token,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NO_CONTENT);

    let list_after: serde_json::Value =
        crate::test_support::get_json(&fixture.state, "/v1/contacts", &account.token).await;
    assert_eq!(
        list_after["total"], 1,
        "a restored contact must come back to the contacts list"
    );
}

#[tokio::test]
async fn contact_restore_twice_is_204_with_marker_gone() {
    let (fixture, account) = contacts_fixture_with_handles(&["+15550100"]).await;
    let list: serde_json::Value =
        crate::test_support::get_json(&fixture.state, "/v1/contacts", &account.token).await;
    let id = list["items"][0]["id"].as_i64().unwrap();
    crate::test_support::post_status(
        &fixture.state,
        &format!("/v1/contacts/{id}/trash"),
        &account.token,
        serde_json::json!({}),
    )
    .await;
    let path = format!("/v1/contacts/{id}/restore");

    for _ in 0..2 {
        let status = crate::test_support::post_status(
            &fixture.state,
            &path,
            &account.token,
            serde_json::json!({}),
        )
        .await;
        assert_eq!(status, axum::http::StatusCode::NO_CONTENT);
    }

    let mut conn = fixture.state.db.acquire().await.unwrap();
    assert_eq!(
        trashed_contact_row_count(&mut conn, account.account_id, id).await,
        0,
        "restoring twice must leave no marker row"
    );
}

#[tokio::test]
async fn contact_trash_404s_for_an_unknown_id() {
    let (fixture, account) = contacts_fixture_with_handles(&[]).await;

    let status = crate::test_support::post_status(
        &fixture.state,
        "/v1/contacts/999999/trash",
        &account.token,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn contact_restore_404s_for_an_unknown_id() {
    let (fixture, account) = contacts_fixture_with_handles(&[]).await;

    let status = crate::test_support::post_status(
        &fixture.state,
        "/v1/contacts/999999/restore",
        &account.token,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn contact_trash_404s_for_another_accounts_contact() {
    let (fixture, alice) = contacts_fixture_with_handles(&["+15550100"]).await;
    let alice_list: serde_json::Value =
        crate::test_support::get_json(&fixture.state, "/v1/contacts", &alice.token).await;
    let alice_contact_id = alice_list["items"][0]["id"].as_i64().unwrap();

    let bob = crate::test_support::register_via_api(&fixture.state, "bob", "hunter2hunter2").await;

    // Bob trashing Alice's contact id must 404, not 403 — a 403 would
    // confirm the id exists in someone else's account.
    let status = crate::test_support::post_status(
        &fixture.state,
        &format!("/v1/contacts/{alice_contact_id}/trash"),
        &bob.token,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);

    let mut conn = fixture.state.db.acquire().await.unwrap();
    assert_eq!(
        trashed_contact_row_count(&mut conn, alice.account_id, alice_contact_id).await,
        0,
        "Bob's request must not trash Alice's contact"
    );
}

#[tokio::test]
async fn contact_restore_404s_for_another_accounts_contact() {
    let (fixture, alice) = contacts_fixture_with_handles(&["+15550100"]).await;
    let alice_list: serde_json::Value =
        crate::test_support::get_json(&fixture.state, "/v1/contacts", &alice.token).await;
    let alice_contact_id = alice_list["items"][0]["id"].as_i64().unwrap();
    let mut conn = fixture.state.db.acquire().await.unwrap();
    sqlx::query("INSERT INTO trashed_contacts (account_id, contact_id) VALUES ($1, $2)")
        .bind(alice.account_id)
        .bind(alice_contact_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    drop(conn);

    let bob = crate::test_support::register_via_api(&fixture.state, "bob", "hunter2hunter2").await;

    let status = crate::test_support::post_status(
        &fixture.state,
        &format!("/v1/contacts/{alice_contact_id}/restore"),
        &bob.token,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);

    let mut conn = fixture.state.db.acquire().await.unwrap();
    assert_eq!(
        trashed_contact_row_count(&mut conn, alice.account_id, alice_contact_id).await,
        1,
        "Bob's request must not restore Alice's contact"
    );
}

/// A long comma list is refused as a search with too many parts before any SQL
/// is built, because SQLite refuses the `OR` chain it would become and the
/// request would answer 500.
#[tokio::test]
async fn a_long_comma_list_is_refused_as_too_many_parts() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let values = vec!["0"; 1020].join(",");
    let q = format!("groups:{values}");
    assert!(q.len() <= 2048, "{}", q.len());
    let server = crate::test_support::serve(&fixture.state).await;
    let response = reqwest::Client::new()
        .get(format!("{}/v1/contacts", server.base()))
        .query(&[("q", q.as_str())])
        .bearer_auth(&user.token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::UNPROCESSABLE_ENTITY);
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["detail"], "The search has too many parts.", "{body}");
}

// --- #1105: an identity in a conversation never leaves its contact for no
// contact ---

/// A one-to-one conversation with Ada at +15555550123, imported into
/// `account`, and Ada's contact id.
async fn ada_in_a_conversation(conn: &mut sqlx::SqliteConnection, account: i64) -> i64 {
    crate::test_support::import_jsonl_text(
        conn,
        account,
        "imessage",
        r#"{"schema_version":4,"export":{"source":"imessage","tool":"test","tool_version":"0","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+15555550123","conversation_type":"individual","group_title":null,"participants":[{"handle":"+15555550123","display_name":"Ada"}],"stats":{"message_count":1,"attachment_count":0,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183462000}}}
{"guid":"g-ada-1105","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"imessage","message_kind":"imessage","sender_handle":"+15555550123","sender_display_name":null,"subject":null,"text":"hi","attachments":[],"imessage":null,"source":null}
"#,
    )
    .await;
    sqlx::query_scalar("SELECT id FROM contacts WHERE account_id = $1 AND preferred_name = 'Ada'")
        .bind(account)
        .fetch_one(&mut *conn)
        .await
        .unwrap()
}

/// The name of the contact `raw` is on.
async fn holder_name(conn: &mut sqlx::SqliteConnection, raw: &str) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT ct.preferred_name FROM handles h
         JOIN contact_handles ch ON ch.handle_id = h.id
         JOIN contacts ct ON ct.id = ch.contact_id
         WHERE h.raw = $1",
    )
    .bind(raw)
    .fetch_all(&mut *conn)
    .await
    .unwrap()
}

#[tokio::test]
async fn removing_an_identity_in_a_conversation_puts_it_on_a_new_unknown_contact() {
    let fixture = test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    let ada = ada_in_a_conversation(&mut conn, account).await;

    mutate_committed(
        &mut conn,
        account,
        ada,
        &UpdateContactRequest {
            name: None,
            add_identity: None,
            update_identity: None,
            remove_identity: Some(RemoveContactIdentityRequest {
                address: "+15555550123".into(),
                service: None,
            }),
        },
    )
    .await
    .unwrap();

    assert_eq!(
        holder_name(&mut conn, "+15555550123").await,
        [String::new()]
    );
    crate::test_support::assert_every_person_is_on_a_contact(&mut conn, "remove_identity").await;
}

#[tokio::test]
async fn replacing_an_identity_in_a_conversation_puts_the_old_one_on_a_new_unknown_contact() {
    let fixture = test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    let ada = ada_in_a_conversation(&mut conn, account).await;

    mutate_committed(
        &mut conn,
        account,
        ada,
        &UpdateContactRequest {
            name: None,
            add_identity: None,
            update_identity: Some(UpdateContactIdentityRequest {
                previous_address: "+15555550123".into(),
                address: "+15555550199".into(),
                service: None,
            }),
            remove_identity: None,
        },
    )
    .await
    .unwrap();

    assert_eq!(holder_name(&mut conn, "+15555550199").await, ["Ada"]);
    assert_eq!(
        holder_name(&mut conn, "+15555550123").await,
        [String::new()]
    );
    crate::test_support::assert_every_person_is_on_a_contact(&mut conn, "update_identity").await;
}

/// Two contacts that are one person are joined by removing an identity from
/// one and adding it to the other. The removed identity, which a
/// conversation uses, waits on a new contact with no name, and adding it to
/// the other contact takes it from there and leaves no empty contact behind.
#[tokio::test]
async fn an_identity_removed_from_one_contact_can_be_added_to_another() {
    let fixture = test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    let ada = ada_in_a_conversation(&mut conn, account).await;
    let lovelace: i64 = sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name) VALUES ($1, 'Ada Lovelace') RETURNING id",
    )
    .bind(account)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let edit = |remove: bool| UpdateContactRequest {
        name: None,
        add_identity: (!remove).then(|| AddContactIdentityRequest {
            address: "+15555550123".into(),
            service: None,
        }),
        update_identity: None,
        remove_identity: remove.then(|| RemoveContactIdentityRequest {
            address: "+15555550123".into(),
            service: None,
        }),
    };

    mutate_committed(&mut conn, account, ada, &edit(true))
        .await
        .unwrap();
    mutate_committed(&mut conn, account, lovelace, &edit(false))
        .await
        .unwrap();

    assert_eq!(
        holder_name(&mut conn, "+15555550123").await,
        ["Ada Lovelace"]
    );
    let nameless: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM contacts WHERE account_id = $1 AND trim(preferred_name) = ''",
    )
    .bind(account)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(nameless, 0, "the contact the identity waited on is gone");
    crate::test_support::assert_every_person_is_on_a_contact(&mut conn, "a move by hand").await;
}

/// [`mutate_contact`] in a write transaction of its own, committed when the
/// edit succeeds, as `update_contact` runs it.
async fn mutate_committed(
    conn: &mut SqliteConnection,
    account_id: i64,
    contact_id: i64,
    body: &UpdateContactRequest,
) -> Result<bool, ContactEditError> {
    let mut tx = crate::db::begin_write(conn).await?;
    let changed = mutate_contact(&mut tx, account_id, contact_id, body).await?;
    tx.commit().await?;
    Ok(changed)
}
