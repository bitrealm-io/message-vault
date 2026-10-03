//! Router assembly, shared state, auth resolution, and HTTP plumbing.
//!
//! Domain handlers live in their own modules: `session_api` (logging in and
//! out), `accounts_api` (the accounts collection and their API tokens),
//! `contacts_api`, `conversations_api`, `exports_api` (Export Runs),
//! `imports_api` (JSONL ingest and Import Runs), and `assets_api` (asset bytes and
//! multipart uploads). This module
//! keeps the pieces they share: [`AppState`], [`ApiError`], Bearer token
//! resolution, body-streaming helpers, and `http_app`, which assembles the
//! router.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use futures_util::StreamExt;
use serde::Serialize;
use sqlx::SqliteConnection;
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
use crate::db::permissions::Permissions;
use crate::db::schema;
use crate::db::session_tokens;
use crate::keyed_locks::KeyedLocks;
use crate::open_db::OpenDb;
use crate::problem::{Problem, ProblemType};

/// What a Bearer credential is allowed to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthCapability {
    /// Logged-in session on an ordinary account. Carries the account's own
    /// permissions.
    Session {
        /// What the account may do.
        permissions: Permissions,
    },
    /// Logged-in owner. Carries no permissions at all, so every guard
    /// that asks for one refuses it and the owner cannot reach message data.
    /// See `docs/adr/0008-the-owner-holds-no-messages.md`.
    Owner,
    /// Named API token. Already intersected with its owner's permissions.
    ApiToken(Permissions),
}

/// Authenticated account from a session token or named API token.
#[derive(Debug, Clone)]
pub struct AuthIdentity {
    /// The authenticated account.
    pub account_id: i64,
    /// What this credential is allowed to do.
    pub capability: AuthCapability,
    /// The credential as the Audit Trail records it on a run it starts: a
    /// Session and its app, or an API token's label and hint.
    pub credential: crate::db::audit_trail::CredentialUsed,
}

impl AuthIdentity {
    /// What this credential may do, account and token already intersected.
    /// The owner has nothing: holding no messages is what the owner is.
    pub fn permissions(&self) -> Permissions {
        match self.capability {
            AuthCapability::Session { permissions } | AuthCapability::ApiToken(permissions) => {
                permissions
            }
            AuthCapability::Owner => Permissions::none(),
        }
    }

    /// True only for the logged-in owner. An API token can never be the
    /// owner, because no token resolves to [`AuthCapability::Owner`].
    pub fn is_owner(&self) -> bool {
        matches!(self.capability, AuthCapability::Owner)
    }

    /// True when the credential is a logged-in session on an ordinary account:
    /// not a token, and not the owner.
    pub fn is_session(&self) -> bool {
        matches!(self.capability, AuthCapability::Session { .. })
    }

    /// True when a person logged in, whether as the owner or on an
    /// ordinary account. False for every API token.
    pub fn is_logged_in(&self) -> bool {
        self.is_session() || self.is_owner()
    }
}

/// Reject API tokens and the owner's session on routes that require a
/// logged-in session on an ordinary account.
///
/// # Errors
///
/// Returns `403 Forbidden` when the credential is a named API token or the
/// owner's session.
pub fn require_full_access(auth: &AuthIdentity) -> Result<(), ApiError> {
    if auth.is_session() {
        return Ok(());
    }
    Err(ApiError::InsufficientScope(
        "this endpoint requires a logged-in session; use an API token only for import/export"
            .into(),
    ))
}

/// Reject anything that is not the logged-in owner.
///
/// # Errors
///
/// Returns forbidden for ordinary sessions and for every API token.
pub fn require_owner(auth: &AuthIdentity) -> Result<(), ApiError> {
    if auth.is_owner() {
        return Ok(());
    }
    Err(ApiError::NotTheOwner(
        "this endpoint requires the owner's session".into(),
    ))
}

/// Allow any logged-in person, owner or ordinary account, and reject
/// API tokens. The guard for the routes under `/v1/accounts/{id}`, where a
/// handler then decides whether the caller is the owner or the account
/// itself; the owner needs them for its own row as much as anyone.
///
/// # Errors
///
/// Returns forbidden when the credential is a named API token.
pub fn require_logged_in(auth: &AuthIdentity) -> Result<(), ApiError> {
    if auth.is_logged_in() {
        return Ok(());
    }
    Err(ApiError::InsufficientScope(
        "this endpoint requires a logged-in session; use an API token only for import/export"
            .into(),
    ))
}

