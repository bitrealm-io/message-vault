//! Router assembly, shared state, auth resolution, and HTTP plumbing.
//!
//! Domain handlers live in their own modules: `auth` (login and session),
//! `profile` (account settings), `contacts_api`, `conversations_api`,
//! `export_api` (messages and counts), `import` (JSONL ingest and import
//! sessions), and `assets` (asset bytes and multipart uploads). This module
//! keeps the pieces they share: [`AppState`], [`ApiError`], Bearer token
//! resolution, body-streaming helpers, and `http_app`, which assembles the
//! router.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use futures_util::StreamExt;
use serde::Serialize;
use sqlx::AnyConnection;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tower_http::cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::services::ServeDir;

use crate::asset_uploads;
use crate::config::Config;
use crate::db::account_profile;
use crate::db::api_tokens;
use crate::db::engine::{self, DbEngine};
use crate::db::permissions::Permissions;
use crate::db::schema;
use crate::db::session_tokens;

/// What a Bearer credential is allowed to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthCapability {
    /// Signed-in session. Carries the account's own permissions.
    Session {
        /// The account may manage users.
        is_admin: bool,
        /// What the account may do.
        permissions: Permissions,
    },
    /// Named API token. Already intersected with its owner's permissions.
    ApiToken(Permissions),
}

/// Authenticated vault account from a session token or named API token.
#[derive(Debug, Clone)]
pub struct AuthIdentity {
    /// The authenticated vault account.
    pub account_id: String,
    /// What this credential is allowed to do.
    pub capability: AuthCapability,
}

impl AuthIdentity {
    /// What this credential may do, account and token already intersected.
    pub fn permissions(&self) -> Permissions {
        match self.capability {
            AuthCapability::Session { permissions, .. } | AuthCapability::ApiToken(permissions) => {
                permissions
            }
        }
    }

    /// True only for a signed-in administrator, never for an API token.
    pub fn is_admin(&self) -> bool {
        matches!(
            self.capability,
            AuthCapability::Session { is_admin: true, .. }
        )
    }

    /// True when the credential is a signed-in session rather than a token.
    pub fn is_session(&self) -> bool {
        matches!(self.capability, AuthCapability::Session { .. })
    }
}

/// Reject API tokens on routes that require a GUI session.
///
/// # Errors
///
/// Returns forbidden when the credential is a named API token.
pub fn require_full_access(auth: &AuthIdentity) -> Result<(), ApiError> {
    if auth.is_session() {
        return Ok(());
    }
    Err(ApiError::Forbidden(
        "this endpoint requires a signed-in session; use an API token only for import/export"
            .into(),
    ))
}

/// Reject anything that is not a signed-in administrator.
///
/// # Errors
///
/// Returns forbidden for ordinary sessions and for every API token.
pub fn require_admin(auth: &AuthIdentity) -> Result<(), ApiError> {
    if auth.is_admin() {
        return Ok(());
    }
    Err(ApiError::Forbidden(
        "this endpoint requires an administrator session".into(),
    ))
}

/// Allow a credential that may import.
///
/// # Errors
///
/// Returns forbidden when import is not permitted.
pub fn require_import_access(auth: &AuthIdentity) -> Result<(), ApiError> {
    if auth.permissions().import {
        return Ok(());
    }
    Err(ApiError::Forbidden("import is not permitted".into()))
}

/// Allow a credential that may export.
///
/// # Errors
///
/// Returns forbidden when export is not permitted.
pub fn require_export_access(auth: &AuthIdentity) -> Result<(), ApiError> {
    if auth.permissions().export {
        return Ok(());
    }
    Err(ApiError::Forbidden("export is not permitted".into()))
}

/// Allow a credential that may import or export, for asset probes.
///
/// # Errors
///
/// Returns forbidden when neither is permitted.
pub fn require_import_or_export_access(auth: &AuthIdentity) -> Result<(), ApiError> {
    let p = auth.permissions();
    if p.import || p.export {
        return Ok(());
    }
    Err(ApiError::Forbidden(
        "this credential cannot access assets".into(),
    ))
}

/// Allow a credential that may destroy message data.
///
/// # Errors
///
/// Returns forbidden when deletion is not permitted.
pub fn require_delete_access(auth: &AuthIdentity) -> Result<(), ApiError> {
    if auth.permissions().delete {
        return Ok(());
    }
    Err(ApiError::Forbidden(
        "deleting messages is not permitted for this account".into(),
    ))
}

