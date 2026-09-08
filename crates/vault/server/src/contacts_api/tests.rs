use super::*;

use crate::db::account_profile;
use crate::test_support::{
    RegisteredAccount, TestVault, post_json, post_status, register_via_api, test_vault,
};
use axum::http::StatusCode;

/// A vault, a signed-in account, and `handles` linked as contacts (one
/// contact per phone, named `Contact 0`, `Contact 1`, ...).
async fn contacts_fixture_with_handles(handles: &[&str]) -> (TestVault, String, RegisteredAccount) {
    let vault = test_vault().await;
    let account = register_via_api(&vault.state, "alice", "hunter2hunter2").await;
    if !handles.is_empty() {
        let mut conn = vault.state.db.acquire().await.unwrap();
        for (i, handle) in handles.iter().enumerate() {
            insert_contact_with_handle(
                &mut conn,
                &account.account_id,
                &format!("Contact {i}"),
                handle,
            )
            .await;
        }
    }
    let token = account.token.clone();
    (vault, token, account)
}

/// A vault, a signed-in account, and one contact linked to `handle` that
/// is then trashed.
async fn contacts_fixture_with_trashed_handle(
    handle: &str,
) -> (TestVault, String, RegisteredAccount) {
    let vault = test_vault().await;
    let account = register_via_api(&vault.state, "alice", "hunter2hunter2").await;
    let mut conn = vault.state.db.acquire().await.unwrap();
    let contact_id =
        insert_contact_with_handle(&mut conn, &account.account_id, "Trashed", handle).await;
    sqlx::query("INSERT INTO trashed_contacts (account_id, contact_id) VALUES ($1, $2)")
        .bind(&account.account_id)
        .bind(contact_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    let token = account.token.clone();
    (vault, token, account)
}

/// A second signed-in account in the same vault, with `handle` linked to
/// one of its contacts. Used to prove `/v1/contacts/match` is scoped to
/// the calling account rather than the whole vault database.
async fn account_with_handle(vault: &TestVault, handle: &str) -> RegisteredAccount {
    let account = register_via_api(&vault.state, "bob", "hunter2hunter2").await;
    let mut conn = vault.state.db.acquire().await.unwrap();
    insert_contact_with_handle(&mut conn, &account.account_id, "Other", handle).await;
    account
}

#[tokio::test]
async fn contact_match_reports_only_the_identifiers_the_vault_does_not_have() {
    let (vault, token, _account) = contacts_fixture_with_handles(&["+15550100"]).await;
    let body = serde_json::json!({ "identifiers": ["+15550100", "+15550999"] });
    let response =
        post_json::<serde_json::Value>(&vault.state, "/v1/contacts/match", &token, body).await;
    assert_eq!(response["unknown"], serde_json::json!(["+15550999"]));
}

#[tokio::test]
async fn contact_match_ignores_blank_identifiers_and_de_duplicates() {
    let (vault, token, _account) = contacts_fixture_with_handles(&[]).await;
    let body = serde_json::json!({ "identifiers": ["+15550999", "  ", "+15550999", ""] });
    let response =
        post_json::<serde_json::Value>(&vault.state, "/v1/contacts/match", &token, body).await;
    assert_eq!(response["unknown"], serde_json::json!(["+15550999"]));
}

#[tokio::test]
async fn contact_match_collapses_duplicates_by_normalized_form() {
    // Two spellings of the same phone number must read as one new
    // person, not two — otherwise Gate 1's "N new to your vault" count
    // double-counts a single human written two ways.
    let (vault, token, _account) = contacts_fixture_with_handles(&[]).await;
    let body = serde_json::json!({ "identifiers": ["+1 (555) 010-0100", "+15550100100"] });
    let response =
        post_json::<serde_json::Value>(&vault.state, "/v1/contacts/match", &token, body).await;
    assert_eq!(
        response["unknown"],
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
    let (vault, token, _account) = contacts_fixture_with_handles(&["+15550100"]).await;
    let body = serde_json::json!({ "identifiers": ["+1 555 0100"] });
    let response =
        post_json::<serde_json::Value>(&vault.state, "/v1/contacts/match", &token, body).await;
    assert_eq!(
        response["unknown"],
        serde_json::json!([]),
        "the differently-spelled identifier normalizes to the stored value, so it is known"
    );
}

#[tokio::test]
async fn contact_match_preserves_order_across_multiple_unknowns() {
    let (vault, token, _account) = contacts_fixture_with_handles(&["+15550100"]).await;
    let body = serde_json::json!({ "identifiers": ["+15550100", "+15550200", "+15550300"] });
    let response =
        post_json::<serde_json::Value>(&vault.state, "/v1/contacts/match", &token, body).await;
    assert_eq!(
        response["unknown"],
        serde_json::json!(["+15550200", "+15550300"])
    );
}

#[tokio::test]
async fn contact_match_counts_a_trashed_contact_as_known() {
    // Trash sets a person aside; it does not make them absent. An import
    // that meets this handle attaches to the trashed contact (see
    // `import::contact_name`), so telling the gate "this person is new"
    // would promise a contact the import is not going to create (#328).
    let (vault, token, _account) = contacts_fixture_with_trashed_handle("+15550100").await;
    let body = serde_json::json!({ "identifiers": ["+15550100"] });
    let response =
        post_json::<serde_json::Value>(&vault.state, "/v1/contacts/match", &token, body).await;
    assert_eq!(response["unknown"], serde_json::json!([]));
}

#[tokio::test]
async fn contact_match_is_scoped_to_the_calling_account() {
    let (vault, token, _mine) = contacts_fixture_with_handles(&[]).await;
    let _other = account_with_handle(&vault, "+15550100").await;
    let body = serde_json::json!({ "identifiers": ["+15550100"] });
    let response =
        post_json::<serde_json::Value>(&vault.state, "/v1/contacts/match", &token, body).await;
    assert_eq!(response["unknown"], serde_json::json!(["+15550100"]));
}

/// A refusal reaches the person as a 400 carrying the sentence written
/// for them, and a request that names no edit at all is refused the same
/// way. Both went through a downcast on the error's type before
/// `ContactEditError` existed.
#[tokio::test]
async fn a_refused_contact_edit_answers_422_with_the_persons_sentence() {
    let (vault, token, account) = contacts_fixture_with_handles(&[]).await;
    let mut conn = vault.state.db.acquire().await.unwrap();
    let first =
        insert_contact_with_handle(&mut conn, &account.account_id, "Ada", "+15555550100").await;
    insert_contact_with_handle(&mut conn, &account.account_id, "Grace", "+15555550200").await;
    drop(conn);

    // Taking a handle that is already another contact's.
    let (status, sentence) = crate::test_support::patch_failure(
        &vault.state,
        &format!("/v1/contacts/{first}"),
        &token,
        serde_json::json!({ "add_handle": { "handle": "+15555550200" } }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(sentence, "handle already linked to another contact");

    // No edit named at all.
    let status = crate::test_support::patch_status(
        &vault.state,
        &format!("/v1/contacts/{first}"),
        &token,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn contact_match_rejects_an_oversized_batch() {
    let (vault, token, _account) = contacts_fixture_with_handles(&[]).await;
    let identifiers: Vec<String> = (0..MAX_MATCH_IDENTIFIERS + 1)
        .map(|i| format!("+1555{i:06}"))
        .collect();
    let status = post_status(
        &vault.state,
        "/v1/contacts/match",
        &token,
        serde_json::json!({ "identifiers": identifiers }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn list_contacts_uses_preferred_name_and_handle_ids() {
    let vault = test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000c1", "alice")
        .await;
    let mut conn = vault.conn().await;
    let contact_id: i64 = sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name) VALUES ($1, 'Pat') RETURNING id",
    )
    .bind(&account)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let handle_id = account_profile::link_account_handle(
        &mut conn,
        &account,
        "+15555550100",
        HandleType::Phone,
    )
    .await
    .unwrap();
    // link_account_handle puts it on account_handles; also link as contact handle.
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

    let page = list_contacts(
        &mut conn,
        &account,
        "",
        DEFAULT_LIST_LIMIT,
        0,
        crate::search::tests::clock(),
    )
    .await
    .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].name, "Pat");
    assert_eq!(page.items[0].handle_count, 1);
    assert!(
        page.items[0]
            .handles
            .iter()
            .any(|h| h.contains("5555550100") || h.contains("+15555550100")),
        "handles={:?}",
        page.items[0].handles
    );
}

#[tokio::test]
async fn list_contacts_filters_and_paginates() {
    let vault = test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000c1", "alice")
        .await;
    let mut conn = vault.conn().await;
    for (name, phone) in [
        ("Pat", "+15555550100"),
        ("Sam", "+15555550200"),
        ("Alex", "+15555550300"),
    ] {
        let contact_id: i64 = sqlx::query_scalar(
            "INSERT INTO contacts (account_id, preferred_name) VALUES ($1, $2) RETURNING id",
        )
        .bind(&account)
        .bind(name)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
        let handle_id =
            account_profile::link_account_handle(&mut conn, &account, phone, HandleType::Phone)
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
    }

    let by_name = list_contacts(
        &mut conn,
        &account,
        "sam",
        DEFAULT_LIST_LIMIT,
        0,
        crate::search::tests::clock(),
    )
    .await
    .unwrap();
    assert_eq!(by_name.total, 1);
    assert_eq!(by_name.items[0].name, "Sam");

    let by_handle = list_contacts(
        &mut conn,
        &account,
        "handle:5555550200",
        DEFAULT_LIST_LIMIT,
        0,
        crate::search::tests::clock(),
    )
    .await
    .unwrap();
    assert_eq!(by_handle.total, 1);
    assert_eq!(by_handle.items[0].name, "Sam");

    let page0 = list_contacts(&mut conn, &account, "", 2, 0, crate::search::tests::clock())
        .await
        .unwrap();
    assert_eq!(page0.total, 3);
    assert_eq!(page0.limit, 2);
    assert_eq!(page0.offset, 0);
    assert_eq!(page0.items.len(), 2);
    let page1 = list_contacts(&mut conn, &account, "", 2, 2, crate::search::tests::clock())
        .await
        .unwrap();
    assert_eq!(page1.total, 3);
    assert_eq!(page1.offset, 2);
    assert_eq!(page1.items.len(), 1);
}

#[tokio::test]
async fn get_contact_detail_counts_direct_group_and_messages() {
    let vault = test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000c1", "alice")
        .await;
    let mut conn = vault.conn().await;
    let contact_id: i64 = sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name) VALUES ($1, 'Sam') RETURNING id",
    )
    .bind(&account)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let peer = account_profile::link_account_handle(
        &mut conn,
        &account,
        "+15555550200",
        HandleType::Phone,
    )
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO contact_handles (account_id, handle_id, contact_id)
         VALUES ($1, $2, $3)",
    )
    .bind(&account)
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
    for (body, ts) in [
        ("hi", "2024-06-01T12:00:00Z"),
        ("there", "2024-06-01T13:00:00Z"),
    ] {
        sqlx::query(
            "INSERT INTO messages (
                conversation_id, account_id, source, timestamp, is_from_me, sort_order, body
             ) VALUES (1, $1, 'imessage', $2, 0, 0, $3)",
        )
        .bind(&account)
        .bind(ts)
        .bind(body)
        .execute(&mut *conn)
        .await
        .unwrap();
    }

    // Group conversation that includes Sam, with 1 message.
    let group_chat = account_profile::link_account_handle(
        &mut conn,
        &account,
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
    .bind(&account)
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
            conversation_id, account_id, source, timestamp, is_from_me, sort_order, body
         ) VALUES (2, $1, 'imessage', '2024-07-01T12:00:00Z', 0, 0, 'group hi')",
    )
    .bind(&account)
    .execute(&mut *conn)
    .await
    .unwrap();

    // Unrelated conversation should not be counted.
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
            id, account_id, chat_handle_id, conversation_type, source_file
         ) VALUES (9, $1, $2, 'individual', 'other.jsonl')",
    )
    .bind(&account)
    .bind(other)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO messages (
            conversation_id, account_id, source, timestamp, is_from_me, sort_order, body
         ) VALUES (9, $1, 'imessage', '2024-08-01T12:00:00Z', 0, 0, 'nope')",
    )
    .bind(&account)
    .execute(&mut *conn)
    .await
    .unwrap();

    let detail = get_contact_detail(&mut conn, &account, contact_id)
        .await
        .unwrap()
        .expect("contact exists");
    assert_eq!(detail.name, "Sam");
    assert_eq!(detail.direct_conversations, 1);
    assert_eq!(detail.group_conversations, 1);
    assert_eq!(detail.total_messages, 3);
    assert_eq!(detail.handles.len(), 1);
    assert!(
        detail.handles[0].handle.contains("5555550200")
            || detail.handles[0].handle.contains("+15555550200"),
        "handle={:?}",
        detail.handles[0].handle
    );
    assert_eq!(detail.handles[0].individual_conversations, 1);
    assert_eq!(detail.handles[0].group_conversations, 1);
    assert_eq!(detail.handles[0].individual_message_count, 2);
    assert_eq!(detail.handles[0].group_message_count, 1);
}

