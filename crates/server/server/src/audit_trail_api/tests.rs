//! The Audit Trail through the router: what each act leaves in it, who reads
//! it, and what survives the account (#619).

use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::problem::ProblemType;
use crate::server::{APP_HEADER, APP_VERSION_HEADER};
use crate::test_support::{
    PASSWORD, claim_as_owner, delete_status, expect_problem, get_json, get_raw, log_in,
    login_status, patch_status, post_created_json, register_via_api, serve, test_fixture,
};

/// The items of an Audit Trail page.
async fn trail(state: &crate::server::AppState, path: &str, token: &str) -> Vec<Value> {
    let page: Value = get_json(state, path, token).await;
    page["items"].as_array().cloned().unwrap_or_default()
}

/// The `action` of each item, in the order the page lists them.
fn actions(items: &[Value]) -> Vec<&str> {
    items
        .iter()
        .map(|item| item["action"].as_str().unwrap())
        .collect()
}

/// Log in as `username` from `app` at Build `build`, and return the token.
async fn log_in_from(
    state: &crate::server::AppState,
    username: &str,
    app: &str,
    build: &str,
) -> String {
    let server = serve(state).await;
    let response = reqwest::Client::new()
        .post(format!("{}/v1/session", server.base()))
        .header(APP_HEADER, app)
        .header(APP_VERSION_HEADER, build)
        .json(&json!({ "username": username, "password": PASSWORD }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let body: Value = response.json().await.unwrap();
    body["token"].as_str().unwrap().to_string()
}

/// The owner reads every account's entries in one list, newest first, runs
/// included, and each says who acted on which account.
#[tokio::test]
async fn the_owner_reads_every_accounts_entries_and_runs_newest_first() {
    let fixture = test_fixture().await;
    let state = &fixture.state;
    let owner = claim_as_owner(state, "keeper", PASSWORD).await;
    let alice = register_via_api(state, "alice", PASSWORD).await;
    let _: (String, Value) = post_created_json(
        state,
        "/v1/exports",
        &alice.token,
        json!({ "scope": { "kind": "everything" } }),
    )
    .await;
    assert_eq!(
        patch_status(
            state,
            &format!("/v1/accounts/{}", alice.account_id),
            &owner.token,
            json!({ "can_export": false }),
        )
        .await,
        StatusCode::OK
    );

    let items = trail(state, "/v1/audit-trail", &owner.token).await;
    assert_eq!(
        actions(&items),
        [
            "permissions_changed",
            "export_run",
            "logged_in",
            "account_created",
            "logged_in"
        ]
    );
    assert_eq!(items[0]["actor"], "owner");
    assert_eq!(items[0]["account_id"], alice.account_id);
    assert_eq!(items[0]["username"], "alice");
    assert_eq!(items[0]["permissions_removed"], json!(["export"]));
    assert_eq!(items[0]["permissions_added"], json!([]));
    assert_eq!(items[1]["actor"], "holder");
    assert_eq!(items[1]["credential"], "session");
    assert_eq!(items[1]["scope_kind"], "everything");
    assert_eq!(items[3]["actor"], "anonymous");
    assert_eq!(items[4]["username"], "keeper");
    assert_eq!(items[4]["actor"], "owner");
}

/// An account reads every entry about itself, the owner's changes to it
/// included, and nobody else's; only the owner reads the whole trail.
#[tokio::test]
async fn an_account_reads_its_own_entries_including_the_owners_changes() {
    let fixture = test_fixture().await;
    let state = &fixture.state;
    let owner = claim_as_owner(state, "keeper", PASSWORD).await;
    let alice = register_via_api(state, "alice", PASSWORD).await;
    let bob = register_via_api(state, "bob", PASSWORD).await;
    assert_eq!(
        patch_status(
            state,
            &format!("/v1/accounts/{}", alice.account_id),
            &owner.token,
            json!({ "can_delete": false }),
        )
        .await,
        StatusCode::OK
    );

    let own = format!("/v1/accounts/{}/audit-trail", alice.account_id);
    let items = trail(state, &own, &alice.token).await;
    assert_eq!(
        actions(&items),
        ["permissions_changed", "logged_in", "account_created"]
    );
    assert_eq!(items[0]["actor"], "owner");
    assert!(
        items.iter().all(|item| item["username"] == "alice"),
        "only alice's entries: {items:?}"
    );
    // The owner reads the same entries under the account.
    assert_eq!(trail(state, &own, &owner.token).await, items);

    let (status, text) = get_raw(state, &own, &bob.token).await;
    expect_problem(status, &text, ProblemType::NotTheOwner);
    let (status, text) = get_raw(state, "/v1/audit-trail", &alice.token).await;
    expect_problem(status, &text, ProblemType::NotTheOwner);
}

/// A login on the desktop while the website is logged in takes the account's
/// one Session, and the website's is recorded as replaced; logging out
/// records the desktop's as logged out.
#[tokio::test]
async fn a_desktop_login_records_the_website_session_as_replaced() {
    let fixture = test_fixture().await;
    let state = &fixture.state;
    let owner = claim_as_owner(state, "keeper", PASSWORD).await;
    let alice = register_via_api(state, "alice", PASSWORD).await;
    log_in_from(state, "alice", "website", "0.10.0+aaaa1111").await;
    let desktop = log_in_from(state, "alice", "desktop", "0.10.0+bbbb2222").await;
    assert_eq!(
        delete_status(state, "/v1/session", &desktop).await,
        StatusCode::NO_CONTENT
    );

    let path = format!("/v1/accounts/{}/audit-trail", alice.account_id);
    let items = trail(state, &path, &owner.token).await;
    assert_eq!(
        actions(&items),
        [
            "session_ended",
            "logged_in",
            "session_ended",
            "logged_in",
            "session_ended",
            "logged_in",
            "account_created"
        ]
    );
    assert_eq!(items[0]["reason"], "logged_out");
    assert_eq!(items[1]["app"], "desktop");
    assert_eq!(items[1]["app_build"], "0.10.0+bbbb2222");
    assert_eq!(items[2]["reason"], "replaced");
    assert_eq!(items[3]["app"], "website");
    // Registering opened the first Session, which the website login replaced.
    assert_eq!(items[4]["reason"], "replaced");
}

/// A refused login for a username nobody holds is kept as typed, belongs to
/// no account, and is gone once it is more than ninety days old. A wrong
/// password is kept under the account, and both get the same answer.
#[tokio::test]
async fn a_refused_login_for_an_unknown_username_is_kept_ninety_days() {
    let fixture = test_fixture().await;
    let state = &fixture.state;
    let owner = claim_as_owner(state, "keeper", PASSWORD).await;
    let alice = register_via_api(state, "alice", PASSWORD).await;
    assert_eq!(
        login_status(state, "  Nobody-Here  ", "guess").await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login_status(state, "alice", "guess").await,
        StatusCode::UNAUTHORIZED
    );

    let items = trail(state, "/v1/audit-trail", &owner.token).await;
    assert_eq!(items[0]["action"], "login_refused");
    assert_eq!(items[0]["reason"], "wrong_password");
    assert_eq!(items[0]["account_id"], alice.account_id);
    assert_eq!(items[1]["action"], "login_refused");
    assert_eq!(items[1]["reason"], "unknown_username");
    assert_eq!(items[1]["account_id"], Value::Null);
    assert_eq!(items[1]["username"], "Nobody-Here");
    // The account reads the refusals for its own username, not the stranger's.
    let own = trail(
        state,
        &format!("/v1/accounts/{}/audit-trail", alice.account_id),
        &alice.token,
    )
    .await;
    assert_eq!(own[0]["reason"], "wrong_password");
    assert!(own.iter().all(|item| item["username"] == "alice"));

    // Ninety-one days on, the next login trims the stranger's refusal and
    // keeps alice's.
    let long_ago = (chrono::Utc::now() - chrono::Duration::days(91)).to_rfc3339();
    sqlx::query("UPDATE audit_entries SET at = $1 WHERE action = 'login_refused'")
        .bind(&long_ago)
        .execute(&mut *fixture.conn().await)
        .await
        .unwrap();
    log_in(state, "alice", PASSWORD).await;
    let items = trail(state, "/v1/audit-trail", &owner.token).await;
    let refused: Vec<&Value> = items
        .iter()
        .filter(|item| item["action"] == "login_refused")
        .collect();
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert_eq!(refused[0]["reason"], "wrong_password");
}

/// A rate-limited login is turned away before anything is checked, and
/// leaves nothing in the Audit Trail.
#[tokio::test]
async fn a_rate_limited_login_writes_nothing() {
    let fixture = test_fixture().await;
    let state = &fixture.state;
    let owner = claim_as_owner(state, "keeper", PASSWORD).await;
    for _ in 0..crate::credentials::AUTH_RATE_MAX {
        login_status(state, "nobody", "guess").await;
    }
    assert_eq!(
        login_status(state, "nobody", "guess").await,
        StatusCode::TOO_MANY_REQUESTS
    );
    let items = trail(state, "/v1/audit-trail", &owner.token).await;
    let refused = items
        .iter()
        .filter(|item| item["action"] == "login_refused")
        .count();
    assert_eq!(refused, crate::credentials::AUTH_RATE_MAX);
}

/// Deleting an account leaves its entries and runs readable under its old
/// username, ends its live Session as `revoked`, adds an `account_deleted`
/// entry, and drops the export's search text.
#[tokio::test]
async fn deleting_an_account_keeps_its_entries_and_runs_under_its_username() {
    let fixture = test_fixture().await;
    let state = &fixture.state;
    let owner = claim_as_owner(state, "keeper", PASSWORD).await;
    let alice = register_via_api(state, "alice", PASSWORD).await;
    let (_, run): (String, Value) = post_created_json(
        state,
        "/v1/exports",
        &alice.token,
        json!({ "scope": { "kind": "query", "list": "messages", "q": "secret plans" } }),
    )
    .await;
    assert_eq!(
        delete_status(
            state,
            &format!("/v1/accounts/{}", alice.account_id),
            &owner.token
        )
        .await,
        StatusCode::NO_CONTENT
    );

    let items = trail(state, "/v1/audit-trail", &owner.token).await;
    assert_eq!(
        actions(&items[..5]),
        [
            "account_deleted",
            "session_ended",
            "export_run",
            "logged_in",
            "account_created"
        ]
    );
    for item in &items[..5] {
        assert_eq!(item["username"], "alice", "{item}");
        assert_eq!(item["account_id"], Value::Null, "{item}");
    }
    assert_eq!(items[0]["actor"], "owner");
    assert_eq!(items[1]["reason"], "revoked");
    assert_eq!(items[1]["actor"], "owner");
    assert_eq!(items[2]["id"], run["id"]);
    assert_eq!(items[2]["scope_kind"], "query");
    assert_eq!(items[2]["scope_list"], "messages");
    assert!(!items[2].to_string().contains("secret plans"));
    let kept: Option<String> = sqlx::query_scalar("SELECT scope_query FROM exports WHERE id = $1")
        .bind(run["id"].as_i64().unwrap())
        .fetch_one(&mut *fixture.conn().await)
        .await
        .unwrap();
    assert_eq!(kept, None, "the search text goes with the account");
}

/// A run started with an API token names the token, by label and hint as
/// they were, after the token is deleted.
#[tokio::test]
async fn a_run_started_by_a_deleted_api_token_still_names_it() {
    let fixture = test_fixture().await;
    let state = &fixture.state;
    let alice = register_via_api(state, "alice", PASSWORD).await;
    let tokens = format!("/v1/accounts/{}/api-tokens", alice.account_id);
    let (location, created): (String, Value) = post_created_json(
        state,
        &tokens,
        &alice.token,
        json!({ "label": "nightly backup", "can_import": false, "can_export": true }),
    )
    .await;
    let api_token = created["token"].as_str().unwrap();
    let hint = created["token_hint"].as_str().unwrap();
    let _: (String, Value) = post_created_json(
        state,
        "/v1/exports",
        api_token,
        json!({ "scope": { "kind": "everything" } }),
    )
    .await;
    assert_eq!(
        delete_status(state, &location, &alice.token).await,
        StatusCode::NO_CONTENT
    );

    let path = format!("/v1/accounts/{}/audit-trail", alice.account_id);
    let items = trail(state, &path, &alice.token).await;
    assert_eq!(
        actions(&items[..3]),
        ["api_token_deleted", "export_run", "api_token_created"]
    );
    for item in &items[..3] {
        assert_eq!(item["api_token_label"], "nightly backup", "{item}");
        assert_eq!(item["api_token_hint"], hint, "{item}");
    }
    assert_eq!(items[1]["credential"], "api_token");
    assert!(
        !items
            .iter()
            .any(|item| item.to_string().contains(api_token))
    );
}

/// A session that runs out with nothing to end it reads as expired at its
/// expiry, though nothing was written when it ran out.
#[tokio::test]
async fn a_session_that_ran_out_reads_as_expired() {
    let fixture = test_fixture().await;
    let state = &fixture.state;
    let owner = claim_as_owner(state, "keeper", PASSWORD).await;
    let alice = register_via_api(state, "alice", PASSWORD).await;
    // Registered thirty-one days ago, so the Session ran out a day ago.
    let registered = (chrono::Utc::now() - chrono::Duration::days(31)).to_rfc3339();
    let a_day_ago = (chrono::Utc::now() - chrono::Duration::days(1)).to_rfc3339();
    sqlx::query(
        "UPDATE audit_entries SET at = $1,
                session_expires_at = CASE WHEN action = 'logged_in' THEN $2 END
         WHERE account_id = $3",
    )
    .bind(&registered)
    .bind(&a_day_ago)
    .bind(alice.account_id)
    .execute(&mut *fixture.conn().await)
    .await
    .unwrap();

    let path = format!("/v1/accounts/{}/audit-trail", alice.account_id);
    let items = trail(state, &path, &owner.token).await;
    assert_eq!(
        actions(&items),
        ["session_ended", "logged_in", "account_created"]
    );
    assert_eq!(items[0]["reason"], "expired");
    assert_eq!(items[0]["at"], a_day_ago);
}

/// The owner's changes to an account are each recorded once, with what
/// changed: disabling, re-enabling, setting the password, and deleting its
/// messages. A flag sent with the value it already has records nothing.
#[tokio::test]
async fn the_owners_changes_to_an_account_are_recorded() {
    let fixture = test_fixture().await;
    let state = &fixture.state;
    let owner = claim_as_owner(state, "keeper", PASSWORD).await;
    let alice = register_via_api(state, "alice", PASSWORD).await;
    let account = format!("/v1/accounts/{}", alice.account_id);
    for body in [
        json!({ "disabled": true }),
        json!({ "disabled": true }),
        json!({ "disabled": false, "can_import": true }),
    ] {
        assert_eq!(
            patch_status(state, &account, &owner.token, body).await,
            StatusCode::OK
        );
    }
    assert_eq!(
        crate::test_support::put_status(
            state,
            &format!("{account}/password"),
            &owner.token,
            json!({ "password": "new-one", "password_confirmation": "new-one" }),
        )
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        delete_status(state, &format!("{account}/messages"), &owner.token).await,
        StatusCode::OK
    );

    let items = trail(state, &format!("{account}/audit-trail"), &alice.token).await;
    assert_eq!(
        actions(&items[..4]),
        [
            "messages_deleted",
            "password_set",
            "account_enabled",
            "account_disabled"
        ]
    );
    assert!(items[..4].iter().all(|item| item["actor"] == "owner"));
    assert_eq!(items[0]["conversations"], 0);
}
