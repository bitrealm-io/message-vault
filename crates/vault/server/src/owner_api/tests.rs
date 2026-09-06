use axum::http::StatusCode;

use super::*;
use crate::test_support::{
    claim_vault_as_owner, delete_json, delete_status, get_json, get_status, login_status,
    patch_status, post_json, post_status, put_status, register_via_api, seed_one_message,
    test_vault,
};

/// One case per route: an ordinary session gets 403 on every handler, not
/// just the list. The `Owner` extractor makes a missing guard a compile error
/// — a handler cannot take the wrong parameter type unnoticed — so this
/// test's job is the wire behaviour: 403, on every route, through the real
/// HTTP stack, which the type system alone does not promise.
#[tokio::test]
async fn every_owner_route_refuses_an_ordinary_session() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let _owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let ordinary = register_via_api(&state, "bob", "hunter2hunter2").await;
    let target = &ordinary.account_id;

    assert_eq!(
        get_status(&state, "/v1/owner/accounts", &ordinary.token).await,
        StatusCode::FORBIDDEN,
        "GET /v1/owner/accounts"
    );
    assert_eq!(
        post_status(
            &state,
            "/v1/owner/accounts",
            &ordinary.token,
            serde_json::json!({ "username": "carol", "password": "hunter2hunter2" }),
        )
        .await,
        StatusCode::FORBIDDEN,
        "POST /v1/owner/accounts"
    );
    assert_eq!(
        patch_status(
            &state,
            &format!("/v1/owner/accounts/{target}"),
            &ordinary.token,
            serde_json::json!({ "can_export": false }),
        )
        .await,
        StatusCode::FORBIDDEN,
        "PATCH /v1/owner/accounts/{{id}}"
    );
    assert_eq!(
        put_status(
            &state,
            &format!("/v1/owner/accounts/{target}/password"),
            &ordinary.token,
            serde_json::json!({ "password": "irrelevant123" }),
        )
        .await,
        StatusCode::FORBIDDEN,
        "PUT /v1/owner/accounts/{{id}}/password"
    );
    assert_eq!(
        delete_status(
            &state,
            &format!("/v1/owner/accounts/{target}/messages"),
            &ordinary.token,
        )
        .await,
        StatusCode::FORBIDDEN,
        "DELETE /v1/owner/accounts/{{id}}/messages"
    );
    assert_eq!(
        delete_status(
            &state,
            &format!("/v1/owner/accounts/{target}"),
            &ordinary.token
        )
        .await,
        StatusCode::FORBIDDEN,
        "DELETE /v1/owner/accounts/{{id}}"
    );
}

/// No API token resolves to the owner, whichever account issued it. This is
/// what bounds a leaked token: the worst it does is reach message data inside
/// one account's permissions, never the vault's account list.
#[tokio::test]
async fn api_tokens_never_resolve_to_the_owner() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;

    let mut conn = state.db.acquire().await.unwrap();
    let auth = crate::server::resolve_auth_on_conn(&mut conn, &owner.token)
        .await
        .unwrap();
    assert!(auth.is_owner(), "the owner's session is the owner");
    assert!(
        auth.permissions() == crate::db::permissions::Permissions::none(),
        "the owner holds no permissions at all"
    );

    // A token issued on the owner's own account still resolves to a token.
    let token_auth = crate::server::AuthIdentity {
        account_id: auth.account_id.clone(),
        capability: crate::server::AuthCapability::ApiToken(auth.permissions()),
    };
    assert!(!token_auth.is_owner());
    assert!(crate::server::require_owner(&token_auth).is_err());
}

/// The owner is refused by every guard that asks for a permission, so none of
/// the message-data routes is reachable with the owner's session.
#[tokio::test]
async fn the_owner_holds_no_message_permissions() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;

    let mut conn = state.db.acquire().await.unwrap();
    let auth = crate::server::resolve_auth_on_conn(&mut conn, &owner.token)
        .await
        .unwrap();

    assert!(crate::server::require_import_access(&auth).is_err());
    assert!(crate::server::require_export_access(&auth).is_err());
    assert!(crate::server::require_delete_access(&auth).is_err());
    assert!(crate::server::require_import_or_export_access(&auth).is_err());
    assert!(crate::server::require_full_delete_access(&auth).is_err());
    // `FullAccess` means an ordinary account's session; the owner has none.
    assert!(crate::server::require_full_access(&auth).is_err());
    // The two routes a principal points at its own record still admit them.
    assert!(crate::server::require_signed_in(&auth).is_ok());
}

