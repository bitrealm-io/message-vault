//! Shared HTTP helpers for the server's own tests. [`serve`] is the one place
//! that binds a listener and spawns the app; every helper below issues one
//! request through it, reads the whole response, and lets the server drop.
//!
//! Distinct from `server.rs`'s `test_state()`, which returns a four-tuple
//! `(TempDir, AppState, String, i64)` for handler-level tests that call a
//! handler function directly. This module drives the whole stack over real
//! HTTP, for tests in `auth.rs`, `owner_api.rs`, `api_tokens_api.rs`, and
//! any route whose contract is worth checking end to end.

use axum::http::StatusCode;
use serde::de::DeserializeOwned;
use tempfile::TempDir;

use crate::server::{AppState, http_app};

/// A vault plus its temp directory. Drop the `TempDir` last.
pub struct TestVault {
    /// Keeps the temp directory alive for the test's lifetime.
    tmp: TempDir,
    /// The server state every helper drives.
    pub state: AppState,
}

/// An account created through the API, with its session token.
pub struct RegisteredAccount {
    /// The new account's id.
    pub account_id: String,
    /// The username it was created with.
    pub username: String,
    /// A live session token for it.
    pub token: String,
}

/// A running instance of the real axum app on an ephemeral port.
///
/// The task is aborted when this value drops, so it must stay alive until the
/// response body has been read. A helper must not hand back a response whose
/// server has already been told to stop, so the body is always read before
/// this value drops, regardless of how the runtime handles shutdown.
pub struct TestServer {
    base: String,
    handle: tokio::task::JoinHandle<()>,
}

impl TestServer {
    /// `http://127.0.0.1:<port>`, to prefix a path with.
    pub fn base(&self) -> &str {
        &self.base
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

/// Start the real axum app for `state` on an ephemeral port.
///
/// Every HTTP helper below goes through this; a test that serves a router
/// other than `http_app` (the public auth router on its own, say) takes
/// [`serve_router`] directly.
pub async fn serve(state: &AppState) -> TestServer {
    serve_router(http_app(state.clone())).await
}

/// Start `app` on an ephemeral port.
///
/// This is the one place in the test suite that binds a listener. The server
/// task is aborted when the returned [`TestServer`] drops.
pub async fn serve_router(app: axum::Router) -> TestServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    TestServer {
        base: format!("http://{address}"),
        handle,
    }
}

/// True when `MV_TEST_POSTGRES_URL` points the suite at Postgres.
///
/// A test whose subject is SQLite itself (a pragma, the FTS5 table, the
/// `nocase` collation, `sqlite_stat1`, a trigger written in SQLite's syntax)
/// returns early on this and says why in a comment; where the same promise
/// matters on Postgres, a `_pg` twin carries it there.
pub fn on_postgres() -> bool {
    crate::pg_test_url().is_some()
}

/// An empty vault with schema applied and no accounts.
///
/// Public registration is turned on, because most of the suite reaches the
/// vault through `register_via_api` and a real vault ships with it off. Tests
/// whose subject is the closed vault turn it back off and say so.
pub async fn test_vault() -> TestVault {
    let (pool, tmp) = crate::db::engine::test_pool().await;
    {
        let mut conn = pool.acquire().await.unwrap();
        crate::db::schema::ensure_vault_schema(&mut conn)
            .await
            .unwrap();
        crate::db::schema::ensure_accounts_schema(&mut conn)
            .await
            .unwrap();
        crate::db::vault_settings::set_public_registration(&mut conn, true)
            .await
            .unwrap();
    }
    let state = crate::server::test_app_state(pool, tmp.path()).await;
    TestVault { tmp, state }
}

impl TestVault {
    /// A connection from this vault's pool, for a test that seeds or asserts
    /// with SQL directly.
    pub async fn conn(&self) -> sqlx::pool::PoolConnection<sqlx::Any> {
        self.state.db.acquire().await.unwrap()
    }

    /// The vault's temp directory, for a test that needs a real path on disk.
    pub fn dir(&self) -> &std::path::Path {
        self.tmp.path()
    }

    /// Insert an `accounts` row with a chosen id, for a test that asserts on
    /// the id itself. Returns the id it was given, so a caller can bind the
    /// result rather than repeat the literal.
    pub async fn account_with_id(&self, id: &str, username: &str) -> String {
        let mut conn = self.conn().await;
        sqlx::query("INSERT INTO accounts (id, username) VALUES ($1, $2)")
            .bind(id)
            .bind(username)
            .execute(&mut *conn)
            .await
            .unwrap();
        id.to_string()
    }

