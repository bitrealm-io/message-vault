use axum::http::StatusCode;

use super::*;
use crate::test_support::{
    SeedConversation, SeedMessage, claim_as_owner, get_json, get_status, patch_status, post_status,
    post_status_logged_out, register_via_api, seed_conversation, test_fixture,
};

/// Turn public registration off, the way a real server ships.
async fn close_registration(state: &AppState) {
    let mut conn = state.db.acquire().await.unwrap();
    server_settings::set_public_registration(&mut conn, false)
        .await
        .unwrap();
}

#[tokio::test]
async fn an_unowned_server_reports_unclaimed() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();

    let body: Server = get_json(&state, "/v1/server", "").await;
    assert_eq!(body.state, ServerState::Unclaimed);
}

/// Unclaimed wins over the registration setting: a Message Crate with no owner has
/// one thing to offer, and joining it is not that thing.
#[tokio::test]
async fn public_registration_does_not_make_an_unowned_server_open() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();

    let body: Server = get_json(&state, "/v1/server", "").await;
    assert_eq!(
        body.state,
        ServerState::Unclaimed,
        "test fixtures open registration; being unclaimed still comes first"
    );
}

/// The Demo Account is reported for as long as it exists, whatever the state
/// is: before anyone claims the Message Crate, after, and no longer once the
/// owner has deleted it.
#[tokio::test]
async fn the_server_reports_the_demo_account_while_it_exists() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();

    let body: Server = get_json(&state, "/v1/server", "").await;
    assert!(!body.demo_account, "no Demo Account has been seeded");

    let demo = fixture.demo_account().await;
    let body: Server = get_json(&state, "/v1/server", "").await;
    assert_eq!(body.state, ServerState::Unclaimed);
    assert!(body.demo_account);

    let owner = claim_as_owner(&state, "keeper", "hunter2hunter2").await;
    let body: Server = get_json(&state, "/v1/server", "").await;
    assert!(body.demo_account, "claiming changes nothing about it");

    assert_eq!(
        crate::test_support::delete_status(&state, &format!("/v1/accounts/{demo}"), &owner.token)
            .await,
        StatusCode::NO_CONTENT
    );
    let body: Server = get_json(&state, "/v1/server", "").await;
    assert!(!body.demo_account);
}

#[tokio::test]
async fn a_claimed_server_is_closed_until_registration_is_opened() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    close_registration(&state).await;
    let _owner = claim_as_owner(&state, "keeper", "hunter2hunter2").await;

    let body: Server = get_json(&state, "/v1/server", "").await;
    assert_eq!(body.state, ServerState::Closed);
}

#[tokio::test]
async fn a_claimed_server_with_registration_on_is_open() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let _owner = claim_as_owner(&state, "keeper", "hunter2hunter2").await;

    let body: Server = get_json(&state, "/v1/server", "").await;
    assert_eq!(body.state, ServerState::Open);
}

/// The route reports this Message Crate's state to anyone, logged in or not. The
/// Create Owner screen has no credential to present.
#[tokio::test]
async fn the_state_route_needs_no_credential() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();

    assert_eq!(get_status(&state, "/v1/server", "").await, StatusCode::OK);
    assert_eq!(
        get_status(&state, "/v1/server", "not-a-real-token").await,
        StatusCode::OK,
        "a stale token must not stop the entry screen loading"
    );
}

/// The server says which code it runs and which schema it carries to anyone:
/// an app has to read both before anybody is logged in, and neither is secret.
#[tokio::test]
async fn the_state_route_carries_the_build_and_the_schema_fingerprint() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();

    let body: Server = get_json(&state, "/v1/server", "").await;

    assert_eq!(body.version, crate::BUILD);
    assert!(
        body.version.starts_with(env!("CARGO_PKG_VERSION")),
        "a Build starts with the Product Version, got {}",
        body.version
    );
    assert_eq!(
        body.schema_fingerprint,
        crate::db::schema::SCHEMA_FINGERPRINT
    );
}

#[tokio::test]
async fn claiming_an_unowned_server_creates_the_owner_and_signs_them_in() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();

    let server = crate::test_support::serve(&state).await;
    let response = reqwest::Client::new()
        .post(format!("{}/v1/server/claim", server.base()))
        .json(&serde_json::json!({ "username": "keeper", "password": "hunter2hunter2" }))
        .send()
        .await
        .unwrap();
    // A claim makes the owner's Session, so it is a creation that names it.
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(
        response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok()),
        Some("/v1/session")
    );
    let body: serde_json::Value = response.json().await.unwrap();

    assert_eq!(body["account_id"], account_profile::OWNER_ACCOUNT_ID);
    assert_eq!(body["username"], "keeper");

    // The token it hands back is the owner's session, usable at once.
    let token = body["token"].as_str().unwrap();
    let mut conn = state.db.acquire().await.unwrap();
    let auth = crate::server::resolve_auth_on_conn(&mut conn, token, None)
        .await
        .unwrap();
    assert!(auth.is_owner());
    drop(conn);

    let after: Server = get_json(&state, "/v1/server", "").await;
    assert_eq!(
        after.state,
        ServerState::Open,
        "test fixtures open registration"
    );
}

