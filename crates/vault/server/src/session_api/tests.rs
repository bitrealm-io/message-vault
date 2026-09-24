use axum::http::StatusCode;

use super::*;
use crate::problem::ProblemType;
use crate::test_support::{
    RegisteredAccount, claim_vault_as_owner, delete_status, expect_problem, get_raw, get_status,
    log_in, login_status, post_created_json, post_raw, put_status, register_via_api, test_vault,
    vault_with_account,
};

const TEST_ACCOUNT: i64 = 7;

/// The Session is a singleton: logging in answers `201 Created` with a
/// `Location` naming `/v1/session` itself, `GET` reads it back without an
/// `ok` flag, and `DELETE` ends it with `204 No Content`.
#[tokio::test]
async fn a_session_is_created_read_and_deleted_at_one_path() {
    let (vault, _) = vault_with_account().await;
    let state = vault.state.clone();

    let created = crate::test_support::log_in(&state, "alice", "hunter2hunter2").await;
    assert_eq!(created["username"], "alice");
    let token = created["token"].as_str().unwrap().to_string();

    let body: serde_json::Value =
        crate::test_support::get_json(&state, "/v1/session", &token).await;
    assert_eq!(body["username"], "alice");
    assert_eq!(body["account_id"], created["account_id"]);
    assert!(body["sources"].is_array(), "{body}");
    assert!(
        body.get("ok").is_none() && body.get("account_ok").is_none(),
        "{body}"
    );

    let status = crate::test_support::delete_status(&state, "/v1/session", &token).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let status = crate::test_support::get_status(&state, "/v1/session", &token).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "a deleted session no longer names an account"
    );
}

/// Logging out ends a Session and nothing else. An API token is not a
/// Session: `DELETE /v1/session` with one is refused, and the token keeps
/// working. It used to answer `204` and do nothing, which told a program it
/// had ended something it had not.
#[tokio::test]
async fn logging_out_with_an_api_token_is_refused_and_leaves_the_token_working() {
    let (vault, alice) = vault_with_account().await;
    let state = vault.state.clone();
    let mut conn = vault.conn().await;
    let token = crate::db::api_tokens::create_api_token(
        &mut conn,
        alice.account_id,
        "push",
        crate::db::permissions::Permissions::all(),
        None,
    )
    .await
    .unwrap()
    .token;
    drop(conn);

    let (status, text) = crate::test_support::delete_raw(&state, "/v1/session", &token).await;
    crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::InsufficientScope,
    );
    assert_eq!(
        crate::test_support::get_status(&state, "/v1/imports", &token).await,
        StatusCode::OK,
        "the token still works"
    );
}

/// A token that names no Session is a failed credential, `401`, like on
/// every other route.
#[tokio::test]
async fn logging_out_with_a_token_that_names_nothing_is_a_401() {
    let (vault, alice) = vault_with_account().await;
    let state = vault.state.clone();

    let (status, text) =
        crate::test_support::delete_raw(&state, "/v1/session", "mv-user-not-a-session").await;
    crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::AuthenticationRequired,
    );

    let status = crate::test_support::delete_status(&state, "/v1/session", &alice.token).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, text) = crate::test_support::delete_raw(&state, "/v1/session", &alice.token).await;
    crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::AuthenticationRequired,
    );
}

/// A disabled account can still log out: its Session is ended even though
/// every other route refuses it.
#[tokio::test]
async fn a_disabled_account_can_still_log_out() {
    let (vault, alice) = vault_with_account().await;
    let state = vault.state.clone();
    let mut conn = vault.conn().await;
    sqlx::query("UPDATE accounts SET disabled = 1 WHERE id = $1")
        .bind(alice.account_id)
        .execute(&mut *conn)
        .await
        .unwrap();

    assert_eq!(
        crate::test_support::delete_status(&state, "/v1/session", &alice.token).await,
        StatusCode::NO_CONTENT
    );
    let sessions: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM account_session_tokens WHERE account_id = $1")
            .bind(alice.account_id)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(sessions, 0, "the Session row is gone");
}