/// Allow a signed-in session that may destroy message data: the guard for
/// permanent deletion out of the trash. Both halves matter. Trash is a GUI
/// affair, so an API token is refused the way every trash route refuses it,
/// and the account's own `can_delete` grant is what keeps the demo account
/// from deleting anything while it keeps every other privilege.
///
/// # Errors
///
/// Returns forbidden when the credential is an API token or the account may
/// not delete.
pub fn require_full_delete_access(auth: &AuthIdentity) -> Result<(), ApiError> {
    require_full_access(auth)?;
    require_delete_access(auth)
}

/// Extract the Bearer credential: handlers take `auth: AuthIdentity` (or one
/// of the capability wrappers below) instead of hand-rolling the
/// `resolve_auth` + `require_*` preamble. Rejections are the same
/// [`ApiError`] responses the preamble produced.
impl axum::extract::FromRequestParts<AppState> for AuthIdentity {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        resolve_auth(&parts.headers, state).await
    }
}

/// Define a newtype extractor that resolves the Bearer credential and runs one
/// `require_*` capability check, so a route cannot compile without its guard.
macro_rules! auth_guard {
    ($(#[$doc:meta])* $name:ident, $check:path) => {
        $(#[$doc])*
        pub struct $name(pub AuthIdentity);

        impl axum::extract::FromRequestParts<AppState> for $name {
            type Rejection = ApiError;

            async fn from_request_parts(
                parts: &mut axum::http::request::Parts,
                state: &AppState,
            ) -> Result<Self, Self::Rejection> {
                let auth = resolve_auth(&parts.headers, state).await?;
                $check(&auth)?;
                Ok(Self(auth))
            }
        }
    };
}

auth_guard!(
    /// Signed-in session (API tokens rejected); wraps [`require_full_access`].
    FullAccess,
    require_full_access
);
auth_guard!(
    /// Signed-in administrator session; wraps [`require_admin`].
    Admin,
    require_admin
);
auth_guard!(
    /// Credential that may import; wraps [`require_import_access`].
    ImportAccess,
    require_import_access
);
auth_guard!(
    /// Credential that may export; wraps [`require_export_access`].
    ExportAccess,
    require_export_access
);
auth_guard!(
    /// Credential that may import or export, for asset probes; wraps
    /// [`require_import_or_export_access`].
    ImportOrExportAccess,
    require_import_or_export_access
);
auth_guard!(
    /// Credential that may destroy message data; wraps
    /// [`require_delete_access`].
    DeleteAccess,
    require_delete_access
);
auth_guard!(
    /// Signed-in session whose account may destroy message data; wraps
    /// [`require_full_delete_access`].
    FullDeleteAccess,
    require_full_delete_access
);

/// Shared server state passed to every HTTP handler.
#[derive(Clone)]
pub struct AppState {
    /// Loaded configuration.
    pub cfg: Arc<Config>,
    /// Connection pool (SQLite file or `[database] url`). Handlers acquire
    /// short-lived connections from here.
    pub db: sqlx::AnyPool,
    /// Engine the pool was opened for (SQLite by default, Postgres via URL).
    pub db_engine: DbEngine,
    /// Per-account import mutex: same-account imports stay serialized so staging
    /// rows (the temporary import area) for that tenant are not wiped mid-run.
    /// Different accounts may overlap at the lock layer; SQLite write-ahead
    /// logging plus `busy_timeout` serialize writers.
    pub(crate) account_import_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    /// Serialize multipart complete per (account, sha256) so two clients cannot
    /// race `store_verified` on the same SHA-256 fingerprint.
    pub(crate) asset_complete_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    /// Sliding-window hit counts for the unauthenticated auth endpoints. Held
    /// here, not in a static, so tests in one binary cannot rate-limit each
    /// other; a served vault has a single state, so the limit still spans it.
    pub(crate) auth_rate_limits: crate::auth::AuthRateLimits,
    /// Multipart / asset size limits from `[server]` (env may override part size).
    pub(crate) upload_limits: asset_uploads::UploadLimits,
    /// Axum request body cap (single PUT or one part); equals `asset_max_bytes`.
    pub(crate) max_body_bytes: usize,
}

/// The body of every failure: one sentence, with the HTTP status carrying the meaning.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ErrorBody {
    /// Human-readable description of the failure.
    pub error: String,
}

/// API error returned as a JSON envelope with a matching HTTP status.
#[derive(Debug)]
pub enum ApiError {
    /// `401` — no valid session or API token.
    Unauthorized(String),
    /// `403` — the credential lacks permission for this route.
    Forbidden(String),
    /// `400` — malformed request or invalid parameter.
    BadRequest(String),
    /// `409` — the request conflicts with current state.
    Conflict(String),
    /// `404` — the requested resource does not exist.
    NotFound(String),
    /// `405` — the path exists but not for this method.
    MethodNotAllowed(String),
    /// `429` — rate limit hit.
    TooManyRequests(String),
    /// `503` — a dependency is temporarily unavailable.
    ServiceUnavailable(String),
    /// `500` — unexpected failure.
    Internal(String),
    /// An explicit status the caller already picked, such as Axum's own
    /// answer to a rejected `Json` extraction (413 over the body limit, 415
    /// for the wrong `Content-Type`). ADR-0005 says the status carries the
    /// meaning, so these must not be flattened to 400.
    Status(StatusCode, String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Unauthorized(m) => (StatusCode::UNAUTHORIZED, m),
            Self::Forbidden(m) => (StatusCode::FORBIDDEN, m),
            Self::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            Self::Conflict(m) => (StatusCode::CONFLICT, m),
            Self::NotFound(m) => (StatusCode::NOT_FOUND, m),
            Self::MethodNotAllowed(m) => (StatusCode::METHOD_NOT_ALLOWED, m),
            Self::TooManyRequests(m) => (StatusCode::TOO_MANY_REQUESTS, m),
            Self::ServiceUnavailable(m) => (StatusCode::SERVICE_UNAVAILABLE, m),
            Self::Internal(m) => {
                // Keep diagnostics server-side; clients only see a stable message.
                eprintln!("internal error: {m}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal server error".into(),
                )
            }
            Self::Status(status, m) => (status, m),
        };
        (status, Json(ErrorBody { error: message })).into_response()
    }
}