#[tokio::test]
async fn a_server_can_only_be_claimed_once() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let _owner = claim_as_owner(&state, "keeper", "hunter2hunter2").await;

    let status = post_status(
        &state,
        "/v1/server/claim",
        "",
        serde_json::json!({ "username": "usurper", "password": "hunter2hunter2" }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // And nothing was created for the second caller.
    let mut conn = state.db.acquire().await.unwrap();
    let taken: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts WHERE username = 'usurper'")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(taken, 0);
}

/// A new Message Crate already holds the Demo Account, so a claim under its
/// username answers `409 Conflict` as `username-taken`, and the reference
/// lists that type for the claim beside `state-conflict`.
#[tokio::test]
async fn a_claim_under_a_taken_username_answers_username_taken_as_the_reference_says() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    fixture.account("demo").await;

    let (status, text) = crate::test_support::post_logged_out(
        &state,
        "/v1/server/claim",
        serde_json::json!({ "username": "demo", "password": "hunter2hunter2" }),
    )
    .await;
    crate::test_support::expect_problem(status, &text, crate::problem::ProblemType::UsernameTaken);

    let doc: serde_json::Value =
        serde_json::from_str(&crate::openapi::dump_openapi_json()).unwrap();
    let listed = &doc["paths"]["/v1/server/claim"]["post"]["responses"]["409"]
        [crate::openapi::shared_parts::PROBLEM_TYPES];
    assert!(
        listed
            .as_array()
            .into_iter()
            .flatten()
            .any(|t| *t == crate::problem::ProblemType::UsernameTaken.url()),
        "the claim's 409 lists {listed}"
    );
}

