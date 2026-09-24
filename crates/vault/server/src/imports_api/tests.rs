use super::*;
use crate::assets_api;
use crate::test_support::{
    RegisteredAccount, TestVault, get_json, patch_json, post_created_json, post_json, test_vault,
    vault_with_account,
};
use tempfile::TempDir;

const TEST_ACCOUNT: i64 = 7;

fn write_jsonl(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, body).unwrap();
    path
}

/// A vault holding one live import session at `awaiting_gate_1` whose
/// `summary_json` already carries `summary` — as if an earlier
/// `PATCH /v1/imports/{id}` recorded a gate approval.
async fn session_with_summary(summary: serde_json::Value) -> (TestVault, RegisteredAccount, i64) {
    let (vault, account) = vault_with_account().await;
    let (_, created): (String, serde_json::Value) = post_created_json(
        &vault.state,
        "/v1/imports",
        &account.token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    let import_id = created["id"].as_i64().expect("created session has an id");
    let mut conn = vault.state.db.acquire().await.unwrap();
    crate::db::vault_imports::set_import_stage(
        &mut conn,
        account.account_id,
        import_id,
        crate::db::vault_imports::ImportStage::AwaitingGate1,
        Some(&summary.to_string()),
    )
    .await
    .unwrap();
    (vault, account, import_id)
}

/// The session's stored `summary_json`, decoded, or `None` when the
/// column is null.
async fn stored_summary(vault: &TestVault, import_id: i64) -> Option<serde_json::Value> {
    let mut conn = vault.state.db.acquire().await.unwrap();
    let raw: Option<String> =
        sqlx::query_scalar("SELECT summary_json FROM vault_imports WHERE id = $1")
            .bind(import_id)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    raw.map(|s| serde_json::from_str(&s).expect("stored summary_json is valid JSON"))
}

#[tokio::test]
async fn a_stage_change_with_a_summary_stores_it() {
    // The gate screen posts what the user approved so it survives a
    // reload — recomputing the summary from the folder is a different
    // question from what was actually approved.
    let (vault, account) = vault_with_account().await;
    let (location, created): (String, serde_json::Value) = post_created_json(
        &vault.state,
        "/v1/imports",
        &account.token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    let import_id = created["id"].as_i64().unwrap();
    assert_eq!(location, format!("/v1/imports/{import_id}"));

    patch_json::<serde_json::Value>(
        &vault.state,
        &format!("/v1/imports/{import_id}"),
        &account.token,
        serde_json::json!({"stage": "awaiting_gate_1", "summary": {"approved": true}}),
    )
    .await;

    assert_eq!(
        stored_summary(&vault, import_id).await,
        Some(serde_json::json!({"approved": true}))
    );
}

#[tokio::test]
async fn active_session_reports_the_summary_a_stage_change_stored() {
    // The completion call is allowed to overwrite summary_json with the
    // outcome once the run finishes — that is the intended history
    // record. But mid-session, between an approval and completion, a
    // reload has nowhere else to read the approved plan back from:
    // the running run on GET /v1/imports?status=running must expose it too.
    let (vault, account, import_id) =
        session_with_summary(serde_json::json!({"approved": true})).await;

    let page: serde_json::Value =
        get_json(&vault.state, "/v1/imports?status=running", &account.token).await;
    let active = &page["items"][0];

    assert_eq!(active["id"], serde_json::json!(import_id));
    assert_eq!(active["summary"], serde_json::json!({"approved": true}));
}

#[tokio::test]
async fn a_stage_change_without_a_summary_does_not_erase_the_stored_one() {
    // Most stage changes carry nothing. Treating absent as null would
    // throw away the plan the outcome is judged against.
    let (vault, account, import_id) =
        session_with_summary(serde_json::json!({"approved": true})).await;

    let run: serde_json::Value = patch_json(
        &vault.state,
        &format!("/v1/imports/{import_id}"),
        &account.token,
        serde_json::json!({"stage": "pushing"}),
    )
    .await;
    assert_eq!(
        run["id"],
        serde_json::json!(import_id),
        "a PATCH answers the run, the same record GET /v1/imports/{{id}} returns"
    );

    assert_eq!(
        stored_summary(&vault, import_id).await,
        Some(serde_json::json!({"approved": true}))
    );
}

/// Open a verify connection to an on-disk test database.
async fn open_verify(db: &Path) -> (sqlx::AnyPool, sqlx::pool::PoolConnection<sqlx::Any>) {
    let pool = engine::open_pool_for_path(db).await.unwrap();
    let conn = pool.acquire().await.unwrap();
    (pool, conn)
}

#[tokio::test]
async fn append_skips_existing_guids_and_keeps_id_map() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("vault.db");
    let assets = tmp.path().join("assets");

    let first = write_jsonl(
        tmp.path(),
        "a.jsonl",
        r#"{"schema_version":4,"export":{"source":"sms-backup-restore","tool":"test","tool_version":"0","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+14075551234","conversation_type":"individual","group_title":null,"participants":[{"handle":"+14075551234","display_name":null}],"stats":{"message_count":2,"attachment_count":0,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183522000}}}
{"guid":"g-keep","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"sms","message_kind":"sms","sender_handle":"+14075551234","sender_display_name":null,"subject":null,"text":"one","attachments":[],"imessage":null,"source":null}
{"guid":"g-dup","timestamp_unix_ms":1426183522000,"direction":"outgoing","service":"sms","message_kind":"sms","sender_handle":null,"sender_display_name":null,"subject":null,"text":"two","attachments":[],"imessage":null,"source":null}
"#,
    );
    let first_stats = import_jsonl_files(
        &db,
        &[first],
        &ImportOptions::fixed(FixedImportArgs {
            assets_dir: &assets,
            asset_root: tmp.path(),
            contacts: None,
            overwrite_contacts: false,
            mode: ImportMode::Replace,
            source: "sms-backup-restore",
            account_id: TEST_ACCOUNT,
            fill_content_keys: true,
            import_id: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(first_stats.messages, 2);

    let second = write_jsonl(
        tmp.path(),
        "b.jsonl",
        r#"{"schema_version":4,"export":{"source":"sms-backup-restore","tool":"test","tool_version":"0","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+14075551234","conversation_type":"individual","group_title":null,"participants":[{"handle":"+14075551234","display_name":null}],"stats":{"message_count":3,"attachment_count":0,"first_timestamp_unix_ms":1426183522000,"last_timestamp_unix_ms":1426183642000}}}
{"guid":"g-dup","timestamp_unix_ms":1426183522000,"direction":"outgoing","service":"sms","message_kind":"sms","sender_handle":null,"sender_display_name":null,"subject":null,"text":"two again","attachments":[],"imessage":null,"source":null}
{"guid":"g-new","timestamp_unix_ms":1426183582000,"direction":"incoming","service":"sms","message_kind":"sms","sender_handle":"+14075551234","sender_display_name":null,"subject":null,"text":"three","attachments":[],"imessage":null,"source":null}
{"guid":"","timestamp_unix_ms":1426183642000,"direction":"incoming","service":"sms","message_kind":"sms","sender_handle":"+14075551234","sender_display_name":null,"subject":null,"text":"empty guid always inserts","attachments":[],"imessage":null,"source":null}
"#,
    );
    let second_stats = import_jsonl_files(
        &db,
        &[second],
        &ImportOptions::fixed(FixedImportArgs {
            assets_dir: &assets,
            asset_root: tmp.path(),
            contacts: None,
            overwrite_contacts: false,
            mode: ImportMode::Append,
            source: "sms-backup-restore",
            account_id: TEST_ACCOUNT,
            fill_content_keys: false,
            import_id: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(second_stats.messages_appended, 2);
    assert_eq!(second_stats.messages_deduped, 1);

    let (_pool, mut conn) = open_verify(&db).await;
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(n, 4);
    let dup_body: String = sqlx::query_scalar("SELECT body FROM messages WHERE guid = 'g-dup'")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(dup_body, "two");

    // Deferred full-text search during promote must still index new bodies
    // and restore triggers.
    let fts_three: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM messages_fts WHERE messages_fts MATCH 'three'")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(fts_three, 1);
    let fts_one: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM messages_fts WHERE messages_fts MATCH 'one'")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(fts_one, 1);
    let triggers: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND name LIKE '%_fts_%'",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(triggers, 6);
}

fn replace_opts<'a>(assets: &'a Path, root: &'a Path, source: &'a str) -> ImportOptions<'a> {
    ImportOptions::fixed(FixedImportArgs {
        assets_dir: assets,
        asset_root: root,
        contacts: None,
        overwrite_contacts: false,
        mode: ImportMode::Replace,
        source,
        account_id: TEST_ACCOUNT,
        fill_content_keys: false,
        import_id: None,
    })
}

fn missing_attachment_json(name: &str) -> String {
    format!(
        r#"[{{"path":"attachments/{name}","original_name":"{name}","mime_type":"application/octet-stream","digest_sha256":null,"is_sticker":false,"transcription":null,"sticker_effect":null,"size_bytes":12,"missing_reason":"not_found"}}]"#
    )
}

const TAPBACK_IMESSAGE: &str = r#"{"is_reply":false,"in_reply_to_guid":null,"thread_originator_part":null,"num_replies":null,"is_deleted":false,"send_effect":null,"shared_location":null,"announcement":null,"read_receipt_rfc3339":null,"parts":null,"edits":null,"tapbacks":[{"emoji":null,"is_from_me":false,"kind":"liked","part_index":0,"sender":"+15555550999"}],"app":null,"balloon_bundle_id":null,"balloon_kind":null,"associated_guid":null,"associated_part":null,"tapback_kind":null,"tapback_emoji":null,"tapback_action":null}"#;

fn chunk_boundary_jsonl() -> String {
    let header = r#"{"schema_version":4,"export":{"source":"imessage","tool":"test","tool_version":"0","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+15555550123","conversation_type":"individual","group_title":null,"participants":[{"handle":"+15555550123","display_name":null},{"handle":"+15555550999","display_name":null}],"stats":{"message_count":56,"attachment_count":2,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183517000}}}"#;
    let mut lines = vec![header.to_string()];
    for i in 0..56 {
        let guid = format!("g-{i:02}");
        let ts = 1_426_183_462_000i64 + i64::from(i) * 1000;
        let attachments = if i == 0 {
            missing_attachment_json("first.bin")
        } else if i == 55 {
            missing_attachment_json("last.bin")
        } else {
            "[]".to_string()
        };
        let imessage = if i == 1 { TAPBACK_IMESSAGE } else { "null" };
        lines.push(format!(
            r#"{{"guid":"{guid}","timestamp_unix_ms":{ts},"direction":"incoming","service":"imessage","message_kind":"imessage","sender_handle":"+15555550123","sender_display_name":null,"subject":null,"text":"msg {i}","attachments":{attachments},"imessage":{imessage},"source":null}}"#
        ));
    }
    lines.join("\n")
}

#[tokio::test]
async fn staging_chunks_56_messages_and_keeps_children_on_right_rows() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("vault.db");
    let assets = tmp.path().join("assets");
    let path = write_jsonl(tmp.path(), "chunk-boundary.jsonl", &chunk_boundary_jsonl());
    let stats = import_jsonl_files(&db, &[path], &replace_opts(&assets, tmp.path(), "imessage"))
        .await
        .unwrap();
    assert_eq!(stats.messages, 56);
    assert_eq!(stats.attachments, 2);
    assert_eq!(stats.tapbacks, 1);
    assert_eq!(stats.messages_deduped, 0);

    let (_pool, mut conn) = open_verify(&db).await;
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(n, 56);
    let first_atts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM attachments WHERE message_id = (SELECT id FROM messages WHERE guid = 'g-00')",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let last_atts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM attachments WHERE message_id = (SELECT id FROM messages WHERE guid = 'g-55')",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let second_taps: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tapbacks WHERE message_id = (SELECT id FROM messages WHERE guid = 'g-01')",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(first_atts, 1);
    assert_eq!(last_atts, 1);
    assert_eq!(second_taps, 1);
}

#[tokio::test]
async fn staging_skips_duplicate_guid_in_same_file_and_keeps_first_attachment() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("vault.db");
    let assets = tmp.path().join("assets");
    let header = r#"{"schema_version":4,"export":{"source":"imessage","tool":"test","tool_version":"0","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+15555550123","conversation_type":"individual","group_title":null,"participants":[{"handle":"+15555550123","display_name":null},{"handle":"+15555550999","display_name":null}],"stats":{"message_count":2,"attachment_count":2,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183463000}}}"#;
    let first = format!(
        r#"{{"guid":"g-once","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"imessage","message_kind":"imessage","sender_handle":"+15555550123","sender_display_name":null,"subject":null,"text":"first","attachments":{},"imessage":null,"source":null}}"#,
        missing_attachment_json("first.bin")
    );
    let second = format!(
        r#"{{"guid":"g-once","timestamp_unix_ms":1426183463000,"direction":"incoming","service":"imessage","message_kind":"imessage","sender_handle":"+15555550123","sender_display_name":null,"subject":null,"text":"second","attachments":{},"imessage":{TAPBACK_IMESSAGE},"source":null}}"#,
        missing_attachment_json("second.bin")
    );
    let path = write_jsonl(
        tmp.path(),
        "dup-guid.jsonl",
        &format!("{header}\n{first}\n{second}\n"),
    );
    let stats = import_jsonl_files(&db, &[path], &replace_opts(&assets, tmp.path(), "imessage"))
        .await
        .unwrap();
    assert_eq!(stats.messages, 1);
    assert_eq!(stats.messages_deduped, 1);
    assert_eq!(stats.attachments, 1);
    assert_eq!(stats.tapbacks, 0);

    let (_pool, mut conn) = open_verify(&db).await;
    let (body, attachments, tapbacks): (String, i64, i64) = sqlx::query_as(
        r"
        SELECT m.body, COUNT(DISTINCT a.id), COUNT(DISTINCT t.id)
        FROM messages m
        LEFT JOIN attachments a ON a.message_id = m.id
        LEFT JOIN tapbacks t ON t.message_id = m.id
        WHERE m.guid = 'g-once'
        GROUP BY m.id
        ",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(body, "first");
    assert_eq!(attachments, 1);
    assert_eq!(tapbacks, 0);
    let name: String = sqlx::query_scalar(
        "SELECT original_name FROM attachments WHERE message_id = (SELECT id FROM messages WHERE guid = 'g-once')",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(name, "first.bin");
}

#[tokio::test]
async fn staging_keeps_both_rows_when_guids_differ_only_by_whitespace() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("vault.db");
    let assets = tmp.path().join("assets");
    let header = r#"{"schema_version":4,"export":{"source":"imessage","tool":"test","tool_version":"0","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+15555550123","conversation_type":"individual","group_title":null,"participants":[{"handle":"+15555550123","display_name":null}],"stats":{"message_count":2,"attachment_count":2,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183463000}}}"#;
    let first = format!(
        r#"{{"guid":"g-space","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"imessage","message_kind":"imessage","sender_handle":"+15555550123","sender_display_name":null,"subject":null,"text":"trimmed","attachments":{},"imessage":null,"source":null}}"#,
        missing_attachment_json("trim.bin")
    );
    let second = format!(
        r#"{{"guid":" g-space","timestamp_unix_ms":1426183463000,"direction":"incoming","service":"imessage","message_kind":"imessage","sender_handle":"+15555550123","sender_display_name":null,"subject":null,"text":"padded","attachments":{},"imessage":null,"source":null}}"#,
        missing_attachment_json("pad.bin")
    );
    let path = write_jsonl(
        tmp.path(),
        "guid-whitespace.jsonl",
        &format!("{header}\n{first}\n{second}\n"),
    );
    let stats = import_jsonl_files(&db, &[path], &replace_opts(&assets, tmp.path(), "imessage"))
        .await
        .unwrap();
    assert_eq!(stats.messages, 2);
    assert_eq!(stats.messages_deduped, 0);
    assert_eq!(stats.attachments, 2);

    let (_pool, mut conn) = open_verify(&db).await;
    let names: Vec<String> =
        sqlx::query_scalar("SELECT original_name FROM attachments ORDER BY original_name")
            .fetch_all(&mut *conn)
            .await
            .unwrap();
    assert_eq!(names, vec!["pad.bin".to_string(), "trim.bin".to_string()]);
}

#[tokio::test]
async fn append_existing_guid_adds_missing_children() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("vault.db");
    let assets = tmp.path().join("assets");
    let header = r#"{"schema_version":4,"export":{"source":"imessage","tool":"test","tool_version":"0","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+15555550123","conversation_type":"individual","group_title":null,"participants":[{"handle":"+15555550123","display_name":null},{"handle":"+15555550999","display_name":null}],"stats":{"message_count":1,"attachment_count":0,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183462000}}}"#;
    let first = write_jsonl(
        tmp.path(),
        "children-first.jsonl",
        &format!(
            "{header}\n{}\n",
            r#"{"guid":"g-children","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"imessage","message_kind":"imessage","sender_handle":"+15555550123","sender_display_name":null,"subject":null,"text":"original body","attachments":[],"imessage":null,"source":null}"#
        ),
    );
    let options = ImportOptions::fixed(FixedImportArgs {
        assets_dir: &assets,
        asset_root: tmp.path(),
        contacts: None,
        overwrite_contacts: false,
        mode: ImportMode::Append,
        source: "imessage",
        account_id: TEST_ACCOUNT,
        fill_content_keys: false,
        import_id: None,
    });
    import_jsonl_files(&db, &[first], &options).await.unwrap();

    let second = write_jsonl(
        tmp.path(),
        "children-second.jsonl",
        &format!(
            "{header}\n{}\n",
            r#"{"guid":"g-children","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"imessage","message_kind":"imessage","sender_handle":"+15555550123","sender_display_name":null,"subject":null,"text":"replacement body","attachments":[{"path":"attachments/missing.bin","original_name":"missing.bin","mime_type":"application/octet-stream","digest_sha256":null,"is_sticker":false,"transcription":null,"sticker_effect":null,"size_bytes":12,"missing_reason":"not_found"}],"imessage":{"is_reply":false,"in_reply_to_guid":null,"thread_originator_part":null,"num_replies":null,"is_deleted":false,"send_effect":null,"shared_location":null,"announcement":null,"read_receipt_rfc3339":null,"parts":null,"edits":null,"tapbacks":[{"emoji":null,"is_from_me":false,"kind":"liked","part_index":0,"sender":"+15555550999"}],"app":null,"balloon_bundle_id":null,"balloon_kind":null,"associated_guid":null,"associated_part":null,"tapback_kind":null,"tapback_emoji":null,"tapback_action":null},"source":null}"#
        ),
    );

    for _ in 0..2 {
        import_jsonl_files(&db, std::slice::from_ref(&second), &options)
            .await
            .unwrap();
    }

    let (_pool, mut conn) = open_verify(&db).await;
    let (body, attachments, tapbacks): (String, i64, i64) = sqlx::query_as(
        r"
        SELECT m.body, COUNT(DISTINCT a.id), COUNT(DISTINCT t.id)
        FROM messages m
        LEFT JOIN attachments a ON a.message_id = m.id
        LEFT JOIN tapbacks t ON t.message_id = m.id
        WHERE m.guid = 'g-children'
        GROUP BY m.id
        ",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(body, "original body");
    assert_eq!(attachments, 1);
    assert_eq!(tapbacks, 1);
}

#[tokio::test]
async fn append_with_a_found_file_fills_in_the_missing_attachment() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("vault.db");
    let assets = tmp.path().join("assets");
    let path = write_jsonl(
        tmp.path(),
        "found.jsonl",
        &format!(
            "{}\n{}\n",
            r#"{"schema_version":4,"export":{"source":"imessage","tool":"test","tool_version":"0","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+15555550123","conversation_type":"individual","group_title":null,"participants":[{"handle":"+15555550123","display_name":null}],"stats":{"message_count":1,"attachment_count":1,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183462000}}}"#,
            format_args!(
                r#"{{"guid":"g-found","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"imessage","message_kind":"imessage","sender_handle":"+15555550123","sender_display_name":null,"subject":null,"text":"see attached","attachments":{},"imessage":null,"source":null}}"#,
                missing_attachment_json("found.bin")
            )
        ),
    );
    let options = ImportOptions::fixed(FixedImportArgs {
        assets_dir: &assets,
        asset_root: tmp.path(),
        contacts: None,
        overwrite_contacts: false,
        mode: ImportMode::Append,
        source: "imessage",
        account_id: TEST_ACCOUNT,
        fill_content_keys: false,
        import_id: None,
    });

    // The first import has no file on disk, so the attachment is stored as missing.
    import_jsonl_files(&db, std::slice::from_ref(&path), &options)
        .await
        .unwrap();

    // The person uploads the file and imports the same conversation again.
    fs::create_dir_all(tmp.path().join("attachments")).unwrap();
    fs::write(tmp.path().join("attachments/found.bin"), b"found-bytes").unwrap();
    import_jsonl_files(&db, std::slice::from_ref(&path), &options)
        .await
        .unwrap();

    let (_pool, mut conn) = open_verify(&db).await;
    let rows: Vec<(Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
        r"
        SELECT a.sha256, a.assets_path, a.missing_reason
        FROM attachments a
        JOIN messages m ON m.id = a.message_id
        WHERE m.guid = 'g-found'
        ",
    )
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        rows.len(),
        1,
        "one attachment row, not a second one: {rows:?}"
    );
    let (sha256, assets_path, missing_reason) = &rows[0];
    assert_eq!(
        sha256.as_deref(),
        Some(assets_api::sha256_hex(b"found-bytes").as_str())
    );
    assert!(assets_path.is_some());
    assert_eq!(missing_reason, &None);
}

#[tokio::test]
async fn repeated_append_keeps_one_fts_posting_per_message() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("vault.db");
    let assets = tmp.path().join("assets");
    let path = write_jsonl(
        tmp.path(),
        "fts-append.jsonl",
        r#"{"schema_version":4,"export":{"source":"imessage","tool":"test","tool_version":"0","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+15555550123","conversation_type":"individual","group_title":null,"participants":[{"handle":"+15555550123","display_name":null}],"stats":{"message_count":1,"attachment_count":0,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183462000}}}
{"guid":"g-fts","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"imessage","message_kind":"imessage","sender_handle":"+15555550123","sender_display_name":null,"subject":null,"text":"zzuniqueterm body","attachments":[],"imessage":null,"source":null}
"#,
    );
    let options = ImportOptions::fixed(FixedImportArgs {
        assets_dir: &assets,
        asset_root: tmp.path(),
        contacts: None,
        overwrite_contacts: false,
        mode: ImportMode::Append,
        source: "imessage",
        account_id: TEST_ACCOUNT,
        fill_content_keys: false,
        import_id: None,
    });

    import_jsonl_files(&db, std::slice::from_ref(&path), &options)
        .await
        .unwrap();
    // Rows of the full-text search index storage: a redundant re-index writes a new
    // segment even when the indexed text is unchanged.
    async fn index_rows(db: &Path) -> i64 {
        let (_pool, mut conn) = open_verify(db).await;
        sqlx::query_scalar("SELECT COUNT(*) FROM messages_fts_data")
            .fetch_one(&mut *conn)
            .await
            .unwrap()
    }
    let after_first_import = index_rows(&db).await;
    for _ in 0..2 {
        import_jsonl_files(&db, std::slice::from_ref(&path), &options)
            .await
            .unwrap();
    }
    assert_eq!(
        index_rows(&db).await,
        after_first_import,
        "repeated append must not write additional FTS index entries"
    );

    let (_pool, mut conn) = open_verify(&db).await;
    let messages: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(messages, 1);

    // `MATCH` collapses repeated postings for one rowid, so read the index
    // itself: fts5vocab reports how many entries each term really has.
    sqlx::query("CREATE VIRTUAL TABLE fts_vocab USING fts5vocab(messages_fts, row);")
        .execute(&mut *conn)
        .await
        .unwrap();
    let term_entries = async |conn: &mut AnyConnection| {
        let (docs, cnts): (i64, i64) = sqlx::query_as(
            "SELECT COALESCE(SUM(doc), 0), COALESCE(SUM(cnt), 0)
             FROM fts_vocab WHERE term = 'zzuniqueterm'",
        )
        .fetch_one(&mut *conn)
        .await
        .unwrap();
        (docs, cnts)
    };
    assert_eq!(
        term_entries(&mut conn).await,
        (1, 1),
        "repeated append must not add extra index entries for an already indexed message"
    );
    let matches: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM messages_fts WHERE messages_fts MATCH 'zzuniqueterm'",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(matches, 1);

    sqlx::query("DELETE FROM messages WHERE guid = 'g-fts'")
        .execute(&mut *conn)
        .await
        .unwrap();
    assert_eq!(
        term_entries(&mut conn).await,
        (0, 0),
        "deleting the message must not leave stale search terms behind"
    );
}

#[tokio::test]
async fn deferred_fts_indexes_attachment_text_after_promote() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("vault.db");
    let assets = tmp.path().join("assets");
    fs::create_dir_all(&assets).unwrap();
    let att_dir = tmp.path().join("attachments");
    fs::create_dir_all(&att_dir).unwrap();
    fs::write(att_dir.join("receipt.pdf"), b"%PDF-fixture").unwrap();

    let path = write_jsonl(
        tmp.path(),
        "att.jsonl",
        r#"{"schema_version":4,"export":{"source":"imessage","tool":"test","tool_version":"0","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+15555550123","conversation_type":"individual","group_title":null,"participants":[{"handle":"+15555550123","display_name":null}],"stats":{"message_count":1,"attachment_count":1,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183462000}}}
{"guid":"g-att","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"sms","message_kind":"mms","sender_handle":"+15555550123","sender_display_name":null,"subject":null,"text":"see attached","attachments":[{"path":"attachments/receipt.pdf","original_name":"uniqueinvoice.pdf","mime_type":"application/pdf","digest_sha256":null,"is_sticker":false,"transcription":null,"sticker_effect":null}],"imessage":null,"source":null}
"#,
    );
    import_jsonl_files(
        &db,
        &[path],
        &ImportOptions::fixed(FixedImportArgs {
            assets_dir: &assets,
            asset_root: tmp.path(),
            contacts: None,
            overwrite_contacts: false,
            mode: ImportMode::Append,
            source: "imessage",
            account_id: TEST_ACCOUNT,
            fill_content_keys: false,
            import_id: None,
        }),
    )
    .await
    .unwrap();

    let (_pool, mut conn) = open_verify(&db).await;
    let hits: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM messages_fts WHERE messages_fts MATCH 'uniqueinvoice'",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        hits, 1,
        "attachment original_name must be searchable after deferred FTS"
    );
}

#[tokio::test]
async fn promote_stamps_messages_with_import_id() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("vault.db");
    let assets = tmp.path().join("assets");
    let path = write_jsonl(
        tmp.path(),
        "import-id.jsonl",
        r#"{"schema_version":4,"export":{"source":"imessage","tool":"test","tool_version":"0","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+15555550123","conversation_type":"individual","group_title":null,"participants":[{"handle":"+15555550123","display_name":null}],"stats":{"message_count":1,"attachment_count":0,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183462000}}}
{"guid":"g-import","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"sms","message_kind":"sms","sender_handle":"+15555550123","sender_display_name":null,"subject":null,"text":"linked","attachments":[],"imessage":null,"source":null}
"#,
    );

    let (_pool, mut conn) = open_verify(&db).await;
    schema::ensure_vault_schema(&mut conn).await.unwrap();
    crate::db::account_profile::ensure_account_row(&mut conn, TEST_ACCOUNT)
        .await
        .unwrap();
    let import_id = crate::db::vault_imports::start_import(
        &mut conn,
        &crate::db::vault_imports::StartImportArgs::new(
            TEST_ACCOUNT,
            "imessage",
            "append",
            Some("test"),
        ),
    )
    .await
    .unwrap();

    let stats = import_jsonl_files_on_conn(
        &mut conn,
        &[path],
        &ImportOptions::fixed(FixedImportArgs {
            assets_dir: &assets,
            asset_root: tmp.path(),
            contacts: None,
            overwrite_contacts: false,
            mode: ImportMode::Append,
            source: "imessage",
            account_id: TEST_ACCOUNT,
            fill_content_keys: false,
            import_id: Some(import_id),
        }),
        ImportSchemaMode::AssumeReady,
    )
    .await
    .unwrap();
    assert_eq!(stats.messages, 1);

    let stamped: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE import_id = $1")
        .bind(import_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(stamped, 1);

    let row = crate::db::vault_imports::complete_import(
        &mut conn,
        TEST_ACCOUNT,
        import_id,
        &crate::db::vault_imports::CompleteImportArgs {
            status: "completed".into(),
            message_count: Some(stats.messages as i64),
            attachment_count: Some(0),
            bytes_uploaded: Some(0),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(row.status, "completed");
    assert_eq!(row.message_count, 1);

    let (listed, total) = crate::db::vault_imports::list_imports_page(
        &mut conn,
        TEST_ACCOUNT,
        None,
        &crate::db::vault_imports::DEFAULT_IMPORT_SORT,
        10,
        0,
    )
    .await
    .unwrap();
    assert_eq!((listed.len(), total), (1, 1));
    assert_eq!(listed[0].source, "imessage");
    assert!(!listed[0].started_at.is_empty());
    assert!(listed[0].finished_at.is_some());
    assert_eq!(
        crate::db::storage::attachment_bytes(
            &mut conn,
            crate::db::storage::Scope::Account(TEST_ACCOUNT),
        )
        .await
        .unwrap(),
        0
    );
    assert!(
        crate::db::vault_imports::top_attachments_by_size(&mut conn, TEST_ACCOUNT, 5)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn trunk_zero_phone_imports_digits_with_review_note() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("vault.db");
    let assets = tmp.path().join("assets");
    let path = write_jsonl(
        tmp.path(),
        "trunk-zero.jsonl",
        r#"{"schema_version":4,"export":{"source":"imessage","tool":"test","tool_version":"0","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"020 7946 0000","conversation_type":"individual","group_title":null,"participants":[{"handle":"020 7946 0000","display_name":null}],"stats":{"message_count":1,"attachment_count":0,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183462000}}}
{"guid":"g-trunk-zero","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"sms","message_kind":"sms","sender_handle":"020 7946 0000","sender_display_name":null,"subject":null,"text":"hello","attachments":[],"imessage":null,"source":null}
"#,
    );

    let (_pool, mut conn) = open_verify(&db).await;
    schema::ensure_vault_schema(&mut conn).await.unwrap();
    crate::db::account_profile::ensure_account_row(&mut conn, TEST_ACCOUNT)
        .await
        .unwrap();

    let stats = import_jsonl_files_on_conn(
        &mut conn,
        &[path],
        &ImportOptions::fixed(FixedImportArgs {
            assets_dir: &assets,
            asset_root: tmp.path(),
            contacts: None,
            overwrite_contacts: false,
            mode: ImportMode::Append,
            source: "imessage",
            account_id: TEST_ACCOUNT,
            fill_content_keys: false,
            import_id: None,
        }),
        ImportSchemaMode::AssumeReady,
    )
    .await
    .unwrap();
    assert_eq!(stats.phones_needing_review, 1);

    // Guarded policy: normalized mirrors the digits (never +02079460000)
    // and the handles row carries a review note.
    let (normalized, note): (String, Option<String>) = sqlx::query_as(
        "SELECT normalized, normalized_note FROM handles
         WHERE account_id = $1 AND handle_type = 'phone'",
    )
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(normalized, "02079460000");
    assert!(
        note.as_deref().is_some(),
        "trunk-zero import must carry a review note"
    );
}

#[tokio::test]
async fn source_from_jsonl_stamps_export_source_and_assets() {
    use crate::config::PathsConfig;
    use media::MediaMode;

    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("vault.db");
    let data_dir = tmp.path().join("data");
    let paths = PathsConfig {
        db: db.clone(),
        data_dir: data_dir.clone(),
        assets_dir: "assets".into(),
        assets_converted_dir: "assets_converted".into(),
    };
    let placeholder = tmp.path().join("unused-assets");
    fs::create_dir_all(tmp.path().join("media")).unwrap();
    fs::write(tmp.path().join("media/photo.jpg"), b"jpeg-bytes").unwrap();

    let path = write_jsonl(
        tmp.path(),
        "c.jsonl",
        r#"{"schema_version":4,"export":{"source":"go-sms-pro","tool":"test","tool_version":"0","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+15555550100","conversation_type":"individual","group_title":null,"participants":[{"handle":"+15555550100","display_name":null}],"stats":{"message_count":1,"attachment_count":1,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183462000}}}
{"guid":"g1","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"sms","message_kind":"sms","sender_handle":"+15555550100","sender_display_name":null,"subject":null,"text":"hi","attachments":[{"path":"media/photo.jpg","original_name":"photo.jpg","mime_type":"image/jpeg","digest_sha256":null,"is_sticker":false,"transcription":null,"sticker_effect":null}],"imessage":null,"source":null}
"#,
    );
    let stats = import_jsonl_files(
        &db,
        &[path],
        &ImportOptions {
            assets_dir: &placeholder,
            asset_root: tmp.path(),
            contacts: None,
            overwrite_contacts: false,
            mode: ImportMode::Replace,
            source: "",
            account_id: TEST_ACCOUNT,
            fill_content_keys: true,
            import_id: None,
            source_from_jsonl: true,
            paths: Some(&paths),
            media: MediaMode::Clone,
            wipe_sources: Some(vec!["go-sms-pro".into()]),
        },
    )
    .await
    .unwrap();
    assert_eq!(stats.messages, 1);
    assert_eq!(stats.assets_copied, 1);

    let (_pool, mut conn) = open_verify(&db).await;
    let source: String = sqlx::query_scalar("SELECT source FROM messages WHERE guid = 'g1'")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(source, "go-sms-pro");
    let assets_root = paths.assets_dir_for_account(TEST_ACCOUNT, "go-sms-pro");
    assert!(assets_root.is_dir());
}

#[tokio::test]
async fn media_none_skips_attachment_copy() {
    use crate::config::PathsConfig;
    use media::MediaMode;

    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("vault.db");
    let data_dir = tmp.path().join("data");
    let paths = PathsConfig {
        db: db.clone(),
        data_dir,
        assets_dir: "assets".into(),
        assets_converted_dir: "assets_converted".into(),
    };
    let placeholder = tmp.path().join("unused-assets");
    fs::create_dir_all(tmp.path().join("media")).unwrap();
    fs::write(tmp.path().join("media/photo.jpg"), b"jpeg-bytes").unwrap();

    let path = write_jsonl(
        tmp.path(),
        "c.jsonl",
        r#"{"schema_version":4,"export":{"source":"sms","tool":"test","tool_version":"0","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+15555550100","conversation_type":"individual","group_title":null,"participants":[{"handle":"+15555550100","display_name":null}],"stats":{"message_count":1,"attachment_count":1,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183462000}}}
{"guid":"g1","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"sms","message_kind":"sms","sender_handle":"+15555550100","sender_display_name":null,"subject":null,"text":"hi","attachments":[{"path":"media/photo.jpg","original_name":"photo.jpg","mime_type":"image/jpeg","digest_sha256":null,"is_sticker":false,"transcription":null,"sticker_effect":null}],"imessage":null,"source":null}
"#,
    );
    let stats = import_jsonl_files(
        &db,
        &[path],
        &ImportOptions {
            assets_dir: &placeholder,
            asset_root: tmp.path(),
            contacts: None,
            overwrite_contacts: false,
            mode: ImportMode::Replace,
            source: "",
            account_id: TEST_ACCOUNT,
            fill_content_keys: false,
            import_id: None,
            source_from_jsonl: true,
            paths: Some(&paths),
            media: MediaMode::Disabled,
            wipe_sources: Some(vec!["sms".into()]),
        },
    )
    .await
    .unwrap();
    assert_eq!(stats.messages, 1);
    assert_eq!(stats.attachments, 0);
    assert_eq!(stats.assets_copied, 0);
}

#[tokio::test]
async fn name_only_participant_becomes_a_contact_with_no_identity() {
    sqlx::any::install_default_drivers();
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("vault.db");
    let assets = tmp.path().join("assets");
    // A rescue export: the source names the other party and records no
    // address for them anywhere.
    let path = write_jsonl(
        tmp.path(),
        "name-only.jsonl",
        r#"{"schema_version":4,"export":{"source":"openextract","tool":"test","tool_version":"0","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"Sarah_Vale","conversation_type":"individual","group_title":null,"participants":[{"display_name":"Sarah Vale"}],"stats":{"message_count":1,"attachment_count":0,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183462000}}}
{"guid":"g-name-only","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"sms","message_kind":"sms","sender_handle":null,"sender_display_name":"Sarah Vale","subject":null,"text":"hi","attachments":[],"imessage":null,"source":null}
"#,
    );
    let opts = ImportOptions::fixed(FixedImportArgs {
        assets_dir: &assets,
        asset_root: tmp.path(),
        contacts: None,
        overwrite_contacts: false,
        mode: ImportMode::Append,
        source: "openextract",
        account_id: TEST_ACCOUNT,
        fill_content_keys: false,
        import_id: None,
    });
    import_jsonl_files(&db, &[path], &opts).await.unwrap();

    let (_pool, mut conn) = open_verify(&db).await;

    // The name is carried by a contact, because nothing else can hold a
    // name with no address.
    let name: String = sqlx::query_scalar(
        "SELECT preferred_name FROM contacts WHERE account_id = $1 AND preferred_name = 'Sarah Vale'",
    )
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(name, "Sarah Vale");

    // No address was invented for her.
    let identity_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM handles h
         WHERE h.account_id = $1 AND h.raw = 'Sarah Vale'",
    )
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(identity_count, 0, "the source recorded no address for her");

    // The promoted participant points at the contact and carries no identity.
    let rows: Vec<(Option<i64>, Option<i64>)> = sqlx::query_as(
        "SELECT p.handle_id, p.contact_id FROM participants p
         JOIN conversations c ON c.id = p.conversation_id
         WHERE c.account_id = $1",
    )
    .bind(TEST_ACCOUNT)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert!(
        rows.iter().any(|(h, c)| h.is_none() && c.is_some()),
        "expected a participant with a contact and no identity, got {rows:?}"
    );
}

/// A group chat's identifier names the conversation, not a person, so only
/// the people in the group become contacts.
#[tokio::test]
async fn a_group_chat_identifier_never_becomes_a_contact() {
    sqlx::any::install_default_drivers();
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("vault.db");
    let assets = tmp.path().join("assets");
    let path = write_jsonl(
        tmp.path(),
        "group.jsonl",
        r#"{"schema_version":4,"export":{"source":"imessage","tool":"test","tool_version":"0","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"chat1000000005","conversation_type":"group","group_title":"Trip","participants":[{"handle":"+15555550123","display_name":null},{"handle":"+15555550999","display_name":null}],"stats":{"message_count":1,"attachment_count":0,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183462000}}}
{"guid":"g-group","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"imessage","message_kind":"imessage","sender_handle":"+15555550123","sender_display_name":null,"subject":null,"text":"hi","attachments":[],"imessage":null,"source":null}
"#,
    );
    let opts = replace_opts(&assets, tmp.path(), "imessage");
    import_jsonl_files(&db, &[path], &opts).await.unwrap();

    let (_pool, mut conn) = open_verify(&db).await;

    let linked: Vec<String> = sqlx::query_scalar(
        "SELECT h.raw FROM contact_handles ch
         JOIN handles h ON h.id = ch.handle_id
         WHERE ch.account_id = $1
         ORDER BY h.raw",
    )
    .bind(TEST_ACCOUNT)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(linked, ["+15555550123", "+15555550999"]);

    let contacts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM contacts WHERE account_id = $1")
        .bind(TEST_ACCOUNT)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(contacts, 2, "one contact per person in the group");

    // The conversation itself is still stored under its group identifier.
    let chat: String = sqlx::query_scalar(
        "SELECT h.raw FROM conversations c
         JOIN handles h ON h.id = c.chat_handle_id
         WHERE c.account_id = $1",
    )
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(chat, "chat1000000005");
}

/// `resolve_name_only_participant` returns `(None, None)` when the source
/// recorded neither an address nor a name for a participant, but the
/// insert that follows it in `staging.rs` runs unconditionally — so this
/// pins that a participant record carrying neither still cannot reach the
/// `participants` table with `handle_id` and `name_alias` both NULL, the
/// shape `participant_names::load_for_conversations`'s COALESCE-to-`''`
/// fallback assumes never exists.
#[tokio::test]
async fn a_participant_with_no_address_and_no_name_is_never_created() {
    sqlx::any::install_default_drivers();
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("vault.db");
    let assets = tmp.path().join("assets");
    // Neither the roster entry nor the message's sender names this person
    // or records any address for them.
    let path = write_jsonl(
        tmp.path(),
        "nameless.jsonl",
        r#"{"schema_version":4,"export":{"source":"openextract","tool":"test","tool_version":"0","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"Nameless_Chat","conversation_type":"individual","group_title":null,"participants":[{"display_name":null}],"stats":{"message_count":1,"attachment_count":0,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183462000}}}
{"guid":"g-nameless","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"sms","message_kind":"sms","sender_handle":null,"sender_display_name":null,"subject":null,"text":"hi","attachments":[],"imessage":null,"source":null}
"#,
    );
    let opts = ImportOptions::fixed(FixedImportArgs {
        assets_dir: &assets,
        asset_root: tmp.path(),
        contacts: None,
        overwrite_contacts: false,
        mode: ImportMode::Append,
        source: "openextract",
        account_id: TEST_ACCOUNT,
        fill_content_keys: false,
        import_id: None,
    });
    import_jsonl_files(&db, &[path], &opts).await.unwrap();

    let (_pool, mut conn) = open_verify(&db).await;
    let rows: Vec<(Option<i64>, Option<String>)> = sqlx::query_as(
        "SELECT p.handle_id, p.name_alias FROM participants p
         JOIN conversations c ON c.id = p.conversation_id
         WHERE c.account_id = $1",
    )
    .bind(TEST_ACCOUNT)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert!(
        rows.iter().all(|(h, n)| h.is_some() || n.is_some()),
        "expected no participant with both handle_id and name_alias NULL, got {rows:?}"
    );
}

#[tokio::test]
async fn persists_missing_reason_with_null_sha256() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("vault.db");
    let assets = tmp.path().join("assets");
    fs::create_dir_all(&assets).unwrap();

    let path = write_jsonl(
        tmp.path(),
        "missing-att.jsonl",
        r#"{"schema_version":4,"export":{"source":"sms-backup-restore","tool":"test","tool_version":"0","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+15555550123","conversation_type":"individual","group_title":null,"participants":[{"handle":"+15555550123","display_name":null}],"stats":{"message_count":1,"attachment_count":1,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183462000}}}
{"guid":"g-missing","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"sms","message_kind":"mms","sender_handle":"+15555550123","sender_display_name":null,"subject":null,"text":"see attached","attachments":[{"path":"attachments/gone.bin","original_name":"gone.bin","mime_type":"application/octet-stream","digest_sha256":null,"is_sticker":false,"transcription":null,"sticker_effect":null,"size_bytes":999,"missing_reason":"too_large"}],"imessage":null,"source":null}
"#,
    );
    let stats = import_jsonl_files(
        &db,
        &[path],
        &ImportOptions::fixed(FixedImportArgs {
            assets_dir: &assets,
            asset_root: tmp.path(),
            contacts: None,
            overwrite_contacts: false,
            mode: ImportMode::Append,
            source: "sms-backup-restore",
            account_id: TEST_ACCOUNT,
            fill_content_keys: false,
            import_id: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(stats.messages, 1);
    assert_eq!(stats.attachments, 1);

    let (_pool, mut conn) = open_verify(&db).await;
    let (sha256, missing_reason, size_bytes, original_name): (
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT sha256, missing_reason, size_bytes, original_name FROM attachments LIMIT 1",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert!(sha256.is_none());
    assert_eq!(missing_reason.as_deref(), Some("too_large"));
    assert_eq!(size_bytes, Some(999));
    assert_eq!(original_name.as_deref(), Some("gone.bin"));
}

#[tokio::test]
async fn claimed_import_rejects_corrupt_existing_asset() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("vault.db");
    let assets = tmp.path().join("assets");
    let sha = assets_api::sha256_hex(b"expected-asset");
    let corrupt = assets.join(assets_api::shard_rel_path(&sha, ".bin"));
    fs::create_dir_all(corrupt.parent().unwrap()).unwrap();
    fs::write(&corrupt, b"corrupt-asset").unwrap();

    let message = format!(
        r#"{{"guid":"g-corrupt-asset","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"imessage","message_kind":"imessage","sender_handle":"+15555550123","sender_display_name":null,"subject":null,"text":"missing asset","attachments":[{{"path":"attachments/missing.bin","original_name":"missing.bin","mime_type":"application/octet-stream","digest_sha256":"{sha}","is_sticker":false,"transcription":null,"sticker_effect":null}}],"imessage":null,"source":null}}"#
    );
    let jsonl = format!(
        "{}\n{}\n",
        r#"{"schema_version":4,"export":{"source":"imessage","tool":"test","tool_version":"0","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+15555550123","conversation_type":"individual","group_title":null,"participants":[{"handle":"+15555550123","display_name":null}],"stats":{"message_count":1,"attachment_count":1,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183462000}}}"#,
        message
    );
    let path = write_jsonl(tmp.path(), "corrupt-existing.jsonl", &jsonl);

    let stats = import_jsonl_files(
        &db,
        &[path],
        &ImportOptions::fixed(FixedImportArgs {
            assets_dir: &assets,
            asset_root: tmp.path(),
            contacts: None,
            overwrite_contacts: false,
            mode: ImportMode::Append,
            source: "imessage",
            account_id: TEST_ACCOUNT,
            fill_content_keys: false,
            import_id: None,
        }),
    )
    .await
    .unwrap();

    assert_eq!(stats.assets_deduped, 0);
    assert_eq!(stats.assets_missing, 1);
    let (_pool, mut conn) = open_verify(&db).await;
    let assets_path: Option<String> =
        sqlx::query_scalar("SELECT assets_path FROM attachments LIMIT 1")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert!(assets_path.is_none());
}

#[tokio::test]
async fn rejects_attachment_path_traversal() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("vault.db");
    let assets = tmp.path().join("assets");
    let export_dir = tmp.path().join("export");
    fs::create_dir_all(&assets).unwrap();
    fs::create_dir_all(&export_dir).unwrap();
    fs::write(tmp.path().join("secret.txt"), b"secret-bytes").unwrap();

    let path = write_jsonl(
        &export_dir,
        "traverse.jsonl",
        r#"{"schema_version":4,"export":{"source":"sms-backup-restore","tool":"test","tool_version":"0","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+15555550123","conversation_type":"individual","group_title":null,"participants":[{"handle":"+15555550123","display_name":null}],"stats":{"message_count":1,"attachment_count":1,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183462000}}}
{"guid":"g-trav","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"sms","message_kind":"mms","sender_handle":"+15555550123","sender_display_name":null,"subject":null,"text":"x","attachments":[{"path":"../secret.txt","original_name":"secret.txt","mime_type":"text/plain","digest_sha256":null,"is_sticker":false,"transcription":null,"sticker_effect":null,"size_bytes":12,"missing_reason":null}],"imessage":null,"source":null}
"#,
    );
    let err = import_jsonl_files(
        &db,
        &[path],
        &ImportOptions::fixed(FixedImportArgs {
            assets_dir: &assets,
            asset_root: &export_dir,
            contacts: None,
            overwrite_contacts: false,
            mode: ImportMode::Append,
            source: "sms-backup-restore",
            account_id: TEST_ACCOUNT,
            fill_content_keys: false,
            import_id: None,
        }),
    )
    .await
    .unwrap_err();
    assert!(
        err.to_string().contains(message_ir::UNSAFE_ATTACHMENT_PATH),
        "expected path rejection, got: {err}"
    );
}

#[tokio::test]
async fn failed_replace_keeps_existing_messages() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("vault.db");
    let assets = tmp.path().join("assets");
    let export_dir = tmp.path().join("export");
    fs::create_dir_all(&assets).unwrap();
    fs::create_dir_all(&export_dir).unwrap();

    let first = write_jsonl(
        &export_dir,
        "ok.jsonl",
        r#"{"schema_version":4,"export":{"source":"sms-backup-restore","tool":"test","tool_version":"0","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+14075551234","conversation_type":"individual","group_title":null,"participants":[{"handle":"+14075551234","display_name":null}],"stats":{"message_count":1,"attachment_count":0,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183462000}}}
{"guid":"g-keep-replace","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"sms","message_kind":"sms","sender_handle":"+14075551234","sender_display_name":null,"subject":null,"text":"keep me","attachments":[],"imessage":null,"source":null}
"#,
    );
    import_jsonl_files(
        &db,
        &[first],
        &ImportOptions::fixed(FixedImportArgs {
            assets_dir: &assets,
            asset_root: &export_dir,
            contacts: None,
            overwrite_contacts: false,
            mode: ImportMode::Replace,
            source: "sms-backup-restore",
            account_id: TEST_ACCOUNT,
            fill_content_keys: false,
            import_id: None,
        }),
    )
    .await
    .unwrap();

    let bad = write_jsonl(
        &export_dir,
        "bad.jsonl",
        r#"{"schema_version":4,"export":{"source":"sms-backup-restore","tool":"test","tool_version":"0","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+14075551234","conversation_type":"individual","group_title":null,"participants":[{"handle":"+14075551234","display_name":null}],"stats":{"message_count":1,"attachment_count":1,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183462000}}}
{"guid":"g-bad","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"sms","message_kind":"mms","sender_handle":"+14075551234","sender_display_name":null,"subject":null,"text":"nope","attachments":[{"path":"../secret.txt","original_name":"secret.txt","mime_type":"text/plain","digest_sha256":null,"is_sticker":false,"transcription":null,"sticker_effect":null,"size_bytes":1,"missing_reason":null}],"imessage":null,"source":null}
"#,
    );
    let err = import_jsonl_files(
        &db,
        &[bad],
        &ImportOptions::fixed(FixedImportArgs {
            assets_dir: &assets,
            asset_root: &export_dir,
            contacts: None,
            overwrite_contacts: false,
            mode: ImportMode::Replace,
            source: "sms-backup-restore",
            account_id: TEST_ACCOUNT,
            fill_content_keys: false,
            import_id: None,
        }),
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains(message_ir::UNSAFE_ATTACHMENT_PATH));

    let (_pool, mut conn) = open_verify(&db).await;
    let body: String =
        sqlx::query_scalar("SELECT body FROM messages WHERE guid = 'g-keep-replace'")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(body, "keep me");
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(n, 1);
}

/// One individual conversation with `handle`, holding one incoming message.
fn one_message_conversation(guid: &str, handle: &str) -> String {
    format!(
        r#"{{"schema_version":4,"export":{{"source":"sms-backup-restore","tool":"test","tool_version":"0","owner_handle":null,"owner_display_name":null}},"conversation":{{"chat_identifier":"{handle}","conversation_type":"individual","group_title":null,"participants":[{{"handle":"{handle}","display_name":null}}],"stats":{{"message_count":1,"attachment_count":0,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183462000}}}}}}
{{"guid":"{guid}","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"sms","message_kind":"sms","sender_handle":"{handle}","sender_display_name":null,"subject":null,"text":"hi","attachments":[],"imessage":null,"source":null}}
"#
    )
}

/// Make every later INSERT INTO messages fail, so an import gets through
/// staging and fails inside promote.
async fn fail_every_message_insert(conn: &mut AnyConnection) {
    let statements: &[&str] = match dialect::engine_of(conn) {
        engine::DbEngine::Sqlite => &["CREATE TRIGGER fail_promote BEFORE INSERT ON messages
             BEGIN SELECT RAISE(ABORT, 'promote fails on purpose'); END"],
        engine::DbEngine::Postgres => &[
            "CREATE FUNCTION fail_promote() RETURNS trigger LANGUAGE plpgsql AS
             $$ BEGIN RAISE EXCEPTION 'promote fails on purpose'; END $$",
            "CREATE TRIGGER fail_promote BEFORE INSERT ON messages
             FOR EACH ROW EXECUTE FUNCTION fail_promote()",
        ],
    };
    for sql in statements {
        sqlx::raw_sql(sql).execute(&mut *conn).await.unwrap();
    }
}

/// An import that fails in promote imports nothing, and that includes its
/// contacts. Staging meets every handle first and, by ADR-0013, discards a
/// trashed contact whose handle the backup holds, and makes contacts for
/// people new to the vault. Those writes must not outlive a promote that
/// rolled back: the person keeps the trashed contact's name and groups, and
/// the vault gains no contacts with no messages.
#[tokio::test]
async fn failed_promote_keeps_the_trashed_contact_and_adds_no_contacts() {
    let (pool, dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    let assets = dir.path().join("assets");
    let export_dir = dir.path().join("export");
    fs::create_dir_all(&export_dir).unwrap();
    let opts = ImportOptions::fixed(FixedImportArgs {
        assets_dir: &assets,
        asset_root: &export_dir,
        contacts: None,
        overwrite_contacts: false,
        mode: ImportMode::Append,
        source: "sms-backup-restore",
        account_id: TEST_ACCOUNT,
        fill_content_keys: false,
        import_id: None,
    });

    // A first import makes Ada's contact; the person names it, puts it in a
    // group, and moves it to the Trash.
    let first = write_jsonl(
        &export_dir,
        "ada.jsonl",
        &one_message_conversation("g-ada-1", "+15555550960"),
    );
    import_jsonl_files_on_conn(&mut conn, &[first], &opts, ImportSchemaMode::Ensure)
        .await
        .unwrap();
    let ada: i64 = sqlx::query_scalar("SELECT id FROM contacts WHERE account_id = $1")
        .bind(TEST_ACCOUNT)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    sqlx::query("UPDATE contacts SET preferred_name = 'Ada (work)', origin = 'user' WHERE id = $1")
        .bind(ada)
        .execute(&mut *conn)
        .await
        .unwrap();
    let group_id: i64 = sqlx::query_scalar(
        "INSERT INTO contact_groups (account_id, name) VALUES ($1, 'Colleagues') RETURNING id",
    )
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    sqlx::query("INSERT INTO contact_group_members (contact_id, group_id) VALUES ($1, $2)")
        .bind(ada)
        .bind(group_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    assert!(
        crate::db::trash::move_to_trash(
            &mut conn,
            TEST_ACCOUNT,
            crate::db::trash::Trashable::Contact(ada)
        )
        .await
        .unwrap()
    );

    // A second backup holds Ada again and someone new; its promote fails.
    fail_every_message_insert(&mut conn).await;
    let again = write_jsonl(
        &export_dir,
        "ada-again.jsonl",
        &one_message_conversation("g-ada-2", "+15555550960"),
    );
    let newcomer = write_jsonl(
        &export_dir,
        "newcomer.jsonl",
        &one_message_conversation("g-new-1", "+15555550961"),
    );
    let err = import_jsonl_files_on_conn(
        &mut conn,
        &[again, newcomer],
        &opts,
        ImportSchemaMode::AssumeReady,
    )
    .await
    .unwrap_err();
    assert!(
        format!("{err:#}").contains("promote fails on purpose"),
        "expected the promote failure, got: {err:#}"
    );

    let contacts: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, preferred_name FROM contacts WHERE account_id = $1")
            .bind(TEST_ACCOUNT)
            .fetch_all(&mut *conn)
            .await
            .unwrap();
    assert_eq!(
        contacts,
        vec![(ada, "Ada (work)".to_string())],
        "Ada keeps her name and the failed import leaves no new contact"
    );
    let trashed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM trashed_contacts WHERE account_id = $1 AND contact_id = $2",
    )
    .bind(TEST_ACCOUNT)
    .bind(ada)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(trashed, 1, "Ada is still in the Trash");
    let groups: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM contact_group_members WHERE contact_id = $1")
            .bind(ada)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(groups, 1, "Ada is still in her group");
}

/// A registered account may import with its session token: `can_import`
/// is on by default, which `server.rs`'s `can_import = 0` test relies on
/// to prove the opposite case.
async fn importer() -> (
    crate::server::AppState,
    crate::test_support::TestVault,
    String,
) {
    let vault = crate::test_support::test_vault().await;
    let account =
        crate::test_support::register_via_api(&vault.state, "importer", "hunter2hunter2").await;
    let state = vault.state.clone();
    (state, vault, account.token)
}

/// Create an Import Run for `source` and hand back the path its batches
/// are posted to.
async fn batches_path(state: &crate::server::AppState, token: &str, source: &str) -> String {
    let (_, created): (String, serde_json::Value) = post_created_json(
        state,
        "/v1/imports",
        token,
        serde_json::json!({ "source": source }),
    )
    .await;
    format!("/v1/imports/{}/batches", created["id"].as_i64().unwrap())
}

#[tokio::test]
async fn http_import_of_a_schema_3_file_is_a_400_naming_both_versions() {
    let (state, _vault, token) = importer().await;
    let path = batches_path(&state, &token, "whatsapp").await;
    let body = concat!(
        r#"{"schema_version":3,"export":{"source":"whatsapp","tool":"t","owner_handle":"+15550000001","owner_display_name":"Me"},"#,
        r#""conversation":{"chat_identifier":"+15550000002","conversation_type":"individual","participants":[{"handle":"+15550000002","display_name":"Sam"}]}}"#,
        "\n",
    );
    let (status, text) =
        crate::test_support::post_raw(&state, &path, &token, "application/jsonl", body).await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST, "{text}");
    let err: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        err["detail"],
        "This file is schema version 3; the vault reads version 4 (line 1)."
    );
}

#[tokio::test]
async fn http_import_of_a_line_that_is_not_json_is_a_400_naming_the_line() {
    let (state, _vault, token) = importer().await;
    let path = batches_path(&state, &token, "whatsapp").await;
    let (status, text) = crate::test_support::post_raw(
        &state,
        &path,
        &token,
        "application/jsonl",
        "this is not json\n",
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST, "{text}");
    let err: serde_json::Value = serde_json::from_str(&text).unwrap();
    let message = err["detail"].as_str().unwrap();
    assert!(
        message.starts_with("Could not read line 1 of the file:"),
        "{message}"
    );
}

/// A batch is refused before its body is read when the run is over: the
/// row, not the request, says what a batch imports under, and a discarded
/// run has nothing to import under.
#[tokio::test]
async fn a_batch_into_a_run_that_is_not_running_is_a_state_conflict() {
    let (state, _vault, token) = importer().await;
    let path = batches_path(&state, &token, "whatsapp").await;
    let id = path
        .trim_start_matches("/v1/imports/")
        .trim_end_matches("/batches")
        .to_string();
    post_json::<serde_json::Value>(
        &state,
        &format!("/v1/imports/{id}/discard"),
        &token,
        serde_json::json!({}),
    )
    .await;

    let (status, text) =
        crate::test_support::post_raw(&state, &path, &token, "application/jsonl", "{}\n").await;
    let problem = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::StateConflict,
    );
    assert_eq!(
        problem.detail.as_deref(),
        Some(format!("import {id} is not running (status=cancelled)").as_str())
    );
}

/// One conversation with `chat`, holding one message per guid, as a
/// replace run's batch.
fn replace_run_batch(chat: &str, guids: &[&str]) -> String {
    let mut lines = vec![format!(
        concat!(
            r#"{{"schema_version":4,"export":{{"source":"whatsapp","tool":"t","tool_version":"0","owner_handle":"+15550000001","owner_display_name":"Me"}},"#,
            r#""conversation":{{"chat_identifier":"{chat}","conversation_type":"individual","group_title":null,"#,
            r#""participants":[{{"handle":"{chat}","display_name":null}}],"#,
            r#""stats":{{"message_count":{n},"attachment_count":0,"first_timestamp_unix_ms":1700000000000,"last_timestamp_unix_ms":1700000000000}}}}}}"#,
        ),
        chat = chat,
        n = guids.len(),
    )];
    for guid in guids {
        lines.push(format!(
            r#"{{"guid":"{guid}","timestamp_unix_ms":1700000000000,"direction":"incoming","service":"whatsapp","message_kind":"sms","sender_handle":"{chat}","sender_display_name":null,"subject":null,"text":"{guid}","attachments":[],"imessage":null,"source":null}}"#
        ));
    }
    lines.join("\n") + "\n"
}

/// A replace run wipes the source on its first batch only, and the push
/// client posts a batch again when the first attempt times out. So the
/// second batch must not wipe the first, and a retried batch must add
/// nothing. This guards both against a change to how a batch picks wipe
/// or append (today: whether the run has stamped a message yet).
///
/// Every message carries a guid, as every exporter writes one. A message
/// with an empty guid would be inserted again by the retry, in a replace
/// run or an append run alike, because append skips by guid only.
#[tokio::test]
async fn a_retried_batch_in_a_replace_run_keeps_every_message_once() {
    let (state, _vault, token) = importer().await;
    let (_, created): (String, serde_json::Value) = post_created_json(
        &state,
        "/v1/imports",
        &token,
        serde_json::json!({ "source": "whatsapp", "mode": "replace" }),
    )
    .await;
    let path = format!("/v1/imports/{}/batches", created["id"].as_i64().unwrap());

    let first = replace_run_batch("+15550000002", &["g-1a", "g-1b"]);
    let second = replace_run_batch("+15550000003", &["g-2a", "g-2b"]);
    for body in [&first, &second, &second] {
        let (status, text) =
            crate::test_support::post_raw(&state, &path, &token, "application/jsonl", body.clone())
                .await;
        assert_eq!(status, axum::http::StatusCode::OK, "{text}");
    }

    let mut conn = state.db.acquire().await.unwrap();
    let guids: Vec<String> =
        sqlx::query_scalar("SELECT guid FROM messages WHERE source = 'whatsapp' ORDER BY guid")
            .fetch_all(&mut *conn)
            .await
            .unwrap();
    assert_eq!(guids, ["g-1a", "g-1b", "g-2a", "g-2b"]);
}

/// One `source` conversation with `+15550000002`, one message per guid. The
/// message `g-gone` carries a missing attachment and a tapback, so a wipe
/// that leaves children behind shows.
fn wipe_test_batch(source: &str, guids: &[&str]) -> String {
    let mut lines = vec![format!(
        concat!(
            r#"{{"schema_version":4,"export":{{"source":"{source}","tool":"t","tool_version":"0","owner_handle":"+15550000001","owner_display_name":"Me"}},"#,
            r#""conversation":{{"chat_identifier":"+15550000002","conversation_type":"individual","group_title":null,"#,
            r#""participants":[{{"handle":"+15550000002","display_name":null}}],"#,
            r#""stats":{{"message_count":{n},"attachment_count":0,"first_timestamp_unix_ms":1700000000000,"last_timestamp_unix_ms":1700000000000}}}}}}"#,
        ),
        source = source,
        n = guids.len(),
    )];
    let tapback = TAPBACK_IMESSAGE.replace("+15555550999", "+15550000002");
    for guid in guids {
        let (attachments, imessage) = if *guid == "g-gone" {
            (missing_attachment_json("gone.bin"), tapback.as_str())
        } else {
            ("[]".to_string(), "null")
        };
        lines.push(format!(
            r#"{{"guid":"{guid}","timestamp_unix_ms":1700000000000,"direction":"incoming","service":"sms","message_kind":"sms","sender_handle":"+15550000002","sender_display_name":null,"subject":null,"text":"{guid}","attachments":{attachments},"imessage":{imessage},"source":null}}"#
        ));
    }
    lines.join("\n") + "\n"
}

/// Create an Import Run for `source` in `mode`, post `body` as its one
/// batch, and complete the run so the account may start another.
async fn import_one_batch(
    state: &crate::server::AppState,
    token: &str,
    source: &str,
    mode: &str,
    body: String,
) {
    let (_, created): (String, serde_json::Value) = post_created_json(
        state,
        "/v1/imports",
        token,
        serde_json::json!({ "source": source, "mode": mode }),
    )
    .await;
    let id = created["id"].as_i64().unwrap();
    let (status, text) = crate::test_support::post_raw(
        state,
        &format!("/v1/imports/{id}/batches"),
        token,
        "application/jsonl",
        body,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK, "{text}");
    let _: serde_json::Value = post_json(
        state,
        &format!("/v1/imports/{id}/complete"),
        token,
        serde_json::json!({ "status": "completed" }),
    )
    .await;
}

/// A replace run deletes the account's existing messages for its source
/// before it promotes the new ones, so a message dropped from the new
/// export leaves the vault, with its attachments and tapbacks. Nothing
/// else moves: another source's messages, another account's messages for
/// the same source, and a message that pointed at a deleted one as its
/// duplicate (which now points nowhere).
#[tokio::test]
async fn a_replace_run_deletes_only_its_own_sources_old_messages() {
    let (state, _vault, token) = importer().await;
    let other = crate::test_support::register_via_api(&state, "other", "hunter2hunter2").await;

    import_one_batch(
        &state,
        &token,
        "whatsapp",
        "replace",
        wipe_test_batch("whatsapp", &["g-keep", "g-gone"]),
    )
    .await;
    import_one_batch(
        &state,
        &token,
        "sms-backup-restore",
        "append",
        wipe_test_batch("sms-backup-restore", &["g-sms"]),
    )
    .await;
    import_one_batch(
        &state,
        &other.token,
        "whatsapp",
        "replace",
        wipe_test_batch("whatsapp", &["g-other", "g-gone"]),
    )
    .await;

    let mut conn = state.db.acquire().await.unwrap();
    let children = |table: &'static str| {
        format!(
            "SELECT COUNT(*) FROM {table} t JOIN messages m ON m.id = t.message_id \
             WHERE m.guid = 'g-gone'"
        )
    };
    for table in ["attachments", "tapbacks"] {
        let n: i64 = sqlx::query_scalar(&children(table))
            .fetch_one(&mut *conn)
            .await
            .unwrap();
        assert_eq!(n, 2, "both accounts' g-gone carry one row in {table}");
    }
    sqlx::query(
        "UPDATE messages SET duplicate_of = (
             SELECT id FROM messages WHERE guid = 'g-gone' AND account_id != $1
         ) WHERE guid = 'g-sms'",
    )
    .bind(other.account_id)
    .execute(&mut *conn)
    .await
    .unwrap();
    drop(conn);

    import_one_batch(
        &state,
        &token,
        "whatsapp",
        "replace",
        wipe_test_batch("whatsapp", &["g-keep"]),
    )
    .await;

    let mut conn = state.db.acquire().await.unwrap();
    let rows: Vec<(i64, String, String)> =
        sqlx::query_as("SELECT account_id, source, guid FROM messages")
            .fetch_all(&mut *conn)
            .await
            .unwrap();
    let mut rows: Vec<(bool, &str, &str)> = rows
        .iter()
        .map(|(a, s, g)| (*a == other.account_id, s.as_str(), g.as_str()))
        .collect();
    rows.sort_unstable();
    assert_eq!(
        rows,
        [
            (false, "sms-backup-restore", "g-sms"),
            (false, "whatsapp", "g-keep"),
            (true, "whatsapp", "g-gone"),
            (true, "whatsapp", "g-other"),
        ]
    );
    for table in ["attachments", "tapbacks"] {
        let n: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(&mut *conn)
            .await
            .unwrap();
        assert_eq!(
            n, 1,
            "only the other account's g-gone keeps a row in {table}"
        );
    }
    let duplicate_of: Option<i64> =
        sqlx::query_scalar("SELECT duplicate_of FROM messages WHERE guid = 'g-sms'")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(duplicate_of, None, "no message may point at a deleted one");
}

/// The import body is JSON Lines and nothing else. `multipart/form-data`
/// used to be accepted (a `jsonl` field plus `file` parts) but nothing
/// sent it: vault-push posts JSON Lines and uploads attachments through
/// `/v1/assets`. The wrong media type is a 415, not a 400: the request is
/// well formed, it is simply not something this route reads.
#[tokio::test]
async fn a_multipart_body_is_an_unsupported_media_type() {
    let (vault, user) = crate::test_support::vault_with_account().await;

    let boundary = "MessageVaultTestBoundary";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"jsonl\"\r\n\r\n{{}}\r\n--{boundary}--\r\n"
    );
    let path = batches_path(&vault.state, &user.token, "imessage").await;
    let (status, text) = crate::test_support::post_raw(
        &vault.state,
        &path,
        &user.token,
        &format!("multipart/form-data; boundary={boundary}"),
        body,
    )
    .await;
    let parsed: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|_| panic!("non-JSON body: {text}"));
    assert_eq!(
        status,
        axum::http::StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "{text}"
    );
    assert_eq!(
        parsed["detail"],
        "Content-Type must be application/x-ndjson or application/jsonl"
    );
}

/// A run belongs to the account whose token created it. Another account's
/// token posting into it finds no such run: the id is scoped to the
/// account, so an outsider cannot tell it exists.
#[tokio::test]
async fn a_batch_into_another_accounts_run_is_not_found() {
    let (vault, alice) = crate::test_support::vault_with_account().await;
    let bob = crate::test_support::register_via_api(&vault.state, "bob", "hunter2hunter2").await;
    let bobs_run = batches_path(&vault.state, &bob.token, "imessage").await;

    let (status, text) = crate::test_support::post_raw(
        &vault.state,
        &bobs_run,
        &alice.token,
        "application/jsonl",
        "{}\n",
    )
    .await;
    crate::test_support::expect_problem(status, &text, crate::problem::ProblemType::NotFound);

    // Positive control: her own run takes the batch as far as reading it.
    let own = batches_path(&vault.state, &alice.token, "imessage").await;
    let (status, _) = crate::test_support::post_raw(
        &vault.state,
        &own,
        &alice.token,
        "application/jsonl",
        "{}\n",
    )
    .await;
    assert_ne!(status, axum::http::StatusCode::NOT_FOUND);
}

/// Import `path` in append mode on `conn`, as the serve path does once the
/// schema is in place.
async fn append_on_conn(conn: &mut AnyConnection, path: &Path, root: &Path, source: &str) {
    let assets = root.join("assets");
    import_jsonl_files_on_conn(
        conn,
        &[path.to_path_buf()],
        &ImportOptions::fixed(FixedImportArgs {
            assets_dir: &assets,
            asset_root: root,
            contacts: None,
            overwrite_contacts: false,
            mode: ImportMode::Append,
            source,
            account_id: TEST_ACCOUNT,
            fill_content_keys: false,
            import_id: None,
        }),
        ImportSchemaMode::AssumeReady,
    )
    .await
    .unwrap();
}

/// Each conversation of `TEST_ACCOUNT` with its number of participant rows,
/// and the account's number of contacts.
async fn participant_and_contact_counts(conn: &mut AnyConnection) -> (Vec<(i64, i64)>, i64) {
    let rows: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT c.id, COUNT(p.id) FROM conversations c
         LEFT JOIN participants p ON p.conversation_id = c.id
         WHERE c.account_id = $1
         GROUP BY c.id ORDER BY c.id",
    )
    .bind(TEST_ACCOUNT)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    let contacts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM contacts WHERE account_id = $1")
        .bind(TEST_ACCOUNT)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    (rows, contacts)
}

/// A participant the source named with no address has no handle, and a
/// uniqueness rule that includes a NULL column never matches. Importing the
/// same file twice must still leave one row per person in the conversation.
#[tokio::test]
async fn reimporting_a_file_with_a_name_only_participant_adds_no_participant() {
    let vault = test_vault().await;
    let tmp = TempDir::new().unwrap();
    let path = write_jsonl(
        tmp.path(),
        "group-with-name-only.jsonl",
        r#"{"schema_version":4,"export":{"source":"openextract","tool":"test","tool_version":"0","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"chat2000000001","conversation_type":"group","group_title":"Trip","participants":[{"handle":"+15555550123","display_name":"Ada"},{"display_name":"Sarah Vale"}],"stats":{"message_count":1,"attachment_count":0,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183462000}}}
{"guid":"g-name-only-twice","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"sms","message_kind":"sms","sender_handle":"+15555550123","sender_display_name":null,"subject":null,"text":"hi","attachments":[],"imessage":null,"source":null}
"#,
    );
    let mut conn = vault.state.db.acquire().await.unwrap();

    append_on_conn(&mut conn, &path, tmp.path(), "openextract").await;
    let (first_rows, first_contacts) = participant_and_contact_counts(&mut conn).await;
    assert_eq!(first_rows.len(), 1, "one conversation: {first_rows:?}");
    assert_eq!(first_rows[0].1, 2, "Ada and Sarah Vale: {first_rows:?}");
    assert_eq!(first_contacts, 2);

    append_on_conn(&mut conn, &path, tmp.path(), "openextract").await;
    let (second_rows, second_contacts) = participant_and_contact_counts(&mut conn).await;
    assert_eq!(
        second_rows, first_rows,
        "the second import added participants"
    );
    assert_eq!(second_contacts, first_contacts);
}

/// ADR-0013: an import that meets a trashed contact's handle discards the
/// contact and makes a fresh one. The discard clears `participants.contact_id`
/// on the existing row, so the re-import must not add a second row for the
/// same handle beside it; the conversation lists the person once.
#[tokio::test]
async fn reimporting_after_trashing_a_contact_lists_the_person_once() {
    let vault = test_vault().await;
    let tmp = TempDir::new().unwrap();
    let path = write_jsonl(
        tmp.path(),
        "ada.jsonl",
        r#"{"schema_version":4,"export":{"source":"imessage","tool":"test","tool_version":"0","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+15555550123","conversation_type":"individual","group_title":null,"participants":[{"handle":"+15555550123","display_name":"Ada"}],"stats":{"message_count":1,"attachment_count":0,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183462000}}}
{"guid":"g-trashed-twice","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"imessage","message_kind":"imessage","sender_handle":"+15555550123","sender_display_name":null,"subject":null,"text":"hi","attachments":[],"imessage":null,"source":null}
"#,
    );
    let mut conn = vault.state.db.acquire().await.unwrap();

    append_on_conn(&mut conn, &path, tmp.path(), "imessage").await;
    let ada: i64 = sqlx::query_scalar(
        "SELECT id FROM contacts WHERE account_id = $1 AND preferred_name = 'Ada'",
    )
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    sqlx::query("INSERT INTO trashed_contacts (account_id, contact_id) VALUES ($1, $2)")
        .bind(TEST_ACCOUNT)
        .bind(ada)
        .execute(&mut *conn)
        .await
        .unwrap();

    append_on_conn(&mut conn, &path, tmp.path(), "imessage").await;

    let trashed: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM trashed_contacts WHERE account_id = $1")
            .bind(TEST_ACCOUNT)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(trashed, 0, "the import replaced the trashed contact");

    let conversation_id: i64 =
        sqlx::query_scalar("SELECT id FROM conversations WHERE account_id = $1")
            .bind(TEST_ACCOUNT)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    let loaded =
        crate::db::participant_names::load_for_conversations(&mut conn, &[conversation_id])
            .await
            .unwrap();
    let names: Vec<&str> = loaded[&conversation_id]
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(names, ["Ada"], "the conversation lists Ada once");
}

/// Each message records the account holder's address it was held at: its
/// own owner handle when the backup names one per message, the header's
/// otherwise. The holder is never a participant and never gets a contact
/// (ADR-0015), so neither owner address reaches Contacts.
#[tokio::test]
async fn a_message_is_held_at_its_own_owner_else_the_headers_and_the_owner_gets_no_contact() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("vault.db");
    let assets = tmp.path().join("assets");
    let file = write_jsonl(
        tmp.path(),
        "owner.jsonl",
        r#"{"schema_version":4,"export":{"source":"imessage","tool":"test","tool_version":"0","owner_handle":"+14155550100","owner_display_name":null},"conversation":{"chat_identifier":"+14075551234","conversation_type":"individual","group_title":null,"participants":[{"handle":"+14075551234","display_name":null}],"stats":{"message_count":3,"attachment_count":0,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183582000}}}
{"guid":"g-out","timestamp_unix_ms":1426183462000,"direction":"outgoing","service":"imessage","message_kind":"imessage","sender_handle":"+14155550100","sender_display_name":null,"subject":null,"text":"sent from the phone","attachments":[],"imessage":null,"source":null}
{"guid":"g-email","timestamp_unix_ms":1426183522000,"direction":"incoming","service":"imessage","message_kind":"imessage","sender_handle":"+14075551234","sender_display_name":null,"owner_handle":"me@icloud.com","subject":null,"text":"received at the email","attachments":[],"imessage":null,"source":null}
{"guid":"g-in","timestamp_unix_ms":1426183582000,"direction":"incoming","service":"imessage","message_kind":"imessage","sender_handle":"+14075551234","sender_display_name":null,"subject":null,"text":"received at the phone","attachments":[],"imessage":null,"source":null}
"#,
    );
    import_jsonl_files(&db, &[file], &replace_opts(&assets, tmp.path(), "imessage"))
        .await
        .unwrap();

    let (_pool, mut conn) = open_verify(&db).await;
    let held: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT m.guid, h.normalized FROM messages m
         LEFT JOIN handles h ON h.id = m.owner_handle_id
         ORDER BY m.timestamp",
    )
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        held,
        vec![
            ("g-out".to_string(), Some("+14155550100".to_string())),
            ("g-email".to_string(), Some("me@icloud.com".to_string())),
            ("g-in".to_string(), Some("+14155550100".to_string())),
        ]
    );
    let owner_contacts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM contact_handles ch
         JOIN handles h ON h.id = ch.handle_id
         WHERE h.normalized IN ('+14155550100', 'me@icloud.com')",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(owner_contacts, 0, "the holder's addresses get no contact");
    let owner_participants: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM participants p
         JOIN handles h ON h.id = p.handle_id
         WHERE h.normalized IN ('+14155550100', 'me@icloud.com')",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(owner_participants, 0, "the holder is never a participant");
}