impl From<crate::db::vault_imports::ImportLookupError> for ApiError {
    fn from(e: crate::db::vault_imports::ImportLookupError) -> Self {
        match e {
            crate::db::vault_imports::ImportLookupError::NotFound { import_id } => {
                Self::NotFound(format!("import {import_id} not found for this account"))
            }
            crate::db::vault_imports::ImportLookupError::InvalidSession { message } => {
                Self::BadRequest(message)
            }
            crate::db::vault_imports::ImportLookupError::Db(err) => Self::Internal(err.to_string()),
        }
    }
}

impl From<crate::db::vault_imports::StartImportError> for ApiError {
    fn from(e: crate::db::vault_imports::StartImportError) -> Self {
        match e {
            err @ crate::db::vault_imports::StartImportError::AlreadyActive => {
                // One wording for the 409, shared with the CLI paths that
                // surface the same error through anyhow.
                Self::Conflict(err.to_string())
            }
            crate::db::vault_imports::StartImportError::Db(err) => Self::Internal(err.to_string()),
        }
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        Self::Internal(e.to_string())
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        Self::Internal(e.to_string())
    }
}

/// Origins the packaged desktop app runs from. A Tauri window is not a page on
/// the web, so its origin is fixed by the platform rather than chosen by
/// anyone: `tauri://localhost` on Linux and macOS, and `http(s)://tauri.localhost`
/// on Windows.
///
/// These are allowed whatever the config says. A vault built from source starts
/// with `cors_origins` commented out, and the desktop app pointed at it then
/// fails in a way that reads as a network problem — the browser refuses the
/// response before any code can see it, so the app reports the server as
/// unreachable while `curl` to the same port succeeds. That sends people to
/// their firewall for a missing line of TOML.
///
/// Allowing them by default gives away nothing a listed origin does not. The
/// browser sets `Origin` itself and a page on the web cannot claim to be one of
/// these, so this widens what the desktop app can reach, not what a website can.
pub(crate) const PACKAGED_DESKTOP_ORIGINS: &[&str] = &[
    "tauri://localhost",
    "http://tauri.localhost",
    "https://tauri.localhost",
];