/// The credential names the account. There is no `account=` parameter on the
/// singleton, so a query string naming someone else is refused like any
/// parameter the route does not take, never obeyed and never quietly dropped.
#[tokio::test]
async fn a_session_read_refuses_an_account_parameter() {
    let (vault, alice) = vault_with_account().await;
    let state = vault.state.clone();
    let bob = register_via_api(&state, "bob", "hunter2hunter2").await;

    let (status, text) = crate::test_support::get_raw(
        &state,
        &format!("/v1/session?account={}", bob.username),
        &alice.token,
    )
    .await;
    let problem = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::ValidationFailed,
    );
    assert_eq!(
        problem.errors.unwrap(),
        ["unknown query parameter 'account'; this route takes no query parameters"]
    );
}

#[tokio::test]
async fn logout_on_conn_leaves_registered_account() {
    let vault = test_vault().await;
    let mut conn = vault.conn().await;
    account_profile::insert_account_at(&mut conn, TEST_ACCOUNT, "alice", None, None)
        .await
        .unwrap();
    let token = session_tokens::insert_account_session_token(&mut conn, TEST_ACCOUNT)
        .await
        .unwrap();

    logout_on_conn(&mut conn, &token).await.unwrap();

    assert_eq!(
        account_profile::username_for_account(&mut conn, TEST_ACCOUNT)
            .await
            .unwrap()
            .as_deref(),
        Some("alice")
    );
    assert!(
        session_tokens::lookup_session(&mut conn, &token)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn disabled_account_cannot_log_in() {
    let (vault, created) = vault_with_account().await;
    let state = vault.state.clone();

    let mut conn = state.db.acquire().await.unwrap();
    sqlx::query("UPDATE accounts SET disabled = 1 WHERE id = $1")
        .bind(created.account_id)
        .execute(&mut *conn)
        .await
        .unwrap();

    let status = login_status(&state, "alice", "hunter2hunter2").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// How a credential stops working, seen from a real route
// ---------------------------------------------------------------------------

/// A browse route every account reaches with a live session, and no token.
const BROWSE: &str = "/v1/conversations";

/// Assert `token` is refused on `GET path` with `authentication-required`.
async fn assert_refused(state: &crate::server::AppState, path: &str, token: &str, why: &str) {
    let (status, text) = get_raw(state, path, token).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{why}: {text}");
    expect_problem(status, &text, ProblemType::AuthenticationRequired);
}

/// A session past its expiry is refused on a browse route as if it had never
/// existed, and presenting it removes its row. The expiry is moved into the
/// past in the database rather than waited out.
#[tokio::test]
async fn an_expired_session_is_refused_on_a_browse_route() {
    let (vault, alice) = vault_with_account().await;
    let state = vault.state.clone();
    assert_eq!(
        get_status(&state, BROWSE, &alice.token).await,
        StatusCode::OK,
        "the session works before it expires"
    );

    let mut conn = vault.conn().await;
    sqlx::query("UPDATE account_session_tokens SET expires_at = '1' WHERE account_id = $1")
        .bind(alice.account_id)
        .execute(&mut *conn)
        .await
        .unwrap();

    assert_refused(&state, BROWSE, &alice.token, "an expired session").await;
    assert_eq!(
        session_rows(&mut conn, alice.account_id).await,
        0,
        "an expired session's row is removed when it is presented"
    );
}

/// Logging out ends the session everywhere, not only at `/v1/session`.
#[tokio::test]
async fn a_logged_out_session_is_refused_on_a_browse_route() {
    let (vault, alice) = vault_with_account().await;
    let state = vault.state.clone();
    assert_eq!(
        get_status(&state, BROWSE, &alice.token).await,
        StatusCode::OK
    );

    let status = delete_status(&state, "/v1/session", &alice.token).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    assert_refused(&state, BROWSE, &alice.token, "a logged-out session").await;
}

/// An export-scoped API token for `account`, as `(id, secret)`.
async fn export_token(
    state: &crate::server::AppState,
    account: &RegisteredAccount,
) -> (i64, String) {
    let (_location, created): (String, serde_json::Value) = post_created_json(
        state,
        &format!("/v1/accounts/{}/api-tokens", account.account_id),
        &account.token,
        serde_json::json!({ "label": "pull", "can_import": false, "can_export": true }),
    )
    .await;
    (
        created["id"].as_i64().unwrap(),
        created["token"].as_str().unwrap().to_string(),
    )
}

/// A deleted API token stops reaching the export routes it used: it can
/// neither page the Export Run it started nor start another.
#[tokio::test]
async fn a_deleted_api_token_is_refused_on_the_export_routes() {
    let (vault, alice) = vault_with_account().await;
    let state = vault.state.clone();
    let (token_id, api_token) = export_token(&state, &alice).await;

    let everything = serde_json::json!({ "scope": { "kind": "everything" } });
    let (_location, run): (String, serde_json::Value) =
        post_created_json(&state, "/v1/exports", &api_token, everything.clone()).await;
    let run_messages = format!("/v1/exports/{}/messages", run["id"]);
    assert_eq!(
        get_status(&state, &run_messages, &api_token).await,
        StatusCode::OK,
        "the token pages its own run before it is deleted"
    );

    let status = delete_status(
        &state,
        &format!("/v1/accounts/{}/api-tokens/{token_id}", alice.account_id),
        &alice.token,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    assert_refused(
        &state,
        &run_messages,
        &api_token,
        "a deleted token paging its run",
    )
    .await;
    let (status, text) = post_raw(
        &state,
        "/v1/exports",
        &api_token,
        "application/json",
        everything.to_string(),
    )
    .await;
    expect_problem(status, &text, ProblemType::AuthenticationRequired);
    assert_eq!(
        get_status(&state, BROWSE, &alice.token).await,
        StatusCode::OK,
        "deleting a token leaves the session that deleted it alone"
    );
}

/// A token past its expiry is refused on an export route just as a deleted
/// one is.
#[tokio::test]
async fn an_expired_api_token_is_refused_on_an_export_route() {
    let (vault, alice) = vault_with_account().await;
    let state = vault.state.clone();
    let (token_id, api_token) = export_token(&state, &alice).await;
    assert_eq!(
        get_status(&state, "/v1/exports", &api_token).await,
        StatusCode::OK,
        "the token lists runs before it expires"
    );

    let mut conn = vault.conn().await;
    sqlx::query("UPDATE account_api_tokens SET expires_at = '1' WHERE id = $1")
        .bind(token_id)
        .execute(&mut *conn)
        .await
        .unwrap();

    assert_refused(&state, "/v1/exports", &api_token, "an expired token").await;
}

/// A Session is one per logged-in account (`CONTEXT.md`, and "Credentials and
/// reach" in `docs/architecture/http-api.md`), so logging in again replaces
/// it: the newer token works and the older one, on whatever device holds it,
/// is refused.
#[tokio::test]
async fn a_second_login_replaces_the_first_session() {
    let (vault, first) = vault_with_account().await;
    let state = vault.state.clone();
    let second = log_in(&state, "alice", "hunter2hunter2").await;
    let second = second["token"].as_str().unwrap();
    assert_ne!(second, first.token, "a login issues a new token");

    assert_eq!(
        get_status(&state, BROWSE, second).await,
        StatusCode::OK,
        "the newest login works"
    );
    assert_refused(&state, BROWSE, &first.token, "the replaced session").await;

    let mut conn = vault.conn().await;
    assert_eq!(
        session_rows(&mut conn, first.account_id).await,
        1,
        "one account holds one session"
    );
}

/// The owner setting an account's password sets the password and nothing
/// more (`CONTEXT.md`, Owner Home): the account's session keeps browsing.
#[tokio::test]
async fn an_owner_password_reset_leaves_the_session_browsing() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let bob = register_via_api(&state, "bob", "hunter2hunter2").await;

    let status = put_status(
        &state,
        &format!("/v1/accounts/{}/password", bob.account_id),
        &owner.token,
        serde_json::json!({ "password": "resetbytheowner", "password_confirmation": "resetbytheowner" }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    assert_eq!(
        get_status(&state, BROWSE, &bob.token).await,
        StatusCode::OK,
        "bob's session carries on after the owner's reset"
    );
}

/// How many session rows `account` holds.
async fn session_rows(conn: &mut sqlx::AnyConnection, account: i64) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM account_session_tokens WHERE account_id = $1")
        .bind(account)
        .fetch_one(conn)
        .await
        .unwrap()
}
