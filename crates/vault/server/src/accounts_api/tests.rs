use axum::http::StatusCode;

use super::*;
use crate::db::api_tokens;
use crate::db::permissions::Permissions;
use crate::problem::ProblemType;
use crate::test_support::{
    SeedConversation, SeedMessage, claim_vault_as_owner, delete_json, delete_json_with_body,
    delete_status, delete_status_with_body, expect_problem, get_json, get_raw, get_status, log_in,
    login_status, patch_failure, patch_json, patch_status, post_created_json, post_status,
    post_status_logged_out, put_json, put_raw, put_status, register_via_api, seed_conversation,
    seed_one_message, test_vault,
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
            serde_json::json!({ "password": "irrelevant123", "password_confirmation": "irrelevant123" }),
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
    for history in ["identities", "imports", "imports/1", "exports"] {
        assert_eq!(
            get_status(
                &state,
                &format!("{}/{history}", member(target)),
                &alice.token
            )
            .await,
            StatusCode::FORBIDDEN,
            "GET /v1/accounts/{{id}}/{history}"
        );
    }

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
    let auth = crate::server::resolve_auth_on_conn(&mut conn, &owner.token, None)
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
    let auth = crate::server::resolve_auth_on_conn(&mut conn, &owner.token, None)
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
    assert!(crate::server::require_logged_in(&auth).is_ok());
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

    let body: crate::paging::Page<AccountResponse> =
        get_json(&state, "/v1/accounts", &owner.token).await;

    assert_eq!(body.items.len(), 3, "the owner, alice and bob");
    let bob = body.items.iter().find(|a| a.username == "bob").unwrap();
    assert_eq!(bob.message_count, 0);
    assert!(!bob.disabled);
}

/// The owner is an account of this vault too: the list opens with it, ahead
/// of a username that would sort before its own.
#[tokio::test]
async fn the_owner_leads_the_account_list() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let _alice = register_via_api(&state, "alice", "hunter2hunter2").await;

    let body: crate::paging::Page<AccountResponse> =
        get_json(&state, "/v1/accounts", &owner.token).await;

    assert_eq!(body.total, 2);
    let usernames: Vec<&str> = body.items.iter().map(|a| a.username.as_str()).collect();
    assert_eq!(usernames, ["keeper", "alice"]);
    assert_eq!(body.items[0].account_id, account_profile::OWNER_ACCOUNT_ID);
    assert!(body.items[0].is_owner);
    assert!(!body.items[1].is_owner);
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
const ACCOUNT_FIELDS: [&str; 18] = [
    "account_id",
    "app",
    "app_version",
    "can_delete",
    "can_export",
    "can_import",
    "disabled",
    "emails",
    "is_demo",
    "is_owner",
    "last_login_at",
    "message_count",
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
/// the owner's choice survives one login and no longer. The owner's
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
        created["must_set_up_profile"], true,
        "an owner-created account owes profile setup, since the owner named nothing but a username"
    );
    assert_eq!(created["can_import"], true, "otherwise an ordinary account");
    assert_eq!(
        login_status(&state, "carol", "hunter2hunter2").await,
        StatusCode::CREATED,
        "the owner's password logs in once"
    );
}

/// A user account has no password length rule, so the owner may create one
/// with a single character or with none, and both log in.
#[tokio::test]
async fn the_owner_creates_accounts_with_a_short_password_or_none() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;

    for body in [
        serde_json::json!({ "username": "carol" }),
        serde_json::json!({ "username": "dave", "password": "a" }),
    ] {
        let status = post_status(&state, "/v1/accounts", &owner.token, body).await;
        assert_eq!(status, StatusCode::CREATED);
    }

    assert_eq!(login_status(&state, "carol", "").await, StatusCode::CREATED);
    assert_eq!(login_status(&state, "dave", "a").await, StatusCode::CREATED);
}

