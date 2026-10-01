use super::*;

const TEST_ACCOUNT: i64 = 7;

/// Import one individual conversation holding one message, `guid`, through
/// the whole pipeline: staging, then promote, in the import's transaction.
async fn import_one_message(conn: &mut SqliteConnection, dir: &std::path::Path, guid: &str) {
    let path = dir.join(format!("{guid}.jsonl"));
    std::fs::write(
        &path,
        format!(
            r#"{{"schema_version":4,"export":{{"source":"sms-backup-restore","tool":"test","tool_version":"0","owner_handle":null,"owner_display_name":null}},"conversation":{{"chat_identifier":"+15555550300","conversation_type":"individual","group_title":null,"participants":[{{"handle":"+15555550300","display_name":null}}],"stats":{{"message_count":1,"attachment_count":0,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183462000}}}}}}
{{"guid":"{guid}","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"sms","message_kind":"sms","sender_handle":"+15555550300","sender_display_name":null,"subject":null,"text":"hi","attachments":[],"imessage":null,"source":null}}
"#
        ),
    )
    .unwrap();
    let assets = dir.join("assets");
    let stats = super::super::import_jsonl_files_on_conn(
        conn,
        &[path],
        &super::super::ImportOptions::fixed(super::super::FixedImportArgs {
            assets_dir: &assets,
            asset_root: dir,
            mode: ImportMode::Append,
            source: "sms-backup-restore",
            account_id: TEST_ACCOUNT,
            fill_content_keys: false,
            import_id: None,
        }),
        super::super::ImportSchemaMode::Ensure,
    )
    .await
    .unwrap();
    assert_eq!(stats.messages, 1, "the import must insert its message");
}

/// The import runs ANALYZE before it opens its transaction, so promote's
/// guid join has statistics to plan with.
#[tokio::test]
async fn an_import_analyzes_import_tables_before_begin() {
    let (pool, dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    import_one_message(&mut conn, dir.path(), "analyze-guid-1").await;
    let stat_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_stat1 WHERE tbl IN ('messages', 'attachments', 'tapbacks')",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert!(
        stat_rows >= 1,
        "ANALYZE before BEGIN must write sqlite_stat1 for import tables"
    );
    import_one_message(&mut conn, dir.path(), "analyze-guid-2").await;
}

#[tokio::test]
async fn promote_message_map_ignores_other_accounts() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    for statement in [
        "CREATE TABLE messages (id INTEGER PRIMARY KEY, account_id INTEGER NOT NULL)",
        "INSERT INTO messages (id, account_id) VALUES
            (1, 7),
            (2, 8)",
    ] {
        sqlx::query(statement).execute(&mut *conn).await.unwrap();
    }
    let mut promote = Promote {
        tx: &mut conn,
        account_id: TEST_ACCOUNT,
        mode: ImportMode::Append,
        stats: PromoteStats::default(),
        started: Instant::now(),
    };
    let mut map = HashMap::new();

    promote
        .zip_new_message_ids(&mut map, vec![101], 0, |n, p| {
            format!("unexpected mapping counts: staging={n} production={p}")
        })
        .await
        .unwrap();

    assert_eq!(map, HashMap::from([(101, 1)]));
}
