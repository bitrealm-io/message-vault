//! What a logged-out browser is allowed to know about this Message Crate, and the one
//! act it is allowed to perform: claiming an unclaimed Message Crate.
//!
//! `GET /v1/server` reports this Message Crate's state as a single value rather than the
//! two facts behind it — whether an owner exists, and whether public
//! registration is on — so that the rule joining them is stated once, on the
//! server. A browser and a desktop app that each derived the entry screen from
//! raw fields would be two copies of one rule, free to drift apart.
//!
//! It also reports the attachment size limit, because a program that uploads
//! has to know it before it starts and only the owner reads the settings.
//!
//! These are the server's only unauthenticated routes besides logging in and
//! a stranger's `POST /v1/accounts`, and the first read routes that do not
//! require a session: the entry screen cannot have one yet, which is the
//! whole of the exception. See
//! `docs/adr/0008-the-owner-holds-no-messages.md`.

use axum::extract::State;
use serde::{Deserialize, Serialize};

use crate::db::{account_profile, server_settings, storage};
use crate::extract::Json;
use crate::server::{ApiError, AppState, Created, Owner};

/// What state a Message Crate is in, from outside.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ServerState {
    /// Nobody owns this Message Crate yet. The only thing to do is claim it.
    Unclaimed,
    /// Owned, and only the owner creates accounts.
    Closed,
    /// Owned, and anyone reaching the server may create their own account.
    Open,
}

/// The state of this Message Crate, for the screen a logged-out person sees.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ServerInfo {
    /// `unclaimed` shows Create Owner alone; `closed` shows Login alone;
    /// `open` shows Login and Create Account.
    pub state: ServerState,
    /// Whether the Demo Account exists. While it does, the screen offers a
    /// way into it beside whatever `state` shows: it has no password, so
    /// there is nothing to type.
    pub demo_account: bool,
    /// The server's Build: its Product Version, plus the commit it was built
    /// from unless it is a release. An app compares the Product Version with
    /// its own and says so when they differ; the server serves it either way.
    pub version: String,
    /// The Schema Fingerprint, the number this server stamps into its database.
    pub schema_fingerprint: i64,
    /// The attachment size limit, in bytes: the largest asset the server
    /// accepts, as one `PUT` or as the declared total of a multipart upload.
    /// The owner sets it in the server settings. An app reads it here before
    /// it prepares attachments for upload.
    pub asset_max_bytes: u64,
}

/// Body for claiming a Message Crate.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ClaimRequest {
    /// Login username for the owner.
    pub username: String,
    /// Password for the owner. Must satisfy the server's password policy.
    pub password: String,
}

/// Read this Message Crate's state on an existing connection.
async fn state_on_conn(conn: &mut sqlx::SqliteConnection) -> Result<ServerState, ApiError> {
    if !account_profile::is_claimed(conn).await? {
        return Ok(ServerState::Unclaimed);
    }
    let settings = server_settings::load(conn).await?;
    Ok(if settings.public_registration {
        ServerState::Open
    } else {
        ServerState::Closed
    })
}

/// Report whether this Message Crate is unclaimed, closed, or open.
///
/// Also reports the server's Build, its Schema Fingerprint, whether the Demo
/// Account exists, and the attachment size limit.
#[utoipa::path(
    get,
    path = "/v1/server",
    tag = "Server",
    responses((status = 200, body = ServerInfo))
)]
pub async fn get_server(State(state): State<AppState>) -> Result<Json<ServerInfo>, ApiError> {
    let mut conn = state.db.acquire().await?;
    Ok(Json(ServerInfo {
        state: state_on_conn(&mut conn).await?,
        demo_account: account_profile::username_for_account(
            &mut conn,
            account_profile::DEMO_ACCOUNT_ID,
        )
        .await?
        .is_some(),
        version: crate::BUILD.to_string(),
        schema_fingerprint: crate::db::schema::SCHEMA_FINGERPRINT,
        asset_max_bytes: server_settings::load(&mut conn).await?.asset_max_bytes,
    }))
}