#[tokio::test]
async fn get_contact_summaries_counts_two_contacts_in_one_query() {
    let vault = test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000c1", "alice")
        .await;
    let mut conn = vault.conn().await;

    let sam_id: i64 = sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name) VALUES ($1, 'Sam') RETURNING id",
    )
    .bind(&account)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let sam_handle = account_profile::link_account_handle(
        &mut conn,
        &account,
        "+15555550200",
        HandleType::Phone,
    )
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO contact_handles (account_id, handle_id, contact_id)
         VALUES ($1, $2, $3)",
    )
    .bind(&account)
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
    .bind(&account)
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
                conversation_id, account_id, source, timestamp, is_from_me, sort_order, body
             ) VALUES (1, $1, 'imessage', $2, 0, 0, $3)",
        )
        .bind(&account)
        .bind(ts)
        .bind(body)
        .execute(&mut *conn)
        .await
        .unwrap();
    }
    let group_chat = account_profile::link_account_handle(
        &mut conn,
        &account,
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
    .bind(&account)
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
            conversation_id, account_id, source, timestamp, is_from_me, sort_order, body
         ) VALUES (2, $1, 'imessage', '2024-07-01T12:00:00Z', 0, 0, 'group hi')",
    )
    .bind(&account)
    .execute(&mut *conn)
    .await
    .unwrap();

    let pat_id: i64 = sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name) VALUES ($1, 'Pat') RETURNING id",
    )
    .bind(&account)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let pat_handle = account_profile::link_account_handle(
        &mut conn,
        &account,
        "+15555550100",
        HandleType::Phone,
    )
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO contact_handles (account_id, handle_id, contact_id)
         VALUES ($1, $2, $3)",
    )
    .bind(&account)
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
    .bind(&account)
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
            conversation_id, account_id, source, timestamp, is_from_me, sort_order, body
         ) VALUES (3, $1, 'imessage', '2024-05-01T09:00:00Z', 0, 0, 'hey')",
    )
    .bind(&account)
    .execute(&mut *conn)
    .await
    .unwrap();

    let summaries = get_contact_summaries(&mut conn, &account, &[sam_id, pat_id, 99_999])
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

