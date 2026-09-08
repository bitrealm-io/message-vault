use axum::http::StatusCode;

use super::*;
use crate::test_support::{
    claim_vault_as_owner, get_json, get_status, patch_status, post_json, post_status,
    post_status_signed_out, register_via_api, test_vault,
};

/// Turn public registration off, the way a real vault ships.
async fn close_registration(state: &AppState) {
    let mut conn = state.db.acquire().await.unwrap();
    vault_settings::set_public_registration(&mut conn, false)
        .await
        .unwrap();
}

#[tokio::test]
async fn an_unowned_vault_reports_unclaimed() {
    let vault = test_vault().await;
    let state = vault.state.clone();

    let body: VaultResponse = get_json(&state, "/v1/vault", "").await;
    assert_eq!(body.state, VaultState::Unclaimed);
}

/// Unclaimed wins over the registration setting: a vault with no owner has
/// one thing to offer, and joining it is not that thing.
#[tokio::test]
async fn public_registration_does_not_make_an_unowned_vault_open() {
    let vault = test_vault().await;
    let state = vault.state.clone();

    let body: VaultResponse = get_json(&state, "/v1/vault", "").await;
    assert_eq!(
        body.state,
        VaultState::Unclaimed,
        "test vaults open registration; being unclaimed still comes first"
    );
}

#[tokio::test]
async fn a_claimed_vault_is_closed_until_registration_is_opened() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    close_registration(&state).await;
    let _owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;

    let body: VaultResponse = get_json(&state, "/v1/vault", "").await;
    assert_eq!(body.state, VaultState::Closed);
}

#[tokio::test]
async fn a_claimed_vault_with_registration_on_is_open() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let _owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;

    let body: VaultResponse = get_json(&state, "/v1/vault", "").await;
    assert_eq!(body.state, VaultState::Open);
}

/// The route reports the vault's state to anyone, signed in or not. The
/// Create Vault Owner screen has no credential to present.
#[tokio::test]
async fn the_state_route_needs_no_credential() {
    let vault = test_vault().await;
    let state = vault.state.clone();

    assert_eq!(get_status(&state, "/v1/vault", "").await, StatusCode::OK);
    assert_eq!(
        get_status(&state, "/v1/vault", "not-a-real-token").await,
        StatusCode::OK,
        "a stale token must not stop the entry screen loading"
    );
}

#[tokio::test]
async fn claiming_an_unowned_vault_creates_the_owner_and_signs_them_in() {
    let vault = test_vault().await;
    let state = vault.state.clone();

    let body: serde_json::Value = post_json(
        &state,
        "/v1/vault/claim",
        "",
        serde_json::json!({ "username": "keeper", "password": "hunter2hunter2" }),
    )
    .await;

    assert_eq!(body["account_id"], account_profile::OWNER_ACCOUNT_ID);
    assert_eq!(body["username"], "keeper");

    // The token it hands back is the owner's session, usable at once.
    let token = body["token"].as_str().unwrap();
    let mut conn = state.db.acquire().await.unwrap();
    let auth = crate::server::resolve_auth_on_conn(&mut conn, token)
        .await
        .unwrap();
    assert!(auth.is_owner());
    drop(conn);

    let after: VaultResponse = get_json(&state, "/v1/vault", "").await;
    assert_eq!(
        after.state,
        VaultState::Open,
        "test vaults open registration"
    );
}

#[tokio::test]
async fn a_vault_can_only_be_claimed_once() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let _owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;

    let status = post_status(
        &state,
        "/v1/vault/claim",
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

#[tokio::test]
async fn claiming_enforces_the_password_policy() {
    let vault = test_vault().await;
    let state = vault.state.clone();

    let status = post_status(
        &state,
        "/v1/vault/claim",
        "",
        serde_json::json!({ "username": "keeper", "password": "admin" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a real owner's password cannot be five characters"
    );

    let after: VaultResponse = get_json(&state, "/v1/vault", "").await;
    assert_eq!(after.state, VaultState::Unclaimed);
}

#[tokio::test]
async fn registration_is_refused_while_the_vault_is_closed() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    close_registration(&state).await;

    let status = post_status_signed_out(
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
    let vault = test_vault().await;
    let state = vault.state.clone();
    close_registration(&state).await;
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;

    let settings: VaultSettingsResponse =
        get_json(&state, "/v1/vault/settings", &owner.token).await;
    assert!(!settings.public_registration);

    assert_eq!(
        post_status_signed_out(
            &state,
            "/v1/accounts",
            serde_json::json!({ "username": "stranger", "password": "hunter2hunter2" }),
        )
        .await,
        StatusCode::FORBIDDEN
    );

    let opened: VaultSettingsResponse = crate::test_support::patch_json(
        &state,
        "/v1/vault/settings",
        &owner.token,
        serde_json::json!({ "public_registration": true }),
    )
    .await;
    assert!(opened.public_registration);

    let joined = register_via_api(&state, "stranger", "hunter2hunter2").await;
    assert_eq!(joined.username, "stranger");

    let body: VaultResponse = get_json(&state, "/v1/vault", "").await;
    assert_eq!(body.state, VaultState::Open);
}

#[tokio::test]
async fn only_the_owner_reaches_the_vault_settings() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let _owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let ordinary = register_via_api(&state, "bob", "hunter2hunter2").await;

    assert_eq!(
        get_status(&state, "/v1/vault/settings", &ordinary.token).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        patch_status(
            &state,
            "/v1/vault/settings",
            &ordinary.token,
            serde_json::json!({ "public_registration": false }),
        )
        .await,
        StatusCode::FORBIDDEN
    );
}

/// Claiming the vault puts a row at the owner id and nowhere else.
#[tokio::test]
async fn claiming_the_vault_creates_exactly_one_owner() {
    let vault = test_vault().await;
    let state = vault.state.clone();

    let mut conn = state.db.acquire().await.unwrap();
    assert!(!account_profile::vault_is_claimed(&mut conn).await.unwrap());
    drop(conn);

    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    assert_eq!(owner.account_id, account_profile::OWNER_ACCOUNT_ID);

    let mut conn = state.db.acquire().await.unwrap();
    assert!(account_profile::vault_is_claimed(&mut conn).await.unwrap());
    let owners: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts WHERE id = $1")
        .bind(account_profile::OWNER_ACCOUNT_ID)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(owners, 1);
}

/// The vault owner holds no messages, so profile setup would ask for a name
/// shown against messages, a zone to read them in, and handles that mark one
/// as theirs: three questions with no answer. The owner is never sent there.
#[tokio::test]
async fn the_vault_owner_owes_no_profile_setup() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;

    let mut conn = state.db.acquire().await.unwrap();
    let auth = account_profile::load_account_auth(&mut conn, owner.account_id)
        .await
        .unwrap()
        .unwrap();
    assert!(!auth.must_set_up_profile);
}
