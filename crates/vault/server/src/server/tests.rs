use super::*;
use crate::extract::{Json, Path as AxumPath};
use crate::import::ImportMode;
use crate::import::{
    CompleteImportBody, CompleteImportIssueBody, CreateImportBody, SetImportStageBody,
    imports_complete_handler, imports_create_handler, imports_discard_handler, imports_get_handler,
    imports_list_handler, imports_patch_handler,
};
use axum::extract::State;
use tempfile::TempDir;

fn auth_public_router() -> Router<AppState> {
    limited_auth_router().0
}

#[test]
fn jsonl_content_type_accepts_x_ndjson() {
    assert!(is_jsonl_content_type("application/x-ndjson"));
    assert!(is_jsonl_content_type("application/jsonl"));
    assert!(is_jsonl_content_type("Application/X-NDJSON"));
    assert!(!is_jsonl_content_type("multipart/form-data"));
    assert!(!is_jsonl_content_type("application/json"));
}

#[test]
fn error_chain_keeps_every_context_layer() {
    let err = anyhow::Error::new(std::io::Error::other("disk full"))
        .context("write staging row")
        .context("stage conversation chat-1");
    assert_eq!(
        error_chain(&err),
        "stage conversation chat-1: write staging row: disk full"
    );
}

#[test]
fn internal_error_keeps_the_chain_and_answers_500() {
    let err = ApiError::from(anyhow::anyhow!("disk full").context("stage conversation"));
    let ApiError::Internal(inner) = &err else {
        panic!("expected Internal, got {err:?}");
    };
    assert_eq!(format!("{inner:#}"), "stage conversation: disk full");
    assert_eq!(
        err.into_response().status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
}

const TEST_ACCOUNT: i64 = 7;

/// Test database with the vault schema applied. The temp dir is returned
/// too: dropping it deletes the database file out from under the checked-out
/// connection, after which SQLite rejects writes with SQLITE_READONLY.
async fn test_conn() -> (TempDir, sqlx::pool::PoolConnection<sqlx::Any>) {
    let (pool, dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    schema::ensure_vault_schema(&mut conn).await.unwrap();
    (dir, conn)
}

#[tokio::test]
async fn api_token_cannot_exceed_its_owner() {
    let (_dir, mut conn) = test_conn().await;
    account_profile::insert_account_at(&mut conn, TEST_ACCOUNT, "alice", None, None)
        .await
        .unwrap();
    sqlx::query("UPDATE accounts SET can_import = 0 WHERE id = $1")
        .bind(TEST_ACCOUNT)
        .execute(&mut *conn)
        .await
        .unwrap();
    let created =
        api_tokens::create_api_token(&mut conn, TEST_ACCOUNT, "tool", Permissions::all(), None)
            .await
            .unwrap();

    let identity = resolve_auth_on_conn(&mut conn, &created.token)
        .await
        .unwrap();

    assert!(
        !identity.permissions().import,
        "the account lost import, so its token must not have it"
    );
    assert!(identity.permissions().export);
}

#[tokio::test]
async fn disabling_an_account_kills_its_live_session() {
    let (_dir, mut conn) = test_conn().await;
    account_profile::insert_account_at(&mut conn, TEST_ACCOUNT, "alice", None, None)
        .await
        .unwrap();
    let token = session_tokens::insert_account_session_token(&mut conn, TEST_ACCOUNT)
        .await
        .unwrap();

    // The token works while the account is active.
    resolve_auth_on_conn(&mut conn, &token).await.unwrap();

    sqlx::query("UPDATE accounts SET disabled = 1 WHERE id = $1")
        .bind(TEST_ACCOUNT)
        .execute(&mut *conn)
        .await
        .unwrap();

    let err = resolve_auth_on_conn(&mut conn, &token).await.unwrap_err();
    assert!(
        matches!(err, ApiError::AccountDisabled(_)),
        "a disabled account's existing token must stop working, got {err:?}"
    );
}

#[tokio::test]
async fn disabling_an_account_kills_its_live_api_token() {
    let (_dir, mut conn) = test_conn().await;
    account_profile::insert_account_at(&mut conn, TEST_ACCOUNT, "alice", None, None)
        .await
        .unwrap();
    let created =
        api_tokens::create_api_token(&mut conn, TEST_ACCOUNT, "tool", Permissions::all(), None)
            .await
            .unwrap();
    let token = created.token;

    // The API token works while the account is active.
    resolve_auth_on_conn(&mut conn, &token).await.unwrap();

    sqlx::query("UPDATE accounts SET disabled = 1 WHERE id = $1")
        .bind(TEST_ACCOUNT)
        .execute(&mut *conn)
        .await
        .unwrap();

    let err = resolve_auth_on_conn(&mut conn, &token).await.unwrap_err();
    assert!(
        matches!(err, ApiError::AccountDisabled(_)),
        "a disabled account's existing API token must stop working, got {err:?}"
    );
}

async fn test_state() -> (TempDir, AppState, String, i64) {
    let (pool, tmp) = crate::db::engine::test_pool().await;
    let data_dir = tmp.path().join("data");
    {
        let mut conn = pool.acquire().await.unwrap();
        schema::ensure_vault_schema(&mut conn).await.unwrap();
        schema::ensure_accounts_schema(&mut conn).await.unwrap();
        crate::db::account_profile::ensure_account_row(&mut conn, TEST_ACCOUNT)
            .await
            .unwrap();
    }
    let token = crate::db::session_tokens::insert_account_session_token(
        &mut pool.acquire().await.unwrap(),
        TEST_ACCOUNT,
    )
    .await
    .unwrap();
    let import_id = crate::db::vault_imports::start_import(
        &mut pool.acquire().await.unwrap(),
        &crate::db::vault_imports::StartImportArgs::new(
            TEST_ACCOUNT,
            "ios",
            "append",
            Some("message-vault-server"),
        ),
    )
    .await
    .unwrap();

    let state = test_app_state(pool, &data_dir);

    (tmp, state, token, import_id)
}

fn auth_headers(token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        format!("Bearer {token}").parse().unwrap(),
    );
    headers
}

/// The account's running Import Run through `GET /v1/imports?status=running`,
/// as the desktop app finds it.
async fn running_import(
    state: &AppState,
    token: &str,
) -> Option<crate::db::vault_imports::ImportSummary> {
    imports_list_handler(
        State(state.clone()),
        import_access(state, token).await,
        crate::extract::Query(crate::import::ListImportsQuery {
            sort: None,
            status: Some("running".into()),
            limit: None,
            offset: None,
        }),
    )
    .await
    .unwrap()
    .0
    .items
    .into_iter()
    .next()
}

/// Resolve the token the way the `ImportAccess` extractor would, for
/// tests that call import handlers directly instead of over HTTP.
async fn import_access(state: &AppState, token: &str) -> ImportAccess {
    let auth = resolve_auth(&auth_headers(token), state).await.unwrap();
    require_import_access(&auth).unwrap();
    ImportAccess(auth)
}

async fn get_path(state: AppState, path: &str) -> reqwest::Response {
    let server = crate::test_support::serve(&state).await;
    reqwest::Client::new()
        .get(format!("{}{path}", server.base()))
        .send()
        .await
        .unwrap()
}

fn with_cors(mut state: AppState, origins: &[&str]) -> AppState {
    let mut cfg = (*state.cfg).clone();
    cfg.server.as_mut().unwrap().cors_origins = origins.iter().map(|s| (*s).to_string()).collect();
    state.cfg = Arc::new(cfg);
    state
}

async fn cors_preflight(state: AppState, origin: &str) -> reqwest::Response {
    let server = crate::test_support::serve(&state).await;
    reqwest::Client::new()
        .request(
            reqwest::Method::OPTIONS,
            format!("{}/health", server.base()),
        )
        .header("Origin", origin)
        .header("Access-Control-Request-Method", "GET")
        .header("Access-Control-Request-Headers", "content-type")
        .send()
        .await
        .unwrap()
}

fn allow_origin(response: &reqwest::Response) -> Option<&str> {
    response
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        .and_then(|value| value.to_str().ok())
}

#[tokio::test]
async fn cors_preflight_allows_packaged_desktop_and_vite_origins() {
    let (_dir, state, _token, _import_id) = test_state().await;
    let origins = [
        "http://localhost:5173",
        "http://127.0.0.1:5173",
        "https://tauri.localhost",
        "http://tauri.localhost",
        "tauri://localhost",
    ];
    for origin in origins {
        let response = cors_preflight(with_cors(state.clone(), &origins), origin).await;
        assert_eq!(
            allow_origin(&response),
            Some(origin),
            "preflight Origin {origin}"
        );
    }
}

/// A vault built from source starts with `cors_origins` commented out. The
/// desktop app still has to reach it, so the packaged origins do not wait
/// to be configured.
#[tokio::test]
async fn cors_preflight_allows_packaged_desktop_without_configuration() {
    let (_dir, state, _token, _import_id) = test_state().await;
    for origin in PACKAGED_DESKTOP_ORIGINS {
        let response = cors_preflight(with_cors(state.clone(), &[]), origin).await;
        assert_eq!(
            allow_origin(&response),
            Some(*origin),
            "unconfigured preflight Origin {origin}"
        );
    }
}

/// Built in does not mean open: everything else still has to be listed.
#[tokio::test]
async fn cors_preflight_rejects_unknown_origin_without_configuration() {
    let (_dir, state, _token, _import_id) = test_state().await;
    let response = cors_preflight(with_cors(state, &[]), "https://evil.example").await;
    assert_eq!(allow_origin(&response), None);
}

#[tokio::test]
async fn cors_preflight_rejects_unknown_origin() {
    let (_dir, state, _token, _import_id) = test_state().await;
    let response = cors_preflight(
        with_cors(state, &["tauri://localhost"]),
        "https://evil.example",
    )
    .await;
    assert_eq!(allow_origin(&response), None);
}

#[tokio::test]
async fn health_still_ok() {
    let (_dir, state, _token, _import_id) = test_state().await;
    let response = get_path(state, "/health").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text().await.unwrap(), "ok\n");
}