/// Build the Cross-Origin Resource Sharing (CORS) layer from
/// `[server].cors_origins`. CORS is the browser rule that decides which other
/// websites may call this API.
///
/// - `["*"]` → fully permissive (local debugging only)
/// - otherwise → exact origin allow list, always including
///   [`PACKAGED_DESKTOP_ORIGINS`]
///
/// An empty list is therefore not "no CORS" but "the desktop app and nothing
/// else", which is what an unconfigured vault wants: the browser UI it serves
/// itself is same-origin and needs no header at all.
fn build_cors_layer(origins: &[String]) -> CorsLayer {
    if origins.iter().any(|o| o.trim() == "*") {
        return CorsLayer::permissive();
    }
    let mut allowed: Vec<HeaderValue> = Vec::new();
    for origin in origins
        .iter()
        .map(String::as_str)
        .chain(PACKAGED_DESKTOP_ORIGINS.iter().copied())
    {
        let trimmed = origin.trim();
        if trimmed.is_empty() {
            continue;
        }
        // A config that lists a packaged origin by hand is the common case, and
        // naming the same origin twice in the allow list helps no one.
        if let Ok(value) = trimmed.parse::<HeaderValue>()
            && !allowed.contains(&value)
        {
            allowed.push(value);
        }
    }
    CorsLayer::new()
        .allow_origin(AllowOrigin::list(allowed))
        .allow_methods(AllowMethods::mirror_request())
        .allow_headers(AllowHeaders::mirror_request())
}

/// The public auth routes with a small body limit, so password hashing cannot be fed huge requests.
fn limited_auth_router() -> (Router<AppState>, utoipa::openapi::OpenApi) {
    let (router, spec) = crate::openapi::auth_public_openapi().split_for_parts();
    (
        // Auth JSON is tiny; keep a tight limit so Argon2 abuse cannot ship 512 MiB bodies.
        router.layer(RequestBodyLimitLayer::new(32 * 1024)),
        spec,
    )
}

/// A `/v1/…` path no route claims. Static files answer everything else.
async fn api_not_found(uri: axum::http::Uri) -> ApiError {
    ApiError::NotFound(format!("no route at {}", uri.path()))
}

/// A route that exists, asked with a method it does not take.
async fn api_method_not_allowed(method: axum::http::Method, uri: axum::http::Uri) -> ApiError {
    ApiError::MethodNotAllowed(format!("{method} is not allowed at {}", uri.path()))
}

/// `tower_http`'s `RequestBodyLimitLayer` answers a plain-text `413` itself,
/// bypassing every extractor, the moment a `Content-Length` header already
/// announces a payload over the limit — `extract::Json`'s own 413 handling
/// only ever sees a body that had to be read to discover it was too long.
/// Rewrite that one plain-text response into the vault's `{error}` envelope
/// so a body over the limit answers the same way however the client
/// declares its size.
async fn json_body_limit_response(mut response: Response) -> Response {
    if response.status() != StatusCode::PAYLOAD_TOO_LARGE {
        return response;
    }
    let already_json = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/json"));
    if already_json {
        return response;
    }
    let bytes = serde_json::to_vec(&ErrorBody {
        error: "the request body is too large".to_string(),
    })
    .expect("ErrorBody always serializes");
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response.headers_mut().insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&bytes.len().to_string()).expect("digits are a valid header value"),
    );
    *response.body_mut() = axum::body::Body::from(bytes);
    response
}

/// Assemble the full router: API routes, auth routes, the optional OpenAPI UI, CORS, and the static web app.
pub(crate) fn http_app(state: AppState) -> Router {
    let openapi_ui = state
        .cfg
        .server
        .as_ref()
        .map(|s| s.openapi_ui)
        .unwrap_or(false);
    let cors_origins = state
        .cfg
        .server
        .as_ref()
        .map(|s| s.cors_origins.clone())
        .unwrap_or_default();
    let (auth_small, mut spec) = limited_auth_router();
    let (doc_router, rest) = crate::openapi::api_openapi().split_for_parts();
    spec.merge(rest);

    let mut api = Router::new()
        .merge(doc_router)
        .merge(auth_small)
        // `/v1/{*rest}` needs at least one character after the slash, so the
        // bare prefix (with or without a trailing slash) needs its own
        // routes to answer the same JSON 404 instead of falling through to
        // the static file server.
        .route("/v1", axum::routing::any(api_not_found))
        .route("/v1/", axum::routing::any(api_not_found))
        .route("/v1/{*rest}", axum::routing::any(api_not_found))
        .method_not_allowed_fallback(api_method_not_allowed)
        .fallback_service(ServeDir::new("static"))
        .layer(RequestBodyLimitLayer::new(state.max_body_bytes))
        // Rewrite the limit layer's plain-text 413 into `{error}` before CORS
        // sees it, so the response a browser gets is both JSON and CORS-clean.
        .layer(axum::middleware::map_response(json_body_limit_response))
        // Outermost: every response, including one the limit layer answered
        // itself, carries the CORS headers a browser needs to show it.
        .layer(build_cors_layer(&cors_origins));

    if openapi_ui {
        api = api.merge(utoipa_swagger_ui::SwaggerUi::new("/docs").url("/openapi.json", spec));
    }

    api.with_state(state)
}