#[tokio::test]
async fn mutate_contact_add_update_remove_handle_and_rename() {
    let vault = test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000c1", "alice")
        .await;
    let mut conn = vault.conn().await;
    let contact_id: i64 = sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name) VALUES ($1, 'Sam') RETURNING id",
    )
    .bind(&account)
    .fetch_one(&mut *conn)
    .await
    .unwrap();

    assert!(
        mutate_contact(
            &mut conn,
            &account,
            contact_id,
            &ContactMutationBody {
                name: None,
                add_handle: Some(ContactHandlePayload {
                    handle: "+15555550200".into(),
                    service: Some("phone".into()),
                }),
                update_handle: None,
                remove_handle: None,
            },
        )
        .await
        .unwrap()
    );

    let detail = get_contact_detail(&mut conn, &account, contact_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(detail.handles.len(), 1);
    assert!(detail.handles[0].handle.contains("5555550200"));

    assert!(
        mutate_contact(
            &mut conn,
            &account,
            contact_id,
            &ContactMutationBody {
                name: Some("Samantha".into()),
                add_handle: None,
                update_handle: None,
                remove_handle: None,
            },
        )
        .await
        .unwrap()
    );
    let renamed = get_contact_detail(&mut conn, &account, contact_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(renamed.name, "Samantha");

    assert!(
        mutate_contact(
            &mut conn,
            &account,
            contact_id,
            &ContactMutationBody {
                name: None,
                add_handle: None,
                update_handle: Some(ContactUpdateHandlePayload {
                    previous_handle: detail.handles[0].handle.clone(),
                    handle: "sam@example.com".into(),
                    service: Some("email".into()),
                }),
                remove_handle: None,
            },
        )
        .await
        .unwrap()
    );
    let updated = get_contact_detail(&mut conn, &account, contact_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.handles.len(), 1);
    assert_eq!(updated.handles[0].handle, "sam@example.com");

    assert!(
        mutate_contact(
            &mut conn,
            &account,
            contact_id,
            &ContactMutationBody {
                name: None,
                add_handle: None,
                update_handle: None,
                remove_handle: Some(ContactRemoveHandlePayload {
                    handle: "sam@example.com".into(),
                    service: Some("phone".into()),
                }),
            },
        )
        .await
        .unwrap()
    );
    let empty = get_contact_detail(&mut conn, &account, contact_id)
        .await
        .unwrap()
        .unwrap();
    assert!(empty.handles.is_empty());
}

#[tokio::test]
async fn mutate_contact_rejects_trashed_contact() {
    let vault = test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000c1", "alice")
        .await;
    let mut conn = vault.conn().await;
    let contact_id =
        insert_contact_with_handle(&mut conn, &account, "Trashed", "+15555550100").await;
    sqlx::query("INSERT INTO trashed_contacts (account_id, contact_id) VALUES ($1, $2)")
        .bind(&account)
        .bind(contact_id)
        .execute(&mut *conn)
        .await
        .unwrap();

    let changed = mutate_contact(
        &mut conn,
        &account,
        contact_id,
        &ContactMutationBody {
            name: Some("Changed".into()),
            add_handle: None,
            update_handle: None,
            remove_handle: None,
        },
    )
    .await
    .unwrap();

    assert!(!changed);
    let name: String =
        sqlx::query_scalar("SELECT preferred_name FROM contacts WHERE id = $1 AND account_id = $2")
            .bind(contact_id)
            .bind(&account)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(name, "Trashed");
}

async fn contact_last_modified(conn: &mut AnyConnection, account: &str, contact_id: i64) -> String {
    sqlx::query_scalar("SELECT last_modified FROM contacts WHERE id = $1 AND account_id = $2")
        .bind(contact_id)
        .bind(account)
        .fetch_one(&mut *conn)
        .await
        .unwrap()
}

async fn set_contact_last_modified(
    conn: &mut AnyConnection,
    account: &str,
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
    let vault = test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000c1", "alice")
        .await;
    let mut conn = vault.conn().await;
    let contact_id: i64 = sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name) VALUES ($1, 'Sam') RETURNING id",
    )
    .bind(&account)
    .fetch_one(&mut *conn)
    .await
    .unwrap();

    let detail = get_contact_detail(&mut conn, &account, contact_id)
        .await
        .unwrap()
        .unwrap();
    assert!(!detail.last_modified.is_empty());
    let page = list_contacts(
        &mut conn,
        &account,
        "",
        DEFAULT_LIST_LIMIT,
        0,
        crate::search::tests::clock(),
    )
    .await
    .unwrap();
    assert_eq!(page.items[0].last_modified, detail.last_modified);

    const OLD: &str = "2000-01-01 00:00:00";
    set_contact_last_modified(&mut conn, &account, contact_id, OLD).await;
    assert!(
        mutate_contact(
            &mut conn,
            &account,
            contact_id,
            &ContactMutationBody {
                name: Some("Samantha".into()),
                add_handle: None,
                update_handle: None,
                remove_handle: None,
            },
        )
        .await
        .unwrap()
    );
    let after_rename = contact_last_modified(&mut conn, &account, contact_id).await;
    assert_ne!(after_rename, OLD);

    set_contact_last_modified(&mut conn, &account, contact_id, OLD).await;
    assert!(
        mutate_contact(
            &mut conn,
            &account,
            contact_id,
            &ContactMutationBody {
                name: None,
                add_handle: Some(ContactHandlePayload {
                    handle: "+15555550200".into(),
                    service: Some("phone".into()),
                }),
                update_handle: None,
                remove_handle: None,
            },
        )
        .await
        .unwrap()
    );
    let after_add = contact_last_modified(&mut conn, &account, contact_id).await;
    assert_ne!(after_add, OLD);

    // Re-adding the same handle is a no-op and must not bump.
    set_contact_last_modified(&mut conn, &account, contact_id, OLD).await;
    assert!(
        mutate_contact(
            &mut conn,
            &account,
            contact_id,
            &ContactMutationBody {
                name: None,
                add_handle: Some(ContactHandlePayload {
                    handle: "+15555550200".into(),
                    service: Some("phone".into()),
                }),
                update_handle: None,
                remove_handle: None,
            },
        )
        .await
        .unwrap()
    );
    assert_eq!(
        contact_last_modified(&mut conn, &account, contact_id).await,
        OLD
    );

    set_contact_last_modified(&mut conn, &account, contact_id, OLD).await;
    assert!(
        mutate_contact(
            &mut conn,
            &account,
            contact_id,
            &ContactMutationBody {
                name: None,
                add_handle: None,
                update_handle: None,
                remove_handle: Some(ContactRemoveHandlePayload {
                    handle: "+15555550200".into(),
                    service: Some("phone".into()),
                }),
            },
        )
        .await
        .unwrap()
    );
    assert_ne!(
        contact_last_modified(&mut conn, &account, contact_id).await,
        OLD
    );
}

