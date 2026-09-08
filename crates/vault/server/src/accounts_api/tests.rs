use axum::http::StatusCode;

use super::*;
use crate::db::api_tokens;
use crate::db::permissions::Permissions;
use crate::test_support::{
    SeedConversation, SeedMessage, claim_vault_as_owner, delete_json, delete_json_with_body,
    delete_status, delete_status_with_body, get_json, get_raw, get_status, login_status,
    patch_failure, patch_json, patch_status, post_created_json, post_status,
    post_status_signed_out, put_json, put_status, register_via_api, seed_conversation,
    seed_one_message, sign_in, test_vault,
};

fn member(id: i64) -> String {
    format!("/v1/accounts/{id}")
}

// ---------------------------------------------------------------------------
// Who reaches what
// ---------------------------------------------------------------------------

/// The owner reads any row, an account reads its own, and an account
/// addressing another's is refused. The refusal is the same whether the
/// other row exists or not; only the owner learns that an id is absent.
#[tokio::test]
async fn the_owner_reaches_every_row_and_an_account_reaches_its_own() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let alice = register_via_api(&state, "alice", "hunter2hunter2").await;
    let bob = register_via_api(&state, "bob", "hunter2hunter2").await;

    let by_owner: AccountResponse = get_json(&state, &member(bob.account_id), &owner.token).await;
    assert_eq!(by_owner.username, "bob");
    let own: AccountResponse = get_json(&state, &member(bob.account_id), &bob.token).await;
    assert_eq!(own.username, "bob");
    assert_eq!(
        get_status(&state, &member(bob.account_id), &alice.token).await,
        StatusCode::FORBIDDEN,
        "alice does not read bob's row"
    );

    let (status, text) = get_raw(&state, &member(424_242), &alice.token).await;
    crate::test_support::expect_problem(status, &text, crate::problem::ProblemType::NotTheOwner);
    let (status, text) = get_raw(&state, &member(424_242), &owner.token).await;
    crate::test_support::expect_problem(status, &text, crate::problem::ProblemType::NotFound);
}

/// One case per route: an ordinary session is refused on every member route
/// it points at another account, through the real HTTP stack.
#[tokio::test]
async fn every_member_route_refuses_another_accounts_session() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let _owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let alice = register_via_api(&state, "alice", "hunter2hunter2").await;
    let bob = register_via_api(&state, "bob", "hunter2hunter2").await;
    let target = bob.account_id;

    assert_eq!(
        get_status(&state, "/v1/accounts", &alice.token).await,
        StatusCode::FORBIDDEN,
        "GET /v1/accounts"
    );
    assert_eq!(
        get_status(&state, &member(target), &alice.token).await,
        StatusCode::FORBIDDEN,
        "GET /v1/accounts/{{id}}"
    );
    assert_eq!(
        patch_status(
            &state,
            &member(target),
            &alice.token,
            serde_json::json!({ "preferred_name": "Robert" }),
        )
        .await,
        StatusCode::FORBIDDEN,
        "PATCH /v1/accounts/{{id}}"
    );
    assert_eq!(
        delete_status_with_body(
            &state,
            &member(target),
            &alice.token,
            serde_json::json!({ "confirm": true, "current_password": "hunter2hunter2" }),
        )
        .await,
        StatusCode::FORBIDDEN,
        "DELETE /v1/accounts/{{id}}"
    );
    assert_eq!(
        put_status(
            &state,
            &format!("{}/password", member(target)),
            &alice.token,
            serde_json::json!({ "password": "irrelevant123", "current_password": "hunter2hunter2" }),
        )
        .await,
        StatusCode::FORBIDDEN,
        "PUT /v1/accounts/{{id}}/password"
    );
    assert_eq!(
        delete_status_with_body(
            &state,
            &format!("{}/messages", member(target)),
            &alice.token,
            serde_json::json!({ "confirm": true }),
        )
        .await,
        StatusCode::FORBIDDEN,
        "DELETE /v1/accounts/{{id}}/messages"
    );
    assert_eq!(
        get_status(&state, &format!("{}/storage", member(target)), &alice.token).await,
        StatusCode::FORBIDDEN,
        "GET /v1/accounts/{{id}}/storage"
    );

    // And bob is untouched.
    assert_eq!(
        login_status(&state, "bob", "hunter2hunter2").await,
        StatusCode::CREATED
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
        auth.permissions() == Permissions::none(),
        "the owner holds no permissions at all"
    );

    // A token issued on the owner's own account still resolves to a token.
    let token_auth = crate::server::AuthIdentity {
        account_id: auth.account_id,
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
    // The routes under `/v1/accounts/{id}` still admit them, for its own row.
    assert!(crate::server::require_signed_in(&auth).is_ok());
}