#[tokio::test]
async fn the_owner_sees_every_account_but_no_messages() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let _alice = register_via_api(&state, "alice", "hunter2hunter2").await;
    let _bob = register_via_api(&state, "bob", "hunter2hunter2").await;

    let body: ListAccountsResponse = get_json(&state, "/v1/owner/accounts", &owner.token).await;

    assert_eq!(body.items.len(), 2);
    let bob = body.items.iter().find(|a| a.username == "bob").unwrap();
    assert_eq!(bob.message_count, 0);
    assert!(!bob.disabled);
}

/// The list holds the users of this vault, and the owner is not one of them.
#[tokio::test]
async fn the_owner_is_absent_from_the_account_list() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let _alice = register_via_api(&state, "alice", "hunter2hunter2").await;

    let body: ListAccountsResponse = get_json(&state, "/v1/owner/accounts", &owner.token).await;

    assert_eq!(body.items.len(), 1, "only the one ordinary account");
    assert!(
        !body
            .items
            .iter()
            .any(|a| a.account_id == account_profile::OWNER_ACCOUNT_ID),
        "the owner must not list itself"
    );
    assert!(!body.items.iter().any(|a| a.username == "keeper"));
}

/// The owner's own id is not an account the owner manages, on any route. It
/// reads as absent rather than forbidden, matching how it is absent from the
/// list.
#[tokio::test]
async fn the_owners_own_id_is_not_reachable_through_these_routes() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let own = account_profile::OWNER_ACCOUNT_ID;

    assert_eq!(
        patch_status(
            &state,
            &format!("/v1/owner/accounts/{own}"),
            &owner.token,
            serde_json::json!({ "disabled": true }),
        )
        .await,
        StatusCode::NOT_FOUND,
        "the owner cannot disable itself"
    );
    assert_eq!(
        delete_status(&state, &format!("/v1/owner/accounts/{own}"), &owner.token).await,
        StatusCode::NOT_FOUND,
        "the owner cannot delete itself"
    );
    assert_eq!(
        put_status(
            &state,
            &format!("/v1/owner/accounts/{own}/password"),
            &owner.token,
            serde_json::json!({ "password": "hunter3hunter3" }),
        )
        .await,
        StatusCode::NOT_FOUND,
        "the owner changes its own password through /v1/auth/change-password"
    );

    // And the refusals changed nothing: the owner still signs in.
    assert_eq!(
        login_status(&state, "keeper", "hunter2hunter2").await,
        StatusCode::OK
    );
}

/// Sort and return an object's keys. Panics if `v` is not an object —
/// every wire body this test touches is expected to be one.
fn sorted_keys(v: &serde_json::Value) -> Vec<&str> {
    let mut keys: Vec<&str> = v
        .as_object()
        .unwrap_or_else(|| panic!("expected a JSON object, got {v}"))
        .keys()
        .map(|k| k.as_str())
        .collect();
    keys.sort_unstable();
    keys
}

const MANAGED_ACCOUNT_FIELDS: [&str; 9] = [
    "account_id",
    "can_delete",
    "can_export",
    "can_import",
    "disabled",
    "message_count",
    "must_change_password",
    "storage_bytes",
    "username",
];

#[tokio::test]
async fn list_response_has_no_message_content_fields() {
    // Decode into raw JSON, not the typed `ListAccountsResponse` — serde
    // silently drops unknown fields on decode, so asserting on a
    // re-serialized typed value would only prove the struct's own shape,
    // not what the server actually put on the wire. This reads the wire
    // payload directly.
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let alice = register_via_api(&state, "alice", "hunter2hunter2").await;
    seed_one_message(&state, &alice.account_id).await;

    let body: serde_json::Value = get_json(&state, "/v1/owner/accounts", &owner.token).await;
    let item = &body["items"][0];
    assert_eq!(
        sorted_keys(item),
        MANAGED_ACCOUNT_FIELDS.to_vec(),
        "account rows must carry only metadata, never message content"
    );
}