async fn insert_contact_with_handle(
    conn: &mut AnyConnection,
    account: &str,
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
    conn: &mut AnyConnection,
    account: &str,
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
    for (i, ts) in timestamps.iter().enumerate() {
        sqlx::query(
            "INSERT INTO messages (
                conversation_id, account_id, source, service, timestamp, is_from_me, sort_order, body
             ) VALUES ($1, $2, $3, $3, $4, 0, $5, 'hi')",
        )
        .bind(conversation_id)
        .bind(account)
        .bind(service)
        .bind(ts)
        .bind(i as i64)
        .execute(&mut *conn)
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn list_contacts_filters_has_messages_and_never_messaged() {
    let vault = test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000c1", "alice")
        .await;
    let mut conn = vault.conn().await;
    insert_contact_with_handle(&mut conn, &account, "Messaged", "+15555550100").await;
    insert_contact_with_handle(&mut conn, &account, "Silent", "+15555550200").await;
    insert_direct_conversation(
        &mut conn,
        &account,
        1,
        "+15555550100",
        "imessage",
        &["2024-06-01T12:00:00Z"],
    )
    .await;

    let with_msg = list_contacts(
        &mut conn,
        &account,
        "messages:>0",
        DEFAULT_LIST_LIMIT,
        0,
        crate::search::tests::clock(),
    )
    .await
    .unwrap();
    assert_eq!(with_msg.total, 1);
    assert_eq!(with_msg.items[0].name, "Messaged");

    let never = list_contacts(
        &mut conn,
        &account,
        "messages:0",
        DEFAULT_LIST_LIMIT,
        0,
        crate::search::tests::clock(),
    )
    .await
    .unwrap();
    assert_eq!(never.total, 1);
    assert_eq!(never.items[0].name, "Silent");
}

#[tokio::test]
async fn list_contacts_filters_no_handle() {
    let vault = test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000c1", "alice")
        .await;
    let mut conn = vault.conn().await;
    insert_contact_with_handle(&mut conn, &account, "WithHandle", "+15555550100").await;
    sqlx::query("INSERT INTO contacts (account_id, preferred_name) VALUES ($1, $2)")
        .bind(&account)
        .bind("Orphan")
        .execute(&mut *conn)
        .await
        .unwrap();

    let page = list_contacts(
        &mut conn,
        &account,
        "handle:none",
        DEFAULT_LIST_LIMIT,
        0,
        crate::search::tests::clock(),
    )
    .await
    .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].name, "Orphan");
    assert_eq!(page.items[0].handle_count, 0);
}

