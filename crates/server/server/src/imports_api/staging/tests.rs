use std::path::Path;

use sqlx::SqliteConnection;
use tempfile::TempDir;

use super::{is_orphaned_export, store_claimed_or_path};
use crate::assets_api::{self, AssetStats};
use crate::imports_api::{
    FixedImportArgs, ImportMode, ImportOptions, ImportSchemaMode, ImportStats,
    import_jsonl_files_on_conn,
};
use crate::models::AttachmentRecord;

const TEST_ACCOUNT: i64 = 7;

/// The header demo-seed writes for `orphaned.jsonl`: an `individual`
/// conversation whose chat id is `orphaned` and which names nobody.
const ORPHANED_HEADER: &str = r#"{"schema_version":4,"export":{"source":"imessage","tool":"test","tool_version":"0","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"orphaned","conversation_type":"individual","group_title":null,"participants":[],"stats":{"message_count":2,"attachment_count":0,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183462000}}}
"#;

/// An incoming iMessage line from `sender`.
fn incoming(guid: &str, sender: &str) -> String {
    format!(
        r#"{{"guid":"{guid}","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"imessage","message_kind":"imessage","sender_handle":"{sender}","sender_display_name":null,"subject":null,"text":"hi","attachments":[],"imessage":null,"source":null}}"#
    ) + "\n"
}

/// Import one file named `name` with `body` under the fixed source
/// `sms-backup-restore`, through the real entry point.
async fn import_one(
    conn: &mut SqliteConnection,
    name: &str,
    body: &str,
) -> anyhow::Result<ImportStats> {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join(name);
    std::fs::write(&path, body).unwrap();
    let assets = tmp.path().join("assets");
    let opts = ImportOptions::fixed(FixedImportArgs {
        assets_dir: &assets,
        asset_root: tmp.path(),
        mode: ImportMode::Append,
        source: "sms-backup-restore",
        account_id: TEST_ACCOUNT,
        fill_content_keys: false,
        import_id: None,
    });
    import_jsonl_files_on_conn(conn, &[path], &opts, ImportSchemaMode::Ensure).await
}

/// The reason an import was refused: its error text after the file's temp
/// path, which no assertion should read.
fn refusal(result: anyhow::Result<ImportStats>) -> String {
    let err = result.expect_err("the import is refused");
    let text = format!("{err:#}");
    let (_, reason) = text
        .rsplit_once(".jsonl")
        .expect("the refusal names the file");
    reason.trim_start_matches([':', ' ']).to_string()
}

/// An attachment record that names only a stored blob by its fingerprint,
/// with `mime_type` as the export's MIME claim.
fn claimed(sha256: &str, mime_type: Option<&str>) -> AttachmentRecord {
    AttachmentRecord {
        path: None,
        original_name: None,
        mime_type: mime_type.map(str::to_string),
        sha256: Some(sha256.to_string()),
        is_sticker: false,
        transcription: None,
        size_bytes: None,
        missing_reason: None,
    }
}

/// A blob the store already holds under `image/png` is reused when an
/// attachment claims its sha256. The export's MIME type wins over the stored
/// one when the record has one, and the stored one stands when it does not,
/// because the export saw the original file and the store only guessed.
#[test]
fn a_reused_blob_takes_the_export_mime_type_when_the_record_has_one() {
    let tmp = TempDir::new().unwrap();
    let export_dir = tmp.path().join("export");
    let assets_dir = tmp.path().join("assets");
    std::fs::create_dir_all(&export_dir).unwrap();
    let source = export_dir.join("photo.png");
    std::fs::write(&source, b"not really a png").unwrap();
    let sha = assets_api::hash_file(&source).unwrap();
    assets_api::store_verified(&source, &sha, &assets_dir, Some("image/png"), false, false)
        .unwrap();
    let mut stats = AssetStats::default();

    let stored = store_claimed_or_path(
        &claimed(&sha, Some("image/jpeg")),
        &export_dir,
        &assets_dir,
        &mut stats,
    )
    .unwrap()
    .expect("the stored blob is reused");
    assert_eq!(stored.sha256, sha);
    assert_eq!(stored.mime_type.as_deref(), Some("image/jpeg"));

    let stored = store_claimed_or_path(&claimed(&sha, None), &export_dir, &assets_dir, &mut stats)
        .unwrap()
        .expect("the stored blob is reused");
    assert_eq!(stored.mime_type.as_deref(), Some("image/png"));
    assert_eq!(stats.deduped, 2);
    assert_eq!(stats.copied, 0);
    assert_eq!(stats.missing, 0);
}