/// Claim an unclaimed Message Crate by creating its owner.
///
/// Unauthenticated, because a Message Crate with no owner has no credential
/// that could authorize this. Whoever reaches an unclaimed one first may
/// claim it: Message Crate is self-hosted, so its operator installs the
/// software, claims it, and publishes the port, in that order and at times
/// of their choosing. An unclaimed one is also empty, so a lost race
/// destroys nothing and announces itself at once.
#[utoipa::path(
    post,
    path = "/v1/server/claim",
    tag = "Server",
    request_body = ClaimRequest,
    responses(
        (
            status = 201,
            description = "Claimed; the owner's Session is made",
            body = crate::session_api::CreateSessionResponse,
            headers(("Location" = String, description = "`/v1/session`, the Session the claim made"))
        ),
        crate::problem::openapi::StateConflict,
        crate::problem::openapi::RateLimited
    )
)]
pub async fn claim_server(
    State(state): State<AppState>,
    Json(req): Json<ClaimRequest>,
) -> Result<Created<crate::session_api::CreateSessionResponse>, ApiError> {
    let username = crate::credentials::require_valid_username(&req.username)?;
    crate::credentials::check_auth_rate_limit(&state.auth_rate_limits, "claim")?;
    let password_hash = crate::credentials::hash_owner_password(&req.password)?;

    let mut conn = state.db.acquire().await?;
    // The claim check and the insert share a transaction: two requests racing
    // for an unclaimed Message Crate must not both believe they won it.
    let mut tx = sqlx::Connection::begin(&mut *conn).await?;
    if account_profile::is_claimed(&mut tx).await? {
        return Err(ApiError::StateConflict(
            "this Message Crate already has an owner".into(),
        ));
    }
    crate::credentials::require_username_free(&mut tx, &username).await?;
    account_profile::insert_account_at(
        &mut tx,
        account_profile::OWNER_ACCOUNT_ID,
        &username,
        Some(&password_hash),
        None,
    )
    .await
    .map_err(ApiError::Internal)?;
    let token = crate::db::session_tokens::insert_account_session_token(
        &mut tx,
        account_profile::OWNER_ACCOUNT_ID,
    )
    .await
    .map_err(ApiError::Internal)?;
    account_profile::record_login(&mut tx, account_profile::OWNER_ACCOUNT_ID).await?;
    tx.commit().await?;

    // The claim makes the owner's Session, and that is the resource it names.
    Ok(Created {
        location: "/v1/session".to_string(),
        body: crate::session_api::CreateSessionResponse {
            token,
            account_id: account_profile::OWNER_ACCOUNT_ID,
            username,
        },
    })
}

/// The server settings the owner controls.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ServerSettings {
    /// Anyone reaching the server may create their own account.
    pub public_registration: bool,
    /// The attachment size limit, in bytes: the largest asset the server
    /// accepts. 536870912 (512 MiB) until the owner changes it.
    pub asset_max_bytes: u64,
}

impl From<server_settings::ServerSettings> for ServerSettings {
    fn from(settings: server_settings::ServerSettings) -> Self {
        Self {
            public_registration: settings.public_registration,
            asset_max_bytes: settings.asset_max_bytes,
        }
    }
}

/// Body for changing the server settings. Omitted fields are left alone.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateServerSettingsRequest {
    /// Let anyone reaching the server create their own account, or stop them.
    #[serde(default)]
    pub public_registration: Option<bool>,
    /// The new attachment size limit, in bytes. At least 1. A limit below
    /// `[server] asset_part_size` is accepted: the server then hands out
    /// parts the size of the limit.
    #[serde(default)]
    pub asset_max_bytes: Option<u64>,
}