// ---------------------------------------------------------------------------
// The list
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_owner_sees_every_account_but_no_messages() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let _alice = register_via_api(&state, "alice", "hunter2hunter2").await;
    let _bob = register_via_api(&state, "bob", "hunter2hunter2").await;

    let body: ListAccountsResponse = get_json(&state, "/v1/accounts", &owner.token).await;

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

    let body: ListAccountsResponse = get_json(&state, "/v1/accounts", &owner.token).await;

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

/// Every field an account row carries, on the list and on the member alike.
const ACCOUNT_FIELDS: [&str; 16] = [
    "account_id",
    "can_delete",
    "can_export",
    "can_import",
    "disabled",
    "emails",
    "is_demo",
    "is_owner",
    "message_count",
    "must_change_password",
    "must_set_up_profile",
    "phones",
    "preferred_name",
    "storage_bytes",
    "time_zone",
    "username",
];

#[tokio::test]
async fn account_rows_carry_no_message_content_fields() {
    // Decode into raw JSON, not the typed response: serde silently drops
    // unknown fields on decode, so asserting on a re-serialized typed value
    // would only prove the struct's own shape, not what the server put on
    // the wire.
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let alice = register_via_api(&state, "alice", "hunter2hunter2").await;
    seed_one_message(&state, alice.account_id).await;

    let body: serde_json::Value = get_json(&state, "/v1/accounts", &owner.token).await;
    assert_eq!(
        sorted_keys(&body["items"][0]),
        ACCOUNT_FIELDS.to_vec(),
        "account rows must carry only metadata, never message content"
    );
    let one: serde_json::Value = get_json(&state, &member(alice.account_id), &alice.token).await;
    assert_eq!(sorted_keys(&one), ACCOUNT_FIELDS.to_vec());
    assert_eq!(one["message_count"], 1);
}

// ---------------------------------------------------------------------------
// Creating an account
// ---------------------------------------------------------------------------

/// The owner picks a first password and the account holder replaces it, so
/// the owner's choice survives one sign-in and no longer. The owner's
/// creation opens no session.
#[tokio::test]
async fn a_created_account_must_replace_the_password_the_owner_chose() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;

    let (location, created): (String, serde_json::Value) = post_created_json(
        &state,
        "/v1/accounts",
        &owner.token,
        serde_json::json!({ "username": "carol", "password": "hunter2hunter2" }),
    )
    .await;
    let id = created["account_id"].as_i64().unwrap();
    assert_eq!(location, member(id));
    assert!(
        created.get("token").is_none(),
        "the owner's creation opens no session: {created}"
    );
    assert_eq!(created["username"], "carol");
    assert_eq!(
        created["must_change_password"], true,
        "an owner-created account arrives owing a password change"
    );
    assert_eq!(
        created["must_set_up_profile"], true,
        "and owes profile setup, since the owner named nothing but a username"
    );
    assert_eq!(created["can_import"], true, "otherwise an ordinary account");
    assert_eq!(
        login_status(&state, "carol", "hunter2hunter2").await,
        StatusCode::CREATED,
        "the owner's password signs in once"
    );
}

