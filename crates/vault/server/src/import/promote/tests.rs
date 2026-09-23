use super::*;

const TEST_ACCOUNT: i64 = 7;

/// Full-text hit count under the Postgres 'simple' config.
async fn pg_fts_hits(conn: &mut AnyConnection, needle: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM messages WHERE search_tsv @@ plainto_tsquery('simple', $1)",
    )
    .bind(needle)
    .fetch_one(&mut *conn)
    .await
    .unwrap()
}

/// Manual ANALYZE count and last_analyze on this connection's `messages`
/// table: the test schema the pool created, not `public`.
async fn pg_messages_analyze_stat(conn: &mut AnyConnection) -> (i64, Option<String>) {
    let analyze_count: i64 = sqlx::query_scalar(
        "SELECT analyze_count FROM pg_stat_user_tables
         WHERE schemaname = current_schema() AND relname = 'messages'",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let last_analyze: Option<String> = sqlx::query_scalar(
        "SELECT last_analyze::text FROM pg_stat_user_tables
         WHERE schemaname = current_schema() AND relname = 'messages'",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    (analyze_count, last_analyze)
}

/// Postgres-gated: the promote path's disable→bulk-fill→enable FTS cycle
/// on the real engine. Skips unless `MV_TEST_POSTGRES_URL` is set (CI
/// service / `docker-compose.pg.yml`).
#[tokio::test]
async fn promote_fts_cycle_pg() {
    let Some(url) = crate::pg_test_url() else {
        return;
    };
    let pool = crate::db::engine::pg_test_schema_pool(&url).await;
    let mut conn = pool.acquire().await.unwrap();
    schema::ensure_vault_schema(&mut conn).await.unwrap();

    // One account + handle + conversation, and a pre-existing message
    // below the promote watermark, indexed by the insert trigger.
    sqlx::query("INSERT INTO accounts (id, username) VALUES ($1, 'promote-alice')")
        .bind(TEST_ACCOUNT)
        .execute(&mut *conn)
        .await
        .unwrap();
    let handle_id: i64 = sqlx::query_scalar(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES ($1, '+15555550100', '+15555550100', 'phone', 'phone')
         RETURNING id",
    )
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let conversation_id: i64 = sqlx::query_scalar(
        r"
        INSERT INTO conversations (
            account_id, chat_handle_id, conversation_type,
            group_title, exported_at, source_file
        ) VALUES ($1, $2, 'individual', NULL, NULL, 'promote.json')
        RETURNING id
        ",
    )
    .bind(TEST_ACCOUNT)
    .bind(handle_id)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let carriedover_id: i64 = sqlx::query_scalar(
        r"
        INSERT INTO messages (
            conversation_id, account_id, source, guid, timestamp,
            is_from_me, sort_order, body
        ) VALUES ($1, $2, 'sms', 'carriedover', '2020-01-01T00:00:00Z', 0, 0, 'carriedover')
        RETURNING id
        ",
    )
    .bind(conversation_id)
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(pg_fts_hits(&mut conn, "carriedover").await, 1);

    // ── The promote window, driven directly (this is exactly what
    // promote_append does between its staging inserts and the bulk fill):
    // all six by-name ALTERs execute — any wrong trigger name fails here.
    schema::disable_fts_triggers_pg(&mut conn).await.unwrap();

    // FK constraint triggers stay enabled during the window: an
    // attachment pointing at a missing message must fail loudly.
    let fk_err = sqlx::query(
        "INSERT INTO attachments (message_id, original_name)
         VALUES (99999999, 'dangling.jpg')",
    )
    .execute(&mut *conn)
    .await
    .unwrap_err();
    assert!(
        format!("{fk_err}").contains("foreign key"),
        "FK violation must fail loudly while the FTS triggers are disabled: {fk_err}"
    );

    // Raw inserts during the window skip per-row FTS work.
    let fresh_id: i64 = sqlx::query_scalar(
        r"
        INSERT INTO messages (
            conversation_id, account_id, source, guid, timestamp,
            is_from_me, sort_order, body
        ) VALUES ($1, $2, 'sms', 'freshbody', '2020-01-01T00:00:00Z', 0, 0, 'freshbody')
        RETURNING id
        ",
    )
    .bind(conversation_id)
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let unindexed: i64 = sqlx::query_scalar(
        "SELECT CASE WHEN search_tsv IS NULL THEN 1 ELSE 0 END FROM messages WHERE id = $1",
    )
    .bind(fresh_id)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        unindexed, 1,
        "raw insert during the disabled window must leave search_tsv NULL"
    );

    // The promote-map bulk fill touches exactly the rows above the
    // watermark (the temp map as promote fills it: staging id → prod id).
    sqlx::query(
        "CREATE TEMP TABLE IF NOT EXISTS _promote_msg_map (
             staging_id BIGINT PRIMARY KEY,
             prod_id BIGINT NOT NULL
         )",
    )
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query("DELETE FROM _promote_msg_map")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query("INSERT INTO _promote_msg_map (staging_id, prod_id) VALUES ($1, $2), ($3, $4)")
        .bind(1i64)
        .bind(carriedover_id)
        .bind(2i64)
        .bind(fresh_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    let indexed = schema::index_messages_fts_from_promote_map(&mut conn, carriedover_id)
        .await
        .unwrap();
    assert_eq!(
        indexed, 1,
        "bulk fill must index exactly the rows above the watermark"
    );
    assert_eq!(pg_fts_hits(&mut conn, "carriedover").await, 1);
    assert_eq!(pg_fts_hits(&mut conn, "freshbody").await, 1);

    // ── Enable restores the triggers: a post-enable insert is indexed.
    schema::enable_fts_triggers_pg(&mut conn).await.unwrap();
    sqlx::query(
        r"
        INSERT INTO messages (
            conversation_id, account_id, source, guid, timestamp,
            is_from_me, sort_order, body
        ) VALUES ($1, $2, 'sms', 'postenable', '2020-01-01T00:00:00Z', 0, 0, 'postenable')
        ",
    )
    .bind(conversation_id)
    .bind(TEST_ACCOUNT)
    .execute(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        pg_fts_hits(&mut conn, "postenable").await,
        1,
        "insert trigger must fire again after enable"
    );

    // ── The promote branch end-to-end: staging rows → promote_append →
    // the bulk fill indexes the promoted rows above the watermark.
    let staged_handle_id: i64 = sqlx::query_scalar(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES ($1, '+15555550200', '+15555550200', 'phone', 'phone')
         RETURNING id",
    )
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let staging_conv_id: i64 = sqlx::query_scalar(
        r"
        INSERT INTO staging_conversations (
            account_id, chat_handle_id, conversation_type, source_file
        ) VALUES ($1, $2, 'individual', 'staged.json')
        RETURNING id
        ",
    )
    .bind(TEST_ACCOUNT)
    .bind(staged_handle_id)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        r"
        INSERT INTO staging_messages (
            conversation_id, account_id, source, guid, timestamp,
            is_from_me, sort_order, body
        ) VALUES ($1, $2, 'sms', 'staged-guid-1', '2020-01-01T00:00:00Z', 0, 0, 'stagedbody')
        ",
    )
    .bind(staging_conv_id)
    .bind(TEST_ACCOUNT)
    .execute(&mut *conn)
    .await
    .unwrap();
    // ANALYZE belongs to the import, before its transaction opens;
    // an_import_analyzes_import_tables_before_begin_pg covers it.
    let stats = promote_append(&mut conn, ImportMode::Append, TEST_ACCOUNT, false, &[])
        .await
        .unwrap();
    assert_eq!(stats.messages, 1, "one staged message must promote");
    assert_eq!(
        pg_fts_hits(&mut conn, "stagedbody").await,
        1,
        "promoted rows above the watermark must be indexed"
    );

    // And the triggers still fire for brand-new rows after the promote.
    sqlx::query(
        r"
        INSERT INTO messages (
            conversation_id, account_id, source, guid, timestamp,
            is_from_me, sort_order, body
        ) VALUES ($1, $2, 'sms', 'afterpromote', '2020-01-01T00:00:00Z', 0, 0, 'afterpromote')
        ",
    )
    .bind(conversation_id)
    .bind(TEST_ACCOUNT)
    .execute(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        pg_fts_hits(&mut conn, "afterpromote").await,
        1,
        "triggers must fire again after the promote cycle"
    );
}

