use axum::http::StatusCode;

use super::*;
use crate::test_support::{login_status, register_via_api, test_vault};

const TEST_ACCOUNT: i64 = 7;

/// The Session is a singleton: signing in answers `201 Created` with a
/// `Location` naming `/v1/session` itself, `GET` reads it back without an
/// `ok` flag, and `DELETE` ends it with `204 No Content`.
#[tokio::test]
async fn a_session_is_created_read_and_deleted_at_one_path() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    register_via_api(&state, "alice", "hunter2hunter2").await;

    let created = crate::test_support::sign_in(&state, "alice", "hunter2hunter2").await;
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

/// The credential names the account. There is no `account=` parameter on the
/// singleton, so a query string naming someone else is not a refusal: it is
/// nothing, and the reply is still the token's own account.
#[tokio::test]
async fn a_session_read_ignores_a_query_string() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let alice = register_via_api(&state, "alice", "hunter2hunter2").await;
    let bob = register_via_api(&state, "bob", "hunter2hunter2").await;

    let body: serde_json::Value = crate::test_support::get_json(
        &state,
        &format!("/v1/session?account={}", bob.username),
        &alice.token,
    )
    .await;
    assert_eq!(body["username"], "alice");
    assert_eq!(body["account_id"], alice.account_id);
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
        session_tokens::lookup_account_for_token(&mut conn, &token)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn disabled_account_cannot_sign_in() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let created = register_via_api(&state, "alice", "hunter2hunter2").await;

    let mut conn = state.db.acquire().await.unwrap();
    sqlx::query("UPDATE accounts SET disabled = 1 WHERE id = $1")
        .bind(created.account_id)
        .execute(&mut *conn)
        .await
        .unwrap();

    let status = login_status(&state, "alice", "hunter2hunter2").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