/// An attachment path that climbs out of the export folder is refused even
/// when its fingerprint is already stored and the file is never read, because
/// `attachments.path` keeps the path as sent and an Export later writes the
/// file there. The refusal is the one a new fingerprint gets.
#[test]
fn a_path_that_leaves_the_export_folder_is_refused_whether_or_not_its_fingerprint_is_stored() {
    let tmp = TempDir::new().unwrap();
    let export_dir = tmp.path().join("export");
    let assets_dir = tmp.path().join("assets");
    std::fs::create_dir_all(&export_dir).unwrap();
    let source = export_dir.join("photo.png");
    std::fs::write(&source, b"stored bytes").unwrap();
    let stored_sha = assets_api::hash_file(&source).unwrap();
    assets_api::store_verified(&source, &stored_sha, &assets_dir, None, false, false).unwrap();
    let new_sha = assets_api::sha256_hex(b"bytes the store has never seen");

    for sha in [&stored_sha, &new_sha] {
        let att = AttachmentRecord {
            path: Some("../escape.txt".to_string()),
            ..claimed(sha, None)
        };
        let mut stats = AssetStats::default();

        let err = store_claimed_or_path(&att, &export_dir, &assets_dir, &mut stats)
            .expect_err("the path is refused");

        assert_eq!(
            err.to_string(),
            format!("{}: ../escape.txt", message_ir::UNSAFE_ATTACHMENT_PATH)
        );
        assert_eq!(stats.deduped, 0);
    }
}

/// The export says the attachment's bytes hash to one value and the file on
/// disk hashes to another. That is a damaged or swapped file, so the import
/// stops: it neither stores the file under either fingerprint nor records the
/// attachment as missing.
#[tokio::test]
async fn a_file_that_does_not_match_its_claimed_sha256_fails_the_import_and_is_not_stored() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    let tmp = TempDir::new().unwrap();
    let assets = tmp.path().join("assets");
    std::fs::write(tmp.path().join("photo.bin"), b"the bytes on disk").unwrap();
    let claimed_sha = assets_api::sha256_hex(b"the bytes the export saw");
    let header = ORPHANED_HEADER.replace("orphaned", "+15555550701");
    let message = format!(
        r#"{{"guid":"g-mismatch","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"imessage","message_kind":"imessage","sender_handle":"+15555550701","sender_display_name":null,"subject":null,"text":"hi","attachments":[{{"path":"photo.bin","original_name":"photo.bin","mime_type":"application/octet-stream","digest_sha256":"{claimed_sha}","is_sticker":false,"transcription":null,"sticker_effect":null}}],"imessage":null,"source":null}}"#
    );
    let path = tmp.path().join("mismatch.jsonl");
    std::fs::write(&path, format!("{header}{message}\n")).unwrap();
    let opts = ImportOptions::fixed(FixedImportArgs {
        assets_dir: &assets,
        asset_root: tmp.path(),
        mode: ImportMode::Append,
        source: "imessage",
        account_id: TEST_ACCOUNT,
        fill_content_keys: false,
        import_id: None,
    });

    let err = import_jsonl_files_on_conn(&mut conn, &[path], &opts, ImportSchemaMode::Ensure)
        .await
        .expect_err("a mismatched file fails the import");

    assert!(
        format!("{err:#}").contains("sha256 mismatch"),
        "the refusal says why: {err:#}"
    );
    for sha in [claimed_sha, assets_api::sha256_hex(b"the bytes on disk")] {
        assert!(assets_api::lookup_by_sha256(&assets, &sha).is_none());
    }
}

#[test]
fn only_a_file_named_orphaned_is_the_orphaned_export() {
    assert!(is_orphaned_export(Path::new("out/orphaned.jsonl")));
    assert!(is_orphaned_export(Path::new("Orphaned.json")));
    assert!(!is_orphaned_export(Path::new("out/+15555550100.jsonl")));
    assert!(!is_orphaned_export(Path::new("orphaned-2.jsonl")));
}

/// `orphaned.jsonl` is staged as one conversation under the file's own
/// chat id, with its messages under the import's source. The chat id
/// `orphaned` names the file, not a person, so it gets no contact; the
/// sender does.
#[tokio::test]
async fn orphaned_jsonl_is_staged_as_the_orphaned_conversation() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    let body = ORPHANED_HEADER.to_string()
        + &incoming("g-orphan-1", "+15555550701")
        + &incoming("g-orphan-2", "+15555550701");
    let stats = import_one(&mut conn, "orphaned.jsonl", &body)
        .await
        .unwrap();
    assert_eq!(stats.conversations, 1);
    assert_eq!(stats.messages, 2);

    let (chat, kind, file): (String, String, String) = sqlx::query_as(
        "SELECT h.raw, c.conversation_type, c.source_file FROM conversations c
         JOIN handles h ON h.id = c.chat_handle_id
         WHERE c.account_id = $1",
    )
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        (chat.as_str(), kind.as_str(), file.as_str()),
        ("orphaned", "individual", "orphaned.jsonl")
    );

    let sources: Vec<String> =
        sqlx::query_scalar("SELECT DISTINCT source FROM messages WHERE account_id = $1")
            .bind(TEST_ACCOUNT)
            .fetch_all(&mut *conn)
            .await
            .unwrap();
    assert_eq!(sources, ["sms-backup-restore"], "the import's source");

    let contacts: Vec<String> = sqlx::query_scalar(
        "SELECT h.raw FROM contact_handles ch
         JOIN handles h ON h.id = ch.handle_id
         WHERE ch.account_id = $1",
    )
    .bind(TEST_ACCOUNT)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(contacts, ["+15555550701"], "only the sender is a person");
}