/// The owner must give a password: an account with none would sign in with
/// an empty one, and the forced change would have nothing to replace.
#[tokio::test]
async fn the_owner_cannot_create_a_passwordless_account() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;

    let status = post_status(
        &state,
        "/v1/accounts",
        &owner.token,
        serde_json::json!({ "username": "carol" }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

/// A stranger's registration answers the new row and the Session the vault
/// opened on it, so the person is signed in on creation. It never grants
/// anything beyond an ordinary account: the owner is claimed at a fixed id,
/// never promoted from whoever arrived first.
#[tokio::test]
async fn a_stranger_is_signed_in_on_creation_and_never_becomes_the_owner() {
    let vault = test_vault().await;
    let state = vault.state.clone();

    let first = register_via_api(&state, "alice", "hunter2hunter2").await;
    let second = register_via_api(&state, "bob", "hunter2hunter2").await;
    assert_ne!(first.account_id, account_profile::OWNER_ACCOUNT_ID);
    assert_ne!(second.account_id, account_profile::OWNER_ACCOUNT_ID);

    let own: AccountResponse = get_json(&state, &member(first.account_id), &first.token).await;
    assert_eq!(
        own.username, "alice",
        "the token from creation reads the row"
    );
    assert!(
        !own.must_change_password,
        "an account that chose its own password owes no change"
    );
    assert!(
        own.must_set_up_profile,
        "an account registered with no name and no handle still owes setup"
    );

    let mut conn = state.db.acquire().await.unwrap();
    assert!(
        !account_profile::vault_is_claimed(&mut conn).await.unwrap(),
        "registering accounts does not claim the vault"
    );
}

/// A registration that named the account leaves nothing to set up.
#[tokio::test]
async fn a_registration_that_names_the_account_owes_no_profile_setup() {
    let vault = test_vault().await;
    let state = vault.state.clone();

    let status = post_status_signed_out(
        &state,
        "/v1/accounts",
        serde_json::json!({
            "username": "sam",
            "password": "hunter2hunter2",
            "preferred_name": "Sam",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let mut conn = state.db.acquire().await.unwrap();
    let account_id: i64 = sqlx::query_scalar("SELECT id FROM accounts WHERE username = $1")
        .bind("sam")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    let auth = account_profile::load_account_auth(&mut conn, account_id)
        .await
        .unwrap()
        .unwrap();
    assert!(!auth.must_set_up_profile);
}

/// A stranger may register without a password.
#[tokio::test]
async fn a_stranger_may_register_without_a_password() {
    let vault = test_vault().await;
    let state = vault.state.clone();

    let status = post_status_signed_out(
        &state,
        "/v1/accounts",
        serde_json::json!({ "username": "passwordless" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        login_status(&state, "passwordless", "").await,
        StatusCode::CREATED
    );
}

/// A closed vault admits nobody the owner has not admitted, and a signed-in
/// account is not the owner: creating accounts for others is the owner's.
#[tokio::test]
async fn a_closed_vault_and_an_ordinary_session_are_both_refused() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let _owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let alice = register_via_api(&state, "alice", "hunter2hunter2").await;
    {
        let mut conn = state.db.acquire().await.unwrap();
        vault_settings::set_public_registration(&mut conn, false)
            .await
            .unwrap();
    }

    let body = serde_json::json!({ "username": "stranger", "password": "hunter2hunter2" });
    assert_eq!(
        post_status_signed_out(&state, "/v1/accounts", body.clone()).await,
        StatusCode::FORBIDDEN,
        "a stranger on a closed vault"
    );
    assert_eq!(
        post_status(&state, "/v1/accounts", &alice.token, body).await,
        StatusCode::FORBIDDEN,
        "an ordinary session, open or closed"
    );
}

/// The username is the collection's key, taken once.
#[tokio::test]
async fn a_taken_username_is_a_conflict() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let _alice = register_via_api(&state, "alice", "hunter2hunter2").await;

    let status = post_status_signed_out(
        &state,
        "/v1/accounts",
        serde_json::json!({ "username": "alice", "password": "hunter2hunter2" }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

// ---------------------------------------------------------------------------
// Changing an account
// ---------------------------------------------------------------------------

/// The PATCH route is what profile setup saves through: it writes the
/// name, zone and handles and answers with the reloaded account, which
/// the GET route then agrees with.
#[tokio::test]
async fn an_account_patches_its_own_profile_and_reads_it_back() {
    let vault = test_vault().await;
    let account = register_via_api(&vault.state, "alice", "hunter2hunter2").await;
    let path = member(account.account_id);

    let patched: serde_json::Value = patch_json(
        &vault.state,
        &path,
        &account.token,
        serde_json::json!({
            "preferred_name": "Alex",
            "time_zone": "America/New_York",
            "handles": [
                { "handle": "+1 (555) 555-0100", "service": "phone" },
                { "handle": "Alex@Example.com", "service": "email" }
            ]
        }),
    )
    .await;

    assert_eq!(patched["account_id"], account.account_id);
    assert_eq!(patched["username"], "alice");
    assert_eq!(patched["preferred_name"], "Alex");
    assert_eq!(patched["time_zone"], "America/New_York");
    assert_eq!(patched["phones"], serde_json::json!(["+15555550100"]));
    assert_eq!(patched["emails"], serde_json::json!(["alex@example.com"]));
    assert_eq!(patched["must_set_up_profile"], false);
    let read_back: serde_json::Value = get_json(&vault.state, &path, &account.token).await;
    assert_eq!(read_back, patched);
}

#[tokio::test]
async fn patching_with_an_unknown_time_zone_is_a_validation_failure() {
    let vault = test_vault().await;
    let account = register_via_api(&vault.state, "alice", "hunter2hunter2").await;

    let (status, sentence) = patch_failure(
        &vault.state,
        &member(account.account_id),
        &account.token,
        serde_json::json!({ "time_zone": "Mars/Olympus_Mons" }),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        sentence,
        "unknown time zone: Mars/Olympus_Mons; use an IANA name such as America/New_York"
    );
}

/// The flags are the owner's and the profile is the account's. A field the
/// caller may not set is refused whole: nothing in the body is applied.
#[tokio::test]
async fn each_caller_sets_only_its_own_fields() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let bob = register_via_api(&state, "bob", "hunter2hunter2").await;
    let path = member(bob.account_id);

    let (status, sentence) = patch_failure(
        &state,
        &path,
        &bob.token,
        serde_json::json!({ "preferred_name": "Robert", "can_export": false }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "an account does not set its flags"
    );
    assert!(sentence.contains("vault owner"), "{sentence}");

    let (status, sentence) = patch_failure(
        &state,
        &path,
        &owner.token,
        serde_json::json!({ "preferred_name": "Robert", "can_export": false }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "the owner does not set a profile"
    );
    assert!(sentence.contains("its own"), "{sentence}");

    let row: AccountResponse = get_json(&state, &path, &owner.token).await;
    assert_eq!(row.preferred_name, None, "nothing was applied");
    assert!(row.can_export);
}

/// Clearing a permission narrows the account, and every token it has already
/// issued narrows with it, because the two are intersected on each request.
#[tokio::test]
async fn the_owner_clears_a_permission_and_it_takes_effect() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let bob = register_via_api(&state, "bob", "hunter2hunter2").await;

    let row: AccountResponse = patch_json(
        &state,
        &member(bob.account_id),
        &owner.token,
        serde_json::json!({ "can_import": false, "can_export": false }),
    )
    .await;
    assert!(!row.can_import);
    assert!(!row.can_export);

    let mut conn = state.db.acquire().await.unwrap();
    let auth = crate::server::resolve_auth_on_conn(&mut conn, &bob.token)
        .await
        .unwrap();
    assert!(!auth.permissions().import);
    assert!(!auth.permissions().export);
    assert!(auth.permissions().delete, "delete was left alone");
}

/// The owner's own row is the one row its flags do not reach: it holds no
/// messages, so permissions mean nothing, and it cannot lock itself out.
/// Nor can anyone delete it.
#[tokio::test]
async fn the_owners_own_row_cannot_be_disabled_or_deleted() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let own = member(account_profile::OWNER_ACCOUNT_ID);

    let row: AccountResponse = get_json(&state, &own, &owner.token).await;
    assert!(row.is_owner, "the owner reads its own row");

    assert_eq!(
        patch_status(
            &state,
            &own,
            &owner.token,
            serde_json::json!({ "disabled": true })
        )
        .await,
        StatusCode::UNPROCESSABLE_ENTITY,
        "the owner cannot disable itself"
    );
    assert_eq!(
        delete_status_with_body(
            &state,
            &own,
            &owner.token,
            serde_json::json!({ "confirm": true, "current_password": "hunter2hunter2" })
        )
        .await,
        StatusCode::UNPROCESSABLE_ENTITY,
        "the owner cannot delete itself"
    );

    // And the refusals changed nothing: the owner still signs in.
    assert_eq!(
        login_status(&state, "keeper", "hunter2hunter2").await,
        StatusCode::CREATED
    );
}

#[tokio::test]
async fn owner_routes_on_a_missing_account_are_404() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let missing = member(424_242);

    assert_eq!(
        patch_status(
            &state,
            &missing,
            &owner.token,
            serde_json::json!({ "disabled": true })
        )
        .await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        put_status(
            &state,
            &format!("{missing}/password"),
            &owner.token,
            serde_json::json!({ "password": "hunter2hunter2" }),
        )
        .await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        delete_status(&state, &format!("{missing}/messages"), &owner.token).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        delete_status(&state, &missing, &owner.token).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get_status(&state, &format!("{missing}/storage"), &owner.token).await,
        StatusCode::NOT_FOUND
    );
}

// ---------------------------------------------------------------------------
// Passwords
// ---------------------------------------------------------------------------

/// One route, two callers. The owner sets a temporary password without the
/// current one and answers `204`; the account's sessions end, and the new
/// password is one its holder must replace.
#[tokio::test]
async fn the_owner_sets_a_password_that_ends_sessions_and_owes_a_change() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let bob = register_via_api(&state, "bob", "hunter2hunter2").await;
    let path = member(bob.account_id);

    assert_eq!(
        get_status(&state, &path, &bob.token).await,
        StatusCode::OK,
        "bob's session works before the reset"
    );

    let status = put_status(
        &state,
        &format!("{path}/password"),
        &owner.token,
        serde_json::json!({ "password": "resetbytheowner" }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    assert_eq!(
        get_status(&state, &path, &bob.token).await,
        StatusCode::UNAUTHORIZED,
        "bob's old session is gone"
    );
    assert_eq!(
        login_status(&state, "bob", "resetbytheowner").await,
        StatusCode::CREATED
    );
    assert_eq!(
        login_status(&state, "bob", "hunter2hunter2").await,
        StatusCode::UNAUTHORIZED,
        "the old password is gone"
    );
    let row: AccountResponse = get_json(&state, &path, &owner.token).await;
    assert!(
        row.must_change_password,
        "a password the owner set is one the holder must replace"
    );
}

/// The account itself must supply the current password and gets the rotated
/// session token back; the mark the owner set comes off with the password it
/// referred to.
#[tokio::test]
async fn an_account_changes_its_own_password_and_clears_the_forced_change() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;

    let (_, created): (String, serde_json::Value) = post_created_json(
        &state,
        "/v1/accounts",
        &owner.token,
        serde_json::json!({ "username": "carol", "password": "hunter2hunter2" }),
    )
    .await;
    let id = created["account_id"].as_i64().unwrap();
    let login = sign_in(&state, "carol", "hunter2hunter2").await;
    let token = login["token"].as_str().unwrap();
    let path = format!("{}/password", member(id));

    assert_eq!(
        put_status(
            &state,
            &path,
            token,
            serde_json::json!({ "password": "chosen4herself" })
        )
        .await,
        StatusCode::UNPROCESSABLE_ENTITY,
        "an account must give its current password"
    );
    assert_eq!(
        put_status(
            &state,
            &path,
            token,
            serde_json::json!({ "password": "chosen4herself", "current_password": "wrong-one" })
        )
        .await,
        StatusCode::UNAUTHORIZED
    );

    let changed: SetPasswordResponse = put_json(
        &state,
        &path,
        token,
        serde_json::json!({
            "current_password": "hunter2hunter2",
            "password": "chosen4herself",
        }),
    )
    .await;
    assert_ne!(changed.token, token, "the session token rotates");
    assert_eq!(
        get_status(&state, &member(id), &changed.token).await,
        StatusCode::OK
    );

    let row: AccountResponse = get_json(&state, &member(id), &owner.token).await;
    assert!(
        !row.must_change_password,
        "the mark comes off with the password it referred to"
    );
    assert_eq!(
        login_status(&state, "carol", "chosen4herself").await,
        StatusCode::CREATED
    );
}

/// The owner changes its own password the way every account does: with the
/// current one, on its own row.
#[tokio::test]
async fn the_owner_can_change_their_own_password() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;

    let _changed: SetPasswordResponse = put_json(
        &state,
        &format!("{}/password", member(owner.account_id)),
        &owner.token,
        serde_json::json!({
            "current_password": "hunter2hunter2",
            "password": "keeperschoice",
        }),
    )
    .await;

    assert_eq!(
        login_status(&state, "keeper", "keeperschoice").await,
        StatusCode::CREATED
    );
    assert_eq!(
        login_status(&state, "keeper", "hunter2hunter2").await,
        StatusCode::UNAUTHORIZED
    );
}

// ---------------------------------------------------------------------------
// Deleting messages and accounts
// ---------------------------------------------------------------------------

#[tokio::test]
async fn deleting_one_accounts_messages_leaves_the_others_alone() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let alice = register_via_api(&state, "alice", "hunter2hunter2").await;
    let bob = register_via_api(&state, "bob", "hunter2hunter2").await;
    seed_one_message(&state, alice.account_id).await;
    seed_one_message(&state, bob.account_id).await;

    let body: serde_json::Value = delete_json(
        &state,
        &format!("{}/messages", member(alice.account_id)),
        &owner.token,
    )
    .await;
    assert_eq!(
        sorted_keys(&body),
        vec!["attachments", "conversations"],
        "the answer carries counts, never message content"
    );

    let body: ListAccountsResponse = get_json(&state, "/v1/accounts", &owner.token).await;
    let alice_row = body.items.iter().find(|a| a.username == "alice").unwrap();
    let bob_row = body.items.iter().find(|a| a.username == "bob").unwrap();
    assert_eq!(alice_row.message_count, 0);
    assert_eq!(bob_row.message_count, 1, "bob's vault is untouched");
}

/// An account deletes its own messages with the `delete` scope and a
/// confirmation; without the scope it is refused.
#[tokio::test]
async fn deleting_own_messages_needs_the_delete_permission_and_a_confirmation() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let alice = register_via_api(&state, "alice", "hunter2hunter2").await;
    let path = format!("{}/messages", member(alice.account_id));
    seed_one_message(&state, alice.account_id).await;

    assert_eq!(
        delete_status(&state, &path, &alice.token).await,
        StatusCode::UNPROCESSABLE_ENTITY,
        "no body, no confirmation"
    );
    let body: DeleteMessagesResponse = delete_json_with_body(
        &state,
        &path,
        &alice.token,
        serde_json::json!({ "confirm": true }),
    )
    .await;
    assert_eq!(body.conversations, 1);

    let mut conn = state.db.acquire().await.unwrap();
    sqlx::query("UPDATE accounts SET can_delete = 0 WHERE id = $1")
        .bind(alice.account_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    drop(conn);
    assert_eq!(
        delete_status_with_body(
            &state,
            &path,
            &alice.token,
            serde_json::json!({ "confirm": true })
        )
        .await,
        StatusCode::FORBIDDEN
    );
}

/// A token with the delete scope may destroy its account's messages, and may
/// not close the account: that stays a person's act, session only.
#[tokio::test]
async fn a_token_with_delete_may_delete_messages_but_may_not_close_the_account() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let created = register_via_api(&state, "alice", "hunter2hunter2").await;
    let mut conn = state.db.acquire().await.unwrap();
    let token = api_tokens::create_api_token(
        &mut conn,
        created.account_id,
        "tool",
        Permissions::all(),
        None,
    )
    .await
    .unwrap()
    .token;
    drop(conn);

    let deleted = delete_status_with_body(
        &state,
        &format!("{}/messages", member(created.account_id)),
        &token,
        serde_json::json!({ "confirm": true }),
    )
    .await;
    assert_eq!(deleted, StatusCode::OK);

    let closed = delete_status_with_body(
        &state,
        &member(created.account_id),
        &token,
        serde_json::json!({ "confirm": true, "current_password": "hunter2hunter2" }),
    )
    .await;
    assert_eq!(
        closed,
        StatusCode::FORBIDDEN,
        "closing the account stays session-only"
    );
}

/// The owner deletes an account outright, with no body; the demo account is
/// deleted like any other, which is how a demo vault is cleared into a real
/// one.
#[tokio::test]
async fn the_owner_deletes_any_account_outright() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let victim = register_via_api(&state, "bob", "hunter2hunter2").await;
    seed_one_message(&state, victim.account_id).await;
    let demo = vault
        .account_with_id(account_profile::DEMO_ACCOUNT_ID, "demo")
        .await;

    assert_eq!(
        delete_status(&state, &member(victim.account_id), &owner.token).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        delete_status(&state, &member(demo), &owner.token).await,
        StatusCode::NO_CONTENT
    );

    let body: ListAccountsResponse = get_json(&state, "/v1/accounts", &owner.token).await;
    assert!(body.items.is_empty(), "both are gone");
    assert_eq!(
        login_status(&state, "bob", "hunter2hunter2").await,
        StatusCode::UNAUTHORIZED
    );
}

/// An account deletes itself with its confirmation and its current password
/// in the body, and nothing stands above it to refuse; the demo account is
/// the one that refuses its own.
#[tokio::test]
async fn an_account_deletes_itself_with_its_password_and_the_demo_account_refuses() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let alice = register_via_api(&state, "alice", "hunter2hunter2").await;
    let _bob = register_via_api(&state, "bob", "hunter2hunter2").await;
    let path = member(alice.account_id);

    assert_eq!(
        delete_status(&state, &path, &alice.token).await,
        StatusCode::BAD_REQUEST,
        "no body: nothing confirmed and no password"
    );
    assert_eq!(
        delete_status_with_body(
            &state,
            &path,
            &alice.token,
            serde_json::json!({ "confirm": true, "current_password": "not-it" }),
        )
        .await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        delete_status_with_body(
            &state,
            &path,
            &alice.token,
            serde_json::json!({ "confirm": true, "current_password": "hunter2hunter2" }),
        )
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        login_status(&state, "alice", "hunter2hunter2").await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login_status(&state, "bob", "hunter2hunter2").await,
        StatusCode::CREATED,
        "the other account is untouched"
    );

    let demo = vault
        .account_with_id(account_profile::DEMO_ACCOUNT_ID, "demo")
        .await;
    let demo_token = sign_in(&state, "demo", "").await["token"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, text) = crate::test_support::delete_raw_with_body(
        &state,
        &member(demo),
        &demo_token,
        serde_json::json!({ "confirm": true }),
    )
    .await;
    crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::DemoAccountProtected,
    );
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// The storage route sums every attachment row's size and lists the
/// largest ones; a row with no recorded size counts, but is not one of
/// the largest. The owner reads the same numbers.
#[tokio::test]
async fn the_storage_route_sums_attachment_bytes_and_lists_the_largest_first() {
    let vault = test_vault().await;
    let owner = claim_vault_as_owner(&vault.state, "keeper", "hunter2hunter2").await;
    let account = register_via_api(&vault.state, "alice", "hunter2hunter2").await;
    let path = format!("{}/storage", member(account.account_id));
    let empty: serde_json::Value = get_json(&vault.state, &path, &account.token).await;
    assert_eq!(
        empty,
        serde_json::json!({
            "total_bytes": 0,
            "attachment_count": 0,
            "top_attachments": []
        })
    );
    let conversation_id = seed_conversation(
        &vault.state,
        &SeedConversation {
            account_id: account.account_id,
            handle: "+15555550100",
            conversation_type: "individual",
            group_title: None,
            source_file: "seed.jsonl",
            messages: &[SeedMessage {
                source: "imessage",
                timestamp: "2020-01-01T00:00:00Z",
                is_from_me: true,
                body: "photos",
            }],
        },
    )
    .await;
    let mut conn = vault.conn().await;
    let message_id: i64 = sqlx::query_scalar("SELECT id FROM messages WHERE conversation_id = $1")
        .bind(conversation_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    for (name, mime, size) in [
        ("big.mov", "video/quicktime", Some(3000_i64)),
        ("small.jpg", "image/jpeg", Some(1000_i64)),
        ("unsized.bin", "application/octet-stream", None),
    ] {
        sqlx::query(
            "INSERT INTO attachments (message_id, original_name, mime_type, size_bytes)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(message_id)
        .bind(name)
        .bind(mime)
        .bind(size)
        .execute(&mut *conn)
        .await
        .unwrap();
    }
    drop(conn);

    let storage: serde_json::Value = get_json(&vault.state, &path, &account.token).await;
    assert_eq!(storage["total_bytes"], 4000);
    assert_eq!(storage["attachment_count"], 3);
    let top = storage["top_attachments"].as_array().unwrap();
    assert_eq!(top.len(), 2);
    assert_eq!(top[0]["original_name"], "big.mov");
    assert_eq!(top[0]["mime_type"], "video/quicktime");
    assert_eq!(top[0]["size_bytes"], 3000);
    assert_eq!(top[0]["conversation_id"], conversation_id);
    assert_eq!(top[0]["chat_identifier"], "+15555550100");
    assert_eq!(top[1]["original_name"], "small.jpg");
    assert_eq!(top[1]["size_bytes"], 1000);

    let by_owner: serde_json::Value = get_json(&vault.state, &path, &owner.token).await;
    assert_eq!(by_owner, storage);
}

// ---------------------------------------------------------------------------
// The profile update on a connection
// ---------------------------------------------------------------------------

#[tokio::test]
async fn apply_profile_update_sets_name_and_handles() {
    let vault = test_vault().await;
    let account_id = vault.account_with_id(101, "alice").await;
    let mut conn = vault.conn().await;
    apply_profile_update(
        &mut conn,
        account_id,
        Some("Alex"),
        None,
        &[
            ProfileHandleInput {
                handle: "+1 (555) 555-0100".into(),
                service: "phone".into(),
            },
            ProfileHandleInput {
                handle: "Alex@Example.com".into(),
                service: "email".into(),
            },
            ProfileHandleInput {
                handle: "+15555550199".into(),
                service: "whatsapp".into(),
            },
        ],
        &[],
    )
    .await
    .unwrap();

    let loaded = require_account(&mut conn, account_id).await.unwrap();
    assert_eq!(loaded.preferred_name.as_deref(), Some("Alex"));
    assert!(loaded.phones.iter().any(|p| p == "+15555550100"));
    assert!(loaded.phones.iter().any(|p| p == "+15555550199"));
    assert!(loaded.emails.iter().any(|e| e == "alex@example.com"));

    let wa_service: String =
        sqlx::query_scalar("SELECT service FROM handles WHERE account_id = $1 AND normalized = $2")
            .bind(account_id)
            .bind("+15555550199")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(wa_service, "whatsapp");
}

/// Saving a profile is what profile setup is, so the vault stops asking
/// for one. The flag is the answer every client reads, so it has to move
/// when the fact behind it does. An empty-looking profile is not by itself
/// setup owed: the flag says what is owed.
#[tokio::test]
async fn saving_a_profile_clears_the_setup_owed_flag() {
    let vault = test_vault().await;
    let account_id = vault.account_with_id(101, "alice").await;
    let mut conn = vault.conn().await;
    let bare = require_account(&mut conn, account_id).await.unwrap();
    assert_eq!(bare.preferred_name, None);
    assert!(bare.phones.is_empty());
    assert!(
        !bare.must_set_up_profile,
        "an empty profile owes nothing by itself"
    );

    account_profile::set_must_set_up_profile(&mut conn, account_id, true)
        .await
        .unwrap();
    assert!(
        require_account(&mut conn, account_id)
            .await
            .unwrap()
            .must_set_up_profile
    );

    update_profile_on_conn(
        &mut conn,
        account_id,
        &PatchAccountRequest {
            preferred_name: Some("Alex".into()),
            ..PatchAccountRequest::default()
        },
    )
    .await
    .unwrap();

    let auth = account_profile::load_account_auth(&mut conn, account_id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !auth.must_set_up_profile,
        "the cleared flag is written, not just reported"
    );
}

#[tokio::test]
async fn apply_profile_update_removes_handles() {
    let vault = test_vault().await;
    let account_id = vault.account_with_id(101, "alice").await;
    let mut conn = vault.conn().await;
    let both = [
        ProfileHandleInput {
            handle: "+15555550100".into(),
            service: "phone".into(),
        },
        ProfileHandleInput {
            handle: "alex@example.com".into(),
            service: "email".into(),
        },
    ];
    apply_profile_update(&mut conn, account_id, None, None, &both, &[])
        .await
        .unwrap();
    apply_profile_update(&mut conn, account_id, None, None, &[], &both)
        .await
        .unwrap();

    let loaded = require_account(&mut conn, account_id).await.unwrap();
    assert!(loaded.phones.is_empty());
    assert!(loaded.emails.is_empty());
}

#[tokio::test]
async fn profile_update_rolls_back_when_a_handle_service_is_unsupported() {
    let vault = test_vault().await;
    let account_id = vault.account_with_id(101, "alice").await;
    let mut conn = vault.conn().await;

    let result = update_profile_on_conn(
        &mut conn,
        account_id,
        &PatchAccountRequest {
            preferred_name: Some("Changed Name".into()),
            handles: vec![ProfileHandleInput {
                handle: "alice@example.com".into(),
                service: "unsupported".into(),
            }],
            ..PatchAccountRequest::default()
        },
    )
    .await;

    assert!(result.is_err());
    assert_eq!(
        account_profile::load_preferred_name(&mut conn, account_id)
            .await
            .unwrap(),
        None
    );
}

/// The zone is chosen at profile setup and read back on the account; an
/// unknown name is refused before anything is written.
#[tokio::test]
async fn the_account_carries_a_time_zone_and_refuses_an_unknown_one() {
    let vault = test_vault().await;
    let account = register_via_api(&vault.state, "alice", "hunter2hunter2").await;
    let mut conn = vault.conn().await;
    let before = require_account(&mut conn, account.account_id)
        .await
        .unwrap();
    assert_eq!(before.time_zone, "UTC", "a new account starts in UTC");

    update_profile_on_conn(
        &mut conn,
        account.account_id,
        &PatchAccountRequest {
            time_zone: Some("America/New_York".into()),
            ..PatchAccountRequest::default()
        },
    )
    .await
    .unwrap();
    let after = require_account(&mut conn, account.account_id)
        .await
        .unwrap();
    assert_eq!(after.time_zone, "America/New_York");

    let err = update_profile_on_conn(
        &mut conn,
        account.account_id,
        &PatchAccountRequest {
            time_zone: Some("Mars/Olympus_Mons".into()),
            ..PatchAccountRequest::default()
        },
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, ProfileUpdateError::UnknownTimeZone(_)),
        "{err}"
    );
    assert!(matches!(ApiError::from(err), ApiError::ValidationFailed(_)));
    let unchanged = require_account(&mut conn, account.account_id)
        .await
        .unwrap();
    assert_eq!(unchanged.time_zone, "America/New_York");
}
