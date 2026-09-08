use super::*;
use crate::extract::{Json, Path as AxumPath};
use crate::import::ImportMode;
use crate::import::{
    CompleteImportBody, CompleteImportIssueBody, CreateImportBody, SetImportStageBody,
    imports_active_handler, imports_complete_handler, imports_create_handler,
    imports_discard_handler, imports_get_handler, imports_stage_handler,
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

const TEST_ACCOUNT: &str = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";

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
    account_profile::insert_account(&mut conn, TEST_ACCOUNT, "alice", None, None)
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
    account_profile::insert_account(&mut conn, TEST_ACCOUNT, "alice", None, None)
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
        matches!(err, ApiError::Forbidden(_)),
        "a disabled account's existing token must stop working, got {err:?}"
    );
}

#[tokio::test]
async fn disabling_an_account_kills_its_live_api_token() {
    let (_dir, mut conn) = test_conn().await;
    account_profile::insert_account(&mut conn, TEST_ACCOUNT, "alice", None, None)
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
        matches!(err, ApiError::Forbidden(_)),
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

    let state = test_app_state(pool, &data_dir).await;

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
    for path in ["/v1/auth/register", "/v1/auth/login"] {
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
    assert!(matches!(err, ApiError::BadRequest(_)));

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
        ApiError::BadRequest(msg) => {
            assert!(msg.contains("invalid import issue kind"));
        }
        other => panic!("expected bad request, got {other:?}"),
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
        source: "imessage".into(),
        mode: ImportMode::Append,
        tool: Some("message-vault-io".into()),
        account: None,
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

    let active = imports_active_handler(State(state.clone()), import_access(&state, &token).await)
        .await
        .unwrap();
    let session = active.0.session.expect("a live session is reported");
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
        source: "imessage".into(),
        mode: ImportMode::Append,
        tool: None,
        account: None,
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

    let active = imports_active_handler(State(state.clone()), import_access(&state, &token).await)
        .await
        .unwrap();
    let session = active.0.session.expect("a live session is reported");
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
        source: "imessage".into(),
        mode: ImportMode::Append,
        tool: None,
        account: None,
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

    let active = imports_active_handler(State(state.clone()), import_access(&state, &token).await)
        .await
        .unwrap();
    let session = active.0.session.expect("a live session is reported");
    assert_eq!(
        session.source_identities,
        serde_json::json!(["+15550001111", "owner@example.com"])
    );
}

#[tokio::test]
async fn a_second_session_is_refused_with_conflict() {
    let (_dir, state, token, _import_id) = test_state().await;
    let body = CreateImportBody {
        source: "imessage".into(),
        mode: ImportMode::Append,
        tool: None,
        account: None,
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
async fn stage_endpoint_advances_and_rejects_an_unknown_stage() {
    let (_dir, state, token, import_id) = test_state().await;

    let _ = imports_stage_handler(
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
    let active = imports_active_handler(State(state.clone()), import_access(&state, &token).await)
        .await
        .unwrap();
    assert_eq!(active.0.session.unwrap().stage.as_deref(), Some("pushing"));

    let err = imports_stage_handler(
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
    assert!(matches!(err, ApiError::BadRequest(_)));
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
    let active = imports_active_handler(State(state.clone()), import_access(&state, &token).await)
        .await
        .unwrap();
    assert!(active.0.session.is_none());
}

/// `/v1/imports/active` is a literal route registered alongside
/// `/v1/imports/{id}`; if router registration order ever let the `{id}`
/// extractor swallow it, `active` would fail to parse as an `i64` and
/// this would come back 400 instead of 200.
#[tokio::test]
async fn active_route_is_not_captured_by_the_id_route() {
    let (_dir, state, token, _import_id) = test_state().await;
    let status = crate::test_support::get_status(&state, "/v1/imports/active", &token).await;
    assert_eq!(status, StatusCode::OK);
}

/// `/v1/contacts/{id}` takes an `i64`, and three literal routes sit beside
/// it: `summaries`, `match`, and `address-book`. All three are `POST`, and
/// editing a contact is now a `PATCH`, so if the `{id}` route ever
/// swallowed one of them the request would come back 405 (no `POST` on
/// `/v1/contacts/{id}`) instead of reaching its own handler. Each
/// assertion below distinguishes "matched my route and rejected my body"
/// from "matched the wrong route".
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

    for path in [
        "/v1/contacts/summaries",
        "/v1/contacts/match",
        "/v1/contacts/address-book",
    ] {
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
            &format!("/v1/owner/accounts/{}", user.account_id),
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
            &format!("/v1/owner/accounts/{}", user.account_id),
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

/// The `ExportAccess` extractor guards `GET /v1/export/messages/count`:
/// with `can_export` off, the endpoint refuses; turned back on, it
/// succeeds.
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
            &format!("/v1/owner/accounts/{}", user.account_id),
            &owner.token,
            serde_json::json!({ "can_export": false }),
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        crate::test_support::get_status(&state, "/v1/export/messages/count", &user.token).await,
        StatusCode::FORBIDDEN,
        "can_export=false must refuse GET /v1/export/messages/count"
    );

    assert_eq!(
        crate::test_support::patch_status(
            &state,
            &format!("/v1/owner/accounts/{}", user.account_id),
            &owner.token,
            serde_json::json!({ "can_export": true }),
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        crate::test_support::get_status(&state, "/v1/export/messages/count", &user.token).await,
        StatusCode::OK,
        "can_export=true must allow GET /v1/export/messages/count"
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

    let server = crate::test_support::serve(&state).await;
    let response = reqwest::Client::new()
        .post(format!(
            "{}/v1/import?source=imessage&mode=append",
            server.base()
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
    assert!(body["error"].is_string(), "{body}");
}
