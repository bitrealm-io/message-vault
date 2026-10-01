use std::path::Path;

use sqlx::AnyConnection;
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
/// `sms-backup-restore`, through the real entry point on the shared test
/// pool so it runs on Postgres too.
async fn import_one(
    conn: &mut AnyConnection,
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
        contacts: None,
        overwrite_contacts: false,
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