#[tokio::test]
async fn list_contacts_filters_service_or() {
    let vault = test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000c1", "alice")
        .await;
    let mut conn = vault.conn().await;
    insert_contact_with_handle(&mut conn, &account, "IMsg", "+15555550100").await;
    insert_contact_with_handle(&mut conn, &account, "Sms", "+15555550200").await;
    insert_contact_with_handle(&mut conn, &account, "Wa", "+15555550300").await;
    insert_direct_conversation(
        &mut conn,
        &account,
        1,
        "+15555550100",
        "iMessage",
        &["2024-06-01T12:00:00Z"],
    )
    .await;
    insert_direct_conversation(
        &mut conn,
        &account,
        2,
        "+15555550200",
        "sms",
        &["2024-06-01T12:00:00Z"],
    )
    .await;
    insert_direct_conversation(
        &mut conn,
        &account,
        3,
        "+15555550300",
        "whatsapp",
        &["2024-06-01T12:00:00Z"],
    )
    .await;

    let page = list_contacts(
        &mut conn,
        &account,
        "service:imessage,sms",
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

#[test]
fn address_book_upload_name_only_decides_the_format() {
    assert_eq!(
        sanitized_address_book_name("Contacts.vcf"),
        "address-book.vcf"
    );
    assert_eq!(
        sanitized_address_book_name("  contacts.VCARD "),
        "address-book.vcf"
    );
    assert_eq!(
        sanitized_address_book_name("export.csv"),
        "address-book.csv"
    );
    // A name that tries to escape the temp directory is never used as a path.
    assert_eq!(
        sanitized_address_book_name("../../etc/passwd"),
        "address-book.csv"
    );
}

#[tokio::test]
async fn an_address_book_renames_a_contact_an_import_named() {
    let vault = test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000c1", "alice")
        .await;
    let dir = vault.dir();
    let mut conn = vault.conn().await;

    // What an import leaves behind: a contact named by the backup, holding
    // the phone, marked as the import's.
    let discovered = insert_contact_with_handle(&mut conn, &account, "Bobby", "+15551234567").await;
    sqlx::query("UPDATE contacts SET origin = 'import' WHERE id = $1")
        .bind(discovered)
        .execute(&mut *conn)
        .await
        .unwrap();

    let book = dir.join("book.vcf");
    std::fs::write(
        &book,
        "BEGIN:VCARD\nVERSION:3.0\nFN:Robert Smith\nN:Smith;Robert;;;\nTEL:+15551234567\nEND:VCARD\n",
    )
    .unwrap();
    contacts::load_contacts_if_needed(&mut conn, Some(&book), true, &account)
        .await
        .unwrap();

    let names: Vec<String> = sqlx::query_scalar(
        "SELECT preferred_name FROM contacts WHERE account_id = $1 ORDER BY preferred_name",
    )
    .bind(&account)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        names,
        vec!["Robert Smith".to_string()],
        "the book renames the imported contact instead of making a second one: {names:?}"
    );

    let name: String = sqlx::query_scalar("SELECT preferred_name FROM contacts WHERE id = $1")
        .bind(discovered)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(name, "Robert Smith");

    // The identity stays the import's, so a later book that drops the card
    // does not take the person's messages' contact with it.
    let origin: String = sqlx::query_scalar("SELECT origin FROM contacts WHERE id = $1")
        .bind(discovered)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(origin, "import");
}

#[tokio::test]
async fn a_nameless_card_does_not_blank_an_imported_name() {
    let vault = test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000c1", "alice")
        .await;
    let dir = vault.dir();
    let mut conn = vault.conn().await;

    // An import already named this person; the book only lists their
    // number, nothing more.
    let discovered = insert_contact_with_handle(&mut conn, &account, "Bobby", "+15551234567").await;
    sqlx::query("UPDATE contacts SET origin = 'import' WHERE id = $1")
        .bind(discovered)
        .execute(&mut *conn)
        .await
        .unwrap();

    let book = dir.join("book.vcf");
    std::fs::write(
        &book,
        "BEGIN:VCARD\nVERSION:3.0\nTEL:+15551234567\nEND:VCARD\n",
    )
    .unwrap();
    contacts::load_contacts_if_needed(&mut conn, Some(&book), true, &account)
        .await
        .unwrap();

    // A card with no name has nothing to say about who this person is,
    // so it does not get to unname them.
    let name: String = sqlx::query_scalar("SELECT preferred_name FROM contacts WHERE id = $1")
        .bind(discovered)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(name, "Bobby");

    let origin: String = sqlx::query_scalar("SELECT origin FROM contacts WHERE id = $1")
        .bind(discovered)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(origin, "import");
}

#[tokio::test]
async fn an_address_book_does_not_rename_a_contact_the_person_typed() {
    let vault = test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000c1", "alice")
        .await;
    let dir = vault.dir();
    let mut conn = vault.conn().await;

    // An import discovered this person and gave them the name that backup
    // used, holding the phone the book is about to load a card for.
    let hand_typed = insert_contact_with_handle(&mut conn, &account, "Bobby", "+15551234567").await;
    sqlx::query("UPDATE contacts SET origin = 'import' WHERE id = $1")
        .bind(hand_typed)
        .execute(&mut *conn)
        .await
        .unwrap();
    // The person is in a Contact Group they built by hand.
    crate::named_membership::set_membership(
        crate::named_membership::group_spec(),
        &mut conn,
        &account,
        &[hand_typed],
        "Family",
        true,
    )
    .await
    .unwrap();

    // Then the person renamed them in the drawer, the way a person does —
    // through the same route the web app calls. That, not raw SQL, is what
    // makes the row theirs.
    mutate_contact(
        &mut conn,
        &account,
        hand_typed,
        &ContactMutationBody {
            name: Some("My Friend Bob".to_string()),
            add_handle: None,
            update_handle: None,
            remove_handle: None,
        },
    )
    .await
    .unwrap();
    let origin: String = sqlx::query_scalar("SELECT origin FROM contacts WHERE id = $1")
        .bind(hand_typed)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(origin, "user", "naming someone makes the row the person's");

    let book = dir.join("book.vcf");
    std::fs::write(
        &book,
        "BEGIN:VCARD\nVERSION:3.0\nFN:Robert Smith\nN:Smith;Robert;;;\nTEL:+15551234567\nEND:VCARD\n",
    )
    .unwrap();
    contacts::load_contacts_if_needed(&mut conn, Some(&book), true, &account)
        .await
        .unwrap();

    // The name the person typed survives untouched.
    let hand_typed_name: String =
        sqlx::query_scalar("SELECT preferred_name FROM contacts WHERE id = $1")
            .bind(hand_typed)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(hand_typed_name, "My Friend Bob");

    // The card joins that person instead of standing a second contact
    // beside them. A second row would be the worse outcome: the phone is
    // already linked, so the new row would end up with no identity at all
    // and anything the card carried would land on it instead of on the
    // person.
    let ids: Vec<i64> =
        sqlx::query_scalar("SELECT id FROM contacts WHERE account_id = $1 ORDER BY id")
            .bind(&account)
            .fetch_all(&mut *conn)
            .await
            .unwrap();
    assert_eq!(
        ids,
        vec![hand_typed],
        "the card joins the person the vault already has: {ids:?}"
    );

    // They keep the identity that made them findable.
    let handles: Vec<String> = sqlx::query_scalar(
        "SELECT h.raw FROM contact_handles ch JOIN handles h ON h.id = ch.handle_id
         WHERE ch.account_id = $1 AND ch.contact_id = $2",
    )
    .bind(&account)
    .bind(hand_typed)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(handles, vec!["+15551234567".to_string()]);

    // And the Contact Group still points at them, not at a stranded row.
    let members: Vec<i64> = sqlx::query_scalar(
        "SELECT gm.contact_id FROM contact_group_members gm
         JOIN contact_groups g ON g.id = gm.group_id
         WHERE g.account_id = $1 AND g.name = 'Family'",
    )
    .bind(&account)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(members, vec![hand_typed]);
}

#[tokio::test]
async fn loading_an_address_book_replaces_only_its_own_rows() {
    let vault = test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000c1", "alice")
        .await;
    let dir = vault.dir();
    let mut conn = vault.conn().await;

    // An identity the vault learned from imported messages, and a Contact
    // Group the person built by hand.
    let discovered =
        insert_contact_with_handle(&mut conn, &account, "From Import", "+15555550999").await;
    sqlx::query("UPDATE contacts SET origin = 'import' WHERE id = $1")
        .bind(discovered)
        .execute(&mut *conn)
        .await
        .unwrap();
    crate::named_membership::set_membership(
        crate::named_membership::group_spec(),
        &mut conn,
        &account,
        &[discovered],
        "Family",
        true,
    )
    .await
    .unwrap();

    let book = dir.join("book.vcf");
    std::fs::write(
        &book,
        "BEGIN:VCARD\nVERSION:3.0\nFN:Ada Lovelace\nN:Lovelace;Ada;;;\nTEL:+15551234567\nEND:VCARD\n",
    )
    .unwrap();
    contacts::load_contacts_if_needed(&mut conn, Some(&book), true, &account)
        .await
        .unwrap();

    // A second load of a book that dropped Ada removes her, because the
    // vault knows that row was the book's.
    let book2 = dir.join("book2.vcf");
    std::fs::write(
        &book2,
        "BEGIN:VCARD\nVERSION:3.0\nFN:Grace Hopper\nN:Hopper;Grace;;;\nTEL:+15557654321\nEND:VCARD\n",
    )
    .unwrap();
    contacts::load_contacts_if_needed(&mut conn, Some(&book2), true, &account)
        .await
        .unwrap();

    let names: Vec<String> = sqlx::query_scalar(
        "SELECT preferred_name FROM contacts WHERE account_id = $1 ORDER BY preferred_name",
    )
    .bind(&account)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert!(
        names.contains(&"From Import".to_string()),
        "an import-discovered contact must survive a book reload: {names:?}"
    );
    assert!(
        names.contains(&"Grace Hopper".to_string()),
        "the new book's contact must be present: {names:?}"
    );
    assert!(
        !names.contains(&"Ada Lovelace".to_string()),
        "a contact the book dropped must go: {names:?}"
    );

    let groups: Vec<String> =
        sqlx::query_scalar("SELECT name FROM contact_groups WHERE account_id = $1 ORDER BY name")
            .bind(&account)
            .fetch_all(&mut *conn)
            .await
            .unwrap();
    assert_eq!(
        groups,
        vec!["Family".to_string()],
        "a Contact Group the person built must survive a book reload"
    );
}

#[tokio::test]
async fn unknown_group_collects_contacts_missing_a_name_or_an_identity() {
    let vault = test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000c1", "alice")
        .await;
    let mut conn = vault.conn().await;

    // Knows who and how to reach them: not Unknown.
    insert_contact_with_handle(&mut conn, &account, "Ada", "+15555550100").await;
    // Has an identity, no preferred name: Unknown by the second clause.
    insert_contact_with_handle(&mut conn, &account, "", "+15555550200").await;
    // Has a name, no identity at all: Unknown by the first clause.
    crate::db::contacts::create_contact(
        &mut conn,
        &account,
        "Sarah",
        crate::db::contacts::Origin::Import,
    )
    .await
    .unwrap();

    let unknown = list_contacts(
        &mut conn,
        &account,
        "group:unknown",
        DEFAULT_LIST_LIMIT,
        0,
        crate::search::tests::clock(),
    )
    .await
    .unwrap();
    assert_eq!(unknown.total, 2);
    let mut names: Vec<String> = unknown.items.iter().map(|c| c.name.clone()).collect();
    names.sort();
    // The list renders a nameless contact as "(unknown)".
    assert_eq!(names, vec!["(unknown)".to_string(), "Sarah".to_string()]);

    // Naming the nameless one takes it out of Unknown, because membership
    // is computed rather than stored.
    sqlx::query("UPDATE contacts SET preferred_name = 'Ben' WHERE account_id = $1 AND trim(preferred_name) = ''")
        .bind(&account)
        .execute(&mut *conn)
        .await
        .unwrap();
    let after = list_contacts(
        &mut conn,
        &account,
        "group:unknown",
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
    let vault = test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000c1", "alice")
        .await;
    let mut conn = vault.conn().await;
    let family = insert_contact_with_handle(&mut conn, &account, "Ada", "+15555550100").await;
    insert_contact_with_handle(&mut conn, &account, "Ben", "+15555550200").await;
    crate::named_membership::set_membership(
        crate::named_membership::group_spec(),
        &mut conn,
        &account,
        &[family],
        "Family",
        true,
    )
    .await
    .unwrap();

    let grouped = list_contacts(
        &mut conn,
        &account,
        "group:Family",
        DEFAULT_LIST_LIMIT,
        0,
        crate::search::tests::clock(),
    )
    .await
    .unwrap();
    assert_eq!(grouped.total, 1);
    assert_eq!(grouped.items[0].name, "Ada");
    assert_eq!(grouped.items[0].groups, vec!["Family".to_string()]);

    let quoted = list_contacts(
        &mut conn,
        &account,
        r#"group:"Family""#,
        DEFAULT_LIST_LIMIT,
        0,
        crate::search::tests::clock(),
    )
    .await
    .unwrap();
    assert_eq!(quoted.total, 1);

    let none = list_contacts(
        &mut conn,
        &account,
        "group:none",
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
    let (vault, token, account) = contacts_fixture_with_handles(&["+15550100", "+15550101"]).await;
    {
        let mut conn = vault.state.db.acquire().await.unwrap();
        let group_id: i64 = sqlx::query_scalar(
            "INSERT INTO contact_groups (account_id, name) VALUES ($1, 'Family') RETURNING id",
        )
        .bind(&account.account_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
        let first: i64 = sqlx::query_scalar("SELECT MIN(id) FROM contacts WHERE account_id = $1")
            .bind(&account.account_id)
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
    let page: serde_json::Value =
        crate::test_support::get_json(&vault.state, "/v1/contacts?q=group:Family", &token).await;
    assert_eq!(page["total"], 1);
    assert_eq!(page["items"][0]["name"], "Contact 0");
    let page: serde_json::Value =
        crate::test_support::get_json(&vault.state, "/v1/contacts?q=group:none", &token).await;
    assert_eq!(page["total"], 1);
    assert_eq!(page["items"][0]["name"], "Contact 1");
}

#[tokio::test]
async fn contact_list_refuses_a_word_from_another_list() {
    let (vault, token, _account) = contacts_fixture_with_handles(&["+15550100"]).await;
    let status =
        crate::test_support::get_status(&vault.state, "/v1/contacts?q=from:me", &token).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
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
    let vault = crate::test_support::test_vault().await;
    let state = vault.state.clone();
    let user = crate::test_support::register_via_api(&state, "alice", "hunter2hunter2").await;

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

async fn trashed_contact_row_count(conn: &mut AnyConnection, account_id: &str, id: i64) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM trashed_contacts WHERE account_id = $1 AND contact_id = $2",
    )
    .bind(account_id)
    .bind(id)
    .fetch_one(&mut *conn)
    .await
    .unwrap()
}

/// A signed-in account with one named contact on `+15550100`, in one
/// conversation (id 1) holding two messages, and already in the trash.
/// Returns the account and the contact's id.
async fn trashed_contact_fixture() -> (TestVault, RegisteredAccount, i64) {
    let (vault, token, account) = contacts_fixture_with_handles(&["+15550100"]).await;
    let mut conn = vault.conn().await;
    insert_direct_conversation(
        &mut conn,
        &account.account_id,
        1,
        "+15550100",
        "imessage",
        &["2020-01-01T00:00:00Z", "2020-01-02T00:00:00Z"],
    )
    .await;
    let id: i64 = sqlx::query_scalar("SELECT id FROM contacts WHERE account_id = $1")
        .bind(&account.account_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    let status = crate::test_support::post_status(
        &vault.state,
        &format!("/v1/contacts/{id}/trash"),
        &token,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    (vault, account, id)
}

async fn contact_name_and_origin(conn: &mut AnyConnection, id: i64) -> (String, String) {
    sqlx::query_as("SELECT preferred_name, origin FROM contacts WHERE id = $1")
        .bind(id)
        .fetch_one(&mut *conn)
        .await
        .unwrap()
}

#[tokio::test]
async fn contact_delete_makes_it_unknown_and_leaves_its_conversations_alone() {
    let (vault, account, id) = trashed_contact_fixture().await;

    let status = crate::test_support::delete_status(
        &vault.state,
        &format!("/v1/contacts/{id}"),
        &account.token,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let mut conn = vault.conn().await;
    assert_eq!(
        contact_name_and_origin(&mut conn, id).await,
        (String::new(), "import".into()),
        "the name goes and the row is an import's again"
    );
    assert_eq!(
        trashed_contact_row_count(&mut conn, &account.account_id, id).await,
        0,
        "it leaves the trash"
    );
    // Out of the trash and nameless, it opens again — as Unknown — and its
    // conversation counts are what they were.
    let detail: serde_json::Value =
        crate::test_support::get_json(&vault.state, &format!("/v1/contacts/{id}"), &account.token)
            .await;
    assert_eq!(detail["name"], "(unknown)", "{detail}");
    assert_eq!(detail["direct_conversations"], 1, "{detail}");
    assert_eq!(detail["total_messages"], 2, "{detail}");
    let conversations: serde_json::Value = crate::test_support::get_json(
        &vault.state,
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
    let (vault, token, account) = contacts_fixture_with_handles(&["+15550100"]).await;
    let list: serde_json::Value =
        crate::test_support::get_json(&vault.state, "/v1/contacts", &token).await;
    let id = list["items"][0]["id"].as_i64().unwrap();

    let (status, body) =
        crate::test_support::delete_raw(&vault.state, &format!("/v1/contacts/{id}"), &token).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body.contains("not in the trash"), "{body}");
    let mut conn = vault.conn().await;
    assert_eq!(
        contact_name_and_origin(&mut conn, id).await.0,
        "Contact 0",
        "the name stays"
    );
    drop(account);
}

#[tokio::test]
async fn contact_delete_404s_for_an_unknown_id_and_for_another_accounts() {
    let (vault, alice, alices) = trashed_contact_fixture().await;
    let bob = register_via_api(&vault.state, "bob", "hunter2hunter2").await;

    let status =
        crate::test_support::delete_status(&vault.state, "/v1/contacts/999999", &alice.token).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let status = crate::test_support::delete_status(
        &vault.state,
        &format!("/v1/contacts/{alices}"),
        &bob.token,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "Bob must not learn the id exists"
    );
    let mut conn = vault.conn().await;
    assert_eq!(
        contact_name_and_origin(&mut conn, alices).await.0,
        "Contact 0",
        "Bob's request must not touch Alice's contact"
    );
}

#[tokio::test]
async fn contact_delete_needs_the_delete_permission() {
    let (vault, account, id) = trashed_contact_fixture().await;
    {
        let mut conn = vault.conn().await;
        sqlx::query("UPDATE accounts SET can_delete = 0 WHERE id = $1")
            .bind(&account.account_id)
            .execute(&mut *conn)
            .await
            .unwrap();
    }

    let status = crate::test_support::delete_status(
        &vault.state,
        &format!("/v1/contacts/{id}"),
        &account.token,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let mut conn = vault.conn().await;
    assert_eq!(contact_name_and_origin(&mut conn, id).await.0, "Contact 0");
}

#[tokio::test]
async fn contact_delete_requires_auth() {
    let (vault, _account, id) = trashed_contact_fixture().await;
    let status = crate::test_support::delete_status(
        &vault.state,
        &format!("/v1/contacts/{id}"),
        "not-a-token",
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn contact_trash_drops_it_from_the_list() {
    let (vault, token, _account) = contacts_fixture_with_handles(&["+15550100"]).await;
    let list: serde_json::Value =
        crate::test_support::get_json(&vault.state, "/v1/contacts", &token).await;
    let id = list["items"][0]["id"].as_i64().unwrap();

    let status = crate::test_support::post_status(
        &vault.state,
        &format!("/v1/contacts/{id}/trash"),
        &token,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NO_CONTENT);

    let list_after: serde_json::Value =
        crate::test_support::get_json(&vault.state, "/v1/contacts", &token).await;
    assert_eq!(
        list_after["total"], 0,
        "a trashed contact must leave the contacts list"
    );
}

#[tokio::test]
async fn contact_trash_twice_is_204_with_no_second_marker() {
    let (vault, token, account) = contacts_fixture_with_handles(&["+15550100"]).await;
    let list: serde_json::Value =
        crate::test_support::get_json(&vault.state, "/v1/contacts", &token).await;
    let id = list["items"][0]["id"].as_i64().unwrap();
    let path = format!("/v1/contacts/{id}/trash");

    for _ in 0..2 {
        let status =
            crate::test_support::post_status(&vault.state, &path, &token, serde_json::json!({}))
                .await;
        assert_eq!(status, axum::http::StatusCode::NO_CONTENT);
    }

    let mut conn = vault.state.db.acquire().await.unwrap();
    assert_eq!(
        trashed_contact_row_count(&mut conn, &account.account_id, id).await,
        1,
        "trashing twice must not create a second marker row"
    );
}

#[tokio::test]
async fn contact_restore_brings_it_back_to_the_list() {
    let (vault, token, _account) = contacts_fixture_with_handles(&["+15550100"]).await;
    let list: serde_json::Value =
        crate::test_support::get_json(&vault.state, "/v1/contacts", &token).await;
    let id = list["items"][0]["id"].as_i64().unwrap();
    crate::test_support::post_status(
        &vault.state,
        &format!("/v1/contacts/{id}/trash"),
        &token,
        serde_json::json!({}),
    )
    .await;

    let status = crate::test_support::post_status(
        &vault.state,
        &format!("/v1/contacts/{id}/restore"),
        &token,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NO_CONTENT);

    let list_after: serde_json::Value =
        crate::test_support::get_json(&vault.state, "/v1/contacts", &token).await;
    assert_eq!(
        list_after["total"], 1,
        "a restored contact must come back to the contacts list"
    );
}

#[tokio::test]
async fn contact_restore_twice_is_204_with_marker_gone() {
    let (vault, token, account) = contacts_fixture_with_handles(&["+15550100"]).await;
    let list: serde_json::Value =
        crate::test_support::get_json(&vault.state, "/v1/contacts", &token).await;
    let id = list["items"][0]["id"].as_i64().unwrap();
    crate::test_support::post_status(
        &vault.state,
        &format!("/v1/contacts/{id}/trash"),
        &token,
        serde_json::json!({}),
    )
    .await;
    let path = format!("/v1/contacts/{id}/restore");

    for _ in 0..2 {
        let status =
            crate::test_support::post_status(&vault.state, &path, &token, serde_json::json!({}))
                .await;
        assert_eq!(status, axum::http::StatusCode::NO_CONTENT);
    }

    let mut conn = vault.state.db.acquire().await.unwrap();
    assert_eq!(
        trashed_contact_row_count(&mut conn, &account.account_id, id).await,
        0,
        "restoring twice must leave no marker row"
    );
}

#[tokio::test]
async fn contact_trash_404s_for_an_unknown_id() {
    let (vault, token, _account) = contacts_fixture_with_handles(&[]).await;

    let status = crate::test_support::post_status(
        &vault.state,
        "/v1/contacts/999999/trash",
        &token,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn contact_restore_404s_for_an_unknown_id() {
    let (vault, token, _account) = contacts_fixture_with_handles(&[]).await;

    let status = crate::test_support::post_status(
        &vault.state,
        "/v1/contacts/999999/restore",
        &token,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn contact_trash_404s_for_another_accounts_contact() {
    let (vault, alice_token, alice) = contacts_fixture_with_handles(&["+15550100"]).await;
    let alice_list: serde_json::Value =
        crate::test_support::get_json(&vault.state, "/v1/contacts", &alice_token).await;
    let alice_contact_id = alice_list["items"][0]["id"].as_i64().unwrap();

    let bob = crate::test_support::register_via_api(&vault.state, "bob", "hunter2hunter2").await;

    // Bob trashing Alice's contact id must 404, not 403 — a 403 would
    // confirm the id exists in someone else's vault.
    let status = crate::test_support::post_status(
        &vault.state,
        &format!("/v1/contacts/{alice_contact_id}/trash"),
        &bob.token,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);

    let mut conn = vault.state.db.acquire().await.unwrap();
    assert_eq!(
        trashed_contact_row_count(&mut conn, &alice.account_id, alice_contact_id).await,
        0,
        "Bob's request must not trash Alice's contact"
    );
}

#[tokio::test]
async fn contact_restore_404s_for_another_accounts_contact() {
    let (vault, alice_token, alice) = contacts_fixture_with_handles(&["+15550100"]).await;
    let alice_list: serde_json::Value =
        crate::test_support::get_json(&vault.state, "/v1/contacts", &alice_token).await;
    let alice_contact_id = alice_list["items"][0]["id"].as_i64().unwrap();
    let mut conn = vault.state.db.acquire().await.unwrap();
    sqlx::query("INSERT INTO trashed_contacts (account_id, contact_id) VALUES ($1, $2)")
        .bind(&alice.account_id)
        .bind(alice_contact_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    drop(conn);

    let bob = crate::test_support::register_via_api(&vault.state, "bob", "hunter2hunter2").await;

    let status = crate::test_support::post_status(
        &vault.state,
        &format!("/v1/contacts/{alice_contact_id}/restore"),
        &bob.token,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);

    let mut conn = vault.state.db.acquire().await.unwrap();
    assert_eq!(
        trashed_contact_row_count(&mut conn, &alice.account_id, alice_contact_id).await,
        1,
        "Bob's request must not restore Alice's contact"
    );
}

#[tokio::test]
async fn contact_trash_requires_auth() {
    let (vault, token, _account) = contacts_fixture_with_handles(&["+15550100"]).await;
    let list: serde_json::Value =
        crate::test_support::get_json(&vault.state, "/v1/contacts", &token).await;
    let id = list["items"][0]["id"].as_i64().unwrap();

    let status = crate::test_support::post_status(
        &vault.state,
        &format!("/v1/contacts/{id}/trash"),
        "not-a-token",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn contact_restore_requires_auth() {
    let (vault, token, _account) = contacts_fixture_with_handles(&["+15550100"]).await;
    let list: serde_json::Value =
        crate::test_support::get_json(&vault.state, "/v1/contacts", &token).await;
    let id = list["items"][0]["id"].as_i64().unwrap();

    let status = crate::test_support::post_status(
        &vault.state,
        &format!("/v1/contacts/{id}/restore"),
        "not-a-token",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
}

/// The conversations list refuses an offset past `MAX_LIST_OFFSET`
/// (conversations_api.rs). The contacts list shares `page_params` and must
/// answer the same way over HTTP.
#[tokio::test]
async fn the_contacts_route_refuses_an_offset_past_the_ceiling() {
    let vault = crate::test_support::test_vault().await;
    let user = crate::test_support::register_via_api(&vault.state, "alice", "hunter2hunter2").await;

    let (status, text) =
        crate::test_support::get_raw(&vault.state, "/v1/contacts?offset=50001", &user.token).await;
    assert_eq!(
        status,
        axum::http::StatusCode::UNPROCESSABLE_ENTITY,
        "{text}"
    );
    let body: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(body["errors"].is_array(), "{body}");

    let ok =
        crate::test_support::get_status(&vault.state, "/v1/contacts?offset=50000", &user.token)
            .await;
    assert_eq!(
        ok,
        axum::http::StatusCode::OK,
        "the ceiling itself is allowed"
    );
}