#[tokio::test]
async fn delete_messages_response_has_no_message_content_fields() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let victim = register_via_api(&state, "bob", "hunter2hunter2").await;
    seed_one_message(&state, &victim.account_id).await;

    let body: serde_json::Value = delete_json(
        &state,
        &format!("/v1/owner/accounts/{}/messages", victim.account_id),
        &owner.token,
    )
    .await;
    assert_eq!(
        sorted_keys(&body),
        vec!["attachments", "conversations"],
        "delete-messages response must carry only counts, never message content"
    );
}

#[tokio::test]
async fn delete_account_is_an_acknowledgement_with_no_body() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let victim = register_via_api(&state, "bob", "hunter2hunter2").await;
    seed_one_message(&state, &victim.account_id).await;

    let status = delete_status(
        &state,
        &format!("/v1/owner/accounts/{}", victim.account_id),
        &owner.token,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

/// The demo account is deleted like any other, which is how a demo vault is
/// cleared into a real one.
#[tokio::test]
async fn the_owner_can_delete_the_demo_account() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let demo = vault
        .account_with_id(account_profile::DEMO_ACCOUNT_ID, "demo")
        .await;

    let status = delete_status(&state, &format!("/v1/owner/accounts/{demo}"), &owner.token).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let body: ListAccountsResponse = get_json(&state, "/v1/owner/accounts", &owner.token).await;
    assert!(body.items.is_empty(), "the demo account is gone");
}

/// The owner picks a first password and the account holder replaces it, so
/// the owner's choice survives one sign-in and no longer.
#[tokio::test]
async fn a_created_account_must_replace_the_password_the_owner_chose() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;

    let created: ManagedAccount = post_json(
        &state,
        "/v1/owner/accounts",
        &owner.token,
        serde_json::json!({ "username": "carol", "password": "hunter2hunter2" }),
    )
    .await;

    assert!(
        created.must_change_password,
        "an owner-created account arrives owing a password change"
    );
    assert!(created.can_import, "and is otherwise an ordinary account");
    assert_eq!(
        login_status(&state, "carol", "hunter2hunter2").await,
        StatusCode::OK,
        "the owner's password signs in once"
    );
}

/// A self-registered account chose its own password, so nothing is owed.
#[tokio::test]
async fn a_self_registered_account_owes_no_password_change() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let _alice = register_via_api(&state, "alice", "hunter2hunter2").await;

    let body: ListAccountsResponse = get_json(&state, "/v1/owner/accounts", &owner.token).await;
    let alice = body.items.iter().find(|a| a.username == "alice").unwrap();
    assert!(!alice.must_change_password);
}

/// The owner names a username and a password and nothing else, so the account
/// arrives with no display name and no handles and its holder sets that up.
/// The vault records it rather than leaving each client to infer it.
#[tokio::test]
async fn an_account_the_owner_creates_owes_profile_setup() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;

    let created: ManagedAccount = post_json(
        &state,
        "/v1/owner/accounts",
        &owner.token,
        serde_json::json!({ "username": "dana", "password": "hunter2hunter2" }),
    )
    .await;

    let mut conn = state.db.acquire().await.unwrap();
    let auth = crate::db::account_profile::load_account_auth(&mut conn, &created.account_id)
        .await
        .unwrap()
        .unwrap();
    assert!(auth.must_set_up_profile);
    assert!(
        auth.must_change_password,
        "both are owed: the password the owner chose, then the profile"
    );
}

#[tokio::test]
async fn changing_the_password_clears_the_forced_change() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;

    let created: ManagedAccount = post_json(
        &state,
        "/v1/owner/accounts",
        &owner.token,
        serde_json::json!({ "username": "carol", "password": "hunter2hunter2" }),
    )
    .await;
    assert!(created.must_change_password);

    let login: serde_json::Value = post_json(
        &state,
        "/v1/auth/login",
        "",
        serde_json::json!({ "username": "carol", "password": "hunter2hunter2" }),
    )
    .await;
    let token = login["token"].as_str().unwrap();

    let _changed: serde_json::Value = post_json(
        &state,
        "/v1/auth/change-password",
        token,
        serde_json::json!({
            "current_password": "hunter2hunter2",
            "new_password": "chosen4herself",
        }),
    )
    .await;

    let body: ListAccountsResponse = get_json(&state, "/v1/owner/accounts", &owner.token).await;
    let carol = body.items.iter().find(|a| a.username == "carol").unwrap();
    assert!(
        !carol.must_change_password,
        "the mark comes off with the password it referred to"
    );
}