/// Start the HTTP server.
///
/// # Errors
///
/// Returns an error when the database cannot be opened, the operation lock
/// cannot be taken, or the listener cannot bind.
pub async fn run(cfg: Config) -> anyhow::Result<()> {
    let server = cfg.require_server()?.clone();
    let bind = server.bind.clone();
    // Production entry points must install the Any drivers once before any pool
    // connect (idempotent; `engine::test_pool` does the same for tests).
    sqlx::any::install_default_drivers();
    let db_url = cfg.database.url.clone();
    let engine = match &db_url {
        Some(url) => engine::detect_engine(url)?,
        None => DbEngine::Sqlite,
    };
    let lock_path = if engine == DbEngine::Sqlite {
        cfg.paths.db.clone()
    } else {
        cfg.paths.data_dir.join(".operation.lock")
    };
    let _operation_lock = crate::operation_lock::acquire_for_serve(&lock_path)?;
    let upload_limits =
        asset_uploads::UploadLimits::resolve(server.asset_part_size, server.asset_max_bytes);
    let max_body_bytes = upload_limits.max_bytes as usize;

    // Open the pool, warm it, and ensure schema once before serving.
    let pool = engine::DbTarget::new(db_url.as_deref(), &cfg.paths.db)
        .open()
        .await?;
    {
        let mut conn = pool.acquire().await?;
        let _: i32 = sqlx::query_scalar("SELECT 1").fetch_one(&mut *conn).await?; // warmup (i32: INT4 on Postgres, INTEGER on SQLite)
        schema::ensure_vault_schema(&mut conn).await?;
    }
    if engine == DbEngine::Sqlite {
        crate::operation_lock::mark_ready(&cfg.paths.db)?;
        let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|_| "unknown".into());
        eprintln!("  db:   {} (journal_mode={mode})", cfg.paths.db.display());
    }
    eprintln!(
        "  assets: max={} MiB  part_size={} MiB",
        upload_limits.max_bytes / message_ir::MIB,
        upload_limits.part_size as u64 / message_ir::MIB
    );

    let state = AppState {
        cfg: Arc::new(cfg),
        db: pool,
        db_engine: engine,
        account_import_locks: Arc::new(Mutex::new(HashMap::new())),
        asset_complete_locks: Arc::new(Mutex::new(HashMap::new())),
        auth_rate_limits: Arc::new(std::sync::Mutex::new(HashMap::new())),
        upload_limits,
        max_body_bytes,
    };

    let app = http_app(state);
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    eprintln!("message-vault-server serve listening on http://{bind}");
    eprintln!(
        "  routes: `message-vault-server dump-openapi` lists them all; set [server] openapi_ui = true for /docs"
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

/// Resolve on Ctrl-C so axum drains in-flight requests before exiting.
async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    eprintln!("shutting down");
}

/// Report process liveness.
#[utoipa::path(
    get,
    path = "/health",
    tag = "Health",
    responses((status = 200, description = "Process is up", body = String))
)]
pub(crate) async fn health() -> (StatusCode, &'static str) {
    (StatusCode::OK, "ok\n")
}

/// Resolve a username or UUID to an account id, reporting an unknown account as a bad request.
async fn resolve_account_ref_async(
    pool: &sqlx::AnyPool,
    account_ref: &str,
) -> Result<String, ApiError> {
    let mut conn = pool.acquire().await?;
    account_profile::resolve_account_ref(&mut conn, account_ref)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))
}

