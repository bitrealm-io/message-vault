use super::*;
use crate::assets;
use crate::test_support::{TestVault, get_json, post_json, register_via_api, test_vault};
use tempfile::TempDir;

const TEST_ACCOUNT: &str = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";

fn write_jsonl(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, body).unwrap();
    path
}

/// A vault holding one live import session at `awaiting_gate_1` whose
/// `summary_json` already carries `summary` — as if an earlier
/// `POST /v1/imports/{id}/stage` recorded a gate approval.
async fn session_with_summary(summary: serde_json::Value) -> (TestVault, String, i64) {
    let vault = test_vault().await;
    let account = register_via_api(&vault.state, "alice", "hunter2hunter2").await;
    let created: serde_json::Value = post_json(
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
        &account.account_id,
        import_id,
        crate::db::vault_imports::ImportStage::AwaitingGate1,
        Some(&summary.to_string()),
    )
    .await
    .unwrap();
    (vault, account.token, import_id)
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
    let vault = test_vault().await;
    let account = register_via_api(&vault.state, "alice", "hunter2hunter2").await;
    let created: serde_json::Value = post_json(
        &vault.state,
        "/v1/imports",
        &account.token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    let import_id = created["id"].as_i64().unwrap();

    post_json::<serde_json::Value>(
        &vault.state,
        &format!("/v1/imports/{import_id}/stage"),
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
    // GET /v1/imports/active must expose it too.
    let (vault, token, import_id) =
        session_with_summary(serde_json::json!({"approved": true})).await;

    let active: serde_json::Value = get_json(&vault.state, "/v1/imports/active", &token).await;

    assert_eq!(active["session"]["id"], serde_json::json!(import_id));
    assert_eq!(
        active["session"]["summary"],
        serde_json::json!({"approved": true})
    );
}

#[tokio::test]
async fn a_stage_change_without_a_summary_does_not_erase_the_stored_one() {
    // Most stage changes carry nothing. Treating absent as null would
    // throw away the plan the outcome is judged against.
    let (vault, token, import_id) =
        session_with_summary(serde_json::json!({"approved": true})).await;

    post_json::<serde_json::Value>(
        &vault.state,
        &format!("/v1/imports/{import_id}/stage"),
        &token,
        serde_json::json!({"stage": "pushing"}),
    )
    .await;

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
            ok: true,
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

    let listed = crate::db::vault_imports::list_imports_for_account(&mut conn, TEST_ACCOUNT, 10)
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].source, "imessage");
    assert!(!listed[0].started_at.is_empty());
    assert!(listed[0].finished_at.is_some());
    assert_eq!(
        crate::db::vault_imports::account_attachment_bytes(&mut conn, TEST_ACCOUNT)
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

/// `run_import_path` (the `POST /v1/import` path) opens
/// a one-shot session the same way `imports_create_handler` does when the
/// caller does not pass `import_id`. It must map the same
/// `StartImportError::AlreadyActive` collision to `ApiError::Conflict`,
/// not `ApiError::Internal` — otherwise the two endpoints answer the same
/// condition with different status codes. This calls `run_import_path`
/// directly (it is private to this module) rather than going through
/// `import_handler`'s HTTP body/content-type parsing, which is
/// orthogonal to the session check under test.
#[tokio::test]
async fn run_import_path_refuses_a_second_session_with_conflict() {
    let (pool, tmp) = crate::db::engine::test_pool().await;
    {
        let mut conn = pool.acquire().await.unwrap();
        schema::ensure_vault_schema(&mut conn).await.unwrap();
        crate::db::account_profile::ensure_account_row(&mut conn, TEST_ACCOUNT)
            .await
            .unwrap();
        crate::db::vault_imports::start_import(
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
    }
    let data_dir = tmp.path().join("data");
    let state = crate::server::test_app_state(pool, &data_dir).await;

    let query = ImportQuery {
        source: "imessage".into(),
        account: Some(TEST_ACCOUNT.into()),
        mode: "append".into(),
        dedupe: false,
        import_id: None,
    };
    // Never read: the session collision is detected before the jsonl
    // file is opened.
    let jsonl_path = tmp.path().join("unused.jsonl");

    let err = run_import_path(state, query, jsonl_path).await.unwrap_err();
    let ApiError::Conflict(message) = &err else {
        panic!("expected Conflict, got {err:?}");
    };
    // The 409 has to name the way out: the only place a stranded
    // session can be resumed or discarded is the desktop app's Import
    // screen.
    assert!(
        message.contains("Import in the desktop app"),
        "the conflict names how to clear the session: {message}"
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
    let sha = assets::sha256_hex(b"expected-asset");
    let corrupt = assets.join(assets::shard_rel_path(&sha, ".bin"));
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
        err.to_string()
            .contains(message_ir_format::UNSAFE_ATTACHMENT_PATH_PREFIX),
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
    assert!(
        err.to_string()
            .contains(message_ir_format::UNSAFE_ATTACHMENT_PATH_PREFIX)
    );

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

#[tokio::test]
async fn http_import_of_a_schema_3_file_is_a_400_naming_both_versions() {
    let (state, _vault, token) = importer().await;
    let body = concat!(
        r#"{"schema_version":3,"export":{"source":"whatsapp","tool":"t","owner_handle":"+15550000001","owner_display_name":"Me"},"#,
        r#""conversation":{"chat_identifier":"+15550000002","conversation_type":"individual","participants":[{"handle":"+15550000002","display_name":"Sam"}]}}"#,
        "\n",
    );
    let (status, text) = crate::test_support::post_raw(
        &state,
        "/v1/import?source=whatsapp",
        &token,
        "application/jsonl",
        body,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST, "{text}");
    let err: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        err["error"],
        "This file is schema version 3; the vault reads version 4 (line 1)."
    );
}

#[tokio::test]
async fn http_import_of_a_line_that_is_not_json_is_a_400_naming_the_line() {
    let (state, _vault, token) = importer().await;
    let (status, text) = crate::test_support::post_raw(
        &state,
        "/v1/import?source=whatsapp",
        &token,
        "application/jsonl",
        "this is not json\n",
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST, "{text}");
    let err: serde_json::Value = serde_json::from_str(&text).unwrap();
    let message = err["error"].as_str().unwrap();
    assert!(
        message.starts_with("Could not read line 1 of the file:"),
        "{message}"
    );
}

#[tokio::test]
async fn http_import_without_source_is_a_json_400() {
    let (state, _vault, token) = importer().await;
    let (status, text) =
        crate::test_support::post_raw(&state, "/v1/import", &token, "application/jsonl", "{}\n")
            .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST, "{text}");
    let err: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|_| panic!("expected a JSON error body, got: {text}"));
    assert_eq!(err["error"], "query param source is required");
}

/// The import body is JSON Lines and nothing else. `multipart/form-data`
/// used to be accepted (a `jsonl` field plus `file` parts) but nothing
/// sent it: vault-push posts JSON Lines and uploads attachments through
/// `/v1/assets`. The wrong media type is a 415, not a 400: the request is
/// well formed, it is simply not something this route reads.
#[tokio::test]
async fn a_multipart_body_is_an_unsupported_media_type() {
    let vault = crate::test_support::test_vault().await;
    let user = crate::test_support::register_via_api(&vault.state, "alice", "hunter2hunter2").await;

    let boundary = "MessageVaultTestBoundary";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"jsonl\"\r\n\r\n{{}}\r\n--{boundary}--\r\n"
    );
    let (status, text) = crate::test_support::post_raw(
        &vault.state,
        "/v1/import?source=imessage&mode=append",
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
        parsed["error"],
        "Content-Type must be application/x-ndjson or application/jsonl"
    );
}

/// `?account=` naming a different account is refused, even with an
/// otherwise valid token: the account query on `POST /v1/import` is
/// bound to whoever the Bearer token belongs to
/// (`resolve_import_account` in `server.rs`), not a free choice of
/// tenant. This is the only test on that branch — a deleted smoke
/// script was the only thing checking it before.
#[tokio::test]
async fn http_import_refuses_an_account_query_naming_someone_else() {
    let vault = crate::test_support::test_vault().await;
    let alice =
        crate::test_support::register_via_api(&vault.state, "alice", "hunter2hunter2").await;
    let bob = crate::test_support::register_via_api(&vault.state, "bob", "hunter2hunter2").await;

    let body = concat!(
        r#"{"schema_version":4,"export":{"source":"imessage","tool":"test","tool_version":"0","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+15555550123","conversation_type":"individual","group_title":null,"participants":[{"handle":"+15555550123","display_name":null}],"stats":{"message_count":1,"attachment_count":0,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183462000}}}"#,
        "\n",
        r#"{"guid":"g-cross-account","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"imessage","message_kind":"imessage","sender_handle":"+15555550123","sender_display_name":null,"subject":null,"text":"hi","attachments":[],"imessage":null,"source":null}"#,
        "\n",
    );

    let (status, text) = crate::test_support::post_raw(
        &vault.state,
        &format!(
            "/v1/import?source=imessage&mode=append&account={}",
            bob.username
        ),
        &alice.token,
        "application/jsonl",
        body,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN, "{text}");
    let err: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(err["error"], "account query does not match token's account");

    // Positive control: naming her own account must not be refused for
    // the same reason. Without this, the assertion above would still
    // pass if the route started refusing every import outright — it
    // need not succeed, since a minimal body can still fail later for
    // unrelated reasons, but it must not be 403.
    let (status, text) = crate::test_support::post_raw(
        &vault.state,
        &format!(
            "/v1/import?source=imessage&mode=append&account={}",
            alice.username
        ),
        &alice.token,
        "application/jsonl",
        body,
    )
    .await;
    assert_ne!(status, axum::http::StatusCode::FORBIDDEN, "{text}");
}