/// A stranger's registration answers the new row and the Session the vault
/// opened on it, so the person is logged in on creation. It never grants
/// anything beyond an ordinary account: the owner is claimed at a fixed id,
/// never promoted from whoever arrived first.
#[tokio::test]
async fn a_stranger_is_logged_in_on_creation_and_never_becomes_the_owner() {
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

    let status = post_status_logged_out(
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

    let status = post_status_logged_out(
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

/// A closed vault admits nobody the owner has not admitted, and a logged-in
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
        post_status_logged_out(&state, "/v1/accounts", body.clone()).await,
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

    // Usernames are compared ignoring case, which the problem page says out
    // loud. A comparison that stopped ignoring it would let "ALICE" register
    // beside "alice" and leave one of the two unreachable at login, because
    // the lookup is the same comparison.
    for taken in ["alice", "ALICE", "Alice"] {
        assert_eq!(
            post_status_logged_out(
                &state,
                "/v1/accounts",
                serde_json::json!({ "username": taken, "password": "otherpassword" }),
            )
            .await,
            StatusCode::CONFLICT,
            "registering {taken} must be refused"
        );
    }

    assert_eq!(
        login_status(&state, "alice", "hunter2hunter2").await,
        StatusCode::CREATED,
        "the original account must still hold the name"
    );
    assert_eq!(
        login_status(&state, "alice", "otherpassword").await,
        StatusCode::UNAUTHORIZED,
        "a refused registration must not have replaced the password"
    );
}

/// Creating an account is the vault's one unauthenticated write, so without a
/// limit it is an offer to fill the disk. The limiter itself is unit-tested in
/// `credentials/tests.rs` and the status mapping in `server/tests.rs`, but
/// nothing put the two together: a route that stopped consulting the limiter
/// passed both.
#[tokio::test]
async fn creating_an_account_is_rate_limited_by_username() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let body =
        |username: &str| serde_json::json!({ "username": username, "password": "hunter2hunter2" });

    for attempt in 0..crate::credentials::AUTH_RATE_MAX {
        let status = post_status_logged_out(&state, "/v1/accounts", body("flood")).await;
        // The first attempt creates the account and the rest collide with it.
        // Either way the attempt counts against the bucket.
        assert!(
            status == StatusCode::CREATED || status == StatusCode::CONFLICT,
            "attempt {attempt} inside the limit answered {status}"
        );
    }

    assert_eq!(
        post_status_logged_out(&state, "/v1/accounts", body("flood")).await,
        StatusCode::TOO_MANY_REQUESTS,
        "the attempt past the limit must be refused"
    );

    // The bucket is named by username, so one client spraying a single name
    // must not shut the vault to everybody else.
    assert_eq!(
        post_status_logged_out(&state, "/v1/accounts", body("bystander")).await,
        StatusCode::CREATED,
        "an unrelated username must still be able to register"
    );
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

/// The flags are the owner's alone. A body naming one is refused whole when
/// the account sends it: nothing in it is applied.
#[tokio::test]
async fn an_account_does_not_set_its_own_flags() {
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

    let row: AccountResponse = get_json(&state, &path, &owner.token).await;
    assert_eq!(row.preferred_name, None, "nothing was applied");
    assert!(row.can_export);
}

/// The owner sets up an account for its holder: a name, a zone and handles,
/// alone or beside a flag. That is not the holder's profile setup, which the
/// account still owes at its first login.
#[tokio::test]
async fn the_owner_sets_a_managed_accounts_profile() {
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
    let path = member(created["account_id"].as_i64().unwrap());

    let patched: serde_json::Value = patch_json(
        &state,
        &path,
        &owner.token,
        serde_json::json!({
            "preferred_name": "Carol",
            "time_zone": "America/New_York",
            "handles": [{ "handle": "Carol@Example.com", "service": "email" }],
            "can_export": false
        }),
    )
    .await;
    assert_eq!(patched["preferred_name"], "Carol");
    assert_eq!(patched["time_zone"], "America/New_York");
    assert_eq!(patched["emails"], serde_json::json!(["carol@example.com"]));
    assert_eq!(patched["can_export"], false);
    assert_eq!(
        patched["must_set_up_profile"], true,
        "the holder's own setup is still owed"
    );

    let patched: serde_json::Value = patch_json(
        &state,
        &path,
        &owner.token,
        serde_json::json!({
            "remove_handles": [{ "handle": "carol@example.com", "service": "email" }]
        }),
    )
    .await;
    assert_eq!(patched["emails"], serde_json::json!([]));
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
    let auth = crate::server::resolve_auth_on_conn(&mut conn, &bob.token, None)
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

    // And the refusals changed nothing: the owner still logs in.
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
            serde_json::json!({ "password": "hunter2hunter2", "password_confirmation": "hunter2hunter2" }),
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
// Last login
// ---------------------------------------------------------------------------

/// Registering and logging in both stamp the account; an account the owner
/// made and nobody has used yet carries no stamp; and the owner setting a
/// password does not count as that account logging in.
#[tokio::test]
async fn last_login_follows_sessions_being_opened() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;

    let alice = register_via_api(&state, "alice", "hunter2hunter2").await;
    let row: AccountResponse = get_json(&state, &member(alice.account_id), &owner.token).await;
    let registered_at = row
        .last_login_at
        .expect("registering opens a session, so it is a login");
    assert!(
        chrono::DateTime::parse_from_rfc3339(&registered_at).is_ok(),
        "RFC 3339: {registered_at}"
    );

    let (_, created): (String, serde_json::Value) = post_created_json(
        &state,
        "/v1/accounts",
        &owner.token,
        serde_json::json!({ "username": "carol", "password": "hunter2hunter2" }),
    )
    .await;
    let carol = created["account_id"].as_i64().unwrap();
    assert!(
        created["last_login_at"].is_null(),
        "an account the owner made has not logged in: {created}"
    );

    let status = put_status(
        &state,
        &format!("{}/password", member(carol)),
        &owner.token,
        serde_json::json!({ "password": "resetbytheowner", "password_confirmation": "resetbytheowner" }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let row: AccountResponse = get_json(&state, &member(carol), &owner.token).await;
    assert!(
        row.last_login_at.is_none(),
        "the owner setting a password is not carol logging in"
    );

    log_in(&state, "carol", "resetbytheowner").await;
    let row: AccountResponse = get_json(&state, &member(carol), &owner.token).await;
    assert!(row.last_login_at.is_some(), "logging in stamps the account");
}

// ---------------------------------------------------------------------------
// Passwords
// ---------------------------------------------------------------------------

/// One route, two callers. The owner sets a password without the current one
/// and answers `204`; the account's session carries on and the new password
/// is simply the account's password from then on.
#[tokio::test]
async fn the_owner_sets_a_password_and_nothing_else_changes() {
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
        serde_json::json!({ "password": "resetbytheowner", "password_confirmation": "resetbytheowner" }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    assert_eq!(
        get_status(&state, &path, &bob.token).await,
        StatusCode::OK,
        "bob's session carries on: the owner's reset changes the password and nothing else"
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
}

/// The account changes its own with its session alone and gets the rotated
/// session token back.
#[tokio::test]
async fn an_account_changes_its_own_password() {
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
    let login = log_in(&state, "carol", "hunter2hunter2").await;
    let token = login["token"].as_str().unwrap();
    let path = format!("{}/password", member(id));

    let changed: SetPasswordResponse = put_json(
        &state,
        &path,
        token,
        serde_json::json!({ "password": "chosen4herself", "password_confirmation": "chosen4herself" }),
    )
    .await;
    assert_ne!(changed.token, token, "the session token rotates");
    assert_eq!(
        get_status(&state, &member(id), &changed.token).await,
        StatusCode::OK
    );

    assert_eq!(
        login_status(&state, "carol", "chosen4herself").await,
        StatusCode::CREATED
    );
}

/// The owner changes its own password on its own row, and must give the one
/// it replaces: without it, or with the wrong one, nothing changes.
#[tokio::test]
async fn the_owner_changes_their_own_password_with_the_current_one() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let path = format!("{}/password", member(owner.account_id));

    assert_eq!(
        put_status(
            &state,
            &path,
            &owner.token,
            serde_json::json!({ "password": "keeperschoice", "password_confirmation": "keeperschoice" }),
        )
        .await,
        StatusCode::UNPROCESSABLE_ENTITY,
        "the session alone does not change the owner's password"
    );
    assert_eq!(
        put_status(
            &state,
            &path,
            &owner.token,
            serde_json::json!({ "password": "keeperschoice", "password_confirmation": "keeperschoice", "current_password": "notthisone" }),
        )
        .await,
        StatusCode::UNAUTHORIZED
    );
    // A refused change leaves the password as it was. Logging in opens a new
    // session, so the change that follows uses its token.
    let login = log_in(&state, "keeper", "hunter2hunter2").await;

    let _changed: SetPasswordResponse = put_json(
        &state,
        &path,
        login["token"].as_str().unwrap(),
        serde_json::json!({ "password": "keeperschoice", "password_confirmation": "keeperschoice", "current_password": "hunter2hunter2" }),
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

/// The checks run in one order, and the first that fails is the only one
/// reported: the current password, then the two new ones agreeing, then the
/// new one differing from the current. Each sentence is one a screen shows
/// as it is.
#[tokio::test]
async fn a_password_change_is_checked_in_a_fixed_order() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let path = format!("{}/password", member(owner.account_id));

    // Wrong current password: the mismatched pair is not mentioned.
    let (status, text) = put_raw(
        &state,
        &path,
        &owner.token,
        "application/json",
        serde_json::json!({
            "password": "one",
            "password_confirmation": "two",
            "current_password": "notthisone",
        })
        .to_string(),
    )
    .await;
    let problem = expect_problem(status, &text, ProblemType::InvalidCredentials);
    assert_eq!(
        problem.detail.as_deref(),
        Some("Current password is incorrect.")
    );

    // Right current password, pair differs.
    let (status, text) = put_raw(
        &state,
        &path,
        &owner.token,
        "application/json",
        serde_json::json!({
            "password": "hunter2hunter2",
            "password_confirmation": "two",
            "current_password": "hunter2hunter2",
        })
        .to_string(),
    )
    .await;
    let problem = expect_problem(status, &text, ProblemType::ValidationFailed);
    assert_eq!(
        problem.errors,
        Some(vec!["New passwords do not match.".to_string()])
    );

    // Pair agrees, but it is the password already in place.
    let (status, text) = put_raw(
        &state,
        &path,
        &owner.token,
        "application/json",
        serde_json::json!({
            "password": "hunter2hunter2",
            "password_confirmation": "hunter2hunter2",
            "current_password": "hunter2hunter2",
        })
        .to_string(),
    )
    .await;
    let problem = expect_problem(status, &text, ProblemType::ValidationFailed);
    assert_eq!(
        problem.errors,
        Some(vec![
            "New password must be different from the current password.".to_string()
        ])
    );

    // Nothing above changed anything.
    assert_eq!(
        login_status(&state, "keeper", "hunter2hunter2").await,
        StatusCode::CREATED
    );

    // A user account sends no current password, but its pair must still agree.
    let bob = register_via_api(&state, "bob", "hunter2hunter2").await;
    let (status, text) = put_raw(
        &state,
        &format!("{}/password", member(bob.account_id)),
        &bob.token,
        "application/json",
        serde_json::json!({ "password": "one", "password_confirmation": "two" }).to_string(),
    )
    .await;
    let problem = expect_problem(status, &text, ProblemType::ValidationFailed);
    assert_eq!(
        problem.errors,
        Some(vec!["New passwords do not match.".to_string()])
    );
}

/// The owner must have a password: one character is enough, none is refused,
/// and the refusal leaves the old password in place.
#[tokio::test]
async fn the_owner_cannot_clear_their_own_password() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let path = format!("{}/password", member(owner.account_id));

    assert_eq!(
        put_status(
            &state,
            &path,
            &owner.token,
            serde_json::json!({
                "password": "",
                "password_confirmation": "",
                "current_password": "hunter2hunter2",
            })
        )
        .await,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    // The refusal stored nothing: the old password still logs in. That opens
    // a new session, so the next change uses its token.
    let login = log_in(&state, "keeper", "hunter2hunter2").await;
    let _changed: SetPasswordResponse = put_json(
        &state,
        &path,
        login["token"].as_str().unwrap(),
        serde_json::json!({ "password": "k", "password_confirmation": "k", "current_password": "hunter2hunter2" }),
    )
    .await;
    assert_eq!(
        login_status(&state, "keeper", "k").await,
        StatusCode::CREATED
    );
}

/// An empty new password clears it. An account clears its own with its
/// current password and can set one again from none; the owner clears a
/// user's the same way it sets one.
#[tokio::test]
async fn a_user_password_can_be_cleared_by_the_account_or_the_owner() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let bob = register_via_api(&state, "bob", "hunter2hunter2").await;
    let path = format!("{}/password", member(bob.account_id));

    let _cleared: SetPasswordResponse = put_json(
        &state,
        &path,
        &bob.token,
        serde_json::json!({ "password": "", "password_confirmation": "" }),
    )
    .await;
    assert_eq!(
        login_status(&state, "bob", "hunter2hunter2").await,
        StatusCode::UNAUTHORIZED,
        "the old password is gone"
    );

    // Logging in opens a new session, so the next change uses its token.
    let login = log_in(&state, "bob", "").await;
    let _set_again: SetPasswordResponse = put_json(
        &state,
        &path,
        login["token"].as_str().unwrap(),
        serde_json::json!({ "password": "b", "password_confirmation": "b" }),
    )
    .await;
    assert_eq!(login_status(&state, "bob", "b").await, StatusCode::CREATED);

    let owner = log_in(&state, "keeper", "hunter2hunter2").await;
    let status = put_status(
        &state,
        &path,
        owner["token"].as_str().unwrap(),
        serde_json::json!({ "password": "", "password_confirmation": "" }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(login_status(&state, "bob", "").await, StatusCode::CREATED);
    assert_eq!(
        login_status(&state, "bob", "b").await,
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

    let body: crate::paging::Page<AccountResponse> =
        get_json(&state, "/v1/accounts", &owner.token).await;
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

    let body: crate::paging::Page<AccountResponse> =
        get_json(&state, "/v1/accounts", &owner.token).await;
    let left: Vec<&str> = body.items.iter().map(|a| a.username.as_str()).collect();
    assert_eq!(
        left,
        ["keeper"],
        "both are gone, and only the owner is left"
    );
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
    let demo_token = log_in(&state, "demo", "").await["token"]
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

/// The identities route lists each of the account's identities with the
/// conversations it takes part in, the first and last message among them, and
/// how many messages sit in the direct and the group ones, trashed
/// conversations and duplicates excluded. The owner reads the same.
#[tokio::test]
async fn the_identities_route_counts_direct_and_group_messages_per_identity() {
    let vault = test_vault().await;
    let owner = claim_vault_as_owner(&vault.state, "keeper", "hunter2hunter2").await;
    let account = register_via_api(&vault.state, "alice", "hunter2hunter2").await;
    let path = format!("{}/identities", member(account.account_id));
    let empty: serde_json::Value = get_json(&vault.state, &path, &account.token).await;
    assert_eq!(
        empty,
        serde_json::json!({ "items": [], "total": 0, "limit": 40, "offset": 0 })
    );

    let _: serde_json::Value = patch_json(
        &vault.state,
        &member(account.account_id),
        &account.token,
        serde_json::json!({
            "handles": [
                { "handle": "+15555550100", "service": "phone" },
                { "handle": "Alice@Example.com", "service": "email" }
            ]
        }),
    )
    .await;

    let two = [
        SeedMessage {
            source: "imessage",
            timestamp: "2020-01-01T00:00:00Z",
            is_from_me: true,
            body: "a",
        },
        SeedMessage {
            source: "imessage",
            timestamp: "2020-01-02T00:00:00Z",
            is_from_me: false,
            body: "b",
        },
    ];
    let direct = seed_conversation(
        &vault.state,
        &SeedConversation {
            account_id: account.account_id,
            handle: "+15555550200",
            conversation_type: "individual",
            group_title: None,
            source_file: "seed.jsonl",
            messages: &two,
        },
    )
    .await;
    let three = [
        SeedMessage {
            source: "imessage",
            timestamp: "2020-02-01T00:00:00Z",
            is_from_me: true,
            body: "c",
        },
        SeedMessage {
            source: "imessage",
            timestamp: "2020-02-02T00:00:00Z",
            is_from_me: false,
            body: "d",
        },
        SeedMessage {
            source: "imessage",
            timestamp: "2020-02-03T00:00:00Z",
            is_from_me: false,
            body: "e",
        },
    ];
    let group = seed_conversation(
        &vault.state,
        &SeedConversation {
            account_id: account.account_id,
            handle: "chat100",
            conversation_type: "group",
            group_title: Some("Trip"),
            source_file: "seed.jsonl",
            messages: &three,
        },
    )
    .await;
    // A third conversation the identity is in, but trashed, so it counts for nothing.
    let trashed = seed_conversation(
        &vault.state,
        &SeedConversation {
            account_id: account.account_id,
            handle: "chat200",
            conversation_type: "group",
            group_title: Some("Old"),
            source_file: "seed.jsonl",
            messages: &three,
        },
    )
    .await;
    let mut conn = vault.conn().await;
    let phone_id: i64 = sqlx::query_scalar(
        "SELECT h.id FROM handles h JOIN account_handles ah ON ah.handle_id = h.id
         WHERE ah.account_id = $1 AND h.handle_type = 'phone'",
    )
    .bind(account.account_id)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    for conversation_id in [direct, group, trashed] {
        sqlx::query("INSERT INTO participants (conversation_id, handle_id) VALUES ($1, $2)")
            .bind(conversation_id)
            .bind(phone_id)
            .execute(&mut *conn)
            .await
            .unwrap();
    }
    sqlx::query(
        "INSERT INTO trashed_conversations (account_id, conversation_id, trashed_at)
         VALUES ($1, $2, '2021-01-01T00:00:00Z')",
    )
    .bind(account.account_id)
    .bind(trashed)
    .execute(&mut *conn)
    .await
    .unwrap();
    drop(conn);

    let page: serde_json::Value = get_json(&vault.state, &path, &account.token).await;
    assert_eq!(page["total"], 2);
    assert_eq!(
        page["items"],
        serde_json::json!([
            {
                "handle": "+15555550100",
                "service": "phone",
                "start_date": "2020-01-01T00:00:00Z",
                "end_date": "2020-02-03T00:00:00Z",
                "conversations": 2,
                "direct_messages": 2,
                "group_messages": 3
            },
            {
                "handle": "alice@example.com",
                "service": "email",
                "start_date": null,
                "end_date": null,
                "conversations": 0,
                "direct_messages": 0,
                "group_messages": 0
            }
        ])
    );
    let by_owner: serde_json::Value = get_json(&vault.state, &path, &owner.token).await;
    assert_eq!(by_owner["items"], page["items"]);
}

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
            "conversation_count": 0,
            "contact_count": 0,
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
    for name in ["Ada", "Pat"] {
        sqlx::query("INSERT INTO contacts (account_id, preferred_name) VALUES ($1, $2)")
            .bind(account.account_id)
            .bind(name)
            .execute(&mut *conn)
            .await
            .unwrap();
    }
    drop(conn);

    let storage: serde_json::Value = get_json(&vault.state, &path, &account.token).await;
    assert_eq!(storage["total_bytes"], 4000);
    assert_eq!(storage["attachment_count"], 3);
    assert_eq!(storage["conversation_count"], 1);
    assert_eq!(storage["contact_count"], 2);
    let top = storage["top_attachments"].as_array().unwrap();
    assert_eq!(top.len(), 2);
    assert_eq!(top[0]["original_name"], "big.mov");
    assert_eq!(top[0]["mime_type"], "video/quicktime");
    assert_eq!(top[0]["size_bytes"], 3000);
    assert_eq!(top[0]["conversation_id"], conversation_id);
    assert_eq!(top[0]["chat_identifier"], "+15555550100");
    assert_eq!(top[1]["original_name"], "small.jpg");
    assert_eq!(top[1]["size_bytes"], 1000);

    // The owner reads the same totals and the same files by name, type and
    // size, and not which conversation a file is in: that says who the
    // account talks to.
    let by_owner: serde_json::Value = get_json(&vault.state, &path, &owner.token).await;
    assert_eq!(by_owner["total_bytes"], storage["total_bytes"]);
    assert_eq!(by_owner["attachment_count"], storage["attachment_count"]);
    assert_eq!(by_owner["conversation_count"], 1);
    assert_eq!(by_owner["contact_count"], 2);
    // Counts and file metadata are the whole of it: no key on the owner's
    // answer names a content column, a contact or a conversation.
    let mut owner_keys: Vec<&str> = by_owner
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    owner_keys.sort_unstable();
    assert_eq!(
        owner_keys,
        [
            "attachment_count",
            "contact_count",
            "conversation_count",
            "top_attachments",
            "total_bytes"
        ]
    );
    let owner_top = by_owner["top_attachments"].as_array().unwrap();
    assert_eq!(owner_top.len(), 2);
    assert_eq!(owner_top[0]["original_name"], "big.mov");
    assert_eq!(owner_top[0]["mime_type"], "video/quicktime");
    assert_eq!(owner_top[0]["size_bytes"], 3000);
    for file in owner_top {
        for held_back in ["conversation_id", "conversation_title", "chat_identifier"] {
            assert!(file.get(held_back).is_none(), "{held_back}: {file}");
        }
    }
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
        true,
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
        true,
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
        true,
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
        true,
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

/// The headers on an ordinary request reach the owner's account list: the
/// owner reads which app each account connects with, and its Build.
#[tokio::test]
async fn the_account_list_shows_the_app_each_account_connects_with() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let bob = register_via_api(&state, "bob", "hunter2hunter2").await;

    let server = crate::test_support::serve(&state).await;
    let status = reqwest::Client::new()
        .get(format!("{}/v1/session", server.base()))
        .bearer_auth(&bob.token)
        .header(crate::server::APP_HEADER, "desktop")
        .header(crate::server::APP_VERSION_HEADER, "0.8.0+1234abcd")
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(status, StatusCode::OK);

    let page: Page<AccountResponse> = get_json(&state, "/v1/accounts", &owner.token).await;
    let row = |name: &str| {
        page.items
            .iter()
            .find(|a| a.username == name)
            .unwrap_or_else(|| panic!("{name} is listed"))
    };
    assert_eq!(
        row("bob").app,
        Some(crate::db::session_tokens::AppKind::Desktop)
    );
    assert_eq!(row("bob").app_version.as_deref(), Some("0.8.0+1234abcd"));
    // The owner's own requests here named no app.
    assert_eq!(row("keeper").app, None);
    assert_eq!(row("keeper").app_version, None);
}

// ---------------------------------------------------------------------------
// Import and export history
// ---------------------------------------------------------------------------

/// An account's import and export history is metadata about it (ADR 0008), so
/// the owner reads what the account reads. The pipelines' own routes still
/// refuse the owner, who holds no import or export permission.
#[tokio::test]
async fn the_owner_and_the_account_read_the_same_import_and_export_history() {
    let vault = test_vault().await;
    let owner = claim_vault_as_owner(&vault.state, "keeper", "hunter2hunter2").await;
    let alice = register_via_api(&vault.state, "alice", "hunter2hunter2").await;
    let base = member(alice.account_id);

    let (_, import): (String, serde_json::Value) = post_created_json(
        &vault.state,
        "/v1/imports",
        &alice.token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    let (_, export): (String, serde_json::Value) = post_created_json(
        &vault.state,
        "/v1/exports",
        &alice.token,
        serde_json::json!({ "scope": { "kind": "everything" }, "tool": "tests" }),
    )
    .await;

    for path in [
        format!("{base}/imports"),
        format!("{base}/imports/{}", import["id"]),
        format!("{base}/exports"),
    ] {
        let by_account: serde_json::Value = get_json(&vault.state, &path, &alice.token).await;
        let by_owner: serde_json::Value = get_json(&vault.state, &path, &owner.token).await;
        assert_eq!(by_owner, by_account, "{path}");
    }

    let imports: serde_json::Value =
        get_json(&vault.state, &format!("{base}/imports"), &owner.token).await;
    assert_eq!(imports["total"], 1);
    assert_eq!(imports["items"][0]["id"], import["id"]);
    let exports: serde_json::Value =
        get_json(&vault.state, &format!("{base}/exports"), &owner.token).await;
    assert_eq!(exports["total"], 1);
    assert_eq!(exports["items"][0]["id"], export["id"]);
    let detail: serde_json::Value = get_json(
        &vault.state,
        &format!("{base}/imports/{}", import["id"]),
        &owner.token,
    )
    .await;
    assert_eq!(detail["source"], "imessage");
    assert_eq!(detail["contacts_new"], 0);

    // The list takes the same `status` filter, and refuses the same unknown value.
    let running: serde_json::Value = get_json(
        &vault.state,
        &format!("{base}/imports?status=completed"),
        &owner.token,
    )
    .await;
    assert_eq!(running["total"], 0);
    assert_eq!(
        get_status(
            &vault.state,
            &format!("{base}/exports?status=backup"),
            &owner.token
        )
        .await,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    for pipeline in ["/v1/imports", "/v1/exports"] {
        assert_eq!(
            get_status(&vault.state, pipeline, &owner.token).await,
            StatusCode::FORBIDDEN,
            "{pipeline} is the pipeline's route and stays closed to the owner"
        );
    }
}

/// An Import Run is read under the account that ran it and nowhere else.
#[tokio::test]
async fn an_import_run_is_a_404_under_another_account() {
    let vault = test_vault().await;
    let owner = claim_vault_as_owner(&vault.state, "keeper", "hunter2hunter2").await;
    let alice = register_via_api(&vault.state, "alice", "hunter2hunter2").await;
    let bob = register_via_api(&vault.state, "bob", "hunter2hunter2").await;
    let (_, import): (String, serde_json::Value) = post_created_json(
        &vault.state,
        "/v1/imports",
        &alice.token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;

    assert_eq!(
        get_status(
            &vault.state,
            &format!("{}/imports/{}", member(bob.account_id), import["id"]),
            &owner.token
        )
        .await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get_status(
            &vault.state,
            &format!("{}/imports", member(9_999)),
            &owner.token
        )
        .await,
        StatusCode::NOT_FOUND,
        "an account that does not exist has no history"
    );
}