#[tokio::test]
async fn openapi_ui_off_does_not_serve_spec() {
    let (_dir, state, _token, _import_id) = test_state().await;
    assert!(!state.cfg.require_server().unwrap().openapi_ui);
    let response = get_path(state, "/openapi.json").await;
    assert_ne!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or(""),
        "application/json"
    );
}

#[tokio::test]
async fn openapi_ui_on_serves_spec_without_token() {
    let (_dir, mut state, _token, _import_id) = test_state().await;
    {
        let cfg = Arc::make_mut(&mut state.cfg);
        cfg.server.as_mut().unwrap().openapi_ui = true;
    }
    let response = get_path(state, "/openapi.json").await;
    assert_eq!(response.status(), StatusCode::OK);
    let v: serde_json::Value = response.json().await.unwrap();
    assert!(v["openapi"].as_str().unwrap().starts_with("3."));
}

async fn auth_route_status(path: &str) -> StatusCode {
    let (_dir, state, _token, _import_id) = test_state().await;
    // The public auth router on its own, not http_app: the point is that
    // these routes are gone from that router, whatever the full app does.
    let server = crate::test_support::serve_router(auth_public_router().with_state(state)).await;
    reqwest::Client::new()
        .post(format!("{}{path}", server.base()))
        .send()
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn try_demo_route_is_gone() {
    // server.rs's own helper returns (TempDir, AppState, token, import_id).
    // The shared harness in test_support.rs does not exist until Task 4.
    let (_dir, state, _token, _import_id) = test_state().await;
    let response = get_path(state, "/v1/auth/try-demo").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn local_auth_routes_exist() {
    for path in ["/v1/accounts", "/v1/session"] {
        assert_ne!(auth_route_status(path).await, StatusCode::NOT_FOUND);
    }
}

#[tokio::test]
async fn imports_complete_and_detail_surface_timings_and_issues() {
    let (_dir, state, token, import_id) = test_state().await;
    let body = CompleteImportBody {
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
        summary: Some(serde_json::json!({
            "parse": { "messages": 10 },
            "convert": { "files": 2 }
        })),
        issues: vec![
            CompleteImportIssueBody {
                kind: "skip".into(),
                step: "convert".into(),
                item: "photo.heic".into(),
                reason: "convert failed".into(),
            },
            CompleteImportIssueBody {
                kind: "error".into(),
                step: "upload".into(),
                item: "archive.zip".into(),
                reason: "upload failed".into(),
            },
        ],
    };

    let response = imports_complete_handler(
        State(state.clone()),
        import_access(&state, &token).await,
        AxumPath(import_id),
        Json(body),
    )
    .await
    .unwrap();
    assert_eq!(response.0.status, "completed");
    assert_eq!(response.0.message_count, 10);
    assert_eq!(response.0.attachment_count, 2);
    assert_eq!(response.0.bytes_uploaded, 100);

    let detail = imports_get_handler(
        State(state.clone()),
        import_access(&state, &token).await,
        AxumPath(import_id),
    )
    .await
    .unwrap();
    let value = detail.0;
    assert_eq!(value.id, import_id);
    assert_eq!(value.duration_ms, Some(48_000));
    assert_eq!(value.parse_ms, Some(18_000));
    assert_eq!(value.attachments_ms, Some(22_000));
    assert_eq!(value.prepare_ms, Some(4_000));
    assert_eq!(value.upload_ms, Some(8_000));
    assert_eq!(value.summary["parse"]["messages"], 10);
    assert_eq!(value.issues.len(), 2);
    assert_eq!(value.issues[0].kind, "skip");
    assert_eq!(value.issues[0].step, "convert");
    assert_eq!(value.issues[1].kind, "error");
    assert_eq!(value.issues[1].step, "upload");
}

#[tokio::test]
async fn imports_complete_stores_completed_with_issues_status() {
    let (_dir, state, token, import_id) = test_state().await;
    let body = CompleteImportBody {
        ok: true,
        status: Some("completed_with_issues".into()),
        message_count: Some(10),
        attachment_count: Some(2),
        bytes_uploaded: Some(100),
        duration_ms: None,
        parse_ms: None,
        attachments_ms: None,
        prepare_ms: None,
        upload_ms: None,
        summary: None,
        issues: Vec::new(),
    };
    let response = imports_complete_handler(
        State(state.clone()),
        import_access(&state, &token).await,
        AxumPath(import_id),
        Json(body),
    )
    .await
    .unwrap();
    assert_eq!(response.0.status, "completed_with_issues");
}

#[tokio::test]
async fn imports_complete_rejects_unknown_status() {
    let (_dir, state, token, import_id) = test_state().await;
    let body = CompleteImportBody {
        ok: true,
        status: Some("victorious".into()),
        message_count: None,
        attachment_count: None,
        bytes_uploaded: None,
        duration_ms: None,
        parse_ms: None,
        attachments_ms: None,
        prepare_ms: None,
        upload_ms: None,
        summary: None,
        issues: Vec::new(),
    };
    let err = imports_complete_handler(
        State(state.clone()),
        import_access(&state, &token).await,
        AxumPath(import_id),
        Json(body),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, ApiError::ValidationFailed(_)));

    // The session is untouched.
    let mut conn = state.db.acquire().await.unwrap();
    let status: String = sqlx::query_scalar("SELECT status FROM vault_imports WHERE id = $1")
        .bind(import_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(status, "running");
}

#[tokio::test]
async fn imports_complete_rejects_invalid_issue_kind_before_db_write() {
    let (_dir, state, token, import_id) = test_state().await;
    let body = CompleteImportBody {
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
        summary: None,
        issues: vec![CompleteImportIssueBody {
            kind: "warning".into(),
            step: "upload".into(),
            item: "archive.zip".into(),
            reason: "not allowed".into(),
        }],
    };

    let err = imports_complete_handler(
        State(state.clone()),
        import_access(&state, &token).await,
        AxumPath(import_id),
        Json(body),
    )
    .await
    .unwrap_err();

    match err {
        ApiError::ValidationFailed(errors) => {
            assert!(errors[0].contains("invalid import issue kind"));
        }
        other => panic!("expected validation-failed, got {other:?}"),
    }

    let status: String = sqlx::query_scalar("SELECT status FROM vault_imports WHERE id = $1")
        .bind(import_id)
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(status, "running");
}

#[tokio::test]
async fn imports_get_handler_returns_not_found_for_missing_import() {
    let (_dir, state, token, import_id) = test_state().await;
    let err = imports_get_handler(
        State(state.clone()),
        import_access(&state, &token).await,
        AxumPath(import_id + 1),
    )
    .await
    .unwrap_err();

    match err {
        ApiError::NotFound(msg) => {
            assert!(msg.contains("import"));
            assert!(msg.contains("not found"));
        }
        other => panic!("expected not found, got {other:?}"),
    }
}

#[tokio::test]
async fn active_session_is_empty_then_reports_the_live_one() {
    let (_dir, state, token, import_id) = test_state().await;

    let body = CreateImportBody {
        dedupe: false,
        source: "imessage".into(),
        mode: ImportMode::Append,
        tool: Some("message-vault-io".into()),
        stage: Some("write".into()),
        staging_dir: Some("/home/u/message-vault/staging-260830".into()),
        device_id: Some("device-a".into()),
        form: Some(serde_json::json!({ "source": "imessage-ios" })),
        source_fingerprint: Some(serde_json::json!({ "size_bytes": 42 })),
        source_identities: None,
    };
    // `test_state` already opened a session; close it so this one can start.
    let _ = imports_discard_handler(
        State(state.clone()),
        import_access(&state, &token).await,
        AxumPath(import_id),
    )
    .await
    .unwrap();

    let created = imports_create_handler(
        State(state.clone()),
        import_access(&state, &token).await,
        Json(body),
    )
    .await
    .unwrap();

    let session = running_import(&state, &token)
        .await
        .expect("a running run is listed");
    assert_eq!(session.id, created.body.id);
    assert_eq!(session.stage.as_deref(), Some("write"));
    assert_eq!(
        session.staging_dir.as_deref(),
        Some("/home/u/message-vault/staging-260830")
    );
    assert_eq!(session.device_id.as_deref(), Some("device-a"));
    assert_eq!(session.form["source"], "imessage-ios");
}

/// A stored form snapshot never carries credentials, whatever the
/// client posts: the row outlives the run, and the secret must not.
#[tokio::test]
async fn a_stored_form_snapshot_drops_credentials() {
    let (_dir, state, token, import_id) = test_state().await;
    let _ = imports_discard_handler(
        State(state.clone()),
        import_access(&state, &token).await,
        AxumPath(import_id),
    )
    .await
    .unwrap();

    let body = CreateImportBody {
        dedupe: false,
        source: "imessage".into(),
        mode: ImportMode::Append,
        tool: None,
        stage: None,
        staging_dir: None,
        device_id: None,
        // A client that has not learned the rule.
        form: Some(serde_json::json!({
            "source": "imessage-ios",
            "backupPassword": "hunter2",
            "whatsappKey": "0123456789abcdef",
        })),
        source_fingerprint: None,
        source_identities: None,
    };
    let _ = imports_create_handler(
        State(state.clone()),
        import_access(&state, &token).await,
        Json(body),
    )
    .await
    .unwrap();

    let session = running_import(&state, &token)
        .await
        .expect("a running run is listed");
    assert_eq!(
        session.form["source"], "imessage-ios",
        "the rest of the snapshot is kept"
    );
    assert!(
        session.form.get("backupPassword").is_none(),
        "backupPassword was stored: {}",
        session.form
    );
    assert!(
        session.form.get("whatsappKey").is_none(),
        "whatsappKey was stored: {}",
        session.form
    );
}

/// The identity list a client read from the backup rides on the session
/// so a resumed Gate 1 can show it without re-reading the backup.
#[tokio::test]
async fn imports_create_stores_source_identities() {
    let (_dir, state, token, import_id) = test_state().await;
    let _ = imports_discard_handler(
        State(state.clone()),
        import_access(&state, &token).await,
        AxumPath(import_id),
    )
    .await
    .unwrap();

    let body = CreateImportBody {
        dedupe: false,
        source: "imessage".into(),
        mode: ImportMode::Append,
        tool: None,
        stage: None,
        staging_dir: None,
        device_id: None,
        form: None,
        source_fingerprint: None,
        source_identities: Some(serde_json::json!(["+15550001111", "owner@example.com"])),
    };
    let _ = imports_create_handler(
        State(state.clone()),
        import_access(&state, &token).await,
        Json(body),
    )
    .await
    .unwrap();

    let session = running_import(&state, &token)
        .await
        .expect("a running run is listed");
    assert_eq!(
        session.source_identities,
        serde_json::json!(["+15550001111", "owner@example.com"])
    );
}

#[tokio::test]
async fn a_second_session_is_refused_with_conflict() {
    let (_dir, state, token, _import_id) = test_state().await;
    let body = CreateImportBody {
        dedupe: false,
        source: "imessage".into(),
        mode: ImportMode::Append,
        tool: None,
        stage: None,
        staging_dir: None,
        device_id: None,
        form: None,
        source_fingerprint: None,
        source_identities: None,
    };
    let err = imports_create_handler(
        State(state.clone()),
        import_access(&state, &token).await,
        Json(body),
    )
    .await
    .unwrap_err();
    let ApiError::StateConflict(message) = &err else {
        panic!("expected StateConflict, got {err:?}");
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
async fn stage_endpoint_advances_and_rejects_an_unknown_stage() {
    let (_dir, state, token, import_id) = test_state().await;

    let _ = imports_patch_handler(
        State(state.clone()),
        import_access(&state, &token).await,
        AxumPath(import_id),
        Json(SetImportStageBody {
            stage: "pushing".into(),
            summary: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(
        running_import(&state, &token)
            .await
            .unwrap()
            .stage
            .as_deref(),
        Some("pushing")
    );

    let err = imports_patch_handler(
        State(state.clone()),
        import_access(&state, &token).await,
        AxumPath(import_id),
        Json(SetImportStageBody {
            stage: "halfway".into(),
            summary: None,
        }),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, ApiError::ValidationFailed(_)));
}

#[tokio::test]
async fn discard_frees_the_slot() {
    let (_dir, state, token, import_id) = test_state().await;
    let _ = imports_discard_handler(
        State(state.clone()),
        import_access(&state, &token).await,
        AxumPath(import_id),
    )
    .await
    .unwrap();
    assert!(running_import(&state, &token).await.is_none());
}

/// `/v1/contacts/{id}` takes an `i64`, and two literal routes sit beside
/// it: `summaries` and `unmatched-handles`. Both are `POST`, and editing a
/// contact is a `PATCH`, so if the `{id}` route ever swallowed one of them
/// the request would come back 405 (no `POST` on `/v1/contacts/{id}`)
/// instead of reaching its own handler. Each assertion below distinguishes
/// "matched my route and rejected my body" from "matched the wrong route".
#[tokio::test]
async fn literal_contact_routes_are_not_captured_by_the_id_route() {
    let vault = crate::test_support::test_vault().await;
    let state = vault.state.clone();
    let user =
        crate::test_support::register_via_api(&state, "contact-routes", "hunter2hunter2").await;

    assert_eq!(
        crate::test_support::get_status(&state, "/v1/contacts", &user.token).await,
        StatusCode::OK
    );

    // A real id still reaches the detail handler: an unknown contact is its
    // 404, not a 400 from a failed `i64` path parse.
    assert_eq!(
        crate::test_support::get_status(&state, "/v1/contacts/999999", &user.token).await,
        StatusCode::NOT_FOUND
    );

    for path in ["/v1/contacts/summaries", "/v1/contacts/unmatched-handles"] {
        let status =
            crate::test_support::post_status(&state, path, &user.token, serde_json::json!({}))
                .await;
        assert_ne!(
            status,
            StatusCode::METHOD_NOT_ALLOWED,
            "{path} was captured by /v1/contacts/{{id}}"
        );
    }
}

/// The `ImportAccess` extractor guards `GET /v1/imports`: with
/// `can_import` off, the endpoint refuses; turned back on, it succeeds.
/// Nothing else in the suite calls this route through the real HTTP
/// stack, so swapping the handler onto a weaker extractor would ship
/// green without this test.
#[tokio::test]
async fn import_endpoint_honors_can_import_flag() {
    let vault = crate::test_support::test_vault().await;
    let state = vault.state.clone();
    let owner =
        crate::test_support::claim_vault_as_owner(&state, "import-guard-keeper", "hunter2hunter2")
            .await;
    let user =
        crate::test_support::register_via_api(&state, "import-guard-user", "hunter2hunter2").await;

    assert_eq!(
        crate::test_support::patch_status(
            &state,
            &format!("/v1/accounts/{}", user.account_id),
            &owner.token,
            serde_json::json!({ "can_import": false }),
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        crate::test_support::get_status(&state, "/v1/imports", &user.token).await,
        StatusCode::FORBIDDEN,
        "can_import=false must refuse GET /v1/imports"
    );

    assert_eq!(
        crate::test_support::patch_status(
            &state,
            &format!("/v1/accounts/{}", user.account_id),
            &owner.token,
            serde_json::json!({ "can_import": true }),
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        crate::test_support::get_status(&state, "/v1/imports", &user.token).await,
        StatusCode::OK,
        "can_import=true must allow GET /v1/imports"
    );
}

/// The `ExportAccess` extractor guards `GET /v1/exports`: with `can_export`
/// off, the endpoint refuses; turned back on, it succeeds.
#[tokio::test]
async fn export_endpoint_honors_can_export_flag() {
    let vault = crate::test_support::test_vault().await;
    let state = vault.state.clone();
    let owner =
        crate::test_support::claim_vault_as_owner(&state, "export-guard-keeper", "hunter2hunter2")
            .await;
    let user =
        crate::test_support::register_via_api(&state, "export-guard-user", "hunter2hunter2").await;

    assert_eq!(
        crate::test_support::patch_status(
            &state,
            &format!("/v1/accounts/{}", user.account_id),
            &owner.token,
            serde_json::json!({ "can_export": false }),
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        crate::test_support::get_status(&state, "/v1/exports", &user.token).await,
        StatusCode::FORBIDDEN,
        "can_export=false must refuse GET /v1/exports"
    );

    assert_eq!(
        crate::test_support::patch_status(
            &state,
            &format!("/v1/accounts/{}", user.account_id),
            &owner.token,
            serde_json::json!({ "can_export": true }),
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        crate::test_support::get_status(&state, "/v1/exports", &user.token).await,
        StatusCode::OK,
        "can_export=true must allow GET /v1/exports"
    );
}

/// `RequestBodyLimitLayer` answers its own 413 the moment a `Content-Length`
/// announces an oversize body, without running any handler. That response
/// must still pass through the CORS layer, or a browser reports a CORS
/// failure instead of showing the 413 the vault sent.
#[tokio::test]
async fn the_fast_413_carries_cors_headers() {
    let vault = crate::test_support::test_vault().await;
    // The default test config's `cors_origins` is empty, which only
    // allows the packaged desktop origins (`build_cors_layer`) — not the
    // browser origin this test sends. Configure it explicitly so the
    // assertion below tests CORS header propagation, not the allow list.
    let mut state = with_cors(vault.state.clone(), &["https://app.example"]);
    state.max_body_bytes = 1024;
    let user = crate::test_support::register_via_api(&state, "alice", "hunter2hunter2").await;

    let (_, created): (String, serde_json::Value) = crate::test_support::post_created_json(
        &state,
        "/v1/imports",
        &user.token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    let server = crate::test_support::serve(&state).await;
    let response = reqwest::Client::new()
        .post(format!(
            "{}/v1/imports/{}/batches",
            server.base(),
            created["id"]
        ))
        .bearer_auth(&user.token)
        .header(header::ORIGIN, "https://app.example")
        .header(header::CONTENT_TYPE, "application/x-ndjson")
        // A sized body, so the limit layer answers from Content-Length alone.
        .body(vec![b'x'; 4096])
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert!(
        response
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN),
        "the fast 413 must carry CORS headers, got: {:?}",
        response.headers()
    );
    let body: serde_json::Value = response.json().await.unwrap();
    assert!(body["detail"].is_string(), "{body}");
}

/// Every response carries an id the server made, a failure repeats it in the
/// body, and an id the client sends is dropped rather than kept.
#[tokio::test]
async fn every_response_carries_a_server_made_request_id_and_a_problem_repeats_it() {
    let vault = crate::test_support::test_vault().await;
    let state = vault.state.clone();
    let user = crate::test_support::register_via_api(&state, "alice", "hunter2hunter2").await;
    let server = crate::test_support::serve(&state).await;
    let client = reqwest::Client::new();

    let ok = client
        .get(format!("{}/v1/conversations", server.base()))
        .bearer_auth(&user.token)
        .send()
        .await
        .unwrap();
    assert_eq!(ok.status(), StatusCode::OK);
    let id = ok.headers()[crate::request_id::HEADER]
        .to_str()
        .unwrap()
        .to_string();
    assert!(uuid::Uuid::parse_str(&id).is_ok(), "not a UUID: {id}");

    let failed = client
        .get(format!("{}/v1/conversations/abc/sources", server.base()))
        .bearer_auth(&user.token)
        .header(crate::request_id::HEADER, "chosen-by-the-client")
        .send()
        .await
        .unwrap();
    let header = failed.headers()[crate::request_id::HEADER]
        .to_str()
        .unwrap()
        .to_string();
    assert_ne!(header, "chosen-by-the-client");
    assert_eq!(
        failed.headers()[header::CONTENT_TYPE],
        crate::problem::Problem::CONTENT_TYPE
    );
    let status = failed.status();
    let text = failed.text().await.unwrap();
    let problem = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::ValidationFailed,
    );
    assert_eq!(problem.request_id.as_deref(), Some(header.as_str()));
    assert!(problem.errors.is_some_and(|e| !e.is_empty()), "{text}");
}

/// A wrong password is `invalid-credentials` at 401, and the attempt after
/// the limit is `rate-limited` with a `Retry-After` the body repeats.
#[tokio::test]
async fn a_wrong_password_is_401_and_the_limit_answers_429_with_retry_after() {
    let vault = crate::test_support::test_vault().await;
    let state = vault.state.clone();
    crate::test_support::register_via_api(&state, "alice", "hunter2hunter2").await;
    let server = crate::test_support::serve(&state).await;
    let client = reqwest::Client::new();
    let login = || {
        client
            .post(format!("{}/v1/session", server.base()))
            .json(&serde_json::json!({ "username": "alice", "password": "not-it-at-all" }))
            .send()
    };

    let wrong = login().await.unwrap();
    let status = wrong.status();
    let text = wrong.text().await.unwrap();
    let problem = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::InvalidCredentials,
    );
    assert_eq!(
        problem.detail.as_deref(),
        Some("invalid username or password")
    );

    for _ in 1..crate::credentials::AUTH_RATE_MAX {
        assert_eq!(login().await.unwrap().status(), StatusCode::UNAUTHORIZED);
    }
    let limited = login().await.unwrap();
    let retry_after: u64 = limited.headers()[header::RETRY_AFTER]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    let status = limited.status();
    let text = limited.text().await.unwrap();
    let problem = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::RateLimited,
    );
    assert_eq!(problem.retry_after, Some(retry_after));
    assert!((1..=crate::credentials::AUTH_RATE_WINDOW.as_secs()).contains(&retry_after));
}

/// `Accept` is checked on the `/v1` routes that produce JSON and nowhere
/// else: not on the static app, and not on the asset download.
#[tokio::test]
async fn accept_is_checked_on_v1_json_routes_only() {
    let vault = crate::test_support::test_vault().await;
    let state = vault.state.clone();
    let user = crate::test_support::register_via_api(&state, "alice", "hunter2hunter2").await;
    let server = crate::test_support::serve(&state).await;
    let client = reqwest::Client::new();

    let refused = client
        .get(format!("{}/v1/conversations", server.base()))
        .bearer_auth(&user.token)
        .header(header::ACCEPT, "text/html")
        .send()
        .await
        .unwrap();
    let status = refused.status();
    let text = refused.text().await.unwrap();
    crate::test_support::expect_problem(status, &text, crate::problem::ProblemType::NotAcceptable);

    for accept in [
        "application/json",
        "*/*",
        "text/html, */*;q=0.1",
        "application/*",
    ] {
        let allowed = client
            .get(format!("{}/v1/conversations", server.base()))
            .bearer_auth(&user.token)
            .header(header::ACCEPT, accept)
            .send()
            .await
            .unwrap();
        assert_eq!(allowed.status(), StatusCode::OK, "Accept: {accept}");
    }

    // A browser navigation: the static app is served, or 404 without a
    // static dir here, but never refused for its Accept.
    let page = client
        .get(format!("{}/", server.base()))
        .header(header::ACCEPT, "text/html")
        .send()
        .await
        .unwrap();
    assert_ne!(page.status(), StatusCode::NOT_ACCEPTABLE);

    // The one /v1 route that streams bytes takes any Accept; the route then
    // refuses for its own reasons (no source named), never for the header.
    let asset = client
        .get(format!(
            "{}/v1/assets/{}",
            server.base(),
            crate::test_support::fake_sha256('a')
        ))
        .bearer_auth(&user.token)
        .header(header::ACCEPT, "image/jpeg")
        .send()
        .await
        .unwrap();
    assert_ne!(asset.status(), StatusCode::NOT_ACCEPTABLE);
}

#[test]
fn every_api_error_answers_the_status_its_problem_type_declares() {
    let cases: Vec<(ApiError, StatusCode)> = vec![
        (
            ApiError::ValidationFailed(vec!["x".into()]),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            ApiError::MissingParameter("x".into()),
            StatusCode::BAD_REQUEST,
        ),
        (ApiError::MalformedBody("x".into()), StatusCode::BAD_REQUEST),
        (
            ApiError::UnsupportedMediaType("x".into()),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
        (
            ApiError::PayloadTooLarge("x".into()),
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
        (
            ApiError::InvalidCredentials("x".into()),
            StatusCode::UNAUTHORIZED,
        ),
        (
            ApiError::AuthenticationRequired("x".into()),
            StatusCode::UNAUTHORIZED,
        ),
        (
            ApiError::RateLimited {
                retry_after_secs: 30,
            },
            StatusCode::TOO_MANY_REQUESTS,
        ),
        (ApiError::UsernameTaken("x".into()), StatusCode::CONFLICT),
        (ApiError::NameTaken("x".into()), StatusCode::CONFLICT),
        (
            ApiError::DemoAccountProtected("x".into()),
            StatusCode::FORBIDDEN,
        ),
        (ApiError::NotTheOwner("x".into()), StatusCode::FORBIDDEN),
        (
            ApiError::InsufficientScope("x".into()),
            StatusCode::FORBIDDEN,
        ),
        (ApiError::AccountDisabled("x".into()), StatusCode::FORBIDDEN),
        (
            ApiError::SearchQueryInvalid {
                detail: "x".into(),
                word: None,
                did_you_mean: None,
            },
            StatusCode::BAD_REQUEST,
        ),
        (ApiError::StateConflict("x".into()), StatusCode::CONFLICT),
        (
            ApiError::AssetUploadInvalid("x".into()),
            StatusCode::BAD_REQUEST,
        ),
        (ApiError::NotFound("x".into()), StatusCode::NOT_FOUND),
        (
            ApiError::MethodNotAllowed("x".into()),
            StatusCode::METHOD_NOT_ALLOWED,
        ),
        (
            ApiError::NotAcceptable("x".into()),
            StatusCode::NOT_ACCEPTABLE,
        ),
        (
            ApiError::Internal(anyhow::anyhow!("x")),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    ];
    for (error, status) in cases {
        assert_eq!(error.status(), status, "{error:?}");
    }
}

#[test]
fn every_api_error_displays_its_detail_sentence() {
    assert_eq!(
        ApiError::ValidationFailed(vec!["name is required".into(), "name is too long".into()])
            .to_string(),
        "name is required; name is too long"
    );
    assert_eq!(
        ApiError::MissingParameter("q is required".into()).to_string(),
        "q is required"
    );
    assert_eq!(
        ApiError::MalformedBody("body is not JSON".into()).to_string(),
        "body is not JSON"
    );
    assert_eq!(
        ApiError::UnsupportedMediaType("send application/json".into()).to_string(),
        "send application/json"
    );
    assert_eq!(
        ApiError::PayloadTooLarge("request body too large".into()).to_string(),
        "request body too large"
    );
    assert_eq!(
        ApiError::InvalidCredentials("wrong password".into()).to_string(),
        "wrong password"
    );
    assert_eq!(
        ApiError::AuthenticationRequired("no bearer token".into()).to_string(),
        "no bearer token"
    );
    assert_eq!(
        ApiError::RateLimited {
            retry_after_secs: 30
        }
        .to_string(),
        "too many authentication attempts; try again in 30 seconds"
    );
    assert_eq!(
        ApiError::UsernameTaken("alice is taken".into()).to_string(),
        "alice is taken"
    );
    assert_eq!(
        ApiError::NameTaken("Book Club is taken".into()).to_string(),
        "Book Club is taken"
    );
    assert_eq!(
        ApiError::DemoAccountProtected("the demo account stays".into()).to_string(),
        "the demo account stays"
    );
    assert_eq!(
        ApiError::NotTheOwner("owner only".into()).to_string(),
        "owner only"
    );
    assert_eq!(
        ApiError::InsufficientScope("needs import".into()).to_string(),
        "needs import"
    );
    assert_eq!(
        ApiError::AccountDisabled("account disabled".into()).to_string(),
        "account disabled"
    );
    assert_eq!(
        ApiError::SearchQueryInvalid {
            detail: "unknown word: frm".into(),
            word: Some("frm"),
            did_you_mean: Some("from"),
        }
        .to_string(),
        "unknown word: frm"
    );
    assert_eq!(
        ApiError::StateConflict("import already active".into()).to_string(),
        "import already active"
    );
    assert_eq!(
        ApiError::AssetUploadInvalid("part 3 is missing".into()).to_string(),
        "part 3 is missing"
    );
    assert_eq!(
        ApiError::NotFound("no such conversation".into()).to_string(),
        "no such conversation"
    );
    assert_eq!(
        ApiError::MethodNotAllowed("no PUT here".into()).to_string(),
        "no PUT here"
    );
    assert_eq!(
        ApiError::NotAcceptable("only JSON".into()).to_string(),
        "only JSON"
    );
    assert_eq!(
        ApiError::Internal(anyhow::anyhow!("disk full").context("stage conversation")).to_string(),
        "stage conversation: disk full"
    );
}

#[test]
fn a_sqlx_error_becomes_an_internal_error_and_keeps_its_message() {
    let error = ApiError::from(sqlx::Error::RowNotFound);

    assert!(matches!(error, ApiError::Internal(_)), "{error:?}");
    assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        error.to_string(),
        "no rows returned by a query that expected to return at least one row"
    );
}

#[tokio::test]
async fn discard_body_drains_a_body_within_the_cap() {
    let body = axum::body::Body::from(vec![7u8; 1024]);

    let drained = discard_body(body, 1024).await;

    assert!(drained.is_ok(), "{drained:?}");
}

#[tokio::test]
async fn discard_body_refuses_a_body_over_the_cap() {
    let body = axum::body::Body::from(vec![7u8; 1025]);

    let error = discard_body(body, 1024).await.unwrap_err();

    assert_eq!(error.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(error.to_string(), "request body too large");
}

#[tokio::test]
async fn discard_body_reports_a_stream_that_fails_midway() {
    let chunks: Vec<Result<axum::body::Bytes, std::io::Error>> = vec![
        Ok(axum::body::Bytes::from_static(b"abc")),
        Err(std::io::Error::other("connection reset")),
    ];
    let body = axum::body::Body::from_stream(futures_util::stream::iter(chunks));

    let error = discard_body(body, 1024).await.unwrap_err();

    assert_eq!(error.status(), StatusCode::BAD_REQUEST);
    assert_eq!(error.to_string(), "failed to read body: connection reset");
}