    /// Insert an `accounts` row under a generated id, for a test that only
    /// needs an account to exist.
    pub async fn account(&self, username: &str) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        self.account_with_id(&id, username).await
    }
}

/// Issue one request against a freshly started app and read the whole
/// response. `body` is a content type and the bytes to send with it.
async fn request(
    state: &AppState,
    method: reqwest::Method,
    path: &str,
    token: Option<&str>,
    body: Option<(&str, reqwest::Body)>,
) -> (StatusCode, String) {
    let server = serve(state).await;
    let mut req = reqwest::Client::new().request(method, format!("{}{path}", server.base()));
    if let Some(token) = token {
        req = req.bearer_auth(token);
    }
    if let Some((content_type, body)) = body {
        req = req
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .body(body);
    }
    let response = req.send().await.unwrap();
    let status = response.status();
    // Read the body before `server` drops and aborts the task.
    let text = response.text().await.unwrap();
    (status, text)
}

/// The JSON body every typed helper sends.
fn json_body(value: serde_json::Value) -> (&'static str, reqwest::Body) {
    (
        "application/json",
        reqwest::Body::from(serde_json::to_vec(&value).expect("test JSON always serializes")),
    )
}

/// Decode a response the caller expects to be `200 OK` with a JSON body.
fn expect_ok<T: DeserializeOwned>(what: &str, status: StatusCode, text: &str) -> T {
    assert_eq!(status, StatusCode::OK, "{what} must succeed, got: {text}");
    serde_json::from_str(text).unwrap_or_else(|e| panic!("{what} returned non-JSON ({e}): {text}"))
}

/// Register an account through the API and return it with a live token.
///
/// The auth rate limiter lives on `AppState` (`auth::AuthRateLimits`), so the
/// hits counted here belong to this vault alone. That matters because the
/// suite reuses a handful of literal usernames ("alice", "bob", ...) across
/// many test functions in one test binary: with a shared limiter, enough tests
/// registering the same name inside one 60-second window would trip
/// `AUTH_RATE_MAX` and fail an unrelated test with a 429.
pub async fn register_via_api(
    state: &AppState,
    username: &str,
    password: &str,
) -> RegisteredAccount {
    let (status, text) = request(
        state,
        reqwest::Method::POST,
        "/v1/auth/register",
        None,
        Some(json_body(
            serde_json::json!({ "username": username, "password": password }),
        )),
    )
    .await;
    let body: serde_json::Value = expect_ok("register", status, &text);
    RegisteredAccount {
        account_id: body["account_id"].as_str().unwrap().to_string(),
        username: body["username"].as_str().unwrap().to_string(),
        token: body["token"].as_str().unwrap().to_string(),
    }
}

/// Claim the test vault: create its owner directly, then sign in as them.
///
/// There is no HTTP route for this in PR 1 — claiming over HTTP arrives with
/// `GET /v1/vault` — so the row goes in through `insert_account` at the
/// well-known owner id, exactly as `create-owner` does it from a shell.
pub async fn claim_vault_as_owner(
    state: &AppState,
    username: &str,
    password: &str,
) -> RegisteredAccount {
    let hash = crate::auth::hash_password(password).expect("hash the owner password");
    let mut conn = state.db.acquire().await.expect("acquire for claim");
    crate::db::account_profile::insert_account(
        &mut conn,
        crate::db::account_profile::OWNER_ACCOUNT_ID,
        username,
        Some(&hash),
        None,
    )
    .await
    .expect("insert the vault owner");
    drop(conn);

    let (status, text) = request(
        state,
        reqwest::Method::POST,
        "/v1/auth/login",
        None,
        Some(json_body(
            serde_json::json!({ "username": username, "password": password }),
        )),
    )
    .await;
    let body: serde_json::Value = expect_ok("owner login", status, &text);
    RegisteredAccount {
        account_id: crate::db::account_profile::OWNER_ACCOUNT_ID.to_string(),
        username: username.to_string(),
        token: body["token"].as_str().unwrap().to_string(),
    }
}