/// Check a new attachment size limit against the rules the owner's change is
/// held to, returning the sentence for the one it breaks. Any limit the
/// database can hold is accepted but zero: the part size follows the limit
/// down, so no limit is too small for the config file.
fn asset_max_bytes_problem(bytes: u64) -> Option<String> {
    if bytes == 0 {
        return Some("asset_max_bytes must be at least 1".to_string());
    }
    if i64::try_from(bytes).is_err() {
        return Some(format!("asset_max_bytes must be at most {}", i64::MAX));
    }
    None
}

/// Read the server settings.
#[utoipa::path(
    get,
    path = "/v1/server/settings",
    tag = "Server",
    security(("session" = ["owner"])),
    responses(
        (status = 200, body = ServerSettings),
    )
)]
pub async fn get_server_settings(
    State(state): State<AppState>,
    Owner(_auth): Owner,
) -> Result<Json<ServerSettings>, ApiError> {
    let mut conn = state.db.acquire().await?;
    Ok(Json(server_settings::load(&mut conn).await?.into()))
}

/// Change the server settings.
///
/// A new attachment size limit holds from the next upload, with no restart.
/// A limit of zero is refused and nothing is changed. The part size the
/// server hands out for a multipart upload is never larger than the limit.
#[utoipa::path(
    patch,
    path = "/v1/server/settings",
    tag = "Server",
    security(("session" = ["owner"])),
    request_body = UpdateServerSettingsRequest,
    responses(
        (status = 200, body = ServerSettings),
    )
)]
pub async fn update_server_settings(
    State(state): State<AppState>,
    Owner(_auth): Owner,
    Json(req): Json<UpdateServerSettingsRequest>,
) -> Result<Json<ServerSettings>, ApiError> {
    // Checked before anything is written, so a refused request changes nothing.
    if let Some(problem) = req.asset_max_bytes.and_then(asset_max_bytes_problem) {
        return Err(ApiError::validation(problem));
    }
    let mut conn = state.db.acquire().await?;
    if let Some(enabled) = req.public_registration {
        server_settings::set_public_registration(&mut conn, enabled).await?;
    }
    if let Some(bytes) = req.asset_max_bytes {
        server_settings::set_asset_max_bytes(&mut conn, bytes).await?;
    }
    Ok(Json(server_settings::load(&mut conn).await?.into()))
}

/// What the whole database holds, summed over every account.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ServerStorage {
    /// Messages across every account.
    pub message_count: i64,
    /// Conversations across every account.
    pub conversation_count: i64,
    /// Contacts across every account.
    pub contact_count: i64,
    /// Attachment rows across every account.
    pub attachment_count: i64,
    /// Attachment bytes across every account, by original file size.
    pub total_bytes: i64,
    /// Bytes the database takes on disk, measured. Attachment files are not
    /// in it; `total_bytes` has those.
    pub database_bytes: i64,
    /// Bytes the messages table and its indexes take, measured, without the
    /// full-text search index.
    pub messages_bytes: i64,
    /// Bytes the full-text search index takes, measured, for the whole
    /// database. It is one shared structure, so there is no per-account figure.
    pub fts_bytes: i64,
    /// Every account, including ones with no messages: the owner first, then
    /// by username, as the User Accounts table lists them.
    pub accounts: Vec<AccountMessages>,
}

/// One account's share of the messages held: an id, a username and numbers.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct AccountMessages {
    pub account_id: i64,
    pub username: String,
    /// Messages the account holds.
    pub message_count: i64,
    /// Bytes of message text the account holds: every body and subject, added up.
    pub text_bytes: i64,
    /// The account's estimated share of `messages_bytes`, split by its share
    /// of all text. The shares add up to `messages_bytes` exactly.
    pub estimated_message_bytes: i64,
}

