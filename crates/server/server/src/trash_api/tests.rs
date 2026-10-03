use axum::http::StatusCode;

use crate::db::trash::{Trashable, move_to_trash};
use crate::test_support::{
    RegisteredAccount, SeedConversation, SeedMessage, TestFixture, attach_stored_file,
    delete_status, fake_sha256, fixture_with_account, get_json, get_status, register_via_api,
    seed_conversation,
};

/// One `imessage` conversation with one message on `handle`, returning its id.
async fn seed(fixture: &TestFixture, account: &RegisteredAccount, handle: &str) -> i64 {
    seed_conversation(
        &fixture.state,
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
async fn seed_named_contact(fixture: &TestFixture, account: &RegisteredAccount, name: &str) -> i64 {
    let mut conn = fixture.conn().await;
    sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name, origin) VALUES ($1, $2, 'user') RETURNING id",
    )
    .bind(account.account_id)
    .bind(name)
    .fetch_one(&mut *conn)
    .await
    .unwrap()
}

async fn trash(fixture: &TestFixture, account: &RegisteredAccount, target: Trashable) {
    let mut conn = fixture.conn().await;
    assert!(
        move_to_trash(&mut conn, account.account_id, target)
            .await
            .unwrap()
    );
}

/// `total` of the conversation list for `q`, already percent-encoded where
/// it needs to be (`#` would otherwise start a fragment).
async fn conversation_total(fixture: &TestFixture, token: &str, q: &str) -> u64 {
    let page: serde_json::Value =
        get_json(&fixture.state, &format!("/v1/conversations?q={q}"), token).await;
    page["total"].as_u64().unwrap()
}

#[tokio::test]
async fn empty_trash_deletes_trashed_conversations_and_forgets_trashed_contacts() {
    let (fixture, alice) = fixture_with_account().await;
    let shared = fake_sha256('a');
    let only_in_doomed = fake_sha256('b');

    let doomed = seed(&fixture, &alice, "+15550001").await;
    let shared_file = attach_stored_file(&fixture.state, alice.account_id, doomed, &shared).await;
    let doomed_file =
        attach_stored_file(&fixture.state, alice.account_id, doomed, &only_in_doomed).await;
    let kept = seed(&fixture, &alice, "+15550002").await;
    // The kept conversation points at the same stored bytes as `shared`.
    attach_stored_file(&fixture.state, alice.account_id, kept, &shared).await;
    let sidecar = doomed_file
        .parent()
        .unwrap()
        .join(format!(".{only_in_doomed}.mime"));

    let trashed_contact = seed_named_contact(&fixture, &alice, "Grace").await;
    let kept_contact = seed_named_contact(&fixture, &alice, "Ada").await;
    trash(&fixture, &alice, Trashable::Conversation(doomed)).await;
    trash(&fixture, &alice, Trashable::Contact(trashed_contact)).await;

    let status = delete_status(&fixture.state, "/v1/trash", &alice.token).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    assert_eq!(
        conversation_total(&fixture, &alice.token, "trashed:yes").await,
        0,
        "nothing is left in the conversation trash"
    );
    let kept_status = get_status(
        &fixture.state,
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
        &fixture.state,
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
        get_json(&fixture.state, "/v1/contacts?q=trashed:yes", &alice.token).await;
    assert_eq!(contacts["total"], 0, "nothing is left in the contact trash");
    let forgotten: serde_json::Value = get_json(
        &fixture.state,
        &format!("/v1/contacts/{trashed_contact}"),
        &alice.token,
    )
    .await;
    assert_eq!(
        forgotten["unknown"], true,
        "the trashed contact is Unknown again and can be opened: {forgotten}"
    );
    let untouched: serde_json::Value = get_json(
        &fixture.state,
        &format!("/v1/contacts/{kept_contact}"),
        &alice.token,
    )
    .await;
    assert_eq!(untouched["name"], "Ada");
}

#[tokio::test]
async fn empty_trash_leaves_another_accounts_trash_alone() {
    let (fixture, alice) = fixture_with_account().await;
    let bob = register_via_api(&fixture.state, "bob", "hunter2hunter2").await;
    let bobs = seed(&fixture, &bob, "+15550001").await;
    trash(&fixture, &bob, Trashable::Conversation(bobs)).await;

    let status = delete_status(&fixture.state, "/v1/trash", &alice.token).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    assert_eq!(
        conversation_total(&fixture, &bob.token, "trashed:yes").await,
        1,
        "Alice emptying her trash must not touch Bob's"
    );
}

#[tokio::test]
async fn empty_trash_needs_the_delete_permission() {
    let (fixture, alice) = fixture_with_account().await;
    let doomed = seed(&fixture, &alice, "+15550001").await;
    trash(&fixture, &alice, Trashable::Conversation(doomed)).await;
    fixture.turn_off_delete(alice.account_id).await;

    let status = delete_status(&fixture.state, "/v1/trash", &alice.token).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        conversation_total(&fixture, &alice.token, "trashed:yes").await,
        1,
        "the trash is untouched when deleting is not permitted"
    );
}