/// The status of a login attempt.
pub async fn login_status(state: &AppState, username: &str, password: &str) -> StatusCode {
    request(
        state,
        reqwest::Method::POST,
        "/v1/auth/login",
        None,
        Some(json_body(
            serde_json::json!({ "username": username, "password": password }),
        )),
    )
    .await
    .0
}

/// GET a path with a Bearer token, returning only the status.
pub async fn get_status(state: &AppState, path: &str, token: &str) -> StatusCode {
    request(state, reqwest::Method::GET, path, Some(token), None)
        .await
        .0
}

/// GET a path with a Bearer token and decode the JSON body.
pub async fn get_json<T: DeserializeOwned>(state: &AppState, path: &str, token: &str) -> T {
    let (status, text) = request(state, reqwest::Method::GET, path, Some(token), None).await;
    expect_ok(&format!("GET {path}"), status, &text)
}

/// POST a JSON body with a Bearer token and decode the JSON response.
pub async fn post_json<T: DeserializeOwned>(
    state: &AppState,
    path: &str,
    token: &str,
    body: serde_json::Value,
) -> T {
    let (status, text) = request(
        state,
        reqwest::Method::POST,
        path,
        Some(token),
        Some(json_body(body)),
    )
    .await;
    expect_ok(&format!("POST {path}"), status, &text)
}

/// POST a JSON body to a route that creates one resource, asserting
/// `201 Created` and a `Location` under the request path, and returning the
/// body. The `Location` is handed back with it so a test can check it names
/// the id the body carries.
pub async fn post_created_json<T: DeserializeOwned>(
    state: &AppState,
    path: &str,
    token: &str,
    body: serde_json::Value,
) -> (String, T) {
    let server = serve(state).await;
    let response = reqwest::Client::new()
        .post(format!("{}{path}", server.base()))
        .bearer_auth(token)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(serde_json::to_vec(&body).expect("test JSON always serializes"))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let location = response
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let text = response.text().await.unwrap();
    assert_eq!(
        status,
        StatusCode::CREATED,
        "POST {path} must answer 201 Created, got: {text}"
    );
    let location =
        location.unwrap_or_else(|| panic!("POST {path} answered 201 without a Location"));
    assert!(
        location.starts_with(&format!("{path}/")),
        "POST {path} Location must name a member under it, got {location}"
    );
    let parsed = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("POST {path} returned non-JSON ({e}): {text}"));
    (location, parsed)
}

/// The problem document a failure answered, or a panic naming the body
/// that was not one.
pub fn problem(text: &str) -> vault_api_types::Problem {
    serde_json::from_str(text).unwrap_or_else(|e| panic!("not a problem document ({e}): {text}"))
}

/// Assert a failure is a problem of `kind` with the status the registry
/// gives it, and hand the document back for any further check.
pub fn expect_problem(
    status: StatusCode,
    text: &str,
    kind: crate::problem::ProblemType,
) -> vault_api_types::Problem {
    let problem = problem(text);
    assert_eq!(status, kind.status(), "{text}");
    assert_eq!(problem.status, kind.status().as_u16(), "{text}");
    assert_eq!(problem.kind, kind.url(), "{text}");
    assert_eq!(problem.title, kind.title(), "{text}");
    assert!(problem.request_id.is_some(), "no request_id: {text}");
    problem
}

/// POST a JSON body with a Bearer token, returning only the status.
pub async fn post_status(
    state: &AppState,
    path: &str,
    token: &str,
    body: serde_json::Value,
) -> StatusCode {
    request(
        state,
        reqwest::Method::POST,
        path,
        Some(token),
        Some(json_body(body)),
    )
    .await
    .0
}

/// PUT a JSON body with a Bearer token, asserting 200 and parsing the body.
pub async fn put_json<T: DeserializeOwned>(
    state: &AppState,
    path: &str,
    token: &str,
    body: serde_json::Value,
) -> T {
    let (status, text) = request(
        state,
        reqwest::Method::PUT,
        path,
        Some(token),
        Some(json_body(body)),
    )
    .await;
    expect_ok(&format!("PUT {path}"), status, &text)
}

/// DELETE with a JSON body and a Bearer token, returning only the status.
/// `DELETE /v1/account` and `DELETE /v1/account/messages` carry their
/// confirmation in the body.
pub async fn delete_status_with_body(
    state: &AppState,
    path: &str,
    token: &str,
    body: serde_json::Value,
) -> StatusCode {
    request(
        state,
        reqwest::Method::DELETE,
        path,
        Some(token),
        Some(json_body(body)),
    )
    .await
    .0
}