/// Read the Bearer token from `Authorization`.
///
/// # Errors
///
/// Returns unauthorized when the header is missing or not a Bearer value.
pub fn bearer_token(headers: &HeaderMap) -> Result<String, ApiError> {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return Err(ApiError::Unauthorized(
            "missing Authorization: Bearer <token>".into(),
        ));
    };
    let value = value
        .to_str()
        .map_err(|_| ApiError::Unauthorized("invalid Authorization header".into()))?;
    let Some(token) = value.strip_prefix("Bearer ") else {
        return Err(ApiError::Unauthorized(
            "Authorization must be Bearer <token>".into(),
        ));
    };
    let token = token.trim();
    if token.is_empty() {
        return Err(ApiError::Unauthorized("empty API token".into()));
    }
    Ok(token.to_string())
}

/// Resolve a session token or named API token to an account.
///
/// # Errors
///
/// Returns unauthorized when the token is missing or invalid.
pub async fn resolve_auth(headers: &HeaderMap, state: &AppState) -> Result<AuthIdentity, ApiError> {
    let token = bearer_token(headers)?;
    // Always look up against SQLite so rotate/delete in Settings takes effect
    // without restarting serve (no process-local token cache).
    let mut conn = state.db.acquire().await?;
    resolve_auth_on_conn(&mut conn, &token).await
}

/// Credential-specific bit not yet folded into `AuthCapability`: a session
/// carries no extra state, an API token carries its own (pre-intersection)
/// permissions. Both kinds load `AccountAuth` the same way so the disabled
/// check in [`resolve_auth_on_conn`] runs exactly once.
enum Credential {
    Session,
    ApiToken(Permissions),
}

/// Resolve a Bearer credential on an existing connection.
///
/// # Errors
///
/// Unauthorized when the token matches nothing; forbidden when the account is
/// disabled.
pub async fn resolve_auth_on_conn(
    conn: &mut AnyConnection,
    token: &str,
) -> Result<AuthIdentity, ApiError> {
    schema::ensure_accounts_schema(conn).await?;

    let resolved = if let Some(account_id) =
        session_tokens::lookup_account_for_token(&mut *conn, token).await?
    {
        Some((account_id, Credential::Session))
    } else {
        api_tokens::lookup_account_for_api_token(&mut *conn, token)
            .await?
            .map(|tok| (tok.account_id, Credential::ApiToken(tok.permissions)))
    };

    let Some((account_id, credential)) = resolved else {
        return Err(ApiError::Unauthorized("invalid API token".into()));
    };

    let auth = account_profile::load_account_auth(&mut *conn, &account_id)
        .await?
        .ok_or_else(|| ApiError::Unauthorized("account no longer exists".into()))?;
    if auth.disabled {
        return Err(ApiError::Forbidden("this account is disabled".into()));
    }

    let capability = match credential {
        Credential::Session => AuthCapability::Session {
            is_admin: auth.is_admin,
            permissions: auth.permissions,
        },
        Credential::ApiToken(tok_permissions) => {
            AuthCapability::ApiToken(auth.permissions.intersect(tok_permissions))
        }
    };

    Ok(AuthIdentity {
        account_id,
        capability,
    })
}

/// Resolve the account id for an import or export: Bearer token binds the account.
/// Optional query may be username or UUID and must match the token.
pub(crate) async fn resolve_import_account(
    auth: &AuthIdentity,
    query_account: Option<&str>,
    pool: &sqlx::AnyPool,
) -> Result<String, ApiError> {
    let query = query_account.and_then(message_ir::trimmed);
    if let Some(q) = query {
        let resolved = resolve_account_ref_async(pool, q).await?;
        if resolved != auth.account_id {
            return Err(ApiError::Forbidden(
                "account query does not match token's account".into(),
            ));
        }
    }
    Ok(auth.account_id.clone())
}

/// The media type from `Content-Type` without its parameters.
pub(crate) fn content_type_base(headers: &HeaderMap) -> Option<&str> {
    let ct = headers.get(header::CONTENT_TYPE)?.to_str().ok()?;
    Some(ct.split(';').next().unwrap_or(ct).trim())
}

/// The upload's declared media type, or `None` when it is missing or the generic octet-stream.
pub(crate) fn upload_content_type(headers: &HeaderMap) -> Option<String> {
    let base = content_type_base(headers)?;
    if base.is_empty() || base.eq_ignore_ascii_case("application/octet-stream") {
        None
    } else {
        Some(base.to_string())
    }
}

