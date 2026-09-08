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
use tower_http::cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::services::ServeDir;
use tower_http::trace::{DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::asset_uploads;
use crate::config::Config;
use crate::db::account_profile;
use crate::db::api_tokens;
use crate::db::engine::DbEngine;
use crate::db::permissions::Permissions;
use crate::db::schema;
use crate::db::session_tokens;
use crate::keyed_locks::KeyedLocks;
use crate::open_vault::OpenVault;
use crate::problem::{Problem, ProblemType};

/// What a Bearer credential is allowed to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthCapability {
    /// Signed-in session on an ordinary account. Carries the account's own
    /// permissions.
    Session {
        /// What the account may do.
        permissions: Permissions,
    },
    /// Signed-in vault owner. Carries no permissions at all, so every guard
    /// that asks for one refuses it and the owner cannot reach message data.
    /// See `docs/adr/0008-the-vault-owner-holds-no-messages.md`.
    Owner,
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
    /// The vault owner has nothing: holding no messages is what the owner is.
    pub fn permissions(&self) -> Permissions {
        match self.capability {
            AuthCapability::Session { permissions } | AuthCapability::ApiToken(permissions) => {
                permissions
            }
            AuthCapability::Owner => Permissions::none(),
        }
    }

    /// True only for the signed-in vault owner. An API token can never be the
    /// owner, because no token resolves to [`AuthCapability::Owner`].
    pub fn is_owner(&self) -> bool {
        matches!(self.capability, AuthCapability::Owner)
    }

    /// True when the credential is a signed-in session on an ordinary account:
    /// not a token, and not the vault owner.
    pub fn is_session(&self) -> bool {
        matches!(self.capability, AuthCapability::Session { .. })
    }

    /// True when a person signed in, whether as the vault owner or on an
    /// ordinary account. False for every API token.
    pub fn is_signed_in(&self) -> bool {
        self.is_session() || self.is_owner()
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
    Err(ApiError::InsufficientScope(
        "this endpoint requires a signed-in session; use an API token only for import/export"
            .into(),
    ))
}

/// Reject anything that is not the signed-in vault owner.
///
/// # Errors
///
/// Returns forbidden for ordinary sessions and for every API token.
pub fn require_owner(auth: &AuthIdentity) -> Result<(), ApiError> {
    if auth.is_owner() {
        return Ok(());
    }
    Err(ApiError::NotTheOwner(
        "this endpoint requires the vault owner's session".into(),
    ))
}

/// Allow any signed-in person, vault owner or ordinary account, and reject
/// API tokens. The guard for the routes a principal points at its own record —
/// changing its password, reading and editing its profile — which the vault
/// owner needs as much as anyone.
///
/// # Errors
///
/// Returns forbidden when the credential is a named API token.
pub fn require_signed_in(auth: &AuthIdentity) -> Result<(), ApiError> {
    if auth.is_signed_in() {
        return Ok(());
    }
    Err(ApiError::InsufficientScope(
        "this endpoint requires a signed-in session; use an API token only for import/export"
            .into(),
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
    Err(ApiError::InsufficientScope(
        "import is not permitted".into(),
    ))
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
    Err(ApiError::InsufficientScope(
        "export is not permitted".into(),
    ))
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
    Err(ApiError::InsufficientScope(
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
    Err(ApiError::InsufficientScope(
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
    /// Signed-in vault owner; wraps [`require_owner`].
    Owner,
    require_owner
);
auth_guard!(
    /// Any signed-in person, owner or ordinary account; wraps
    /// [`require_signed_in`].
    SignedIn,
    require_signed_in
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
#[derive(Debug, Clone)]
pub struct AppState {
    /// Loaded configuration.
    pub cfg: Arc<Config>,
    /// Connection pool (SQLite file or `[database] url`). Handlers acquire
    /// short-lived connections from here.
    pub db: sqlx::AnyPool,
    /// Per-account import mutex: same-account imports stay serialized so staging
    /// rows (the temporary import area) for that tenant are not wiped mid-run.
    /// Different accounts may overlap at the lock layer; SQLite write-ahead
    /// logging plus `busy_timeout` serialize writers.
    pub(crate) account_import_locks: KeyedLocks,
    /// Serialize multipart complete per (account, sha256) so two clients cannot
    /// race `store_verified` on the same SHA-256 fingerprint.
    pub(crate) asset_complete_locks: KeyedLocks,
    /// Sliding-window hit counts for the unauthenticated auth endpoints. Held
    /// here, not in a static, so tests in one binary cannot rate-limit each
    /// other; a served vault has a single state, so the limit still spans it.
    pub(crate) auth_rate_limits: crate::auth::AuthRateLimits,
    /// Multipart / asset size limits from `[server]` (env may override part size).
    pub(crate) upload_limits: asset_uploads::UploadLimits,
    /// Axum request body cap (single PUT or one part); equals `asset_max_bytes`.
    pub(crate) max_body_bytes: usize,
}

impl AppState {
    /// The state every handler shares, over an opened vault. `serve` and the
    /// test harness both come through here, so the locks, the rate limits
    /// and the body cap are assembled in one place.
    pub fn new(vault: OpenVault, upload_limits: asset_uploads::UploadLimits) -> Self {
        Self {
            cfg: Arc::new(vault.cfg),
            db: vault.db,
            account_import_locks: KeyedLocks::default(),
            asset_complete_locks: KeyedLocks::default(),
            auth_rate_limits: Arc::new(std::sync::Mutex::new(HashMap::new())),
            upload_limits,
            max_body_bytes: upload_limits.max_bytes as usize,
        }
    }
}

/// The answer to a request that made one new resource: `201 Created`, a
/// `Location` header naming it, and the JSON body the route documents.
///
/// A create that takes a batch is the exception `docs/agents/http-api-rules.md` records: it makes no
/// single resource, names no URL, and answers `200 OK` with a summary.
#[derive(Debug)]
pub struct Created<T> {
    /// The new resource's path, `/v1/...`.
    pub location: String,
    pub body: T,
}

impl<T: Serialize> IntoResponse for Created<T> {
    fn into_response(self) -> Response {
        (
            StatusCode::CREATED,
            [(header::LOCATION, self.location)],
            Json(self.body),
        )
            .into_response()
    }
}

/// A failure, as one of the registered problem types (`docs/agents/http-api-rules.md`).
///
/// A variant names what went wrong, never a status: the status, the `type`
/// URL and the title come from the type's declaration in [`crate::problem`],
/// so no call site picks a status by hand. The `String` a variant carries is
/// the `detail`, one sentence written for the person reading it; a
/// `500 Internal Server Error` carries the whole error chain for the log and
/// shows the client a fixed sentence.
#[derive(Debug)]
pub enum ApiError {
    /// `422` — fields that parsed and then broke a rule, every one of them.
    ValidationFailed(Vec<String>),
    /// `400` — a required query parameter or body field is absent.
    MissingParameter(String),
    /// `400` — the request cannot be read at all.
    MalformedBody(String),
    /// `415` — `Content-Type` absent or not one the route accepts.
    UnsupportedMediaType(String),
    /// `413` — the body is over the configured cap.
    PayloadTooLarge(String),
    /// `401` — a username, password or current-password check failed.
    InvalidCredentials(String),
    /// `401` — no usable bearer token.
    AuthenticationRequired(String),
    /// `429` — the auth rate limiter refused the attempt; `Retry-After` in seconds.
    RateLimited {
        /// Seconds until an attempt may succeed.
        retry_after_secs: u64,
    },
    /// `409` — the username already belongs to an account.
    UsernameTaken(String),
    /// `409` — a Contact Group, Message Tag or Saved Search name collides.
    NameTaken(String),
    /// `403` — the demo account refuses a destructive operation.
    DemoAccountProtected(String),
    /// `403` — the route belongs to the vault owner.
    NotTheOwner(String),
    /// `403` — the credential is valid but lacks the scope the route needs.
    InsufficientScope(String),
    /// `403` — the account may not sign in or act.
    AccountDisabled(String),
    /// `400` — the search language refused a word.
    SearchQueryInvalid {
        /// The sentence, naming the word and the list.
        detail: String,
        /// The `word:` the query used, when there is one.
        word: Option<&'static str>,
        /// A word the language does have, when one is close.
        did_you_mean: Option<&'static str>,
    },
    /// `409` — the resource is not in a state that allows the operation.
    StateConflict(String),
    /// `400` — a part, upload id or completion does not match the upload.
    AssetUploadInvalid(String),
    /// `404` — the addressed resource does not exist for this account.
    NotFound(String),
    /// `405` — the path exists but not for this method.
    MethodNotAllowed(String),
    /// `406` — `Accept` names nothing the route can produce.
    NotAcceptable(String),
    /// `500` — unexpected failure. The whole context chain goes to the log;
    /// the client sees a fixed sentence and `about:blank`.
    Internal(anyhow::Error),
}

impl ApiError {
    /// A validation failure with one sentence.
    pub fn validation(sentence: impl Into<String>) -> Self {
        Self::ValidationFailed(vec![sentence.into()])
    }

    /// The registered type, or `None` for an internal error.
    #[must_use]
    pub fn problem_type(&self) -> Option<ProblemType> {
        Some(match self {
            Self::ValidationFailed(_) => ProblemType::ValidationFailed,
            Self::MissingParameter(_) => ProblemType::MissingParameter,
            Self::MalformedBody(_) => ProblemType::MalformedBody,
            Self::UnsupportedMediaType(_) => ProblemType::UnsupportedMediaType,
            Self::PayloadTooLarge(_) => ProblemType::PayloadTooLarge,
            Self::InvalidCredentials(_) => ProblemType::InvalidCredentials,
            Self::AuthenticationRequired(_) => ProblemType::AuthenticationRequired,
            Self::RateLimited { .. } => ProblemType::RateLimited,
            Self::UsernameTaken(_) => ProblemType::UsernameTaken,
            Self::NameTaken(_) => ProblemType::NameTaken,
            Self::DemoAccountProtected(_) => ProblemType::DemoAccountProtected,
            Self::NotTheOwner(_) => ProblemType::NotTheOwner,
            Self::InsufficientScope(_) => ProblemType::InsufficientScope,
            Self::AccountDisabled(_) => ProblemType::AccountDisabled,
            Self::SearchQueryInvalid { .. } => ProblemType::SearchQueryInvalid,
            Self::StateConflict(_) => ProblemType::StateConflict,
            Self::AssetUploadInvalid(_) => ProblemType::AssetUploadInvalid,
            Self::NotFound(_) => ProblemType::NotFound,
            Self::MethodNotAllowed(_) => ProblemType::MethodNotAllowed,
            Self::NotAcceptable(_) => ProblemType::NotAcceptable,
            Self::Internal(_) => return None,
        })
    }

    /// The status this failure answers.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        self.problem_type()
            .map_or(StatusCode::INTERNAL_SERVER_ERROR, ProblemType::status)
    }

    /// The problem document this failure answers with. An internal error is
    /// logged here, once, with its whole chain; the document says nothing of it.
    #[must_use]
    pub fn to_problem(&self) -> Problem {
        let request_id = crate::request_id::current();
        let Some(kind) = self.problem_type() else {
            let Self::Internal(err) = self else {
                unreachable!("every variant but Internal has a problem type");
            };
            tracing::error!(error = %error_chain(err), "internal server error");
            return Problem {
                kind: crate::problem::INTERNAL_TYPE.to_string(),
                title: "Internal server error".to_string(),
                status: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                detail: Some("internal server error".to_string()),
                errors: None,
                request_id,
                word: None,
                did_you_mean: None,
                retry_after: None,
            };
        };
        let mut problem = Problem {
            kind: kind.url(),
            title: kind.title().to_string(),
            status: kind.status().as_u16(),
            detail: None,
            errors: None,
            request_id,
            word: None,
            did_you_mean: None,
            retry_after: None,
        };
        match self {
            Self::ValidationFailed(errors) => problem.errors = Some(errors.clone()),
            Self::RateLimited { retry_after_secs } => {
                problem.retry_after = Some(*retry_after_secs);
                problem.detail = Some(format!(
                    "too many authentication attempts; try again in {retry_after_secs} seconds"
                ));
            }
            Self::SearchQueryInvalid {
                detail,
                word,
                did_you_mean,
            } => {
                problem.detail = Some(detail.clone());
                problem.word = word.map(str::to_string);
                problem.did_you_mean = did_you_mean.map(str::to_string);
            }
            Self::MissingParameter(m)
            | Self::MalformedBody(m)
            | Self::UnsupportedMediaType(m)
            | Self::PayloadTooLarge(m)
            | Self::InvalidCredentials(m)
            | Self::AuthenticationRequired(m)
            | Self::UsernameTaken(m)
            | Self::NameTaken(m)
            | Self::DemoAccountProtected(m)
            | Self::NotTheOwner(m)
            | Self::InsufficientScope(m)
            | Self::AccountDisabled(m)
            | Self::StateConflict(m)
            | Self::AssetUploadInvalid(m)
            | Self::NotFound(m)
            | Self::MethodNotAllowed(m)
            | Self::NotAcceptable(m) => problem.detail = Some(m.clone()),
            Self::Internal(_) => unreachable!("handled above"),
        }
        problem
    }
}

/// The message a person reads when the failure does not travel over HTTP —
/// the CLI, a log line, a test assertion. The client-facing body is built by
/// [`IntoResponse`], which hides an internal error's detail; here the whole
/// context chain is shown, because the reader is the operator.
impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Internal(e) => f.write_str(&error_chain(e)),
            Self::ValidationFailed(errors) => f.write_str(&errors.join("; ")),
            Self::RateLimited { retry_after_secs } => write!(
                f,
                "too many authentication attempts; try again in {retry_after_secs} seconds"
            ),
            Self::SearchQueryInvalid { detail, .. } => f.write_str(detail),
            Self::MissingParameter(m)
            | Self::MalformedBody(m)
            | Self::UnsupportedMediaType(m)
            | Self::PayloadTooLarge(m)
            | Self::InvalidCredentials(m)
            | Self::AuthenticationRequired(m)
            | Self::UsernameTaken(m)
            | Self::NameTaken(m)
            | Self::DemoAccountProtected(m)
            | Self::NotTheOwner(m)
            | Self::InsufficientScope(m)
            | Self::AccountDisabled(m)
            | Self::StateConflict(m)
            | Self::AssetUploadInvalid(m)
            | Self::NotFound(m)
            | Self::MethodNotAllowed(m)
            | Self::NotAcceptable(m) => f.write_str(m),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let problem = self.to_problem();
        let status = StatusCode::from_u16(problem.status).expect("a registered status");
        let mut response = (
            status,
            [(header::CONTENT_TYPE, Problem::CONTENT_TYPE)],
            Json(&problem),
        )
            .into_response();
        if let Some(secs) = problem.retry_after {
            response.headers_mut().insert(
                header::RETRY_AFTER,
                HeaderValue::from_str(&secs.to_string()).expect("digits are a valid header value"),
            );
        }
        response
    }
}

/// An error's whole context chain on one line, outermost first, so the log
/// shows the sqlx or io failure under the step that hit it. `Display` alone
/// would print only the outermost message.
pub(crate) fn error_chain(err: &anyhow::Error) -> String {
    format!("{err:#}")
}

impl From<crate::db::vault_imports::ImportLookupError> for ApiError {
    fn from(e: crate::db::vault_imports::ImportLookupError) -> Self {
        match e {
            crate::db::vault_imports::ImportLookupError::NotFound { import_id } => {
                Self::NotFound(format!("import {import_id} not found for this account"))
            }
            crate::db::vault_imports::ImportLookupError::InvalidSession { message } => {
                Self::StateConflict(message)
            }
            crate::db::vault_imports::ImportLookupError::Db(err) => Self::Internal(err),
        }
    }
}

impl From<crate::db::vault_imports::StartImportError> for ApiError {
    fn from(e: crate::db::vault_imports::StartImportError) -> Self {
        match e {
            err @ crate::db::vault_imports::StartImportError::AlreadyActive => {
                // One wording for the 409, shared with the CLI paths that
                // surface the same error through anyhow.
                Self::StateConflict(err.to_string())
            }
            crate::db::vault_imports::StartImportError::Db(err) => Self::Internal(err),
        }
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        Self::Internal(e.into())
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        Self::Internal(e)
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
/// Rewrite that one plain-text response into the `payload-too-large` problem
/// so a body over the limit answers the same way however the client declares
/// its size.
async fn json_body_limit_response(response: Response) -> Response {
    if response.status() != StatusCode::PAYLOAD_TOO_LARGE {
        return response;
    }
    let already_problem = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with(Problem::CONTENT_TYPE));
    if already_problem {
        return response;
    }
    ApiError::PayloadTooLarge("the request body is too large".to_string()).into_response()
}

/// Refuse a request whose `Accept` names nothing this route can produce
/// (`docs/agents/http-api-rules.md`). Narrow on purpose: only when the header is present and none of
/// its members is `application/json`, `application/problem+json`,
/// `application/*` or `*/*`. A missing `Accept` is a request for JSON, which
/// is what every one of the vault's own clients sends.
///
/// Applied to the `/v1` routes only, through `route_layer`, so the static app,
/// `/health` and the OpenAPI UI keep producing what they produce. The asset
/// download is the one `/v1` route that streams something other than JSON,
/// and is let through here by path.
async fn require_json_acceptable(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    if is_asset_download(&request) {
        return next.run(request).await;
    }
    if let Some(accept) = request
        .headers()
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        && !accepts_json(accept)
    {
        return ApiError::NotAcceptable(format!(
            "this route answers application/json, and Accept was {accept}"
        ))
        .into_response();
    }
    next.run(request).await
}

/// `GET /v1/assets/{sha256}`: the asset's own bytes, in its own media type.
fn is_asset_download(request: &axum::extract::Request) -> bool {
    request.method() == axum::http::Method::GET
        && request
            .uri()
            .path()
            .strip_prefix("/v1/assets/")
            .is_some_and(|rest| !rest.is_empty() && !rest.contains('/'))
}

/// Whether an `Accept` header admits a JSON answer.
fn accepts_json(accept: &str) -> bool {
    accept
        .split(',')
        .filter_map(|member| member.split(';').next())
        .map(str::trim)
        .any(|media| {
            media == "*/*"
                || media.eq_ignore_ascii_case("application/*")
                || media.eq_ignore_ascii_case("application/json")
                || media.eq_ignore_ascii_case(Problem::CONTENT_TYPE)
        })
}

/// Assemble the full router: API routes, auth routes, the optional OpenAPI UI, CORS, and the static web app.
pub(crate) fn http_app(state: AppState) -> Router {
    let openapi_ui = state.cfg.server.as_ref().is_some_and(|s| s.openapi_ui);
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
        // `route_layer`, not `layer`: the `Accept` check belongs to the API
        // routes above and never to the static app served by the fallback.
        .route_layer(axum::middleware::from_fn(require_json_acceptable))
        .method_not_allowed_fallback(api_method_not_allowed)
        .fallback_service(ServeDir::new("static"))
        .layer(RequestBodyLimitLayer::new(state.max_body_bytes))
        // Rewrite the limit layer's plain-text 413 into `{error}` before CORS
        // sees it, so the response a browser gets is both JSON and CORS-clean.
        .layer(axum::middleware::map_response(json_body_limit_response))
        // Outermost: every response, including one the limit layer answered
        // itself, carries the CORS headers a browser needs to show it.
        .layer(build_cors_layer(&cors_origins))
        // One `info` line per response (method, path, status, latency), and an
        // `error` line for a 5xx. Runs outside CORS so the status it logs is
        // the one the client receives. The span carries method and path; its
        // level must match the line's or the default `info` filter drops it.
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &axum::extract::Request| {
                    tracing::info_span!(
                        "request",
                        method = %request.method(),
                        uri = %request.uri(),
                        request_id = request
                            .headers()
                            .get(crate::request_id::HEADER)
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("")
                    )
                })
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        // Outermost of all: the request id is made before the trace span
        // reads it and stays in scope while every problem body is built.
        .layer(axum::middleware::from_fn(crate::request_id::layer));

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
    let engine = cfg.db_engine()?;
    let lock_path = if engine == DbEngine::Sqlite {
        cfg.paths.db.clone()
    } else {
        cfg.paths.data_dir.join(".operation.lock")
    };
    let _operation_lock = crate::operation_lock::acquire_for_serve(&lock_path)?;
    let upload_limits =
        asset_uploads::UploadLimits::resolve(server.asset_part_size, server.asset_max_bytes);

    let vault = OpenVault::open(cfg).await?;
    if engine == DbEngine::Sqlite {
        crate::operation_lock::mark_ready(&vault.cfg.paths.db)?;
        let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&vault.db)
            .await
            .unwrap_or_else(|_| "unknown".into());
        eprintln!(
            "  db:   {} (journal_mode={mode})",
            vault.cfg.paths.db.display()
        );
    }
    eprintln!(
        "  assets: max={} MiB  part_size={} MiB",
        upload_limits.max_bytes / message_ir::MIB,
        upload_limits.part_size as u64 / message_ir::MIB
    );

    let app = http_app(AppState::new(vault, upload_limits));
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
        .map_err(|e| ApiError::validation(e.to_string()))
}

/// Read the Bearer token from `Authorization`.
///
/// # Errors
///
/// Returns unauthorized when the header is missing or not a Bearer value.
pub fn bearer_token(headers: &HeaderMap) -> Result<String, ApiError> {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return Err(ApiError::AuthenticationRequired(
            "missing Authorization: Bearer <token>".into(),
        ));
    };
    let value = value
        .to_str()
        .map_err(|_| ApiError::AuthenticationRequired("invalid Authorization header".into()))?;
    let Some(token) = value.strip_prefix("Bearer ") else {
        return Err(ApiError::AuthenticationRequired(
            "Authorization must be Bearer <token>".into(),
        ));
    };
    let token = token.trim();
    if token.is_empty() {
        return Err(ApiError::AuthenticationRequired("empty API token".into()));
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
        return Err(ApiError::AuthenticationRequired("invalid API token".into()));
    };

    let auth = account_profile::load_account_auth(&mut *conn, &account_id)
        .await?
        .ok_or_else(|| ApiError::AuthenticationRequired("account no longer exists".into()))?;
    if auth.disabled {
        return Err(ApiError::AccountDisabled("this account is disabled".into()));
    }

    // A session on the owner's account resolves to `Owner`, which carries no
    // permissions. An API token never does, whichever account issued it, so
    // no token can reach `/v1/owner/*`.
    let capability = match credential {
        Credential::Session if account_profile::is_vault_owner(&account_id) => {
            AuthCapability::Owner
        }
        Credential::Session => AuthCapability::Session {
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
            return Err(ApiError::InsufficientScope(
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
        let chunk =
            chunk.map_err(|e| ApiError::MalformedBody(format!("failed to read body: {e}")))?;
        if out.len().saturating_add(chunk.len()) > max_bytes {
            return Err(ApiError::PayloadTooLarge("request body too large".into()));
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
        let chunk =
            chunk.map_err(|e| ApiError::MalformedBody(format!("failed to read body: {e}")))?;
        seen = seen.saturating_add(chunk.len());
        if seen > max_body_bytes {
            return Err(ApiError::PayloadTooLarge("request body too large".into()));
        }
    }
    Ok(())
}

/// Create `dest` and its parent folders for an upload.
async fn create_dest_file(dest: &Path) -> Result<tokio::fs::File, ApiError> {
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("mkdir {}: {e}", parent.display())))?;
    }
    tokio::fs::File::create(dest)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("create {}: {e}", dest.display())))
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
        let chunk =
            chunk.map_err(|e| ApiError::MalformedBody(format!("failed to read body: {e}")))?;
        written = written.saturating_add(chunk.len() as u64);
        if written > max_body_bytes as u64 {
            return Err(ApiError::PayloadTooLarge("request body too large".into()));
        }
        file.write_all(&chunk)
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("write {}: {e}", dest.display())))?;
    }
    file.flush()
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("flush {}: {e}", dest.display())))?;
    Ok(written)
}

/// Build the `AppState` every test in this crate drives: a real `Config`
/// rooted at `data_dir` (with a sibling `vault.db` path that nothing in the
/// test suite reads from disk — queries go through `pool`), the given pool,
/// and default upload limits. Goes through [`AppState::new`], the same
/// assembly `serve` uses. `#[cfg(test)]`-gated so it never ships in a release
/// build; `pub(crate)` so `test_support` and the other test modules in this
/// crate can reach it.
#[cfg(test)]
pub(crate) fn test_app_state(pool: sqlx::AnyPool, data_dir: &Path) -> AppState {
    let cfg = crate::config::Config {
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
    };
    AppState::new(
        OpenVault { cfg, db: pool },
        asset_uploads::UploadLimits::default(),
    )
}

#[cfg(test)]
mod tests;
