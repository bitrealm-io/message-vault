use axum::http::StatusCode;

use super::*;
use crate::test_support::{
    SeedConversation, SeedMessage, claim_vault_as_owner, get_json, get_status, patch_status,
    post_json, post_status, post_status_logged_out, register_via_api, seed_conversation,
    test_vault,
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

    let body: Vault = get_json(&state, "/v1/vault", "").await;
    assert_eq!(body.state, VaultState::Unclaimed);
}

/// Unclaimed wins over the registration setting: a vault with no owner has
/// one thing to offer, and joining it is not that thing.
#[tokio::test]
async fn public_registration_does_not_make_an_unowned_vault_open() {
    let vault = test_vault().await;
    let state = vault.state.clone();

    let body: Vault = get_json(&state, "/v1/vault", "").await;
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

    let body: Vault = get_json(&state, "/v1/vault", "").await;
    assert_eq!(body.state, VaultState::Closed);
}

#[tokio::test]
async fn a_claimed_vault_with_registration_on_is_open() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let _owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;

    let body: Vault = get_json(&state, "/v1/vault", "").await;
    assert_eq!(body.state, VaultState::Open);
}

/// The route reports the vault's state to anyone, logged in or not. The
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

/// The vault says which code it runs and which schema it carries to anyone:
/// an app has to read both before anybody is logged in, and neither is secret.
#[tokio::test]
async fn the_state_route_carries_the_build_and_the_schema_fingerprint() {
    let vault = test_vault().await;
    let state = vault.state.clone();

    let body: Vault = get_json(&state, "/v1/vault", "").await;

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
    let auth = crate::server::resolve_auth_on_conn(&mut conn, token, None)
        .await
        .unwrap();
    assert!(auth.is_owner());
    drop(conn);

    let after: Vault = get_json(&state, "/v1/vault", "").await;
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
async fn claiming_needs_a_password_of_one_character_or_more() {
    let vault = test_vault().await;
    let state = vault.state.clone();

    let status = post_status(
        &state,
        "/v1/vault/claim",
        "",
        serde_json::json!({ "username": "keeper", "password": "" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "the vault owner must have a password"
    );
    let after: Vault = get_json(&state, "/v1/vault", "").await;
    assert_eq!(after.state, VaultState::Unclaimed);

    let status = post_status(
        &state,
        "/v1/vault/claim",
        "",
        serde_json::json!({ "username": "keeper", "password": "k" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "one character is enough");
}

#[tokio::test]
async fn registration_is_refused_while_the_vault_is_closed() {
    let vault = test_vault().await;
    let state = vault.state.clone();
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
    let vault = test_vault().await;
    let state = vault.state.clone();
    close_registration(&state).await;
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;

    let settings: VaultSettings = get_json(&state, "/v1/vault/settings", &owner.token).await;
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

    let opened: VaultSettings = crate::test_support::patch_json(
        &state,
        "/v1/vault/settings",
        &owner.token,
        serde_json::json!({ "public_registration": true }),
    )
    .await;
    assert!(opened.public_registration);

    let joined = register_via_api(&state, "stranger", "hunter2hunter2").await;
    assert_eq!(joined.username, "stranger");

    let body: Vault = get_json(&state, "/v1/vault", "").await;
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

// ---------------------------------------------------------------------------
// What the vault holds
// ---------------------------------------------------------------------------

/// The vault's totals sum every account, and the answer is counts and byte
/// totals and nothing that names a person or a conversation. The database
/// figures are measured, so they are only checked for sign; the split of
/// message storage across accounts is checked exactly, because it is arithmetic
/// over the measured total.
#[tokio::test]
async fn the_owner_reads_the_vault_totals_summed_over_every_account() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let alice = register_via_api(&state, "alice", "hunter2hunter2").await;
    let bob = register_via_api(&state, "bob", "hunter2hunter2").await;

    let empty: VaultStorage = get_json(&state, "/v1/vault/storage", &owner.token).await;
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
        (bob.account_id, "+15555550200", &["yo"][..]),
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
    let mut conn = vault.conn().await;
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

    let totals: VaultStorage = get_json(&state, "/v1/vault/storage", &owner.token).await;
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
    assert!(
        totals.messages_bytes > 0,
        "messages_bytes {}",
        totals.messages_bytes
    );
    assert!(totals.fts_bytes > 0, "fts_bytes {}", totals.fts_bytes);
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

/// An account holds only its own data, so the vault's totals are the owner's
/// alone; a session that is not the owner's is refused, and no session is
/// unauthorized.
#[tokio::test]
async fn only_the_owner_reaches_the_vault_totals() {
    let vault = test_vault().await;
    let state = vault.state.clone();
    let _owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
    let ordinary = register_via_api(&state, "bob", "hunter2hunter2").await;

    assert_eq!(
        get_status(&state, "/v1/vault/storage", &ordinary.token).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        get_status(&state, "/v1/vault/storage", "").await,
        StatusCode::UNAUTHORIZED
    );
}
