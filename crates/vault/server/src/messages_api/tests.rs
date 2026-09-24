use axum::http::StatusCode;

use crate::problem::ProblemType;
use crate::test_support::{
    RegisteredAccount, SeedConversation, SeedMessage, TestVault, expect_problem, get_json, get_raw,
    get_status, register_via_api, seed_conversation, vault_with_account,
};

/// Two conversations for alice (a direct thread and a group), and one for bob
/// that must never appear in alice's results.
async fn seeded() -> (TestVault, RegisteredAccount, i64, i64) {
    let (vault, alice) = vault_with_account().await;
    let bob = register_via_api(&vault.state, "bob", "hunter2hunter2").await;
    let direct = seed_conversation(
        &vault.state,
        &SeedConversation {
            account_id: alice.account_id,
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
            account_id: alice.account_id,
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
            account_id: bob.account_id,
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
    // `docs/architecture/http-api.md`: a list is {items, total, limit, offset} and nothing else.
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
async fn a_word_the_messages_list_does_not_have_is_a_422_with_a_sentence() {
    let (vault, alice, _direct, _group) = seeded().await;
    let (status, text) = get_raw(
        &vault.state,
        "/v1/messages?q=conversations%3A0",
        &alice.token,
    )
    .await;
    let problem = expect_problem(status, &text, ProblemType::SearchQueryInvalid);
    assert!(
        problem.detail.as_deref().unwrap().contains("conversations"),
        "{text}"
    );
}

/// One IR message line for [`import_reactions_and_flags`].
fn ir_message(
    guid: &str,
    ms: i64,
    text: &str,
    is_sticker: bool,
    imessage: serde_json::Value,
) -> String {
    serde_json::json!({
        "guid": guid,
        "timestamp_unix_ms": ms,
        "direction": "incoming",
        "service": "imessage",
        "message_kind": "imessage",
        "sender_handle": "+15555550123",
        "sender_display_name": null,
        "subject": null,
        "text": text,
        "attachments": [{
            "path": format!("attachments/{guid}.png"),
            "original_name": format!("{guid}.png"),
            "mime_type": "image/png",
            "digest_sha256": null,
            "is_sticker": is_sticker,
            "transcription": null,
            "sticker_effect": null,
            "size_bytes": 12,
            "missing_reason": "not_found"
        }],
        "imessage": imessage,
        "source": null
    })
    .to_string()
}

/// Import, through the whole pipeline, three messages into `account_id`: a
/// reply carrying a sticker and a tapback array of two, an announcement
/// carrying a single tapback object, and a plain message with none of these.
async fn import_reactions_and_flags(vault: &TestVault, account_id: i64) {
    let header = serde_json::json!({
        "schema_version": 4,
        "export": {"source": "imessage", "tool": "test", "tool_version": "0",
                   "owner_handle": null, "owner_display_name": null},
        "conversation": {
            "chat_identifier": "chat-reactions",
            "conversation_type": "group",
            "group_title": "Reactions",
            "participants": [
                {"handle": "+15555550123", "display_name": null},
                {"handle": "+15555550999", "display_name": null},
                {"handle": "+15555550888", "display_name": null}
            ],
            "stats": {"message_count": 3, "attachment_count": 3,
                      "first_timestamp_unix_ms": 1426183462000_i64,
                      "last_timestamp_unix_ms": 1426183464000_i64}
        }
    });
    let reply = ir_message(
        "g-reply",
        1_426_183_462_000,
        "a reply",
        true,
        serde_json::json!({
            "is_reply": true,
            "is_deleted": false,
            "tapbacks": [
                {"kind": "liked", "emoji": null, "part_index": 0,
                 "is_from_me": false, "sender": "+15555550999"},
                {"kind": "emoji", "emoji": "🎉", "part_index": 1,
                 "is_from_me": false, "sender": "+15555550888"}
            ]
        }),
    );
    let announcement = ir_message(
        "g-announce",
        1_426_183_463_000,
        "an announcement",
        false,
        serde_json::json!({
            "is_reply": false,
            "is_deleted": false,
            "announcement": "named the conversation Reactions",
            "tapbacks": {"kind": "loved", "emoji": null, "part_index": 2,
                         "is_from_me": false, "sender": "+15555550999"}
        }),
    );
    let plain = ir_message(
        "g-plain",
        1_426_183_464_000,
        "a plain message",
        false,
        serde_json::Value::Null,
    );
    let dir = vault.dir().join("reactions");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("reactions.jsonl");
    std::fs::write(
        &path,
        format!("{header}\n{reply}\n{announcement}\n{plain}\n"),
    )
    .unwrap();
    let assets = dir.join("assets");
    let mut conn = vault.conn().await;
    let stats = crate::imports_api::import_jsonl_files_on_conn(
        &mut conn,
        &[path],
        &crate::imports_api::ImportOptions::fixed(crate::imports_api::FixedImportArgs {
            assets_dir: &assets,
            asset_root: &dir,
            contacts: None,
            overwrite_contacts: false,
            mode: crate::imports_api::ImportMode::Append,
            source: "imessage",
            account_id,
            fill_content_keys: false,
            import_id: None,
        }),
        crate::imports_api::ImportSchemaMode::Ensure,
    )
    .await
    .unwrap();
    assert_eq!(stats.messages, 3);
    assert_eq!(stats.tapbacks, 3);
}

/// Tapbacks and the reply, announcement and sticker flags survive the trip
/// from an import to the messages route, both when set and when not. A
/// single tapback object is read the same as an array of one.
#[tokio::test]
async fn reactions_and_message_flags_are_read_back_as_imported() {
    let (vault, alice) = vault_with_account().await;
    import_reactions_and_flags(&vault, alice.account_id).await;

    let page: serde_json::Value = get_json(&vault.state, "/v1/messages", &alice.token).await;
    let by_guid = |guid: &str| {
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["guid"] == guid)
            .unwrap_or_else(|| panic!("no message {guid:?} in {page}"))
            .clone()
    };
    let reply = by_guid("g-reply");
    let announcement = by_guid("g-announce");
    let plain = by_guid("g-plain");

    assert_eq!(
        reply["tapbacks"],
        serde_json::json!([
            {"part_index": 0, "kind": "liked",
             "is_from_me": false, "sender": "+15555550999"},
            {"part_index": 1, "kind": "emoji", "emoji": "🎉",
             "is_from_me": false, "sender": "+15555550888"}
        ])
    );
    assert_eq!(
        announcement["tapbacks"],
        serde_json::json!([
            {"part_index": 2, "kind": "loved",
             "is_from_me": false, "sender": "+15555550999"}
        ])
    );
    assert_eq!(plain["tapbacks"], serde_json::json!([]));

    let flags = |m: &serde_json::Value| {
        (
            m["is_reply"].as_bool().unwrap(),
            m["is_announcement"].as_bool().unwrap(),
            // `is_sticker` is left out of the JSON when false.
            m["attachments"][0]["is_sticker"] == true,
        )
    };
    assert_eq!(flags(&reply), (true, false, true), "{reply}");
    assert_eq!(flags(&announcement), (false, true, false), "{announcement}");
    assert_eq!(flags(&plain), (false, false, false), "{plain}");
}

#[tokio::test]
async fn one_message_is_read_by_id_and_only_by_the_account_that_owns_it() {
    let (vault, alice, _direct, _group) = seeded().await;
    let bob = register_via_api(&vault.state, "carol", "hunter2hunter2").await;
    let page: serde_json::Value =
        get_json(&vault.state, "/v1/messages?q=dentist", &alice.token).await;
    let id = page["items"][0]["id"].as_i64().unwrap();
    let text = page["items"][0]["text"].as_str().unwrap().to_string();

    let message: serde_json::Value =
        get_json(&vault.state, &format!("/v1/messages/{id}"), &alice.token).await;
    assert_eq!(message["id"], serde_json::json!(id));
    assert_eq!(message["text"], serde_json::json!(text));

    assert_eq!(
        get_status(&vault.state, &format!("/v1/messages/{id}"), &bob.token).await,
        StatusCode::NOT_FOUND,
        "another account's message is absent, not forbidden"
    );
    assert_eq!(
        get_status(&vault.state, "/v1/messages/999999", &alice.token).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get_status(&vault.state, &format!("/v1/messages/{id}"), "not-a-token").await,
        StatusCode::UNAUTHORIZED
    );
    // An id that is not a number is a problem document like every other
    // failure, not Axum's plain-text rejection.
    let (status, text) = get_raw(&vault.state, "/v1/messages/abc", &alice.token).await;
    expect_problem(status, &text, ProblemType::ValidationFailed);
}

/// `date:today` means today on the account's clock, not UTC's. The account
/// is on Kiritimati, 14 hours ahead of UTC, so the local day starts at 10:00
/// UTC the day before. A message half an hour into the local day is today; one
/// half an hour before it is not, whatever day it is in UTC. A message stamped
/// now is today in every zone.
#[tokio::test]
async fn date_today_is_the_day_on_the_accounts_clock() {
    use chrono::TimeZone;

    let (vault, alice) = vault_with_account().await;
    let zone = chrono_tz::Pacific::Kiritimati;
    let _: serde_json::Value = crate::test_support::patch_json(
        &vault.state,
        &format!("/v1/accounts/{}", alice.account_id),
        &alice.token,
        serde_json::json!({ "time_zone": zone.name() }),
    )
    .await;

    let today = chrono::Utc::now().with_timezone(&zone).date_naive();
    let local = |day: chrono::NaiveDate, h: u32, m: u32| {
        zone.from_local_datetime(&day.and_hms_opt(h, m, 0).unwrap())
            .single()
            .unwrap()
            .with_timezone(&chrono::Utc)
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string()
    };
    let early_today = local(today, 0, 30);
    let late_yesterday = local(today.pred_opt().unwrap(), 23, 30);
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    seed_conversation(
        &vault.state,
        &SeedConversation {
            account_id: alice.account_id,
            handle: "+15555550100",
            conversation_type: "individual",
            group_title: None,
            source_file: "t.json",
            messages: &[
                SeedMessage {
                    source: "imessage",
                    timestamp: &late_yesterday,
                    is_from_me: false,
                    body: "late yesterday",
                },
                SeedMessage {
                    source: "imessage",
                    timestamp: &early_today,
                    is_from_me: false,
                    body: "early today",
                },
                SeedMessage {
                    source: "imessage",
                    timestamp: &now,
                    is_from_me: true,
                    body: "right now",
                },
            ],
        },
    )
    .await;

    let page: serde_json::Value =
        get_json(&vault.state, "/v1/messages?q=date%3Atoday", &alice.token).await;
    let mut bodies: Vec<&str> = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["text"].as_str().unwrap())
        .collect();
    bodies.sort_unstable();
    assert_eq!(bodies, ["early today", "right now"], "{page}");
}