/// Read what the database holds. The counts and the attachment bytes are summed
/// over every account. The database, messages and full-text search sizes
/// are measured on disk. Each account's share of message storage is an
/// estimate from its share of text. Counts and totals only, never a name or
/// a line of text (`docs/adr/0008-the-owner-holds-no-messages.md`,
/// "What the owner may see"). The owner's, because the owner administers this
/// Message Crate and nobody else holds more than their own account.
#[utoipa::path(
    get,
    path = "/v1/server/storage",
    tag = "Server",
    security(("session" = ["owner"])),
    responses(
        (status = 200, body = ServerStorage),
    )
)]
pub async fn get_server_storage(
    State(state): State<AppState>,
    Owner(_auth): Owner,
) -> Result<Json<ServerStorage>, ApiError> {
    let mut conn = state.db.acquire().await?;
    let scope = storage::Scope::AllAccounts;
    let fts_bytes = storage::fts_bytes(&mut conn).await?;
    let messages_bytes = storage::messages_bytes(&mut conn).await?;
    let by_account = storage::text_by_account(&mut conn).await?;
    let shares = storage::split_by_text(messages_bytes, &by_account);
    let accounts = by_account
        .into_iter()
        .zip(shares)
        .map(|(account, estimated_message_bytes)| AccountMessages {
            account_id: account.account_id,
            username: account.username,
            message_count: account.message_count,
            text_bytes: account.text_bytes,
            estimated_message_bytes,
        })
        .collect();
    Ok(Json(ServerStorage {
        message_count: storage::message_count(&mut conn, scope).await?,
        conversation_count: storage::conversation_count(&mut conn, scope).await?,
        contact_count: storage::contact_count(&mut conn, scope).await?,
        attachment_count: storage::attachment_count(&mut conn, scope).await?,
        total_bytes: storage::attachment_bytes(&mut conn, scope).await?,
        database_bytes: storage::database_bytes(&mut conn).await?,
        messages_bytes,
        fts_bytes,
        accounts,
    }))
}

/// How much Demo Data the Demo Account holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DemoDataSize {
    /// About 54,000 messages. A new Message Crate starts with this.
    Medium,
    /// About 613,000 messages. Building it takes about a minute.
    Large,
}

impl From<DemoDataSize> for demo_seed::DemoSize {
    fn from(size: DemoDataSize) -> Self {
        match size {
            DemoDataSize::Medium => Self::Medium,
            DemoDataSize::Large => Self::Large,
        }
    }
}

/// Where the Demo Account stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DemoAccountStatus {
    /// There is no Demo Account.
    Absent,
    /// The server is building it. It may be logged into, and holds part of
    /// its data until the build ends.
    Building,
    /// It exists and no build is running.
    Ready,
    /// The last build failed and the account was removed.
    Failed,
}

/// The Demo Account, as the owner manages it.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DemoAccount {
    /// Whether it exists, is being built, or failed to build.
    pub status: DemoAccountStatus,
    /// The size being built, while `status` is `building`.
    pub size: Option<DemoDataSize>,
    /// Why the last build failed, while `status` is `failed`.
    pub error: Option<String>,
}

/// Body for adding or resetting the Demo Account.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ReplaceDemoAccountRequest {
    /// How much Demo Data to build.
    pub size: DemoDataSize,
}

/// The Demo Account build in this server's memory: none, one running, or the
/// last one failed. A restart forgets it, and the database then says whether
/// the account exists.
#[derive(Debug, Clone, Default)]
pub(crate) struct DemoBuild(std::sync::Arc<std::sync::Mutex<DemoBuildState>>);

#[derive(Debug, Clone, Default)]
enum DemoBuildState {
    #[default]
    Idle,
    Building(DemoDataSize),
    Failed(String),
}