/// Every file, `orphaned.jsonl` included, needs its conversation header
/// before its messages: the reader refuses the file before staging sees it.
#[tokio::test]
async fn a_file_with_messages_and_no_header_is_refused() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    for name in ["orphaned.jsonl", "+15555550100.jsonl"] {
        let result = import_one(&mut conn, name, &incoming("g1", "+15555550100")).await;
        assert_eq!(
            refusal(result),
            "Could not read line 1 of the file: a message appears before the conversation header.",
            "{name}"
        );
    }
}

#[tokio::test]
async fn a_file_with_neither_header_nor_messages_is_refused() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    let result = import_one(&mut conn, "+15555550100.jsonl", "\n").await;
    assert_eq!(
        refusal(result),
        "Could not read line 1 of the file: the file has no conversation header."
    );
}

/// A WhatsApp conversation header with `chat_identifier`, `kind` and the
/// participants JSON array `participants`.
fn whatsapp_header(chat_identifier: &str, kind: &str, participants: &str) -> String {
    format!(
        r#"{{"schema_version":4,"export":{{"source":"whatsapp","tool":"test","tool_version":"0","owner_handle":null,"owner_display_name":null}},"conversation":{{"chat_identifier":"{chat_identifier}","conversation_type":"{kind}","group_title":null,"participants":{participants},"stats":{{"message_count":1,"attachment_count":0,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183462000}}}}}}"#
    ) + "\n"
}

/// An incoming WhatsApp line from `sender`.
fn incoming_whatsapp(guid: &str, sender: &str) -> String {
    incoming(guid, sender).replace(
        r#""service":"imessage","message_kind":"imessage""#,
        r#""service":"whatsapp","message_kind":"unknown""#,
    )
}

/// Every `(raw, handle_type)` the account's handles hold, sorted.
async fn handle_types(conn: &mut SqliteConnection) -> Vec<(String, String)> {
    sqlx::query_as("SELECT raw, handle_type FROM handles WHERE account_id = $1 ORDER BY raw")
        .bind(TEST_ACCOUNT)
        .fetch_all(&mut *conn)
        .await
        .unwrap()
}

/// A WhatsApp group's id ends in `@g.us`, which has an `@` in it, but it is
/// the group's key and nobody's address. Typed by its shape it was stored as
/// an email identity (#1141).
#[tokio::test]
async fn a_group_chat_id_is_stored_as_other_whatever_its_shape() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    let body = whatsapp_header(
        "120363042@g.us",
        "group",
        r#"[{"handle":"+15555550702","display_name":null,"handle_type":"phone"}]"#,
    ) + &incoming_whatsapp("g-group-1", "+15555550702");
    import_one(&mut conn, "120363042@g.us.jsonl", &body)
        .await
        .unwrap();

    assert_eq!(
        handle_types(&mut conn).await,
        [
            ("+15555550702".to_string(), "phone".to_string()),
            ("120363042@g.us".to_string(), "other".to_string()),
        ]
    );
}

/// A one-to-one WhatsApp chat keyed by an internal `@lid` id: the header
/// types its one participant `other`. The chat's identity and the sender of
/// its messages take that type, so neither becomes an email identity, as they
/// did when the chat id and the sender were typed by their shape (#1141).
#[tokio::test]
async fn an_individual_chat_id_takes_the_type_its_participant_has_in_the_header() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    let body = whatsapp_header(
        "123456@lid",
        "individual",
        r#"[{"handle":"123456@lid","display_name":null,"handle_type":"other"}]"#,
    ) + &incoming_whatsapp("g-lid-1", "123456@lid");
    import_one(&mut conn, "123456@lid.jsonl", &body)
        .await
        .unwrap();

    assert_eq!(
        handle_types(&mut conn).await,
        [("123456@lid".to_string(), "other".to_string())]
    );
    let contacts: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM contact_handles WHERE account_id = $1")
            .bind(TEST_ACCOUNT)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(contacts, 1, "the one person in the chat is one contact");
}
