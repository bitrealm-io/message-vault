use super::*;

use axum::http::StatusCode;

use crate::db::trash::{Trashable, move_to_trash};
use crate::test_support::{
    RegisteredAccount, SeedConversation, SeedMessage, TestVault, attach_stored_file, delete_status,
    fake_sha256, get_json, get_status, register_via_api, seed_conversation, test_vault,
};

/// One `imessage` conversation with one message on `handle`, returning its id.
async fn seed(vault: &TestVault, account: &RegisteredAccount, handle: &str) -> i64 {
    seed_conversation(
        &vault.state,
        &SeedConversation {
            account_id: account.account_id,
            handle,
            conversation_type: "individual",
            group_title: None,
            source_file: "seed.jsonl",
            messages: &[SeedMessage {
                source: "imessage",
                timestamp: "2020-01-01T00:00:00Z",
                is_from_me: true,
                body: "hello",
            }],
        },
    )
    .await
}

/// A named contact of `account`, returning its id.
async fn seed_named_contact(vault: &TestVault, account: &RegisteredAccount, name: &str) -> i64 {
    let mut conn = vault.conn().await;
    sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name, origin) VALUES ($1, $2, 'user') RETURNING id",
    )
    .bind(account.account_id)
    .bind(name)
    .fetch_one(&mut *conn)
    .await
    .unwrap()
}

async fn trash(vault: &TestVault, account: &RegisteredAccount, target: Trashable) {
    let mut conn = vault.conn().await;
    assert!(
        move_to_trash(&mut conn, account.account_id, target)
            .await
            .unwrap()
    );
}

/// `total` of the conversation list for `q`, already percent-encoded where
/// it needs to be (`#` would otherwise start a fragment).
async fn conversation_total(vault: &TestVault, token: &str, q: &str) -> u64 {
    let page: serde_json::Value =
        get_json(&vault.state, &format!("/v1/conversations?q={q}"), token).await;
    page["total"].as_u64().unwrap()
}

#[tokio::test]
async fn empty_trash_deletes_trashed_conversations_and_forgets_trashed_contacts() {
    let vault = test_vault().await;
    let alice = register_via_api(&vault.state, "alice", "hunter2hunter2").await;
    let shared = fake_sha256('a');
    let only_in_doomed = fake_sha256('b');

    let doomed = seed(&vault, &alice, "+15550001").await;
    let shared_file = attach_stored_file(&vault.state, alice.account_id, doomed, &shared).await;
    let doomed_file =
        attach_stored_file(&vault.state, alice.account_id, doomed, &only_in_doomed).await;
    let kept = seed(&vault, &alice, "+15550002").await;
    // The kept conversation points at the same stored bytes as `shared`.
    attach_stored_file(&vault.state, alice.account_id, kept, &shared).await;
    let sidecar = doomed_file
        .parent()
        .unwrap()
        .join(format!(".{only_in_doomed}.mime"));

    let trashed_contact = seed_named_contact(&vault, &alice, "Grace").await;
    let kept_contact = seed_named_contact(&vault, &alice, "Ada").await;
    trash(&vault, &alice, Trashable::Conversation(doomed)).await;
    trash(&vault, &alice, Trashable::Contact(trashed_contact)).await;

    let status = delete_status(&vault.state, "/v1/trash", &alice.token).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    assert_eq!(
        conversation_total(&vault, &alice.token, "trashed:yes").await,
        0,
        "nothing is left in the conversation trash"
    );
    let kept_status = get_status(
        &vault.state,
        &format!("/v1/conversations/{kept}"),
        &alice.token,
    )
    .await;
    assert_eq!(
        kept_status,
        StatusCode::OK,
        "the conversation that was not trashed is still there"
    );
    let doomed_status = get_status(
        &vault.state,
        &format!("/v1/conversations/{doomed}"),
        &alice.token,
    )
    .await;
    assert_eq!(
        doomed_status,
        StatusCode::NOT_FOUND,
        "the deleted conversation is gone"
    );

    assert!(
        !doomed_file.exists(),
        "a file only the deleted conversation used is removed"
    );
    assert!(!sidecar.exists(), "its MIME sidecar goes with it");
    assert!(
        shared_file.exists(),
        "a file another conversation still uses stays on disk"
    );

    let contacts: serde_json::Value =
        get_json(&vault.state, "/v1/contacts?q=trashed:yes", &alice.token).await;
    assert_eq!(contacts["total"], 0, "nothing is left in the contact trash");
    let forgotten: serde_json::Value = get_json(
        &vault.state,
        &format!("/v1/contacts/{trashed_contact}"),
        &alice.token,
    )
    .await;
    assert_eq!(
        forgotten["name"], "(unknown)",
        "the trashed contact is Unknown again and can be opened: {forgotten}"
    );
    let untouched: serde_json::Value = get_json(
        &vault.state,
        &format!("/v1/contacts/{kept_contact}"),
        &alice.token,
    )
    .await;
    assert_eq!(untouched["name"], "Ada");
}

#[tokio::test]
async fn empty_trash_leaves_another_accounts_trash_alone() {
    let vault = test_vault().await;
    let alice = register_via_api(&vault.state, "alice", "hunter2hunter2").await;
    let bob = register_via_api(&vault.state, "bob", "hunter2hunter2").await;
    let bobs = seed(&vault, &bob, "+15550001").await;
    trash(&vault, &bob, Trashable::Conversation(bobs)).await;

    let status = delete_status(&vault.state, "/v1/trash", &alice.token).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    assert_eq!(
        conversation_total(&vault, &bob.token, "trashed:yes").await,
        1,
        "Alice emptying her trash must not touch Bob's"
    );
}

#[tokio::test]
async fn empty_trash_needs_the_delete_permission() {
    let vault = test_vault().await;
    let alice = register_via_api(&vault.state, "alice", "hunter2hunter2").await;
    let doomed = seed(&vault, &alice, "+15550001").await;
    trash(&vault, &alice, Trashable::Conversation(doomed)).await;
    {
        let mut conn = vault.conn().await;
        sqlx::query("UPDATE accounts SET can_delete = 0 WHERE id = $1")
            .bind(alice.account_id)
            .execute(&mut *conn)
            .await
            .unwrap();
    }

    let status = delete_status(&vault.state, "/v1/trash", &alice.token).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        conversation_total(&vault, &alice.token, "trashed:yes").await,
        1,
        "the trash is untouched when deleting is not permitted"
    );
}

#[tokio::test]
async fn empty_trash_requires_auth() {
    let vault = test_vault().await;
    let status = delete_status(&vault.state, "/v1/trash", "not-a-token").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[test]
fn a_stored_path_must_stay_under_its_directory() {
    let dir = Path::new("/vault/data/acct/imessage/assets");
    assert_eq!(
        join_under(dir, "ab/abcd.jpg").unwrap(),
        dir.join("ab/abcd.jpg")
    );
    for bad in ["../elsewhere.jpg", "/etc/passwd", "ab/../../x", ""] {
        assert!(join_under(dir, bad).is_err(), "{bad:?} must be refused");
    }
}

#[test]
fn removing_a_file_that_is_already_gone_is_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let present = dir.path().join("present.jpg");
    std::fs::write(&present, b"x").unwrap();

    remove_if_present(&present).unwrap();
    remove_if_present(&dir.path().join("never-existed.jpg")).unwrap();

    assert!(!present.exists());
}