impl DemoBuild {
    fn get(&self) -> DemoBuildState {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn set(&self, state: DemoBuildState) {
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = state;
    }

    /// Mark a build of `size` as running, unless one already is.
    fn start(&self, size: DemoDataSize) -> bool {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if matches!(*state, DemoBuildState::Building(_)) {
            return false;
        }
        *state = DemoBuildState::Building(size);
        true
    }

    /// Whether a build is running. Deleting the Demo Account waits for it.
    pub(crate) fn is_building(&self) -> bool {
        matches!(self.get(), DemoBuildState::Building(_))
    }
}

/// Where the Demo Account stands: the build in memory first, then the database.
async fn demo_account_on_conn(
    state: &AppState,
    conn: &mut sqlx::SqliteConnection,
) -> Result<DemoAccount, ApiError> {
    let exists = account_profile::username_for_account(conn, account_profile::DEMO_ACCOUNT_ID)
        .await?
        .is_some();
    Ok(match state.demo_build.get() {
        DemoBuildState::Building(size) => DemoAccount {
            status: DemoAccountStatus::Building,
            size: Some(size),
            error: None,
        },
        // A failed build removes the account. One that exists anyway was
        // added since, from the command line, and that is the newer fact.
        DemoBuildState::Failed(error) if !exists => DemoAccount {
            status: DemoAccountStatus::Failed,
            size: None,
            error: Some(error),
        },
        DemoBuildState::Idle | DemoBuildState::Failed(_) => DemoAccount {
            status: if exists {
                DemoAccountStatus::Ready
            } else {
                DemoAccountStatus::Absent
            },
            size: None,
            error: None,
        },
    })
}

/// Read where the Demo Account stands.
///
/// A program that started a build reads this until `status` is no longer
/// `building`.
#[utoipa::path(
    get,
    path = "/v1/server/demo-account",
    tag = "Server",
    security(("session" = ["owner"])),
    responses((status = 200, body = DemoAccount))
)]
pub async fn get_demo_account(
    State(state): State<AppState>,
    Owner(_auth): Owner,
) -> Result<Json<DemoAccount>, ApiError> {
    let mut conn = state.db.acquire().await?;
    Ok(Json(demo_account_on_conn(&state, &mut conn).await?))
}

/// Add the Demo Account, or reset it.
///
/// The Demo Account is removed, with everything a visitor changed in it, and
/// built again with Demo Data of the size given. No other account is touched.
/// The build runs after the answer is sent, while the server keeps serving:
/// the answer is `202` with `status` `building`, and `GET` reports when it
/// ends. A second request while one build runs is refused.
#[utoipa::path(
    put,
    path = "/v1/server/demo-account",
    tag = "Server",
    security(("session" = ["owner"])),
    request_body = ReplaceDemoAccountRequest,
    responses(
        (status = 202, description = "The build has started", body = DemoAccount),
        crate::problem::openapi::StateConflict
    )
)]
pub async fn replace_demo_account(
    State(state): State<AppState>,
    Owner(_auth): Owner,
    Json(req): Json<ReplaceDemoAccountRequest>,
) -> Result<(axum::http::StatusCode, Json<DemoAccount>), ApiError> {
    if !state.demo_build.start(req.size) {
        return Err(ApiError::StateConflict(
            "the Demo Account is already being built".into(),
        ));
    }
    let build = state.demo_build.clone();
    let cfg = state.cfg.clone();
    let generate = state.demo_bundle_generator;
    tokio::spawn(async move {
        let started = std::time::Instant::now();
        match crate::reset_demo::build_demo_account(cfg, req.size.into(), generate).await {
            Ok(messages) => {
                tracing::info!(
                    messages,
                    seconds = started.elapsed().as_secs_f64(),
                    "Demo Account built"
                );
                build.set(DemoBuildState::Idle);
            }
            Err(error) => {
                tracing::error!("Demo Account build failed: {error:#}");
                build.set(DemoBuildState::Failed(format!("{error:#}")));
            }
        }
    });
    Ok((
        axum::http::StatusCode::ACCEPTED,
        Json(DemoAccount {
            status: DemoAccountStatus::Building,
            size: Some(req.size),
            error: None,
        }),
    ))
}

#[cfg(test)]
mod tests;
