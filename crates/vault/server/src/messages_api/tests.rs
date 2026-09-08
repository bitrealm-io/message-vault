use axum::http::StatusCode;

use crate::test_support::{
    RegisteredAccount, SeedConversation, SeedMessage, TestVault, get_json, get_raw, get_status,
    register_via_api, seed_conversation, test_vault,
};

/// Two conversations for alice (a direct thread and a group), and one for bob
/// that must never appear in alice's results.
async fn seeded() -> (TestVault, RegisteredAccount, i64, i64) {
    let vault = test_vault().await;
    let alice = register_via_api(&vault.state, "alice", "hunter2hunter2").await;
    let bob = register_via_api(&vault.state, "bob", "hunter2hunter2").await;
    let direct = seed_conversation(
        &vault.state,
        &SeedConversation {
            account_id: &alice.account_id,
            handle: "+15555550100",
            conversation_type: "individual",
            group_title: None,
            source_file: "t.json",
            messages: &[
                SeedMessage {
                    source: "imessage",
                    timestamp: "2024-01-01T10:00:00Z",
                    is_from_me: false,
                    body: "dentist on tuesday",
                },
                SeedMessage {
                    source: "imessage",
                    timestamp: "2024-01-02T10:00:00Z",
                    is_from_me: true,
                    body: "see you there",
                },
            ],
        },
    )
    .await;
    let group = seed_conversation(
        &vault.state,
        &SeedConversation {
            account_id: &alice.account_id,
            handle: "chat100",
            conversation_type: "group",
            group_title: Some("Family"),
            source_file: "t.json",
            messages: &[SeedMessage {
                source: "imessage",
                timestamp: "2024-02-01T10:00:00Z",
                is_from_me: false,
                body: "the dentist called again",
            }],
        },
    )
    .await;
    seed_conversation(
        &vault.state,
        &SeedConversation {
            account_id: &bob.account_id,
            handle: "+15555550999",
            conversation_type: "individual",
            group_title: None,
            source_file: "t.json",
            messages: &[SeedMessage {
                source: "imessage",
                timestamp: "2024-03-01T10:00:00Z",
                is_from_me: false,
                body: "bob's dentist",
            }],
        },
    )
    .await;
    (vault, alice, direct, group)
}

#[tokio::test]
async fn the_messages_route_is_a_page_across_every_conversation() {
    let (vault, alice, _direct, _group) = seeded().await;
    let page: serde_json::Value = get_json(&vault.state, "/v1/messages", &alice.token).await;
    assert_eq!(page["total"], serde_json::json!(3), "{page}");
    assert_eq!(page["limit"], serde_json::json!(40));
    assert_eq!(page["offset"], serde_json::json!(0));
    assert_eq!(page["items"].as_array().unwrap().len(), 3);
    // `docs/agents/http-api-rules.md`: a list is {items, total, limit, offset} and nothing else.
    let keys: Vec<&str> = page
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, ["items", "limit", "offset", "total"]);
}

#[tokio::test]
async fn a_query_narrows_to_matching_messages_and_never_leaks_another_account() {
    let (vault, alice, _direct, _group) = seeded().await;
    let page: serde_json::Value =
        get_json(&vault.state, "/v1/messages?q=dentist", &alice.token).await;
    assert_eq!(page["total"], serde_json::json!(2), "{page}");
    let bodies: Vec<&str> = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["text"].as_str().unwrap())
        .collect();
    assert!(bodies.iter().all(|b| b.contains("dentist")), "{bodies:?}");
    assert!(
        !bodies.iter().any(|b| b.contains("bob")),
        "bob's message must not reach alice: {bodies:?}"
    );
}

#[tokio::test]
async fn in_narrows_a_find_to_one_conversation() {
    // The thread's find box composes `in:#id <term>`, so a find reaches every
    // message in that conversation and nothing outside it (#313).
    let (vault, alice, direct, group) = seeded().await;
    let page: serde_json::Value = get_json(
        &vault.state,
        &format!("/v1/messages?q=in%3A%23{direct}%20dentist"),
        &alice.token,
    )
    .await;
    assert_eq!(page["total"], serde_json::json!(1), "{page}");
    assert_eq!(page["items"][0]["text"], "dentist on tuesday");
    assert_eq!(
        page["items"][0]["conversation"]["id"],
        serde_json::json!(direct)
    );

    let page: serde_json::Value = get_json(
        &vault.state,
        &format!("/v1/messages?q=in%3A%23{group}"),
        &alice.token,
    )
    .await;
    assert_eq!(page["total"], serde_json::json!(1), "{page}");
    assert_eq!(page["items"][0]["text"], "the dentist called again");
}

#[tokio::test]
async fn the_route_pages_by_offset_and_reports_the_total() {
    let (vault, alice, _direct, _group) = seeded().await;
    let page: serde_json::Value =
        get_json(&vault.state, "/v1/messages?limit=2&offset=2", &alice.token).await;
    assert_eq!(page["total"], serde_json::json!(3));
    assert_eq!(page["limit"], serde_json::json!(2));
    assert_eq!(page["offset"], serde_json::json!(2));
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn a_word_the_messages_list_does_not_have_is_a_400_with_a_sentence() {
    let (vault, alice, _direct, _group) = seeded().await;
    let (status, text) = get_raw(
        &vault.state,
        "/v1/messages?q=conversations%3A0",
        &alice.token,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{text}");
    let body: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(
        body["detail"].as_str().unwrap().contains("conversations"),
        "{text}"
    );
}

#[tokio::test]
async fn the_route_refuses_an_offset_past_the_ceiling_and_requires_a_session() {
    let (vault, alice, _direct, _group) = seeded().await;
    assert_eq!(
        get_status(&vault.state, "/v1/messages?offset=50001", &alice.token).await,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        get_status(&vault.state, "/v1/messages?offset=50000", &alice.token).await,
        StatusCode::OK,
        "the ceiling itself is allowed"
    );
    assert_eq!(
        get_status(&vault.state, "/v1/messages", "not-a-token").await,
        StatusCode::UNAUTHORIZED
    );
}