/// Refuse an act the Demo Account is never open to, whoever asks.
///
/// The Demo Account has no password, so anyone at the login card can enter
/// it. Its limits are therefore fixed here, by its id, whatever its
/// permission row says, and are not settings the owner can change
/// (`docs/adr/0016-the-demo-account-is-fixed-not-configured.md`).
///
/// # Errors
///
/// Returns `demo-account-protected` when `target` is the Demo Account. `what`
/// finishes the sentence "The Demo Account's ...".
pub fn refuse_for_demo_account(target: i64, what: &str) -> Result<(), ApiError> {
    if account_profile::is_demo_account(target) {
        return Err(ApiError::DemoAccountProtected(format!(
            "The Demo Account's {what}. The owner can delete the account, and reset-demo restores it."
        )));
    }
    Ok(())
}

/// Allow a credential that may import. The Demo Account never may, whatever
/// its permission row says: an import would put real messages into an
/// account anyone can enter. It is refused every import route, a read of an
/// Import Run included, because it never has one to read.
///
/// # Errors
///
/// Returns `demo-account-protected` for the Demo Account, and forbidden when
/// import is not permitted.
pub fn require_import_access(auth: &AuthIdentity) -> Result<(), ApiError> {
    refuse_for_demo_account(
        auth.account_id,
        "imports are closed, because its messages come only from its seed",
    )?;
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

/// Allow reading an attachment's bytes: a logged-in session, or an API token
/// that may export.
///
/// A person looking at a conversation is not exporting it, so a session
/// reads its own account's attachments whatever its export permission is.
/// An API token has no screen to show them on; fetching bytes with one is
/// taking them out, which is what the export permission decides.
///
/// # Errors
///
/// Returns forbidden for an API token that may not export.
pub fn require_asset_read_access(auth: &AuthIdentity) -> Result<(), ApiError> {
    if auth.is_session() {
        return Ok(());
    }
    require_export_access(auth)
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

/// Allow a credential that may destroy message data. The Demo Account never
/// may, whatever its permission row says: one visitor would empty it for the
/// next.
///
/// # Errors
///
/// Returns `demo-account-protected` for the Demo Account, and forbidden when
/// deletion is not permitted.
pub fn require_delete_access(auth: &AuthIdentity) -> Result<(), ApiError> {
    refuse_for_demo_account(auth.account_id, "data cannot be deleted for good")?;
    if auth.permissions().delete {
        return Ok(());
    }
    Err(ApiError::InsufficientScope(
        "deleting messages is not permitted for this account".into(),
    ))
}

/// Allow a logged-in session that may destroy message data: the guard for
/// permanent deletion out of the trash. Both halves matter. Trash is a GUI
/// affair, so an API token is refused the way every trash route refuses it,
/// and the account must hold the `delete` permission. The Demo Account is
/// refused by its id in [`require_delete_access`], so it uses the trash and
/// deletes nothing for good.
///
/// # Errors
///
/// Returns forbidden when the credential is an API token,
/// `demo-account-protected` for a Demo Account session, and forbidden when
/// the account may not delete.
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

/// `auth: Option<AuthIdentity>`, for the one route a stranger and the owner
/// share, `POST /v1/accounts`: `None` when no `Authorization` header was
/// sent, the resolved credential when one was, and the usual `401` or `403`
/// when the header names nothing usable. A bad credential is never quietly
/// treated as no credential.
impl axum::extract::OptionalFromRequestParts<AppState> for AuthIdentity {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &AppState,
    ) -> Result<Option<Self>, Self::Rejection> {
        if parts.headers.get(header::AUTHORIZATION).is_none() {
            return Ok(None);
        }
        resolve_auth(&parts.headers, state).await.map(Some)
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
    /// Logged-in session on an ordinary account (API tokens and the owner
    /// rejected); wraps [`require_full_access`].
    FullAccess,
    require_full_access
);
auth_guard!(
    /// Logged-in owner; wraps [`require_owner`].
    Owner,
    require_owner
);
auth_guard!(
    /// Any logged-in person, owner or ordinary account; wraps
    /// [`require_logged_in`].
    LoggedIn,
    require_logged_in
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
    /// Logged-in session, or an API token that may export; wraps
    /// [`require_asset_read_access`].
    AssetReadAccess,
    require_asset_read_access
);
auth_guard!(
    /// Credential that may import or export, for asset probes; wraps
    /// [`require_import_or_export_access`].
    ImportOrExportAccess,
    require_import_or_export_access
);
auth_guard!(
    /// Logged-in session whose account may destroy message data; wraps
    /// [`require_full_delete_access`].
    FullDeleteAccess,
    require_full_delete_access
);

/// Shared server state passed to every HTTP handler.
#[derive(Debug, Clone)]
pub struct AppState {
    /// Loaded configuration.
    pub cfg: Arc<Config>,
    /// Connection pool for the database file. Handlers acquire
    /// short-lived connections from here.
    pub db: sqlx::SqlitePool,
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
    /// other; a running server has a single state, so the limit still spans it.
    pub(crate) auth_rate_limits: crate::credentials::AuthRateLimits,
    /// The largest multipart part, from `[server] asset_part_size`. The part
    /// size a client is told is this or the attachment size limit, whichever
    /// is smaller ([`AppState::upload_limits`]). The limit is not held here:
    /// it is a Server Setting, read from the database when a request needs it
    /// ([`AppState::asset_max_bytes`]).
    pub(crate) asset_part_size: usize,
    /// The Demo Account build the owner started, if one is running or the
    /// last one failed. One per server: a second cannot start while one runs.
    pub(crate) demo_build: crate::server_api::DemoBuild,
    /// Writes the demo bundle a build imports. A test swaps in one that
    /// writes a few conversations.
    pub(crate) demo_bundle_generator: crate::reset_demo::BundleGenerator,
}

impl AppState {
    /// The state every handler shares, over an opened database. `serve` and the
    /// test harness both come through here, so the locks and the rate limits
    /// are assembled in one place.
    pub fn new(opened: OpenDb, asset_part_size: usize) -> Self {
        Self {
            cfg: Arc::new(opened.cfg),
            db: opened.db,
            account_import_locks: KeyedLocks::default(),
            asset_complete_locks: KeyedLocks::default(),
            auth_rate_limits: Arc::new(std::sync::Mutex::new(HashMap::new())),
            asset_part_size,
            demo_build: crate::server_api::DemoBuild::default(),
            demo_bundle_generator: crate::reset_demo::generate_bundle,
        }
    }

    /// The upload limits in force at this moment: the attachment size limit,
    /// and the part size the server uses and tells a client, which is the
    /// configured one or the limit, whichever is smaller. Worked out on each
    /// call, so a changed limit moves the part size with no restart.
    pub(crate) async fn upload_limits(&self) -> anyhow::Result<asset_uploads::UploadLimits> {
        Ok(asset_uploads::UploadLimits::within(
            self.asset_part_size,
            self.asset_max_bytes().await?,
        ))
    }

    /// The attachment size limit as the Server Settings hold it at this
    /// moment, in bytes. Read on every request that needs it, so a change the
    /// owner makes holds for the next upload with no restart.
    pub(crate) async fn asset_max_bytes(&self) -> anyhow::Result<u64> {
        let mut conn = self.db.acquire().await?;
        Ok(crate::db::server_settings::load(&mut conn)
            .await?
            .asset_max_bytes)
    }
}

/// The answer to a request that made one new resource: `201 Created`, a
/// `Location` header naming it, and the JSON body the route documents.
///
/// A create that takes a batch is the exception `docs/architecture/http-api.md` records: it makes no
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

/// A failure, as one of the registered problem types (`docs/architecture/http-api.md`).
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
    /// `400` — the request cannot be read at all.
    MalformedBody(String),
    /// `400` — a line of an import batch is not the JSON Lines the server
    /// reads. A `malformed-body` that also carries the line as `line`.
    MalformedImportLine {
        /// The sentence, naming the line as a line of the batch.
        detail: String,
        /// The line of the batch, counted from 1 with blank lines included.
        line: usize,
    },
    /// `422` — lines of an import batch were read and broke a rule. A
    /// `validation-failed` that also carries the first such line as `line`.
    InvalidImportLines {
        /// One sentence per rule, naming the lines as lines of the batch.
        errors: Vec<String>,
        /// The first line that broke a rule, counted from 1 with blank
        /// lines included.
        line: usize,
    },
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
    /// `403` — the route belongs to the owner.
    NotTheOwner(String),
    /// `403` — the server does not let strangers create their own account.
    RegistrationClosed(String),
    /// `403` — the credential is valid but lacks the scope the route needs.
    InsufficientScope(String),
    /// `403` — the account may not log in or act.
    AccountDisabled(String),
    /// `422` — the search language refused a word.
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
    /// `422` — a part, upload id or completion does not match the upload.
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
            Self::ValidationFailed(_) | Self::InvalidImportLines { .. } => {
                ProblemType::ValidationFailed
            }
            Self::MalformedBody(_) | Self::MalformedImportLine { .. } => ProblemType::MalformedBody,
            Self::UnsupportedMediaType(_) => ProblemType::UnsupportedMediaType,
            Self::PayloadTooLarge(_) => ProblemType::PayloadTooLarge,
            Self::InvalidCredentials(_) => ProblemType::InvalidCredentials,
            Self::AuthenticationRequired(_) => ProblemType::AuthenticationRequired,
            Self::RateLimited { .. } => ProblemType::RateLimited,
            Self::UsernameTaken(_) => ProblemType::UsernameTaken,
            Self::NameTaken(_) => ProblemType::NameTaken,
            Self::DemoAccountProtected(_) => ProblemType::DemoAccountProtected,
            Self::NotTheOwner(_) => ProblemType::NotTheOwner,
            Self::RegistrationClosed(_) => ProblemType::RegistrationClosed,
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
                line: None,
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
            line: None,
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
            Self::MalformedImportLine { detail, line } => {
                problem.detail = Some(detail.clone());
                problem.line = Some(*line as u64);
            }
            Self::InvalidImportLines { errors, line } => {
                problem.errors = Some(errors.clone());
                problem.line = Some(*line as u64);
            }
            Self::MalformedBody(m)
            | Self::UnsupportedMediaType(m)
            | Self::PayloadTooLarge(m)
            | Self::InvalidCredentials(m)
            | Self::AuthenticationRequired(m)
            | Self::UsernameTaken(m)
            | Self::NameTaken(m)
            | Self::DemoAccountProtected(m)
            | Self::NotTheOwner(m)
            | Self::RegistrationClosed(m)
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
            Self::ValidationFailed(errors) | Self::InvalidImportLines { errors, .. } => {
                f.write_str(&errors.join("; "))
            }
            Self::RateLimited { retry_after_secs } => write!(
                f,
                "too many authentication attempts; try again in {retry_after_secs} seconds"
            ),
            Self::SearchQueryInvalid { detail, .. } | Self::MalformedImportLine { detail, .. } => {
                f.write_str(detail)
            }
            Self::MalformedBody(m)
            | Self::UnsupportedMediaType(m)
            | Self::PayloadTooLarge(m)
            | Self::InvalidCredentials(m)
            | Self::AuthenticationRequired(m)
            | Self::UsernameTaken(m)
            | Self::NameTaken(m)
            | Self::DemoAccountProtected(m)
            | Self::NotTheOwner(m)
            | Self::RegistrationClosed(m)
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

impl From<crate::db::imports::ImportLookupError> for ApiError {
    fn from(e: crate::db::imports::ImportLookupError) -> Self {
        match e {
            crate::db::imports::ImportLookupError::NotFound { import_id } => {
                Self::NotFound(format!("import {import_id} not found for this account"))
            }
            crate::db::imports::ImportLookupError::InvalidSession { message } => {
                Self::StateConflict(message)
            }
            crate::db::imports::ImportLookupError::Db(err) => Self::Internal(err),
        }
    }
}

impl From<crate::db::imports::StartImportError> for ApiError {
    fn from(e: crate::db::imports::StartImportError) -> Self {
        match e {
            err @ crate::db::imports::StartImportError::AlreadyActive => {
                // One wording for the 409, shared with the CLI paths that
                // surface the same error through anyhow.
                Self::StateConflict(err.to_string())
            }
            crate::db::imports::StartImportError::Db(err) => Self::Internal(err),
        }
    }
}

/// The one place an asset store failure gets its status
/// (`docs/architecture/http-api.md`, "Status codes").
impl From<crate::assets_api::AssetError> for ApiError {
    fn from(e: crate::assets_api::AssetError) -> Self {
        use crate::assets_api::AssetError;
        match e {
            err @ (AssetError::Mismatch { .. } | AssetError::Invalid(_)) => {
                Self::AssetUploadInvalid(err.to_string())
            }
            // The upload is busy, not wrong: a resource in the wrong state.
            err @ AssetError::Locked => Self::StateConflict(err.to_string()),
            err @ AssetError::UploadNotFound => Self::NotFound(err.to_string()),
            AssetError::Internal(err) => Self::Internal(err),
        }
    }
}

/// The one place a sender's import failure gets its status: only a line that
/// is not JSON cannot be read (`400`); everything else was read and broke a
/// rule (`422`). A failure on one line carries it as `line`, a line of the
/// batch, so a client can map it back to a file of its own.
impl From<crate::imports_api::ImportFailure> for ApiError {
    fn from(e: crate::imports_api::ImportFailure) -> Self {
        use crate::imports_api::ImportFailure;
        let detail = e.batch_sentence();
        match (&e, e.line()) {
            (ImportFailure::NotJson { .. }, Some(line)) => {
                Self::MalformedImportLine { detail, line }
            }
            (_, Some(line)) => Self::InvalidImportLines {
                errors: vec![detail],
                line,
            },
            (_, None) => Self::validation(detail),
        }
    }
}

/// An import failure the sender can fix keeps its own status; anything else
/// is a `500` with its cause in the log.
impl From<crate::imports_api::ImportError> for ApiError {
    fn from(e: crate::imports_api::ImportError) -> Self {
        match e {
            crate::imports_api::ImportError::Rejected { failure, .. } => failure.into(),
            crate::imports_api::ImportError::Run(err) => err.into(),
            crate::imports_api::ImportError::Internal(err) => Self::Internal(err),
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
/// These are allowed whatever the config says. A server built from source starts
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
/// else", which is what an unconfigured server wants: the browser UI it serves
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

/// The routes a stranger may call, with a small body limit so password
/// hashing cannot be fed huge requests. `POST /v1/accounts` is among them:
/// the owner's creation shares the route, and a small body is all it needs.
fn limited_auth_router() -> (Router<AppState>, utoipa::openapi::OpenApi) {
    let (router, spec) = crate::openapi::public_openapi().split_for_parts();
    (
        router.layer(RequestBodyLimitLayer::new(MAX_AUTH_BODY_BYTES)),
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

/// The body cap of the routes a stranger may call ([`limited_auth_router`]):
/// 32 KiB, so password hashing cannot be fed a large body.
pub(crate) const MAX_AUTH_BODY_BYTES: usize = 32 * 1024;

/// The cap on a JSON body, the one `crate::extract::Json` reads: 32 MiB. It
/// is sized for the largest body the web app sends, the completion of an
/// Import Run (`POST /v1/imports/{id}/complete`), which carries every issue
/// of the run; at a few hundred bytes an issue, that is about a hundred
/// thousand issues. Without it a JSON body is held to Axum's own 2 MiB
/// default, a figure nobody chose.
pub(crate) const MAX_JSON_BODY_BYTES: usize = 32 * 1024 * 1024;

/// The body cap of every route but the attachment uploads: 512 MiB, the
/// attachment size limit a new Message Crate starts with. It is fixed in the
/// code because the owner's limit must never reach the login or the settings
/// change that would raise it again. Routes that read a body of their own
/// hold it to a smaller figure first.
pub(crate) const MAX_REQUEST_BODY_BYTES: usize = 512 * 1024 * 1024;

/// Hold a request body to its cap. An attachment upload
/// ([`is_attachment_upload`]) is held to the attachment size limit, as the
/// Server Settings have it when the request arrives; every other request to
/// [`MAX_REQUEST_BODY_BYTES`]. A `Content-Length` over the cap is refused
/// before any handler runs; a body with no declared length is cut off once it
/// passes the cap. A request with a safe method carries no body the server
/// reads, so it skips the check. A part of a multipart upload
/// ([`is_upload_part`]) skips it too, because its route holds the body to the
/// part size the upload started with, and the limit as it is now would refuse
/// every remaining part of an upload the owner lowered the limit under.
async fn limit_request_body(
    axum::extract::State(state): axum::extract::State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    use axum::http::Method;
    if matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) || is_upload_part(&request)
    {
        return next.run(request).await;
    }
    let limit = if is_attachment_upload(&request) {
        match state.asset_max_bytes().await {
            Ok(limit) => usize::try_from(limit).unwrap_or(usize::MAX),
            Err(error) => return ApiError::Internal(error).into_response(),
        }
    } else {
        MAX_REQUEST_BODY_BYTES
    };
    let declared = request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    if declared.is_some_and(|bytes| bytes > limit as u64) {
        return ApiError::PayloadTooLarge("the request body is too large".to_string())
            .into_response();
    }
    let request =
        request.map(|body| axum::body::Body::new(http_body_util::Limited::new(body, limit)));
    next.run(request).await
}

/// Refuse a request whose `Accept` names nothing this route can produce
/// (`docs/architecture/http-api.md`). Narrow on purpose: only when the header is present and none of
/// its members is `application/json`, `application/problem+json`,
/// `application/*` or `*/*`. A missing `Accept` is a request for JSON, which
/// is what every one of the server's own clients sends.
///
/// Applied to the `/v1` routes only, through `route_layer`; the static app,
/// `/health` and the OpenAPI UI are mounted outside it, so they keep
/// producing what they produce. Three routes answer bytes, not JSON, and are
/// let through here by path: the asset download and its preview stream the
/// file's own bytes, and the address book export answers `text/csv`.
async fn require_json_acceptable(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    if is_asset_download(&request) || is_address_book_export(&request) {
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

/// `GET /v1/assets/{sha256}` and `GET /v1/assets/{sha256}/preview`: the
/// asset's own bytes or its preview's, each in its own media type.
fn is_asset_download(request: &axum::extract::Request) -> bool {
    request.method() == axum::http::Method::GET
        && request
            .uri()
            .path()
            .strip_prefix("/v1/assets/")
            .is_some_and(|rest| {
                let sha256 = rest.strip_suffix("/preview").unwrap_or(rest);
                !sha256.is_empty() && !sha256.contains('/')
            })
}

/// The path segments after `/v1/assets/` of a `PUT`, or `None` for any other
/// request.
fn asset_put_segments(request: &axum::extract::Request) -> Option<Vec<&str>> {
    if request.method() != axum::http::Method::PUT {
        return None;
    }
    let rest = request.uri().path().strip_prefix("/v1/assets/")?;
    Some(rest.split('/').collect())
}

/// `PUT /v1/assets/{sha256}`: the one request the attachment size limit, as
/// the Server Settings have it when the request arrives, holds.
fn is_attachment_upload(request: &axum::extract::Request) -> bool {
    asset_put_segments(request)
        .is_some_and(|segments| matches!(segments.as_slice(), [sha256] if !sha256.is_empty()))
}

/// `PUT /v1/assets/{sha256}/uploads/{upload_id}/parts/{part}`: one part of a
/// multipart upload, which its route holds to the part size in the upload's
/// manifest.
fn is_upload_part(request: &axum::extract::Request) -> bool {
    asset_put_segments(request).is_some_and(|segments| {
        matches!(
            segments.as_slice(),
            [sha256, "uploads", upload_id, "parts", part]
                if !sha256.is_empty() && !upload_id.is_empty() && !part.is_empty()
        )
    })
}

/// `POST /v1/contacts/address-book`: the address book as `text/csv`.
fn is_address_book_export(request: &axum::extract::Request) -> bool {
    request.method() == axum::http::Method::POST
        && request.uri().path() == "/v1/contacts/address-book"
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
    let static_dir = state
        .cfg
        .server
        .as_ref()
        .map_or_else(|| "static".into(), |s| s.static_dir.clone());
    let (auth_small, mut spec) = limited_auth_router();
    let (health_router, health) = crate::openapi::health_openapi().split_for_parts();
    spec.merge(health);
    let (doc_router, rest) = crate::openapi::api_openapi().split_for_parts();
    spec.merge(rest);
    crate::openapi::finish(&mut spec);
    let declared_queries =
        std::sync::Arc::new(crate::declared_query::DeclaredQueries::from_spec(&spec));

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
        // After the `Accept` check and before the query check: `/health` is
        // outside `/v1`, so a probe sending `Accept: text/plain` gets the
        // plain text it answers, and its query is still checked like every
        // other route's.
        .merge(health_router)
        // After routing, so the matched path names the operation whose
        // declared parameters the query is checked against.
        .route_layer(axum::middleware::from_fn_with_state(
            declared_queries,
            crate::declared_query::refuse_undeclared_query,
        ));
    // After both route layers, so the Swagger UI answers a browser's
    // `Accept: text/html` and is not held to a declared query, and before
    // every layer below, so its responses carry the request id, the CORS
    // headers and the log line every other response does.
    if openapi_ui {
        api = api.merge(utoipa_swagger_ui::SwaggerUi::new("/docs").url("/openapi.json", spec));
    }
    api.method_not_allowed_fallback(api_method_not_allowed)
        .fallback_service(ServeDir::new(static_dir))
        // The cap `extract::Json` reads a body against. The routes that read
        // a body of their own hold it to their own figure instead.
        .layer(DefaultBodyLimit::max(MAX_JSON_BODY_BYTES))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            limit_request_body,
        ))
        // Rewrite the auth routes' limit layer's plain-text 413 into a problem
        // before CORS sees it, so the response a browser gets is both JSON and
        // CORS-clean.
        .layer(axum::middleware::map_response(json_body_limit_response))
        // Outside the limit layer: every response, including one the limit
        // layer answered itself, carries the CORS headers a browser needs to
        // show it.
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
        .layer(axum::middleware::from_fn(crate::request_id::layer))
        .with_state(state)
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
    let _operation_lock = crate::operation_lock::acquire_for_serve(&cfg.paths.db)?;

    // Every new Message Crate starts with the Demo Account: seed first, then
    // listen, so the first page a person loads already offers it (#971).
    if crate::reset_demo::database_is_new(&cfg).await? {
        crate::operation_lock::clear_ready(&cfg.paths.db)?;
        crate::reset_demo::seed_new_database(&cfg).await;
    }
    let opened = OpenDb::create_or_open(cfg).await?;
    crate::operation_lock::mark_ready(&opened.cfg.paths.db)?;
    let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&opened.db)
        .await
        .unwrap_or_else(|_| "unknown".into());
    eprintln!(
        "  db:   {} (journal_mode={mode})",
        opened.cfg.paths.db.display()
    );
    let state = AppState::new(opened, server.asset_part_size);
    if crate::server_api::recover_stopped_demo_build(&state).await? {
        eprintln!(
            "  demo: the server stopped during a Demo Account build; the part-built Demo Account was removed"
        );
    }
    // Reported as they stand now; each upload reads them again. Any stored
    // limit starts the server: a part is never larger than the limit.
    let upload_limits = state.upload_limits().await?;
    eprintln!(
        "  assets: max={} MiB  part_size={} MiB",
        upload_limits.max_bytes / message_ir::MIB,
        upload_limits.part_size as u64 / message_ir::MIB
    );

    let demo_build = state.demo_build.clone();
    let app = http_app(state);
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    eprintln!("message-crate-server serve listening on http://{bind}");
    eprintln!(
        "  routes: `message-crate-server dump-openapi` lists them all; set [server] openapi_ui = true for /docs"
    );
    let served = serve_until_shutdown(listener, app).await;
    // A Demo Account build the owner started would otherwise end part-way
    // when the process exits (#1215).
    demo_build.stop().await;
    served?;
    Ok(())
}

/// Serve `app` on `listener` until a shutdown signal arrives, then stop
/// accepting connections and return once the requests in flight have finished.
async fn serve_until_shutdown(
    listener: tokio::net::TcpListener,
    app: Router,
) -> std::io::Result<()> {
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
}

/// Resolve on Ctrl-C, or on SIGTERM on Unix, so axum drains in-flight
/// requests before exiting. `docker stop` and a service manager send SIGTERM,
/// not Ctrl-C (#1218).
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sigterm) => {
                sigterm.recv().await;
            }
            // Without a SIGTERM handler the process still stops on SIGTERM,
            // only without draining; Ctrl-C keeps working.
            Err(e) => {
                eprintln!("cannot listen for SIGTERM: {e}");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
    eprintln!("shutting down");
}

/// Report process liveness.
#[utoipa::path(
    get,
    path = "/health",
    tag = "Health",
    responses((status = 200, description = "Process is up", body = String))
)]
pub(crate) async fn get_health() -> (StatusCode, &'static str) {
    (StatusCode::OK, "ok\n")
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
    // RFC 7235, section 2.1: the scheme matches without regard to case.
    let Some(token) = value
        .split_once(' ')
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
        .map(|(_, token)| token)
    else {
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
    resolve_auth_on_conn(&mut conn, &token, connecting_app(headers).as_ref()).await
}

pub use message_crate_api_types::{APP_HEADER, APP_VERSION_HEADER};

/// Longest Build the server records. A real one is under thirty characters.
const MAX_APP_BUILD_LEN: usize = 64;

/// The app a request says it comes from. `None` unless both headers are
/// present and well formed: curl, Swagger UI and a script send neither, and
/// are served without anything being recorded.
pub(crate) fn connecting_app(headers: &HeaderMap) -> Option<session_tokens::ConnectingApp> {
    let kind = session_tokens::AppKind::parse(headers.get(APP_HEADER)?.to_str().ok()?)?;
    let build = headers.get(APP_VERSION_HEADER)?.to_str().ok()?.trim();
    let well_formed = !build.is_empty()
        && build.len() <= MAX_APP_BUILD_LEN
        && build.chars().all(|c| c.is_ascii_graphic());
    well_formed.then(|| session_tokens::ConnectingApp {
        kind,
        build: build.to_string(),
    })
}

/// Credential-specific bit not yet folded into `AuthCapability`: a session
/// carries no extra state, an API token carries its own (pre-intersection)
/// permissions. Both kinds load `AccountAuth` the same way so the disabled
/// check in [`resolve_auth_on_conn`] runs exactly once.
enum Credential {
    Session,
    ApiToken(Permissions),
}

use crate::db::audit_trail::CredentialUsed;

/// Resolve a Bearer credential on an existing connection.
///
/// `app` is the app the request named, if it named one. It is recorded on a
/// session and ignored for an API token, which belongs to a program rather
/// than to either app. It never decides whether the request is served.
///
/// # Errors
///
/// Unauthorized when the token matches nothing; forbidden when the account is
/// disabled.
pub async fn resolve_auth_on_conn(
    conn: &mut SqliteConnection,
    token: &str,
    app: Option<&session_tokens::ConnectingApp>,
) -> Result<AuthIdentity, ApiError> {
    schema::ensure_accounts_schema(conn).await?;

    let resolved = if let Some(session) = session_tokens::lookup_session(&mut *conn, token).await? {
        if let Some(app) = app {
            // A record, not a check: a failed write still serves the
            // request (#1189).
            if let Err(err) = session_tokens::record_connecting_app(&mut *conn, &session, app).await
            {
                tracing::warn!(
                    account_id = session.account_id,
                    error = %format!("{err:#}"),
                    "could not record the connecting app"
                );
            }
        }
        // The app this request named, or the one the session last recorded.
        let used = CredentialUsed::Session(app.cloned().or(session.app));
        Some((session.account_id, Credential::Session, used))
    } else {
        api_tokens::lookup_account_for_api_token(&mut *conn, token)
            .await?
            .map(|tok| {
                let used = CredentialUsed::ApiToken {
                    label: tok.label,
                    hint: tok.token_hint,
                };
                (tok.account_id, Credential::ApiToken(tok.permissions), used)
            })
    };

    let Some((account_id, credential, used)) = resolved else {
        return Err(ApiError::AuthenticationRequired("invalid API token".into()));
    };

    let auth = account_profile::load_account_auth(&mut *conn, account_id)
        .await?
        .ok_or_else(|| ApiError::AuthenticationRequired("account no longer exists".into()))?;
    if auth.disabled {
        return Err(ApiError::AccountDisabled("this account is disabled".into()));
    }

    // A session on the owner's account resolves to `Owner`, which carries no
    // permissions. An API token never does, whichever account issued it, so
    // no token can reach what the owner reaches.
    let capability = match credential {
        Credential::Session if account_profile::is_server_owner(account_id) => {
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
        credential: used,
    })
}

/// The account an import or asset route writes to: the one the credential
/// names, always.
///
/// A route never takes an `account=` parameter. A session or an API token
/// belongs to exactly one account, so a parameter could only repeat it or
/// contradict it, and the rules document says the credential names the
/// account (`docs/architecture/http-api.md`, "Credentials and reach").
pub(crate) fn resolve_import_account(auth: &AuthIdentity) -> i64 {
    auth.account_id
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

/// The problem a failed read of a request body answers. `limit_request_body`
/// wraps a body with no `Content-Length` in `Limited`, which ends the stream
/// with a `LengthLimitError` once the body passes its cap: that body is too
/// large. Any other failure leaves the body unreadable.
fn body_read_error(error: axum::Error) -> ApiError {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&error);
    while let Some(cause) = source {
        if cause.is::<http_body_util::LengthLimitError>() {
            return ApiError::PayloadTooLarge("request body too large".into());
        }
        source = cause.source();
    }
    ApiError::MalformedBody(format!("failed to read body: {error}"))
}

/// Read the whole request body into memory, failing once it passes `max_bytes`.
pub(crate) async fn read_body_limited(
    body: axum::body::Body,
    max_bytes: usize,
) -> Result<Vec<u8>, ApiError> {
    let mut out = Vec::new();
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(body_read_error)?;
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
        let chunk = chunk.map_err(body_read_error)?;
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
        let chunk = chunk.map_err(body_read_error)?;
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
/// rooted at `data_dir` (with a sibling `messagecrate.db` path that nothing in the
/// test suite reads from disk — queries go through `pool`), the given pool,
/// and the default part size. Goes through [`AppState::new`], the same
/// assembly `serve` uses. `#[cfg(test)]`-gated so it never ships in a release
/// build; `pub(crate)` so `test_support` and the other test modules in this
/// crate can reach it.
#[cfg(test)]
pub(crate) fn test_app_state(pool: sqlx::SqlitePool, data_dir: &Path) -> AppState {
    let cfg = crate::config::Config {
        paths: crate::config::PathsConfig {
            db: data_dir.join("messagecrate.db"),
            data_dir: data_dir.to_path_buf(),
            assets_dir: "assets".into(),
            assets_converted_dir: "assets_converted".into(),
        },
        server: Some(crate::config::ServerConfig {
            bind: "127.0.0.1:0".into(),
            asset_part_size: 1024 * 1024,
            cors_origins: Vec::new(),
            openapi_ui: false,
            static_dir: "static".into(),
        }),
    };
    AppState::new(OpenDb { cfg, db: pool }, asset_uploads::DEFAULT_PART_SIZE)
}

#[cfg(test)]
mod tests;