/// True when the request body is JSON Lines (one JSON object per line).
pub(crate) fn is_jsonl_content_type(base: &str) -> bool {
    base.eq_ignore_ascii_case("application/jsonl")
        || base.eq_ignore_ascii_case("application/x-ndjson")
}

/// Read the whole request body into memory, failing once it passes `max_bytes`.
pub(crate) async fn read_body_limited(
    body: axum::body::Body,
    max_bytes: usize,
) -> Result<Vec<u8>, ApiError> {
    let mut out = Vec::new();
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| ApiError::BadRequest(format!("failed to read body: {e}")))?;
        if out.len().saturating_add(chunk.len()) > max_bytes {
            return Err(ApiError::Status(
                StatusCode::PAYLOAD_TOO_LARGE,
                "request body too large".into(),
            ));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

/// Drain request body without retaining it (used when asset already exists).
pub(crate) async fn discard_body(
    body: axum::body::Body,
    max_body_bytes: usize,
) -> Result<(), ApiError> {
    let mut stream = body.into_data_stream();
    let mut seen = 0usize;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| ApiError::BadRequest(format!("failed to read body: {e}")))?;
        seen = seen.saturating_add(chunk.len());
        if seen > max_body_bytes {
            return Err(ApiError::Status(
                StatusCode::PAYLOAD_TOO_LARGE,
                "request body too large".into(),
            ));
        }
    }
    Ok(())
}

/// Create `dest` and its parent folders for an upload.
async fn create_dest_file(dest: &Path) -> Result<tokio::fs::File, ApiError> {
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| ApiError::Internal(format!("mkdir {}: {e}", parent.display())))?;
    }
    tokio::fs::File::create(dest)
        .await
        .map_err(|e| ApiError::Internal(format!("create {}: {e}", dest.display())))
}

/// Stream a request body to `dest`, failing once it passes `max_body_bytes`. Returns the bytes written.
pub(crate) async fn stream_body_to_file(
    body: axum::body::Body,
    dest: &Path,
    max_body_bytes: usize,
) -> Result<u64, ApiError> {
    let mut file = create_dest_file(dest).await?;
    let mut written = 0u64;
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| ApiError::BadRequest(format!("failed to read body: {e}")))?;
        written = written.saturating_add(chunk.len() as u64);
        if written > max_body_bytes as u64 {
            return Err(ApiError::Status(
                StatusCode::PAYLOAD_TOO_LARGE,
                "request body too large".into(),
            ));
        }
        file.write_all(&chunk)
            .await
            .map_err(|e| ApiError::Internal(format!("write {}: {e}", dest.display())))?;
    }
    file.flush()
        .await
        .map_err(|e| ApiError::Internal(format!("flush {}: {e}", dest.display())))?;
    Ok(written)
}

/// Build the `AppState` every test in this crate drives: a real `Config`
/// rooted at `data_dir` (with a sibling `vault.db` path that nothing in the
/// test suite reads from disk — queries go through `pool`), the given pool,
/// and default upload limits. `#[cfg(test)]`-gated so it never ships in a
/// release build; `pub(crate)` so `test_support` and the other test modules
/// in this crate can reach it.
#[cfg(test)]
pub(crate) async fn test_app_state(pool: sqlx::AnyPool, data_dir: &Path) -> AppState {
    AppState {
        cfg: Arc::new(crate::config::Config {
            paths: crate::config::PathsConfig {
                db: data_dir.join("vault.db"),
                data_dir: data_dir.to_path_buf(),
                assets_dir: "assets".into(),
                assets_converted_dir: "assets_converted".into(),
            },
            server: Some(crate::config::ServerConfig {
                bind: "127.0.0.1:0".into(),
                asset_max_bytes: 8 * 1024 * 1024,
                asset_part_size: 1024 * 1024,
                cors_origins: Vec::new(),
                openapi_ui: false,
            }),
            database: crate::config::DatabaseConfig::default(),
        }),
        db: pool,
        db_engine: DbEngine::Sqlite,
        account_import_locks: Arc::new(Mutex::new(HashMap::new())),
        asset_complete_locks: Arc::new(Mutex::new(HashMap::new())),
        auth_rate_limits: Arc::new(std::sync::Mutex::new(HashMap::new())),
        upload_limits: asset_uploads::UploadLimits::default(),
        max_body_bytes: asset_uploads::DEFAULT_MAX_BYTES as usize,
    }
}

#[cfg(test)]
mod tests;