/// DELETE with a JSON body and a Bearer token, asserting 200 and parsing
/// the body.
pub async fn delete_json_with_body<T: DeserializeOwned>(
    state: &AppState,
    path: &str,
    token: &str,
    body: serde_json::Value,
) -> T {
    let (status, text) = request(
        state,
        reqwest::Method::DELETE,
        path,
        Some(token),
        Some(json_body(body)),
    )
    .await;
    expect_ok(&format!("DELETE {path}"), status, &text)
}

/// PUT a JSON body with a Bearer token, returning only the status.
pub async fn put_status(
    state: &AppState,
    path: &str,
    token: &str,
    body: serde_json::Value,
) -> StatusCode {
    request(
        state,
        reqwest::Method::PUT,
        path,
        Some(token),
        Some(json_body(body)),
    )
    .await
    .0
}

/// PATCH a JSON body with a Bearer token, returning only the status.
pub async fn patch_status(
    state: &AppState,
    path: &str,
    token: &str,
    body: serde_json::Value,
) -> StatusCode {
    request(
        state,
        reqwest::Method::PATCH,
        path,
        Some(token),
        Some(json_body(body)),
    )
    .await
    .0
}

/// PATCH a JSON body expecting a failure: the status and the `{error}`
/// sentence the vault answered with.
///
/// A route test asserting only a status cannot tell a refusal the person can
/// act on from a different refusal with the same status, so a route that
/// starts answering the wrong sentence stays green.
pub async fn patch_failure(
    state: &AppState,
    path: &str,
    token: &str,
    body: serde_json::Value,
) -> (StatusCode, String) {
    let (status, text) = request(
        state,
        reqwest::Method::PATCH,
        path,
        Some(token),
        Some(json_body(body)),
    )
    .await;
    (status, problem(&text).sentence())
}

/// PATCH a JSON body with a Bearer token and decode the JSON response.
pub async fn patch_json<T: DeserializeOwned>(
    state: &AppState,
    path: &str,
    token: &str,
    body: serde_json::Value,
) -> T {
    let (status, text) = request(
        state,
        reqwest::Method::PATCH,
        path,
        Some(token),
        Some(json_body(body)),
    )
    .await;
    expect_ok(&format!("PATCH {path}"), status, &text)
}

/// DELETE a path with a Bearer token, returning only the status.
pub async fn delete_status(state: &AppState, path: &str, token: &str) -> StatusCode {
    request(state, reqwest::Method::DELETE, path, Some(token), None)
        .await
        .0
}

/// DELETE a path with a Bearer token and decode the JSON response.
pub async fn delete_json<T: DeserializeOwned>(state: &AppState, path: &str, token: &str) -> T {
    let (status, text) = request(state, reqwest::Method::DELETE, path, Some(token), None).await;
    expect_ok(&format!("DELETE {path}"), status, &text)
}

/// POST a body that is not JSON (JSONL, plain text, an empty body) with a
/// Bearer token and an explicit Content-Type, returning the status and the
/// response text. For routes whose contract is the raw body, such as
/// `POST /v1/imports/{id}/batches`.
pub async fn post_raw(
    state: &AppState,
    path: &str,
    token: &str,
    content_type: &str,
    body: impl Into<reqwest::Body>,
) -> (StatusCode, String) {
    request(
        state,
        reqwest::Method::POST,
        path,
        Some(token),
        Some((content_type, body.into())),
    )
    .await
}

/// PUT a body that is not JSON with a Bearer token and an explicit
/// Content-Type, returning the status and the response text. For routes whose
/// contract is the raw body, such as `PUT /v1/assets/{sha256}`.
pub async fn put_raw(
    state: &AppState,
    path: &str,
    token: &str,
    content_type: &str,
    body: impl Into<reqwest::Body>,
) -> (StatusCode, String) {
    request(
        state,
        reqwest::Method::PUT,
        path,
        Some(token),
        Some((content_type, body.into())),
    )
    .await
}

/// GET a path with a Bearer token, returning the status and the raw response
/// text. For asserting on a non-JSON or malformed body, such as the error
/// fallbacks' JSON that a plain `get_json` would panic decoding on failure.
pub async fn get_raw(state: &AppState, path: &str, token: &str) -> (StatusCode, String) {
    request(state, reqwest::Method::GET, path, Some(token), None).await
}