/// Start an Import Run for `imessage`, as an Upload does before it asks
/// about any file, and return its id.
async fn start_run(fixture: &TestFixture, account: &RegisteredAccount) -> i64 {
    let (_, run): (String, serde_json::Value) = crate::test_support::post_created_json(
        &fixture.state,
        "/v1/imports",
        &account.token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    run["id"].as_i64().unwrap()
}

/// One batch of one incoming message whose attachment is the file `sha`.
fn batch_naming(sha: &str) -> String {
    let message = format!(
        r#"{{"guid":"g-new","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"imessage","message_kind":"imessage","sender_handle":"+15555550123","sender_display_name":null,"subject":null,"text":"new","attachments":[{{"path":"attachments/photo.bin","original_name":"photo.bin","mime_type":"application/octet-stream","digest_sha256":"{sha}","is_sticker":false,"transcription":null,"sticker_effect":null}}],"imessage":null,"source":null}}"#
    );
    format!(
        "{}\n{message}\n",
        r#"{"schema_version":4,"export":{"source":"imessage","tool":"test","tool_version":"0","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+15555550123","conversation_type":"individual","group_title":null,"participants":[{"handle":"+15555550123","display_name":null}],"stats":{"message_count":1,"attachment_count":1,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183462000}}}"#,
    )
}

/// S5-2: an Upload told by `HEAD` that the server holds a file skips the
/// upload. Emptying the Trash before the batch that names the file arrives
/// must not leave the imported attachment without its file.
///
/// The run starts before `HEAD`, because that is the order an Upload sends:
/// `message-crate-push` (`crates/libs/push/src/run.rs`) starts the run
/// before any asset request.
#[tokio::test]
async fn a_file_head_reported_present_survives_an_empty_trash_before_the_batch() {
    let (fixture, alice) = fixture_with_account().await;
    let bytes = b"shared photo bytes";
    let sha = crate::assets_api::Sha256::of_bytes(bytes);
    let blob = fixture
        .state
        .cfg
        .paths
        .assets_dir_for_account(alice.account_id)
        .join(crate::assets_api::shard_rel_path(&sha, ""));
    std::fs::create_dir_all(blob.parent().unwrap()).unwrap();
    std::fs::write(&blob, bytes).unwrap();
    let old = seed(&fixture, &alice, "+15555550177").await;
    {
        let mut conn = fixture.conn().await;
        sqlx::query(
            "INSERT INTO attachments (message_id, sha256, assets_path)
             SELECT id, $2, $3 FROM messages WHERE conversation_id = $1",
        )
        .bind(old)
        .bind(sha.as_str())
        .bind(crate::assets_api::shard_rel_path(&sha, ""))
        .execute(&mut *conn)
        .await
        .unwrap();
    }
    trash(&fixture, &alice, Trashable::Conversation(old)).await;

    // 1. The Upload starts its Import Run, then asks whether the server
    //    holds the file: it does.
    let run = start_run(&fixture, &alice).await;
    let server = crate::test_support::serve(&fixture.state).await;
    let head = reqwest::Client::new()
        .head(format!("{}/v1/assets/{sha}", server.base()))
        .bearer_auth(&alice.token)
        .send()
        .await
        .unwrap();
    assert_eq!(head.status(), reqwest::StatusCode::OK);
    // 2. The person empties the Trash in another tab.
    let status = delete_status(&fixture.state, "/v1/trash", &alice.token).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    // 3. The batch that names the file arrives.
    let (status, text) = crate::test_support::post_raw(
        &fixture.state,
        &format!("/v1/imports/{run}/batches"),
        &alice.token,
        "application/jsonl",
        batch_naming(sha.as_str()),
    )
    .await;
    assert!(status.is_success(), "{status} {text}");

    let mut conn = fixture.conn().await;
    let assets_path: Option<String> = sqlx::query_scalar(
        "SELECT a.assets_path FROM attachments a JOIN messages m ON m.id = a.message_id
         WHERE m.body = 'new'",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert!(
        assets_path.is_some() && blob.is_file(),
        "the imported attachment has no file: assets_path={assets_path:?}, blob on disk={}",
        blob.is_file()
    );
    drop(conn);

    complete_run(&fixture, &alice, run).await;
    assert!(
        blob.is_file(),
        "the sweep at the run's end keeps a file the new message names"
    );
}

/// End the Import Run `run` as an Upload does.
async fn complete_run(fixture: &TestFixture, account: &RegisteredAccount, run: i64) {
    let _: serde_json::Value = crate::test_support::post_json(
        &fixture.state,
        &format!("/v1/imports/{run}/complete"),
        &account.token,
        serde_json::json!({ "status": "completed" }),
    )
    .await;
}

/// A file Empty Trash kept because an Import Run was running is removed
/// when that run ends without naming it, so "delete for good" holds once
/// nothing can still need the file.
#[tokio::test]
async fn a_file_kept_for_a_running_import_goes_when_the_run_ends() {
    let (fixture, alice) = fixture_with_account().await;
    let sha = fake_sha256('c');
    let doomed = seed(&fixture, &alice, "+15555550178").await;
    let blob = attach_stored_file(&fixture.state, alice.account_id, doomed, &sha).await;
    let sidecar = blob.parent().unwrap().join(format!(".{sha}.mime"));
    let kept = seed(&fixture, &alice, "+15555550181").await;
    let kept_blob =
        attach_stored_file(&fixture.state, alice.account_id, kept, &fake_sha256('d')).await;
    trash(&fixture, &alice, Trashable::Conversation(doomed)).await;
    let run = start_run(&fixture, &alice).await;

    let status = delete_status(&fixture.state, "/v1/trash", &alice.token).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(
        blob.is_file(),
        "a running Import Run may still need the file"
    );

    complete_run(&fixture, &alice, run).await;
    assert!(!blob.exists(), "nothing names the file once the run ended");
    assert!(!sidecar.exists(), "its MIME sidecar goes with it");
    assert!(
        kept_blob.is_file(),
        "a file a conversation still names is not swept"
    );
}

/// An Import Run that starts after Empty Trash committed, but before its
/// files are removed, can be told by `HEAD` that a file exists. The
/// removal checks for a running run again under the write lock, so that
/// file stays for the batch that names it.
#[tokio::test]
async fn a_run_started_after_the_delete_commits_keeps_its_original() {
    let (fixture, alice) = fixture_with_account().await;
    let sha = fake_sha256('e');
    let doomed = seed(&fixture, &alice, "+15555550182").await;
    let blob = attach_stored_file(&fixture.state, alice.account_id, doomed, &sha).await;
    trash(&fixture, &alice, Trashable::Conversation(doomed)).await;
    let mut conn = fixture.conn().await;
    let files = crate::db::trash::empty_trash(
        &mut conn,
        alice.account_id,
        crate::db::audit_trail::AuditActor::Holder,
    )
    .await
    .unwrap()
    .orphaned;
    assert_eq!(files.len(), 1, "the delete reports the file unnamed");

    drop(conn);
    start_run(&fixture, &alice).await;
    crate::asset_store::remove_unreferenced(
        &fixture.state.db,
        std::sync::Arc::clone(&fixture.state.cfg),
        alice.account_id,
        files,
    )
    .await;

    assert!(
        blob.is_file(),
        "a run that started after the commit may still need the file"
    );
}

/// S5-3: a file that cannot be removed is logged, the other files are still
/// removed, and Empty Trash answers for what the database did.
#[tokio::test]
async fn a_file_that_cannot_be_removed_does_not_stop_the_others() {
    let (fixture, alice) = fixture_with_account().await;
    let stuck_sha = fake_sha256('a');
    let gone_sha = fake_sha256('b');
    let doomed = seed(&fixture, &alice, "+15555550179").await;
    let stuck = attach_stored_file(&fixture.state, alice.account_id, doomed, &stuck_sha).await;
    let gone = attach_stored_file(&fixture.state, alice.account_id, doomed, &gone_sha).await;
    // A folder where the first file belongs: removing it as a file fails.
    std::fs::remove_file(&stuck).unwrap();
    std::fs::create_dir_all(stuck.join("inside")).unwrap();
    trash(&fixture, &alice, Trashable::Conversation(doomed)).await;

    let status = delete_status(&fixture.state, "/v1/trash", &alice.token).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(
        !gone.exists(),
        "the file after the one that failed is removed"
    );
    assert_eq!(
        conversation_total(&fixture, &alice.token, "trashed:yes").await,
        0
    );
}

/// A stored fingerprint shorter than two characters names no shard folder.
/// Empty Trash passes over its sidecar instead of panicking.
#[tokio::test]
async fn a_short_stored_fingerprint_does_not_stop_empty_trash() {
    let (fixture, alice) = fixture_with_account().await;
    let doomed = seed(&fixture, &alice, "+15555550180").await;
    {
        let mut conn = fixture.conn().await;
        sqlx::query(
            "INSERT INTO attachments (message_id, sha256, assets_path)
             SELECT id, 'a', 'a' FROM messages WHERE conversation_id = $1",
        )
        .bind(doomed)
        .execute(&mut *conn)
        .await
        .unwrap();
    }
    trash(&fixture, &alice, Trashable::Conversation(doomed)).await;

    let status = delete_status(&fixture.state, "/v1/trash", &alice.token).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

/// One batch of one incoming message from `source`, on `handle`, whose
/// attachment is the file `sha`.
fn batch_from_source(source: &str, handle: &str, sha: &str) -> String {
    let header = format!(
        r#"{{"schema_version":4,"export":{{"source":"{source}","tool":"test","tool_version":"0","owner_handle":null,"owner_display_name":null}},"conversation":{{"chat_identifier":"{handle}","conversation_type":"individual","group_title":null,"participants":[{{"handle":"{handle}","display_name":null}}],"stats":{{"message_count":1,"attachment_count":1,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183462000}}}}}}"#
    );
    let message = format!(
        r#"{{"guid":"g-{source}","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"sms","message_kind":"sms","sender_handle":"{handle}","sender_display_name":null,"subject":null,"text":"from {source}","attachments":[{{"path":"attachments/photo.jpg","original_name":"photo.jpg","mime_type":"image/jpeg","digest_sha256":"{sha}","is_sticker":false,"transcription":null,"sticker_effect":null}}],"imessage":null,"source":null}}"#
    );
    format!("{header}\n{message}\n")
}

/// Import `bytes` as the attachment of one message from `source`, the way
/// an Upload does: start the Import Run, store the file at
/// `/v1/assets/{sha256}`, send the batch that names it, end the run.
/// Returns the status the file's `PUT` answered.
async fn import_file_from(
    fixture: &TestFixture,
    account: &RegisteredAccount,
    source: &str,
    handle: &str,
    bytes: &[u8],
) -> StatusCode {
    let sha = crate::assets_api::Sha256::of_bytes(bytes);
    let (_, run): (String, serde_json::Value) = crate::test_support::post_created_json(
        &fixture.state,
        "/v1/imports",
        &account.token,
        serde_json::json!({ "source": source }),
    )
    .await;
    let run = run["id"].as_i64().unwrap();
    let (put, text) = crate::test_support::put_raw(
        &fixture.state,
        &format!("/v1/assets/{sha}"),
        &account.token,
        "image/jpeg",
        bytes.to_vec(),
    )
    .await;
    assert!(put.is_success(), "{put} {text}");
    let (status, text) = crate::test_support::post_raw(
        &fixture.state,
        &format!("/v1/imports/{run}/batches"),
        &account.token,
        "application/jsonl",
        batch_from_source(source, handle, sha.as_str()),
    )
    .await;
    assert!(status.is_success(), "{status} {text}");
    complete_run(fixture, account, run).await;
    put
}

/// Every file under `dir`, at any depth, whose name starts with `sha`: the
/// stored copies of one file, not counting its `.<sha>.mime` sidecar.
fn stored_copies(dir: &std::path::Path, sha: &str) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            found.extend(stored_copies(&path, sha));
        } else if entry.file_name().to_string_lossy().starts_with(sha) {
            found.push(path);
        }
    }
    found
}

