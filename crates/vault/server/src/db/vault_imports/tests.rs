use super::*;

const ACCOUNT_ID: &str = "11111111-1111-1111-1111-111111111111";

async fn setup_accounts_only() -> (sqlx::AnyPool, tempfile::TempDir) {
    let (pool, dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    crate::db::schema::ensure_accounts_schema(&mut conn)
        .await
        .unwrap();
    sqlx::query("INSERT INTO accounts (id, username) VALUES ($1, $2)")
        .bind(ACCOUNT_ID)
        .bind("alice")
        .execute(&mut *conn)
        .await
        .unwrap();
    (pool, dir)
}

/// A default session-open for tests that only care that a running
/// import exists, not about its stage or session fields.
fn default_start_args(account_id: &str) -> StartImportArgs<'_> {
    StartImportArgs::new(account_id, "ios", "append", Some("message-vault-io"))
}

#[tokio::test]
async fn complete_import_persists_timings_and_issues() {
    let (pool, _dir) = setup_accounts_only().await;
    let mut conn = pool.acquire().await.unwrap();
    let import_id = start_import(&mut conn, &default_start_args(ACCOUNT_ID))
        .await
        .unwrap();

    let row = complete_import(
        &mut conn,
        ACCOUNT_ID,
        import_id,
        &CompleteImportArgs {
            ok: true,
            status: None,
            message_count: Some(10),
            attachment_count: Some(2),
            bytes_uploaded: Some(100),
            duration_ms: Some(48_000),
            parse_ms: Some(18_000),
            attachments_ms: Some(22_000),
            prepare_ms: Some(4_000),
            upload_ms: Some(8_000),
            summary_json: Some(r#"{"parse":{"messages":10}}"#.into()),
            issues: vec![ImportIssueInput {
                kind: "skip".into(),
                step: "convert".into(),
                item: "photo.heic".into(),
                reason: "convert failed".into(),
            }],
        },
    )
    .await
    .unwrap();

    assert_eq!(row.duration_ms, Some(48_000));
    assert_eq!(row.parse_ms, Some(18_000));
    assert_eq!(row.attachments_ms, Some(22_000));
    assert_eq!(row.prepare_ms, Some(4_000));
    assert_eq!(row.upload_ms, Some(8_000));
    assert_eq!(
        row.summary_json.as_deref(),
        Some(r#"{"parse":{"messages":10}}"#)
    );

    let issue_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM vault_import_issues WHERE import_id = $1")
            .bind(import_id)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(issue_count, 1);
}

#[tokio::test]
async fn complete_import_rejects_invalid_issue_kind() {
    let (pool, _dir) = setup_accounts_only().await;
    let mut conn = pool.acquire().await.unwrap();
    let import_id = start_import(&mut conn, &default_start_args(ACCOUNT_ID))
        .await
        .unwrap();

    let err = complete_import(
        &mut conn,
        ACCOUNT_ID,
        import_id,
        &CompleteImportArgs {
            ok: false,
            status: None,
            message_count: None,
            attachment_count: None,
            bytes_uploaded: None,
            duration_ms: None,
            parse_ms: None,
            attachments_ms: None,
            prepare_ms: None,
            upload_ms: None,
            summary_json: None,
            issues: vec![ImportIssueInput {
                kind: "warning".into(),
                step: "upload".into(),
                item: "archive.zip".into(),
                reason: "not allowed".into(),
            }],
        },
    )
    .await
    .unwrap_err()
    .to_string();

    assert!(err.contains("invalid import issue kind"));

    let issue_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM vault_import_issues WHERE import_id = $1")
            .bind(import_id)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(issue_count, 0);

    let status: String = sqlx::query_scalar("SELECT status FROM vault_imports WHERE id = $1")
        .bind(import_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(status, "running");
}

#[tokio::test]
async fn require_reusable_import_rejects_completed_and_mismatched() {
    let (pool, _dir) = setup_accounts_only().await;
    let mut conn = pool.acquire().await.unwrap();
    let import_id = start_import(&mut conn, &default_start_args(ACCOUNT_ID))
        .await
        .unwrap();
    complete_import(
        &mut conn,
        ACCOUNT_ID,
        import_id,
        &CompleteImportArgs::succeeded(1, 0),
    )
    .await
    .unwrap();

    let err = require_reusable_import(&mut conn, ACCOUNT_ID, import_id, "ios", "append")
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("not running"), "{err}");

    let running = start_import(&mut conn, &default_start_args(ACCOUNT_ID))
        .await
        .unwrap();
    let src_err = require_reusable_import(&mut conn, ACCOUNT_ID, running, "android", "append")
        .await
        .unwrap_err()
        .to_string();
    assert!(src_err.contains("source mismatch"), "{src_err}");
    let mode_err = require_reusable_import(&mut conn, ACCOUNT_ID, running, "ios", "replace")
        .await
        .unwrap_err()
        .to_string();
    assert!(mode_err.contains("mode mismatch"), "{mode_err}");
    assert!(
        require_reusable_import(&mut conn, ACCOUNT_ID, running, "ios", "append")
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn get_import_detail_returns_issues() {
    let (pool, _dir) = setup_accounts_only().await;
    let mut conn = pool.acquire().await.unwrap();
    let import_id = start_import(&mut conn, &default_start_args(ACCOUNT_ID))
        .await
        .unwrap();
    complete_import(
        &mut conn,
        ACCOUNT_ID,
        import_id,
        &CompleteImportArgs {
            ok: true,
            status: None,
            message_count: Some(10),
            attachment_count: Some(2),
            bytes_uploaded: Some(100),
            duration_ms: Some(48_000),
            parse_ms: Some(18_000),
            attachments_ms: Some(22_000),
            prepare_ms: Some(4_000),
            upload_ms: Some(8_000),
            summary_json: Some(r#"{"parse":{"messages":10}}"#.into()),
            issues: vec![
                ImportIssueInput {
                    kind: "skip".into(),
                    step: "convert".into(),
                    item: "photo.heic".into(),
                    reason: "convert failed".into(),
                },
                ImportIssueInput {
                    kind: "error".into(),
                    step: "upload".into(),
                    item: "archive.zip".into(),
                    reason: "upload failed".into(),
                },
            ],
        },
    )
    .await
    .unwrap();

    let detail = get_import_detail(&mut conn, ACCOUNT_ID, import_id)
        .await
        .unwrap();
    assert_eq!(detail.row.duration_ms, Some(48_000));
    assert_eq!(detail.row.parse_ms, Some(18_000));
    assert_eq!(detail.issues.len(), 2);
    assert_eq!(detail.issues[0].kind, "skip");
    assert_eq!(detail.issues[0].step, "convert");
    assert_eq!(detail.issues[1].kind, "error");
    assert_eq!(detail.issues[1].step, "upload");
}

#[tokio::test]
async fn list_imports_includes_duration_ms() {
    let (pool, _dir) = setup_accounts_only().await;
    let mut conn = pool.acquire().await.unwrap();
    let import_id = start_import(&mut conn, &default_start_args(ACCOUNT_ID))
        .await
        .unwrap();
    complete_import(
        &mut conn,
        ACCOUNT_ID,
        import_id,
        &CompleteImportArgs {
            ok: true,
            status: None,
            message_count: Some(10),
            attachment_count: Some(2),
            bytes_uploaded: Some(100),
            duration_ms: Some(48_000),
            parse_ms: None,
            attachments_ms: None,
            prepare_ms: None,
            upload_ms: None,
            summary_json: None,
            issues: vec![],
        },
    )
    .await
    .unwrap();

    let imports = list_imports(&mut conn, ACCOUNT_ID).await.unwrap();
    assert_eq!(imports.len(), 1);
    assert_eq!(imports[0].duration_ms, Some(48_000));
}

#[tokio::test]
async fn active_session_round_trips_and_blocks_a_second() {
    let (pool, _dir) = setup_accounts_only().await;
    let mut conn = pool.acquire().await.unwrap();
    let account = ACCOUNT_ID;

    assert!(
        get_active_import(&mut conn, account)
            .await
            .unwrap()
            .is_none(),
        "no session before one starts"
    );

    let args = StartImportArgs {
        staging_dir: Some("/home/u/message-vault/staging-iphone-260830"),
        device_id: Some("device-a"),
        form_json: Some(r#"{"source":"imessage-ios"}"#),
        source_fingerprint: Some(r#"{"path":"/b","size_bytes":10}"#),
        ..StartImportArgs::new(account, "imessage", "append", Some("message-vault-io"))
    };
    let id = start_import(&mut conn, &args).await.unwrap();

    let active = get_active_import(&mut conn, account)
        .await
        .unwrap()
        .expect("the session is active");
    assert_eq!(active.id, id);
    assert_eq!(active.stage.as_deref(), Some("parse"));
    assert_eq!(
        active.staging_dir.as_deref(),
        Some("/home/u/message-vault/staging-iphone-260830")
    );
    assert_eq!(active.device_id.as_deref(), Some("device-a"));
    assert_eq!(
        active.form_json.as_deref(),
        Some(r#"{"source":"imessage-ios"}"#)
    );

    assert!(
        matches!(
            start_import(&mut conn, &args).await,
            Err(StartImportError::AlreadyActive)
        ),
        "a second session is refused by the index, not by a race-prone check"
    );
}

#[tokio::test]
async fn stage_advances_and_discard_frees_the_slot() {
    let (pool, _dir) = setup_accounts_only().await;
    let mut conn = pool.acquire().await.unwrap();
    let account = ACCOUNT_ID;
    let args = StartImportArgs::new(account, "imessage", "append", None);
    let id = start_import(&mut conn, &args).await.unwrap();

    set_import_stage(&mut conn, account, id, ImportStage::Pushing, None)
        .await
        .unwrap();
    let active = get_active_import(&mut conn, account)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(active.stage.as_deref(), Some("pushing"));

    discard_import(&mut conn, account, id).await.unwrap();
    assert!(
        get_active_import(&mut conn, account)
            .await
            .unwrap()
            .is_none(),
        "a discarded session is no longer active"
    );
    let row = get_owned_import(&mut conn, account, id).await.unwrap();
    assert_eq!(row.status, "cancelled");
    assert!(row.finished_at.is_some(), "a discard closes the run");

    // The slot is genuinely free.
    start_import(&mut conn, &args)
        .await
        .expect("a new session can start");
}

#[tokio::test]
async fn completing_a_session_frees_the_slot_too() {
    let (pool, _dir) = setup_accounts_only().await;
    let mut conn = pool.acquire().await.unwrap();
    let account = ACCOUNT_ID;
    let args = StartImportArgs::new(account, "imessage", "append", None);
    let id = start_import(&mut conn, &args).await.unwrap();
    complete_import(
        &mut conn,
        account,
        id,
        &CompleteImportArgs::succeeded(10, 2),
    )
    .await
    .unwrap();
    assert!(
        get_active_import(&mut conn, account)
            .await
            .unwrap()
            .is_none()
    );
}

#[test]
fn every_stage_round_trips_through_its_string() {
    for stage in [
        ImportStage::Parse,
        ImportStage::Write,
        ImportStage::AwaitingGate1,
        ImportStage::Transcode,
        ImportStage::AwaitingGate2,
        ImportStage::Pushing,
    ] {
        assert_eq!(ImportStage::parse(stage.as_str()), Some(stage));
    }
    assert_eq!(ImportStage::parse("gate_1"), None);
}