/// DELETE a path with a Bearer token, returning the status and the raw
/// response text. For asserting on the body of a fallback response, such as
/// the JSON `{error}` a wrong method produces.
pub async fn delete_raw(state: &AppState, path: &str, token: &str) -> (StatusCode, String) {
    request(state, reqwest::Method::DELETE, path, Some(token), None).await
}

/// One message to seed into a conversation.
pub struct SeedMessage<'a> {
    /// The `messages.source` slug, such as `imessage`.
    pub source: &'a str,
    /// RFC 3339 timestamp, stored as text the way the importer writes it.
    pub timestamp: &'a str,
    /// Whether the account sent it.
    pub is_from_me: bool,
    /// The message text.
    pub body: &'a str,
}

/// A conversation to seed, with its messages in order.
pub struct SeedConversation<'a> {
    /// The account that owns it.
    pub account_id: &'a str,
    /// The peer handle, created as a `handles` row. Must be unique per
    /// account: `handles` is keyed on the normalized value.
    pub handle: &'a str,
    /// `individual` or `group`.
    pub conversation_type: &'a str,
    /// The group's title, for a group conversation.
    pub group_title: Option<&'a str>,
    /// The `conversations.source_file` value.
    pub source_file: &'a str,
    /// Messages, seeded with `sort_order` following this order.
    pub messages: &'a [SeedMessage<'a>],
}

/// Seed one conversation and its messages, returning the new
/// `conversations.id`.
///
/// `messages.conversation_id` and `conversations.chat_handle_id` are integer
/// foreign keys, so this first creates a `handles` row the way every real
/// importer does rather than binding a string straight into `chat_handle_id`.
pub async fn seed_conversation(state: &AppState, c: &SeedConversation<'_>) -> i64 {
    let mut conn = state.db.acquire().await.unwrap();
    let handle_id: i64 = sqlx::query_scalar(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES ($1, $2, $2, 'phone', 'phone') RETURNING id",
    )
    .bind(c.account_id)
    .bind(c.handle)
    .fetch_one(&mut *conn)
    .await
    .unwrap();

    let conversation_id: i64 = sqlx::query_scalar(
        "INSERT INTO conversations (
            account_id, chat_handle_id, conversation_type, group_title, source_file
         ) VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(c.account_id)
    .bind(handle_id)
    .bind(c.conversation_type)
    .bind(c.group_title)
    .bind(c.source_file)
    .fetch_one(&mut *conn)
    .await
    .unwrap();

    for (index, message) in c.messages.iter().enumerate() {
        sqlx::query(
            "INSERT INTO messages (
                conversation_id, account_id, source, timestamp, is_from_me, sort_order, body
             ) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(conversation_id)
        .bind(c.account_id)
        .bind(message.source)
        .bind(message.timestamp)
        .bind(i64::from(message.is_from_me))
        .bind(index as i64)
        .bind(message.body)
        .execute(&mut *conn)
        .await
        .unwrap();
    }

    conversation_id
}

/// Store a real attachment file for the account's `imessage` source and
/// attach it to the newest message of `conversation_id`, returning the
/// file's path so a test can check whether a delete removed it. The MIME
/// sidecar the store writes beside an extensionless blob is written too, so
/// the same test can check that it went with the file.
///
/// `sha` stands in for the content hash; the store never reads the bytes
/// back here, so it only has to be 64 characters long the way a real digest
/// is. The `messages.source` slug (`imessage`) and the per-source assets
/// directory are one and the same name, which is what lets a delete find the
/// file from the row.
pub async fn attach_stored_file(
    state: &AppState,
    account_id: &str,
    conversation_id: i64,
    sha: &str,
) -> std::path::PathBuf {
    let shard = state
        .cfg
        .paths
        .assets_dir_for_account(account_id, "imessage")
        .join(&sha[..2]);
    std::fs::create_dir_all(&shard).unwrap();
    let path = shard.join(format!("{sha}.jpg"));
    std::fs::write(&path, b"jpeg bytes").unwrap();
    std::fs::write(shard.join(format!(".{sha}.mime")), "image/jpeg").unwrap();

    let mut conn = state.db.acquire().await.unwrap();
    let message_id: i64 = sqlx::query_scalar(
        "SELECT id FROM messages WHERE conversation_id = $1 ORDER BY id DESC LIMIT 1",
    )
    .bind(conversation_id)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    sqlx::query("INSERT INTO attachments (message_id, sha256, assets_path) VALUES ($1, $2, $3)")
        .bind(message_id)
        .bind(sha)
        .bind(format!("{}/{sha}.jpg", &sha[..2]))
        .execute(&mut *conn)
        .await
        .unwrap();
    path
}