/// Two claims at once: the second reads the Message Crate unclaimed while the
/// first is still writing its owner. Its deferred transaction then failed at
/// its insert and answered `500`; it must find the owner and answer `409`.
#[tokio::test]
async fn a_claim_that_loses_a_race_answers_conflict() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();

    let mut other_conn = state.db.acquire().await.unwrap();
    let mut other = crate::db::begin_write(&mut other_conn).await.unwrap();
    account_profile::insert_account_at(
        &mut other,
        account_profile::OWNER_ACCOUNT_ID,
        "keeper",
        None,
        None,
    )
    .await
    .unwrap();
    let status = crate::db::write_tx::commit_during(
        other,
        post_status(
            &state,
            "/v1/server/claim",
            "",
            serde_json::json!({ "username": "usurper", "password": "hunter2hunter2" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let mut conn = state.db.acquire().await.unwrap();
    let owner: String = sqlx::query_scalar("SELECT username FROM accounts WHERE id = $1")
        .bind(account_profile::OWNER_ACCOUNT_ID)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(owner, "keeper");
}

#[tokio::test]
async fn claiming_needs_a_password_of_one_character_or_more() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();

    let status = post_status(
        &state,
        "/v1/server/claim",
        "",
        serde_json::json!({ "username": "keeper", "password": "" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "the owner must have a password"
    );
    let after: Server = get_json(&state, "/v1/server", "").await;
    assert_eq!(after.state, ServerState::Unclaimed);

    let status = post_status(
        &state,
        "/v1/server/claim",
        "",
        serde_json::json!({ "username": "keeper", "password": "k" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "one character is enough");
}

/// Claiming takes no credential and makes the most powerful one the server
/// has, so it is rate limited, and once for the whole server: a count per
/// username would let a script trying a new name each time straight through.
#[tokio::test]
async fn claiming_is_rate_limited_across_the_server() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    // An empty password is refused after the limiter has counted the
    // attempt, so every try counts and none claims this Message Crate.
    for attempt in 0..crate::credentials::AUTH_RATE_MAX {
        let status = post_status_logged_out(
            &state,
            "/v1/server/claim",
            serde_json::json!({ "username": format!("keeper{attempt}"), "password": "" }),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "attempt {attempt} inside the limit"
        );
    }
    let (status, text) = crate::test_support::post_logged_out(
        &state,
        "/v1/server/claim",
        serde_json::json!({ "username": "keeper", "password": "hunter2hunter2" }),
    )
    .await;
    let problem = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::RateLimited,
    );
    assert!(problem.retry_after.is_some(), "{text}");
    let after: Server = get_json(&state, "/v1/server", "").await;
    assert_eq!(after.state, ServerState::Unclaimed);
}

#[tokio::test]
async fn registration_is_refused_while_the_server_is_closed() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    close_registration(&state).await;

    let status = post_status_logged_out(
        &state,
        "/v1/accounts",
        serde_json::json!({ "username": "stranger", "password": "hunter2hunter2" }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

/// The owner opens the door, and the same request that was refused succeeds.
#[tokio::test]
async fn the_owner_can_open_and_close_registration() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    close_registration(&state).await;
    let owner = claim_as_owner(&state, "keeper", "hunter2hunter2").await;

    let settings: ServerSettings = get_json(&state, "/v1/server/settings", &owner.token).await;
    assert!(!settings.public_registration);

    assert_eq!(
        post_status_logged_out(
            &state,
            "/v1/accounts",
            serde_json::json!({ "username": "stranger", "password": "hunter2hunter2" }),
        )
        .await,
        StatusCode::FORBIDDEN
    );

    let opened: ServerSettings = crate::test_support::patch_json(
        &state,
        "/v1/server/settings",
        &owner.token,
        serde_json::json!({ "public_registration": true }),
    )
    .await;
    assert!(opened.public_registration);

    let joined = register_via_api(&state, "stranger", "hunter2hunter2").await;
    assert_eq!(joined.username, "stranger");

    let body: Server = get_json(&state, "/v1/server", "").await;
    assert_eq!(body.state, ServerState::Open);
}

#[tokio::test]
async fn only_the_owner_reaches_the_server_settings() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let _owner = claim_as_owner(&state, "keeper", "hunter2hunter2").await;
    let ordinary = register_via_api(&state, "bob", "hunter2hunter2").await;

    assert_eq!(
        get_status(&state, "/v1/server/settings", &ordinary.token).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        patch_status(
            &state,
            "/v1/server/settings",
            &ordinary.token,
            serde_json::json!({ "public_registration": false }),
        )
        .await,
        StatusCode::FORBIDDEN
    );
}

/// 512 MiB, the limit a Message Crate nobody has configured holds an attachment to.
const DEFAULT_LIMIT: u64 = 512 * 1024 * 1024;

/// Nothing seeds the attachment size limit: a Message Crate whose owner never
/// set it reads as 512 MiB, to the owner and to a client with no credential.
#[tokio::test]
async fn the_attachment_size_limit_reads_as_512_mib_until_the_owner_sets_it() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let owner = claim_as_owner(&state, "keeper", "hunter2hunter2").await;

    let settings: serde_json::Value = get_json(&state, "/v1/server/settings", &owner.token).await;
    assert_eq!(settings["asset_max_bytes"], DEFAULT_LIMIT);

    let server: serde_json::Value = get_json(&state, "/v1/server", "").await;
    assert_eq!(server["asset_max_bytes"], DEFAULT_LIMIT);
}

/// The owner changes the limit, and `GET /v1/server` reports the new number
/// to a client that holds no credential: the desktop app reads it there
/// before Staging. The registration setting the request did not name is
/// left alone.
#[tokio::test]
async fn the_owner_changes_the_attachment_size_limit_and_the_server_reports_it() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let owner = claim_as_owner(&state, "keeper", "hunter2hunter2").await;
    let limit: u64 = 100 * 1024 * 1024;

    let changed: serde_json::Value = crate::test_support::patch_json(
        &state,
        "/v1/server/settings",
        &owner.token,
        serde_json::json!({ "asset_max_bytes": limit }),
    )
    .await;
    assert_eq!(changed["asset_max_bytes"], limit);
    assert_eq!(
        changed["public_registration"], true,
        "test fixtures open registration, and the request did not name it"
    );

    let settings: serde_json::Value = get_json(&state, "/v1/server/settings", &owner.token).await;
    assert_eq!(settings["asset_max_bytes"], limit);
    let server: serde_json::Value = get_json(&state, "/v1/server", "").await;
    assert_eq!(server["asset_max_bytes"], limit);

    // Changing registration afterwards keeps the limit.
    let closed: serde_json::Value = crate::test_support::patch_json(
        &state,
        "/v1/server/settings",
        &owner.token,
        serde_json::json!({ "public_registration": false }),
    )
    .await;
    assert_eq!(closed["asset_max_bytes"], limit);
}

/// An account that is not the owner is refused, and the limit stays put.
#[tokio::test]
async fn an_ordinary_account_cannot_change_the_attachment_size_limit() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let _owner = claim_as_owner(&state, "keeper", "hunter2hunter2").await;
    let ordinary = register_via_api(&state, "bob", "hunter2hunter2").await;

    assert_eq!(
        patch_status(
            &state,
            "/v1/server/settings",
            &ordinary.token,
            serde_json::json!({ "asset_max_bytes": 100 * 1024 * 1024 }),
        )
        .await,
        StatusCode::FORBIDDEN
    );
    let server: serde_json::Value = get_json(&state, "/v1/server", "").await;
    assert_eq!(server["asset_max_bytes"], DEFAULT_LIMIT);
}

/// A limit of zero is refused, because no attachment could be uploaded under
/// it, and so is one the database cannot hold. Nothing is written.
#[tokio::test]
async fn a_limit_of_zero_or_past_what_the_server_can_store_is_refused() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let owner = claim_as_owner(&state, "keeper", "hunter2hunter2").await;

    for refused in [0, i64::MAX as u64 + 1] {
        let (status, text) = crate::test_support::patch_raw(
            &state,
            "/v1/server/settings",
            &owner.token,
            serde_json::json!({ "asset_max_bytes": refused }),
        )
        .await;
        let problem = crate::test_support::expect_problem(
            status,
            &text,
            crate::problem::ProblemType::ValidationFailed,
        );
        assert!(
            problem.errors.unwrap()[0].contains("asset_max_bytes"),
            "{text}"
        );
    }
    let server: serde_json::Value = get_json(&state, "/v1/server", "").await;
    assert_eq!(
        server["asset_max_bytes"], DEFAULT_LIMIT,
        "a refused change writes nothing"
    );
}