/// The one conversation of `account` whose messages come from `source`.
async fn conversation_of_source(
    fixture: &TestFixture,
    account: &RegisteredAccount,
    source: &str,
) -> i64 {
    let mut conn = fixture.conn().await;
    sqlx::query_scalar(
        "SELECT DISTINCT conversation_id FROM messages WHERE account_id = $1 AND source = $2",
    )
    .bind(account.account_id)
    .bind(source)
    .fetch_one(&mut *conn)
    .await
    .unwrap()
}

/// #1101: an attachment is the account's, addressed by its SHA-256 alone.
/// One file imported from two sources is stored once, and
/// `GET /v1/assets/{sha256}` answers it with no query.
#[tokio::test]
async fn a_file_imported_from_two_sources_is_stored_once() {
    let (fixture, alice) = fixture_with_account().await;
    let bytes = b"one photo, two backups";
    let sha = crate::assets_api::sha256_hex(bytes);

    let first = import_file_from(&fixture, &alice, "imessage", "+15555550140", bytes).await;
    let second = import_file_from(
        &fixture,
        &alice,
        "sms-backup-restore",
        "+15555550141",
        bytes,
    )
    .await;

    assert_eq!(first, StatusCode::CREATED);
    assert_eq!(
        second,
        StatusCode::OK,
        "the second source's upload finds the file the first one stored"
    );
    let account_dir = fixture
        .state
        .cfg
        .paths
        .data_dir
        .join(alice.account_id.to_string());
    assert_eq!(
        stored_copies(&account_dir, &sha),
        [fixture
            .state
            .cfg
            .paths
            .assets_dir_for_account(alice.account_id)
            .join(format!("{}/{sha}", &sha[..2]))],
        "one copy, in the account's one assets folder"
    );
    let (status, body) =
        crate::test_support::get_raw(&fixture.state, &format!("/v1/assets/{sha}"), &alice.token)
            .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_bytes(), bytes);
}