/// 64 hex-looking characters, distinct per `tag`: the length of a SHA-256
/// digest, for a test that stores a file under a fingerprint of its choosing.
pub fn fake_sha256(tag: char) -> String {
    std::iter::repeat_n(tag, 64).collect()
}

/// Give an account one conversation holding one message, so counts are
/// non-zero.
pub async fn seed_one_message(state: &AppState, account_id: &str) {
    seed_conversation(
        state,
        &SeedConversation {
            account_id,
            handle: &format!("+1555{account_id}"),
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
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_fixture_makes_an_account_with_the_id_a_test_asks_for() {
        let vault = test_vault().await;
        let id = vault
            .account_with_id("00000000-0000-4000-8000-00000000000f", "alice")
            .await;
        assert_eq!(id, "00000000-0000-4000-8000-00000000000f");

        let mut conn = vault.conn().await;
        let username: String = sqlx::query_scalar("SELECT username FROM accounts WHERE id = $1")
            .bind(&id)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
        assert_eq!(username, "alice");

        let other = vault.account("bob").await;
        assert_ne!(other, id, "each account must get its own id");
    }

    #[tokio::test]
    async fn the_seeder_returns_the_conversation_id_it_made() {
        let vault = test_vault().await;
        let account = vault.account("alice").await;
        let id = seed_conversation(
            &vault.state,
            &SeedConversation {
                account_id: &account,
                handle: "+15555550100",
                conversation_type: "group",
                group_title: Some("Book Club"),
                source_file: "backup-a.jsonl",
                messages: &[
                    SeedMessage {
                        source: "imessage",
                        timestamp: "2020-01-01T00:00:00Z",
                        is_from_me: true,
                        body: "first",
                    },
                    SeedMessage {
                        source: "imessage",
                        timestamp: "2020-01-02T00:00:00Z",
                        is_from_me: false,
                        body: "second",
                    },
                ],
            },
        )
        .await;

        let mut conn = vault.conn().await;
        let title: String =
            sqlx::query_scalar("SELECT group_title FROM conversations WHERE id = $1")
                .bind(id)
                .fetch_one(&mut *conn)
                .await
                .unwrap();
        assert_eq!(title, "Book Club");

        let bodies: Vec<String> = sqlx::query_scalar(
            "SELECT body FROM messages WHERE conversation_id = $1 ORDER BY sort_order",
        )
        .bind(id)
        .fetch_all(&mut *conn)
        .await
        .unwrap();
        assert_eq!(bodies, vec!["first".to_string(), "second".to_string()]);
    }

    /// A body far larger than one TCP segment must come back whole. This
    /// pins the ordering in `request`: the response is read before the
    /// `TestServer` drops and aborts the task serving it.
    #[tokio::test]
    async fn a_large_response_body_is_read_before_the_server_stops() {
        let vault = test_vault().await;
        let user = register_via_api(&vault.state, "alice", "hunter2hunter2").await;
        for i in 0..300 {
            seed_conversation(
                &vault.state,
                &SeedConversation {
                    account_id: &user.account_id,
                    handle: &format!("+1555000{i:04}"),
                    conversation_type: "individual",
                    group_title: None,
                    source_file: "seed.jsonl",
                    messages: &[SeedMessage {
                        source: "imessage",
                        timestamp: "2020-01-01T00:00:00Z",
                        is_from_me: true,
                        body: "hello, this is a message long enough to add up",
                    }],
                },
            )
            .await;
        }

        let (status, text) =
            get_raw(&vault.state, "/v1/conversations?limit=300", &user.token).await;
        assert_eq!(status, StatusCode::OK, "{text}");
        assert!(
            text.len() > 64 * 1024,
            "the fixture must produce a body bigger than one segment, got {} bytes",
            text.len()
        );
        let page: serde_json::Value = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("truncated body ({e}): {} bytes", text.len()));
        assert_eq!(page["items"].as_array().unwrap().len(), 300);
    }
}