/// Import one individual conversation holding one message, `guid`, through
/// the whole pipeline: staging, then promote, in the import's transaction.
async fn import_one_message(conn: &mut AnyConnection, dir: &std::path::Path, guid: &str) {
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
            contacts: None,
            overwrite_contacts: false,
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
    if crate::test_support::on_postgres() {
        return; // SQLite-only: reads sqlite_stat1; an_import_analyzes_import_tables_before_begin_pg is the twin
    }
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

/// Postgres-gated: ANALYZE on the shared database must change
/// last_analyze before a second import begins (stop/restart in miniature).
#[tokio::test]
async fn an_import_analyzes_import_tables_before_begin_pg() {
    if crate::pg_test_url().is_none() {
        return;
    }
    // A schema of this test's own (db::engine::test_pool on Postgres), so
    // nothing an earlier run left in staging can make this one promote the
    // wrong rows (#394).
    let (pool, dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    schema::ensure_vault_schema(&mut conn).await.unwrap();
    let (analyze_before, last_analyze_before) = pg_messages_analyze_stat(&mut conn).await;
    import_one_message(&mut conn, dir.path(), "analyze-pg-guid-1").await;
    // Since Postgres 15 the cumulative statistics live in shared memory and
    // a backend's pending counters reach them at transaction end or after
    // PGSTAT_MIN_INTERVAL (one second), read by others through a snapshot.
    // The ANALYZE ran; its count can lag this read by up to that interval,
    // so wait for it rather than assert on the first look (#394).
    let mut analyze_after = analyze_before;
    let mut last_analyze = None;
    for _ in 0..50 {
        (analyze_after, last_analyze) = pg_messages_analyze_stat(&mut conn).await;
        if analyze_after > analyze_before {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(
        analyze_after > analyze_before,
        "ANALYZE before BEGIN must increment analyze_count on messages (before={analyze_before}, after={analyze_after}, last_analyze_before={last_analyze_before:?})"
    );
    assert!(
        last_analyze.is_some(),
        "ANALYZE before BEGIN must set last_analyze on messages before a second import"
    );
    import_one_message(&mut conn, dir.path(), "analyze-pg-guid-2").await;
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
    let engine = crate::db::dialect::engine_of(&conn);
    let mut promote = Promote {
        tx: &mut conn,
        account_id: TEST_ACCOUNT,
        mode: ImportMode::Append,
        engine,
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