#[tokio::test]
async fn setting_a_password_lets_the_new_password_sign_in_and_owes_a_change() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let bob = register_via_api(&state, "bob", "hunter2hunter2").await;

    let status = put_status(
        &state,
        &format!("/v1/owner/accounts/{}/password", bob.account_id),
        &owner.token,
        serde_json::json!({ "password": "resetbytheowner" }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    assert_eq!(
        login_status(&state, "bob", "resetbytheowner").await,
        StatusCode::OK
    );
    assert_eq!(
        login_status(&state, "bob", "hunter2hunter2").await,
        StatusCode::UNAUTHORIZED,
        "the old password is gone"
    );

    let body: ListAccountsResponse = get_json(&state, "/v1/owner/accounts", &owner.token).await;
    let row = body.items.iter().find(|a| a.username == "bob").unwrap();
    assert!(
        row.must_change_password,
        "a password the owner set is one the holder must replace"
    );
}

#[tokio::test]
async fn setting_a_password_invalidates_the_targets_existing_session() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let bob = register_via_api(&state, "bob", "hunter2hunter2").await;

    assert_eq!(
        get_status(&state, "/v1/account/profile", &bob.token).await,
        StatusCode::OK,
        "bob's session works before the reset"
    );

    let status = put_status(
        &state,
        &format!("/v1/owner/accounts/{}/password", bob.account_id),
        &owner.token,
        serde_json::json!({ "password": "resetbytheowner" }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    assert_eq!(
        get_status(&state, "/v1/account/profile", &bob.token).await,
        StatusCode::UNAUTHORIZED,
        "bob's old session is gone"
    );
}

/// Clearing a permission narrows the account, and every token it has already
/// issued narrows with it, because the two are intersected on each request.
#[tokio::test]
async fn clearing_a_permission_takes_effect_on_the_account() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let bob = register_via_api(&state, "bob", "hunter2hunter2").await;

    let status = patch_status(
        &state,
        &format!("/v1/owner/accounts/{}", bob.account_id),
        &owner.token,
        serde_json::json!({ "can_import": false, "can_export": false }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let mut conn = state.db.acquire().await.unwrap();
    let auth = crate::server::resolve_auth_on_conn(&mut conn, &bob.token)
        .await
        .unwrap();
    assert!(!auth.permissions().import);
    assert!(!auth.permissions().export);
    assert!(auth.permissions().delete, "delete was left alone");
}

#[tokio::test]
async fn deleting_one_accounts_messages_leaves_the_others_alone() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let alice = register_via_api(&state, "alice", "hunter2hunter2").await;
    let bob = register_via_api(&state, "bob", "hunter2hunter2").await;
    seed_one_message(&state, &alice.account_id).await;
    seed_one_message(&state, &bob.account_id).await;

    let _body: serde_json::Value = delete_json(
        &state,
        &format!("/v1/owner/accounts/{}/messages", alice.account_id),
        &owner.token,
    )
    .await;

    let body: ListAccountsResponse = get_json(&state, "/v1/owner/accounts", &owner.token).await;
    let alice_row = body.items.iter().find(|a| a.username == "alice").unwrap();
    let bob_row = body.items.iter().find(|a| a.username == "bob").unwrap();
    assert_eq!(alice_row.message_count, 0);
    assert_eq!(bob_row.message_count, 1, "bob's vault is untouched");
}

#[tokio::test]
async fn patch_of_a_missing_account_is_404() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;

    let status = patch_status(
        &state,
        "/v1/owner/accounts/does-not-exist",
        &owner.token,
        serde_json::json!({ "disabled": true }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn setting_a_password_on_a_missing_account_is_404() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;

    let status = put_status(
        &state,
        "/v1/owner/accounts/does-not-exist/password",
        &owner.token,
        serde_json::json!({ "password": "hunter2hunter2" }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn deleting_messages_of_a_missing_account_is_404() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;

    let status = delete_status(
        &state,
        "/v1/owner/accounts/does-not-exist/messages",
        &owner.token,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn deleting_a_missing_account_is_404() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;

    let status = delete_status(&state, "/v1/owner/accounts/does-not-exist", &owner.token).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