/// The limit is whatever the owner sets. One below the part size in the
/// config file is accepted like any other: the server sends smaller parts.
#[tokio::test]
async fn the_owner_sets_a_limit_below_the_configured_part_size() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let owner = claim_as_owner(&state, "keeper", "hunter2hunter2").await;
    let below = state.asset_part_size as u64 - 1;

    let accepted: serde_json::Value = crate::test_support::patch_json(
        &state,
        "/v1/server/settings",
        &owner.token,
        serde_json::json!({ "asset_max_bytes": below }),
    )
    .await;
    assert_eq!(accepted["asset_max_bytes"], below);
    let server: serde_json::Value = get_json(&state, "/v1/server", "").await;
    assert_eq!(server["asset_max_bytes"], below);
}

/// Claiming this Message Crate puts a row at the owner id and nowhere else.
#[tokio::test]
async fn claiming_the_server_creates_exactly_one_owner() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();

    let mut conn = state.db.acquire().await.unwrap();
    assert!(!account_profile::is_claimed(&mut conn).await.unwrap());
    drop(conn);

    let owner = claim_as_owner(&state, "keeper", "hunter2hunter2").await;
    assert_eq!(owner.account_id, account_profile::OWNER_ACCOUNT_ID);

    let mut conn = state.db.acquire().await.unwrap();
    assert!(account_profile::is_claimed(&mut conn).await.unwrap());
    let owners: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts WHERE id = $1")
        .bind(account_profile::OWNER_ACCOUNT_ID)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(owners, 1);
}

/// The owner holds no messages, so profile setup would ask for a name
/// shown against messages, a zone to read them in, and handles that mark one
/// as theirs: three questions with no answer. The owner is never sent there.
#[tokio::test]
async fn the_owner_owes_no_profile_setup() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let owner = claim_as_owner(&state, "keeper", "hunter2hunter2").await;

    let mut conn = state.db.acquire().await.unwrap();
    let auth = account_profile::load_account_auth(&mut conn, owner.account_id)
        .await
        .unwrap()
        .unwrap();
    assert!(!auth.must_set_up_profile);
}

// ---------------------------------------------------------------------------
// What the database holds
// ---------------------------------------------------------------------------