/// #1101: a file is removed only when no message of any source still names
/// it. Deleting one source's conversation for good leaves the file served
/// for the other source's message.
#[tokio::test]
async fn deleting_one_sources_messages_for_good_keeps_the_file_the_other_names() {
    let (fixture, alice) = fixture_with_account().await;
    let bytes = b"one photo, two backups";
    let sha = crate::assets_api::sha256_hex(bytes);
    import_file_from(&fixture, &alice, "imessage", "+15555550140", bytes).await;
    import_file_from(
        &fixture,
        &alice,
        "sms-backup-restore",
        "+15555550141",
        bytes,
    )
    .await;
    let doomed = conversation_of_source(&fixture, &alice, "sms-backup-restore").await;

    trash(&fixture, &alice, Trashable::Conversation(doomed)).await;
    let status = delete_status(
        &fixture.state,
        &format!("/v1/conversations/{doomed}"),
        &alice.token,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let mut conn = fixture.conn().await;
    let left: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM attachments a JOIN messages m ON m.id = a.message_id
         WHERE m.account_id = $1 AND m.source = 'imessage' AND a.sha256 = $2",
    )
    .bind(alice.account_id)
    .bind(&sha)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    drop(conn);
    assert_eq!(left, 1, "the other source's message still names the file");
    let (status, body) =
        crate::test_support::get_raw(&fixture.state, &format!("/v1/assets/{sha}"), &alice.token)
            .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_bytes(), bytes);
}