/// The server's totals sum every account, and the answer is counts and byte
/// totals and nothing that names a person or a conversation. The database
/// figures are measured, so they are only checked against the size of a
/// page; the split of message storage across accounts is checked exactly,
/// because it is arithmetic over the measured total.
#[tokio::test]
async fn the_owner_reads_the_server_totals_summed_over_every_account() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let owner = claim_as_owner(&state, "keeper", "hunter2hunter2").await;
    let alice = register_via_api(&state, "alice", "hunter2hunter2").await;
    let bob = register_via_api(&state, "bob", "hunter2hunter2").await;

    let empty: ServerStorage = get_json(&state, "/v1/server/storage", &owner.token).await;
    assert_eq!(
        (
            empty.message_count,
            empty.conversation_count,
            empty.contact_count,
            empty.attachment_count,
            empty.total_bytes
        ),
        (0, 0, 0, 0, 0)
    );
    // An empty database still has pages, and it lists every account, the
    // owner first, each holding nothing.
    assert!(
        empty.database_bytes > 0,
        "database_bytes {}",
        empty.database_bytes
    );
    assert_eq!(
        empty
            .accounts
            .iter()
            .map(|a| {
                (
                    a.account_id,
                    a.username.as_str(),
                    a.message_count,
                    a.text_bytes,
                    a.estimated_message_bytes,
                )
            })
            .collect::<Vec<_>>(),
        vec![
            (owner.account_id, "keeper", 0, 0, 0),
            (alice.account_id, "alice", 0, 0, 0),
            (bob.account_id, "bob", 0, 0, 0)
        ]
    );

    for (account_id, handle, bodies) in [
        (alice.account_id, "+15555550100", &["hi", "thérè"][..]),
        (bob.account_id, "+15555550135", &["yo"][..]),
    ] {
        let messages: Vec<SeedMessage> = bodies
            .iter()
            .map(|body| SeedMessage {
                source: "imessage",
                timestamp: "2020-01-01T00:00:00Z",
                is_from_me: true,
                body,
            })
            .collect();
        seed_conversation(
            &state,
            &SeedConversation {
                account_id,
                handle,
                conversation_type: "individual",
                group_title: None,
                source_file: "seed.jsonl",
                messages: &messages,
            },
        )
        .await;
    }
    let mut conn = fixture.conn().await;
    for (account_id, size) in [(alice.account_id, 3000_i64), (bob.account_id, 1000)] {
        let message_id: i64 =
            sqlx::query_scalar("SELECT MIN(id) FROM messages WHERE account_id = $1")
                .bind(account_id)
                .fetch_one(&mut *conn)
                .await
                .unwrap();
        sqlx::query(
            "INSERT INTO attachments (message_id, original_name, mime_type, size_bytes)
             VALUES ($1, 'file.bin', 'application/octet-stream', $2)",
        )
        .bind(message_id)
        .bind(size)
        .execute(&mut *conn)
        .await
        .unwrap();
    }
    for (account_id, name) in [
        (alice.account_id, "Ada"),
        (alice.account_id, "Pat"),
        (bob.account_id, "Sam"),
    ] {
        sqlx::query("INSERT INTO contacts (account_id, preferred_name) VALUES ($1, $2)")
            .bind(account_id)
            .bind(name)
            .execute(&mut *conn)
            .await
            .unwrap();
    }
    drop(conn);

    let totals: ServerStorage = get_json(&state, "/v1/server/storage", &owner.token).await;
    assert_eq!(
        (
            totals.message_count,
            totals.conversation_count,
            totals.contact_count,
            totals.attachment_count,
            totals.total_bytes
        ),
        (3, 2, 3, 2, 4000)
    );
    assert!(
        totals.database_bytes > 0,
        "database_bytes {}",
        totals.database_bytes
    );
    // A table that holds a row takes at least one page, and so does the
    // search index over it. 4096 bytes is SQLite's default page size.
    assert!(
        totals.messages_bytes >= 4096,
        "messages_bytes {}",
        totals.messages_bytes
    );
    assert!(totals.fts_bytes >= 4096, "fts_bytes {}", totals.fts_bytes);
    assert!(
        totals.database_bytes >= totals.messages_bytes,
        "messages {} cannot exceed the database {}",
        totals.messages_bytes,
        totals.database_bytes
    );

    // Alice wrote "hi" and "thérè", which is 9 bytes: text is counted in
    // bytes, not characters, and each accented letter is two. Bob wrote "yo"
    // (2 bytes), and the owner wrote nothing. The estimates split
    // messages_bytes by those shares and add up to it exactly, the last
    // account with text taking the rounding.
    let by_account: Vec<_> = totals
        .accounts
        .iter()
        .map(|a| {
            (
                a.account_id,
                a.username.as_str(),
                a.message_count,
                a.text_bytes,
            )
        })
        .collect();
    assert_eq!(
        by_account,
        vec![
            (owner.account_id, "keeper", 0, 0),
            (alice.account_id, "alice", 2, 9),
            (bob.account_id, "bob", 1, 2)
        ]
    );
    let estimates: Vec<i64> = totals
        .accounts
        .iter()
        .map(|a| a.estimated_message_bytes)
        .collect();
    let alice_share = totals.messages_bytes * 9 / 11;
    assert_eq!(
        estimates,
        vec![0, alice_share, totals.messages_bytes - alice_share]
    );
    assert_eq!(estimates.iter().sum::<i64>(), totals.messages_bytes);
}

/// An account holds only its own data, so the server's totals are the owner's
/// alone; a session that is not the owner's is refused, and no session is
/// unauthorized.
#[tokio::test]
async fn only_the_owner_reaches_the_server_totals() {
    let fixture = test_fixture().await;
    let state = fixture.state.clone();
    let _owner = claim_as_owner(&state, "keeper", "hunter2hunter2").await;
    let ordinary = register_via_api(&state, "bob", "hunter2hunter2").await;

    assert_eq!(
        get_status(&state, "/v1/server/storage", &ordinary.token).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        get_status(&state, "/v1/server/storage", "").await,
        StatusCode::UNAUTHORIZED
    );
}

/// Writes a few conversations in place of the built-in data set.
fn tiny_bundle(_size: demo_seed::DemoSize, bundle: &std::path::Path) -> anyhow::Result<()> {
    crate::reset_demo::tests::write_tiny_reset_bundle(bundle);
    Ok(())
}

/// [`tiny_bundle`], slowly, so a test can act while the build is running.
fn slow_tiny_bundle(size: demo_seed::DemoSize, bundle: &std::path::Path) -> anyhow::Result<()> {
    std::thread::sleep(std::time::Duration::from_millis(400));
    tiny_bundle(size, bundle)
}

fn no_bundle(_size: demo_seed::DemoSize, _bundle: &std::path::Path) -> anyhow::Result<()> {
    anyhow::bail!("the generator has nothing to write")
}

/// Ask for a build of the medium set and return the answer.
async fn start_demo_build(state: &AppState, token: &str) -> (StatusCode, String) {
    crate::test_support::put_raw(
        state,
        "/v1/server/demo-account",
        token,
        "application/json",
        r#"{"size":"medium"}"#,
    )
    .await
}

/// Read the Demo Account until its build has ended.
async fn demo_account_after_build(state: &AppState, token: &str) -> DemoAccount {
    for _ in 0..400 {
        let demo: DemoAccount = get_json(state, "/v1/server/demo-account", token).await;
        if demo.status != DemoAccountStatus::Building {
            return demo;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("the Demo Account build did not end");
}

/// The Owner Home action: on a claimed Message Crate with no Demo Account,
/// the owner adds one, the build runs after the answer, and it changes
/// nothing in any other account (#971).
#[tokio::test(flavor = "multi_thread")]
async fn the_owner_adds_the_demo_account_and_no_other_account_changes() {
    let fixture = test_fixture().await;
    let mut state = fixture.state.clone();
    state.demo_bundle_generator = tiny_bundle;
    let owner = claim_as_owner(&state, "keeper", "hunter2hunter2").await;
    let other = fixture.account("someone").await;
    crate::test_support::seed_one_message(&state, other).await;

    let demo: DemoAccount = get_json(&state, "/v1/server/demo-account", &owner.token).await;
    assert_eq!(demo.status, DemoAccountStatus::Absent);

    let (status, body) = start_demo_build(&state, &owner.token).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let started: DemoAccount = serde_json::from_str(&body).unwrap();
    assert_eq!(started.status, DemoAccountStatus::Building);
    assert_eq!(started.size, Some(DemoDataSize::Medium));

    let demo = demo_account_after_build(&state, &owner.token).await;
    assert_eq!(demo.status, DemoAccountStatus::Ready, "{:?}", demo.error);
    let info: Server = get_json(&state, "/v1/server", "").await;
    assert!(info.demo_account);
    assert_ne!(
        info.state,
        ServerState::Unclaimed,
        "the owner is still the owner"
    );

    let mut conn = fixture.conn().await;
    let count = |account: i64| {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM messages WHERE account_id = $1")
            .bind(account)
    };
    assert_eq!(count(other).fetch_one(&mut *conn).await.unwrap(), 1);
    assert!(
        count(account_profile::DEMO_ACCOUNT_ID)
            .fetch_one(&mut *conn)
            .await
            .unwrap()
            >= 1
    );
}

/// `demo` belongs to the Demo Account even while it does not exist. Once the
/// owner has deleted it, neither the owner nor a stranger may give the name
/// to another account, so adding the Demo Account again still succeeds
/// (#1226).
#[tokio::test(flavor = "multi_thread")]
async fn the_demo_username_stays_reserved_after_the_demo_account_is_deleted() {
    let fixture = test_fixture().await;
    let mut state = fixture.state.clone();
    state.demo_bundle_generator = tiny_bundle;
    let demo = fixture.demo_account().await;
    let owner = claim_as_owner(&state, "keeper", "hunter2hunter2").await;
    assert_eq!(
        crate::test_support::delete_status(&state, &format!("/v1/accounts/{demo}"), &owner.token)
            .await,
        StatusCode::NO_CONTENT
    );

    let body =
        |username: &str| serde_json::json!({ "username": username, "password": "hunter2hunter2" });
    assert_eq!(
        post_status(&state, "/v1/accounts", &owner.token, body("demo")).await,
        StatusCode::CONFLICT,
        "the owner may not create an account named demo"
    );
    let (status, text) =
        crate::test_support::post_logged_out(&state, "/v1/accounts", body("Demo")).await;
    crate::test_support::expect_problem(status, &text, crate::problem::ProblemType::UsernameTaken);

    let (status, body) = start_demo_build(&state, &owner.token).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let demo = demo_account_after_build(&state, &owner.token).await;
    assert_eq!(demo.status, DemoAccountStatus::Ready, "{:?}", demo.error);
}

/// While one build runs, a second is refused and so is deleting the Demo
/// Account: either would pull the account out from under the import.
#[tokio::test(flavor = "multi_thread")]
async fn a_running_demo_build_refuses_a_second_build_and_a_delete() {
    let fixture = test_fixture().await;
    let mut state = fixture.state.clone();
    state.demo_bundle_generator = slow_tiny_bundle;
    let owner = claim_as_owner(&state, "keeper", "hunter2hunter2").await;

    let (status, body) = start_demo_build(&state, &owner.token).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");

    let (status, body) = start_demo_build(&state, &owner.token).await;
    crate::test_support::expect_problem(status, &body, crate::problem::ProblemType::StateConflict);
    let demo_path = format!("/v1/accounts/{}", account_profile::DEMO_ACCOUNT_ID);
    let (status, body) = crate::test_support::delete_raw(&state, &demo_path, &owner.token).await;
    crate::test_support::expect_problem(status, &body, crate::problem::ProblemType::StateConflict);

    let demo = demo_account_after_build(&state, &owner.token).await;
    assert_eq!(demo.status, DemoAccountStatus::Ready, "{:?}", demo.error);
    assert_eq!(
        crate::test_support::delete_status(&state, &demo_path, &owner.token).await,
        StatusCode::NO_CONTENT,
        "once the build has ended the owner deletes it as before"
    );
    let demo: DemoAccount = get_json(&state, "/v1/server/demo-account", &owner.token).await;
    assert_eq!(demo.status, DemoAccountStatus::Absent);
}

/// A build that fails says why and leaves no Demo Account, and the next
/// build may start.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_demo_build_reports_why_and_leaves_no_account() {
    let fixture = test_fixture().await;
    let mut state = fixture.state.clone();
    state.demo_bundle_generator = no_bundle;
    let owner = claim_as_owner(&state, "keeper", "hunter2hunter2").await;

    let (status, body) = start_demo_build(&state, &owner.token).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let demo = demo_account_after_build(&state, &owner.token).await;
    assert_eq!(demo.status, DemoAccountStatus::Failed);
    assert!(
        demo.error
            .as_deref()
            .is_some_and(|error| error.contains("the generator has nothing to write")),
        "{:?}",
        demo.error
    );
    let info: Server = get_json(&state, "/v1/server", "").await;
    assert!(!info.demo_account);

    state.demo_bundle_generator = tiny_bundle;
    let (status, body) = start_demo_build(&state, &owner.token).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let demo = demo_account_after_build(&state, &owner.token).await;
    assert_eq!(demo.status, DemoAccountStatus::Ready, "{:?}", demo.error);
}

/// Whether the database holds the record of a Demo Account build that has
/// not finished.
async fn demo_build_is_unfinished(fixture: &crate::test_support::TestFixture) -> bool {
    let mut conn = fixture.conn().await;
    crate::db::demo_account_build::is_unfinished(&mut conn)
        .await
        .unwrap()
}

/// Whether the Demo Account's row is in the database.
async fn demo_account_exists(fixture: &crate::test_support::TestFixture) -> bool {
    let mut conn = fixture.conn().await;
    account_profile::username_for_account(&mut conn, account_profile::DEMO_ACCOUNT_ID)
        .await
        .unwrap()
        .is_some()
}

/// The server stopped during a build from Owner Home: the next start finds
/// the build's record beside the Demo Account it left, removes the account,
/// and reports the build as failed, so a part-built Demo Account is never
/// `ready`. The next build starts as usual (#1215).
#[tokio::test(flavor = "multi_thread")]
async fn a_demo_build_the_server_stopped_is_removed_and_failed_on_the_next_start() {
    let fixture = test_fixture().await;
    let mut state = fixture.state.clone();
    state.demo_bundle_generator = tiny_bundle;
    fixture.demo_account().await;
    {
        let mut conn = fixture.conn().await;
        crate::db::demo_account_build::begin(&mut conn)
            .await
            .unwrap();
    }

    crate::server_api::recover_stopped_demo_build(&state)
        .await
        .expect("the next start removes the stopped build");

    let owner = claim_as_owner(&state, "keeper", "hunter2hunter2").await;
    let demo: DemoAccount = get_json(&state, "/v1/server/demo-account", &owner.token).await;
    assert_eq!(demo.status, DemoAccountStatus::Failed);
    assert!(
        demo.error
            .as_deref()
            .is_some_and(|error| error.contains("stopped")),
        "{:?}",
        demo.error
    );
    let info: Server = get_json(&state, "/v1/server", "").await;
    assert!(!info.demo_account);
    assert!(!demo_build_is_unfinished(&fixture).await);

    let (status, body) = start_demo_build(&state, &owner.token).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let demo = demo_account_after_build(&state, &owner.token).await;
    assert_eq!(demo.status, DemoAccountStatus::Ready, "{:?}", demo.error);
}

/// Stopping the server waits for a running build to stop and remove the
/// Demo Account it was replacing. The build's record stays, so the next
/// start reports the build as failed (#1215).
#[tokio::test(flavor = "multi_thread")]
async fn stopping_the_server_during_a_demo_build_leaves_no_demo_account() {
    let fixture = test_fixture().await;
    let mut state = fixture.state.clone();
    state.demo_bundle_generator = slow_tiny_bundle;
    fixture.demo_account().await;
    let owner = claim_as_owner(&state, "keeper", "hunter2hunter2").await;

    let (status, body) = start_demo_build(&state, &owner.token).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let mut waited = 0;
    while !demo_build_is_unfinished(&fixture).await {
        waited += 1;
        assert!(waited < 200, "the build never wrote its record");
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }

    state.demo_build.stop().await;

    assert!(!demo_account_exists(&fixture).await);
    assert!(demo_build_is_unfinished(&fixture).await);

    // The next start: a new server over the same database.
    let mut next = state.clone();
    next.demo_build = DemoBuild::default();
    crate::server_api::recover_stopped_demo_build(&next)
        .await
        .expect("the next start removes the stopped build");
    let demo: DemoAccount = get_json(&next, "/v1/server/demo-account", &owner.token).await;
    assert_eq!(demo.status, DemoAccountStatus::Failed);
    assert!(!demo_build_is_unfinished(&fixture).await);
}

/// A build task that panics ends the build as failed, and the next build
/// starts. Otherwise the Demo Account would stay `building`, and every build
/// and every delete of it would answer `409 Conflict` until a restart
/// (#1215).
#[tokio::test(flavor = "multi_thread")]
async fn a_demo_build_that_panics_fails_and_the_next_build_starts() {
    let fixture = test_fixture().await;
    let mut state = fixture.state.clone();
    state.demo_bundle_generator = tiny_bundle;
    let owner = claim_as_owner(&state, "keeper", "hunter2hunter2").await;

    assert!(state.demo_build.start(DemoDataSize::Medium));
    state
        .demo_build
        .run(state.cfg.clone(), state.db.clone(), async {
            panic!("the build broke")
        });

    let demo = demo_account_after_build(&state, &owner.token).await;
    assert_eq!(demo.status, DemoAccountStatus::Failed);
    let (status, body) = start_demo_build(&state, &owner.token).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let demo = demo_account_after_build(&state, &owner.token).await;
    assert_eq!(demo.status, DemoAccountStatus::Ready, "{:?}", demo.error);
}

/// While the Demo Account is built it cannot be entered: the login card
/// does not offer it, a login as `demo` is refused, and a Session made
/// before the build ended when the build started. Otherwise a visitor would
/// see conversations still arriving, and lose the account under them when
/// the build failed (#1220).
#[tokio::test(flavor = "multi_thread")]
async fn the_demo_account_cannot_be_entered_while_it_is_built() {
    let fixture = test_fixture().await;
    let mut state = fixture.state.clone();
    state.demo_bundle_generator = slow_tiny_bundle;
    fixture.demo_account().await;
    let owner = claim_as_owner(&state, "keeper", "hunter2hunter2").await;
    let visitor = crate::test_support::log_in(&state, "demo", "").await;
    let visitor_token = visitor["token"].as_str().unwrap().to_string();
    assert_eq!(
        get_status(&state, "/v1/session", &visitor_token).await,
        StatusCode::OK
    );

    let (status, body) = start_demo_build(&state, &owner.token).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");

    let info: Server = get_json(&state, "/v1/server", "").await;
    assert!(!info.demo_account, "the login card offers no Demo Account");
    assert_eq!(
        crate::test_support::login_status(&state, "demo", "").await,
        StatusCode::UNAUTHORIZED,
        "a login as demo is refused while the build runs"
    );
    assert_eq!(
        get_status(&state, "/v1/session", &visitor_token).await,
        StatusCode::UNAUTHORIZED,
        "the Session made before the build ended when the build started"
    );

    let demo = demo_account_after_build(&state, &owner.token).await;
    assert_eq!(demo.status, DemoAccountStatus::Ready, "{:?}", demo.error);
    let info: Server = get_json(&state, "/v1/server", "").await;
    assert!(info.demo_account);
    assert_eq!(
        crate::test_support::login_status(&state, "demo", "").await,
        StatusCode::CREATED,
        "the built Demo Account is entered as before"
    );
}

/// The media pass at the end of a Demo Account build converts the Demo
/// Account's attachments and nothing else. Another account's Upload in
/// progress keeps its `.part` file, and that account's attachment with no
/// preview yet is left for its own pass (#1220).
#[test]
fn a_demo_build_converts_no_other_accounts_attachments() {
    // Taken outside the runtime: holding the guard across an await is what
    // Clippy's `await_holding_lock` refuses.
    let Some(_tools) = media::testutil::real_ffmpeg_test_guard() else {
        return;
    };
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let fixture = test_fixture().await;
            let mut state = fixture.state.clone();
            state.demo_bundle_generator = tiny_bundle;
            let owner = claim_as_owner(&state, "keeper", "hunter2hunter2").await;
            let other = fixture.account("someone").await;
            crate::test_support::seed_one_message(&state, other).await;

            let assets = state.cfg.paths.assets_dir_for_account(other, "imessage");
            let sha = crate::test_support::fake_sha256('c');
            let blob = assets.join(&sha[..2]).join(format!("{sha}.png"));
            std::fs::create_dir_all(blob.parent().unwrap()).unwrap();
            std::fs::write(&blob, crate::process_assets::tests::PNG_1X1_RGB).unwrap();
            let part = assets.join(".incoming").join(format!("{sha}-upload.part"));
            std::fs::create_dir_all(part.parent().unwrap()).unwrap();
            std::fs::write(&part, b"half an attachment").unwrap();
            let attachment: i64 = {
                let mut conn = fixture.conn().await;
                sqlx::query_scalar(
                    "INSERT INTO attachments (message_id, sha256, assets_path)
                     SELECT id, $2, $3 FROM messages WHERE account_id = $1
                     RETURNING id",
                )
                .bind(other)
                .bind(&sha)
                .bind(format!("{}/{sha}.png", &sha[..2]))
                .fetch_one(&mut *conn)
                .await
                .unwrap()
            };

            let (status, body) = start_demo_build(&state, &owner.token).await;
            assert_eq!(status, StatusCode::ACCEPTED, "{body}");
            let demo = demo_account_after_build(&state, &owner.token).await;
            assert_eq!(demo.status, DemoAccountStatus::Ready, "{:?}", demo.error);

            assert!(part.is_file(), "the other account's Upload keeps its .part");
            let mut conn = fixture.conn().await;
            let derived: Option<String> =
                sqlx::query_scalar("SELECT derived_assets_path FROM attachments WHERE id = $1")
                    .bind(attachment)
                    .fetch_one(&mut *conn)
                    .await
                    .unwrap();
            assert_eq!(
                derived, None,
                "the other account's attachment is not converted"
            );
            assert!(
                !state
                    .cfg
                    .paths
                    .assets_converted_dir_for_account(other, "imessage")
                    .exists(),
                "the pass never opened the other account's source"
            );
        });
}
