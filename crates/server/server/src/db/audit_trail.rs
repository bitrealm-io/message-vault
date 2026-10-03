//! The Audit Trail: what each user did on this Message Crate, and when.
//!
//! The record is three tables read as one list. `audit_entries` holds
//! everything that has no record of its own: sessions, refused logins, and the
//! owner's and holders' changes to accounts. Import Runs and Export Runs stay
//! in `imports` and `exports` and are not copied. [`page`] reads all three in
//! time order, and adds a `session_ended` entry with the reason `expired` for
//! each session that ran out with nothing to end it, so expiry needs no write.
//!
//! An entry is written once. No route changes or deletes one; the only
//! deletion is [`trim_refused_logins`], for refused logins as a username no
//! account holds, after [`UNKNOWN_USERNAME_RETENTION_DAYS`]. Deleting an
//! account keeps its entries and runs ([`prepare_account_deletion`]).
//! A password, a session token and an API token never enter the trail, and
//! nothing here says what a message said or which conversation it was in
//! (`docs/adr/0008-the-owner-holds-no-messages.md`,
//! `docs/adr/0020-the-audit-trail-outlives-the-account.md`).

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqliteConnection};

use crate::db::account_profile;
use crate::db::session_tokens::{AppKind, ConnectingApp};

/// Days a refused login as a username no account holds is kept. Such an
/// entry belongs to no one, so without a limit anyone who can reach the
/// server could grow the record forever.
pub const UNKNOWN_USERNAME_RETENTION_DAYS: i64 = 90;

/// The longest username kept from a refused login: the username length limit.
const MAX_TYPED_USERNAME_CHARS: usize = 128;

/// What an entry records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AuditAction {
    /// A login opened a Session: logging in, claiming the Message Crate, or
    /// registering.
    LoggedIn,
    /// A Session ended; `reason` says how.
    SessionEnded,
    /// A login was refused; `reason` says why.
    LoginRefused,
    /// An account was made.
    AccountCreated,
    /// The owner disabled the account.
    AccountDisabled,
    /// The owner re-enabled the account.
    AccountEnabled,
    /// The account's password was set or changed.
    PasswordSet,
    /// The owner changed the account's permissions.
    PermissionsChanged,
    /// Every message of the account was deleted for good.
    MessagesDeleted,
    /// One conversation was deleted for good from the trash.
    ConversationDeleted,
    /// The trash was emptied.
    TrashEmptied,
    /// The account was deleted.
    AccountDeleted,
    /// The owner let strangers create accounts.
    RegistrationOpened,
    /// The owner stopped strangers creating accounts.
    RegistrationClosed,
    /// The holder made an API token.
    ApiTokenCreated,
    /// The holder deleted an API token.
    ApiTokenDeleted,
    /// The holder loaded an address book.
    AddressBookLoaded,
    /// The holder exported the address book.
    AddressBookExported,
    /// An Import Run, read from `imports`.
    ImportRun,
    /// An Export Run, read from `exports`.
    ExportRun,
}

impl AuditAction {
    /// The value as the wire and the database spell it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LoggedIn => "logged_in",
            Self::SessionEnded => "session_ended",
            Self::LoginRefused => "login_refused",
            Self::AccountCreated => "account_created",
            Self::AccountDisabled => "account_disabled",
            Self::AccountEnabled => "account_enabled",
            Self::PasswordSet => "password_set",
            Self::PermissionsChanged => "permissions_changed",
            Self::MessagesDeleted => "messages_deleted",
            Self::ConversationDeleted => "conversation_deleted",
            Self::TrashEmptied => "trash_emptied",
            Self::AccountDeleted => "account_deleted",
            Self::RegistrationOpened => "registration_opened",
            Self::RegistrationClosed => "registration_closed",
            Self::ApiTokenCreated => "api_token_created",
            Self::ApiTokenDeleted => "api_token_deleted",
            Self::AddressBookLoaded => "address_book_loaded",
            Self::AddressBookExported => "address_book_exported",
            Self::ImportRun => "import_run",
            Self::ExportRun => "export_run",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(value.to_string())).ok()
    }
}

/// Who acted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AuditActor {
    /// The owner, through Message Crate.
    Owner,
    /// The account's own holder: the person logged in to it, or a program
    /// holding one of its API tokens.
    Holder,
    /// A command run on the server, such as `reset-owner-password`.
    CommandLine,
    /// The server on its own: building the Demo Account, removing a build
    /// that failed, or a session running out.
    Server,
    /// Someone at the login card, before any login: a refused login, or a
    /// stranger creating an account.
    Anonymous,
}

impl AuditActor {
    /// The value as the database spells it.
    fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Holder => "holder",
            Self::CommandLine => "command_line",
            Self::Server => "server",
            Self::Anonymous => "anonymous",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "owner" => Self::Owner,
            "holder" => Self::Holder,
            "command_line" => Self::CommandLine,
            "anonymous" => Self::Anonymous,
            _ => Self::Server,
        }
    }

    /// The person logged in to `account_id` acting on it: the owner on the
    /// owner's account, its holder on any other.
    #[must_use]
    pub fn logged_in_as(account_id: i64) -> Self {
        if account_profile::is_server_owner(account_id) {
            Self::Owner
        } else {
            Self::Holder
        }
    }
}

/// How a Session ended, or why a login was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AuditReason {
    /// `session_ended`: the holder logged out.
    LoggedOut,
    /// `session_ended`: a new login took the account's one Session.
    Replaced,
    /// `session_ended`: the server ended it, as `reset-owner-password` does.
    Revoked,
    /// `session_ended`: it ran out. Never stored: read from a `logged_in`
    /// entry with no end once its expiry has passed.
    Expired,
    /// `login_refused`: no account has the username.
    UnknownUsername,
    /// `login_refused`: the password was wrong.
    WrongPassword,
    /// `login_refused`: the password was right and the account is disabled.
    AccountDisabled,
}

impl AuditReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::LoggedOut => "logged_out",
            Self::Replaced => "replaced",
            Self::Revoked => "revoked",
            Self::Expired => "expired",
            Self::UnknownUsername => "unknown_username",
            Self::WrongPassword => "wrong_password",
            Self::AccountDisabled => "account_disabled",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(value.to_string())).ok()
    }
}

/// What started a run: a Session or an API token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunCredential {
    /// A logged-in Session; `app` and `app_version` name the app when the
    /// request did.
    Session,
    /// An API token; `api_token_label` and `api_token_hint` name it as it
    /// was when the run started.
    ApiToken,
}

/// The credential a request came with, as the Audit Trail records it on a
/// run: never the token itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialUsed {
    /// A Session, and the app the request named, if it named one.
    Session(Option<ConnectingApp>),
    /// An API token's label and masked hint.
    ApiToken {
        /// The token's label.
        label: String,
        /// The token's masked hint, such as `mc-api-Sd..mE`.
        hint: String,
    },
}

/// The counts and names an entry carries beyond who, what and when, stored as
/// `audit_entries.details`. Every field is optional; each action fills the
/// ones that describe it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Details {
    /// `permissions_changed`: permissions turned on (`import`, `export`, `delete`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions_added: Option<Vec<String>>,
    /// `permissions_changed`: permissions turned off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions_removed: Option<Vec<String>>,
    /// Conversations deleted for good.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversations: Option<i64>,
    /// Attachments deleted for good.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments: Option<i64>,
    /// Contacts: forgotten when the trash was emptied, or written to an
    /// exported address book.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contacts: Option<i64>,
    /// Identities written to an exported address book.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identities: Option<i64>,
    /// `address_book_loaded`: `append` or `edit`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// `address_book_loaded`: contacts the load made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contacts_created: Option<i64>,
    /// `address_book_loaded`: contacts the load renamed or changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contacts_updated: Option<i64>,
    /// `address_book_loaded`: contacts the load removed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contacts_deleted: Option<i64>,
    /// `api_token_created` and `api_token_deleted`: the token's label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_token_label: Option<String>,
    /// `api_token_created` and `api_token_deleted`: the token's masked hint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_token_hint: Option<String>,
}

/// One entry of the Audit Trail as the interface hands it out: an entry of
/// `audit_entries`, an Import Run, or an Export Run. `id` is the row's id in
/// its own table, so `action` and `id` together name an entry. Fields that do
/// not describe the action are left out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct AuditEntry {
    /// The row's id: the entry's, the Import Run's or the Export Run's. For a
    /// session that expired, the id of its `logged_in` entry.
    pub id: i64,
    /// What happened.
    pub action: AuditAction,
    /// When, RFC 3339 UTC: the run's start for a run, the expiry for a
    /// session that expired.
    pub at: String,
    /// Who acted.
    pub actor: AuditActor,
    /// The account the entry is about, or `null` when it is about none, or
    /// the account has been deleted.
    pub account_id: Option<i64>,
    /// The username of the account the entry is about, as it was; for a
    /// refused login, the username as typed. Kept after the account is deleted.
    pub username: Option<String>,
    /// `session_ended`: how the Session ended. `login_refused`: why.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<AuditReason>,
    /// The app the request named, `desktop` or `website`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<AppKind>,
    /// That app's Build, such as `0.9.0+343fe0d8`. Present exactly when `app` is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_version: Option<String>,
    /// A run: what started it. Absent for a run the server started itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<RunCredential>,
    /// The API token's label as it was then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_token_label: Option<String>,
    /// The API token's masked hint as it was then, such as `mc-api-Sd..mE`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_token_hint: Option<String>,
    /// `permissions_changed`: permissions turned on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions_added: Option<Vec<String>>,
    /// `permissions_changed`: permissions turned off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions_removed: Option<Vec<String>>,
    /// A run: its status as the run's own list spells it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// An Import Run: the source it imported, such as `imessage`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// An Export Run: `everything`, `query` or `selection`. Never the query
    /// or the picked ids.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_kind: Option<String>,
    /// An Export Run with a query: the list it was for, `conversations` or
    /// `messages`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_list: Option<String>,
    /// Messages a run accepted or matched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages: Option<i64>,
    /// Conversations an Export Run matched, or deleted for good.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversations: Option<i64>,
    /// Attachments a run accepted or matched, or deleted for good.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments: Option<i64>,
    /// Bytes an Import Run uploaded, or an Export Run's attachments total.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<i64>,
    /// Contacts forgotten when the trash was emptied, or written to an
    /// exported address book.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contacts: Option<i64>,
    /// Identities written to an exported address book.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identities: Option<i64>,
    /// `address_book_loaded`: `append` or `edit`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// `address_book_loaded`: contacts made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contacts_created: Option<i64>,
    /// `address_book_loaded`: contacts renamed or changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contacts_updated: Option<i64>,
    /// `address_book_loaded`: contacts removed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contacts_deleted: Option<i64>,
}

impl AuditEntry {
    /// An entry saying who did what to which account, and when, with every
    /// count and name left out.
    fn new(
        id: i64,
        action: AuditAction,
        at: String,
        actor: AuditActor,
        account_id: Option<i64>,
        username: Option<String>,
    ) -> Self {
        Self {
            id,
            action,
            at,
            actor,
            account_id,
            username,
            reason: None,
            app: None,
            app_version: None,
            credential: None,
            api_token_label: None,
            api_token_hint: None,
            permissions_added: None,
            permissions_removed: None,
            status: None,
            source: None,
            scope_kind: None,
            scope_list: None,
            messages: None,
            conversations: None,
            attachments: None,
            bytes: None,
            contacts: None,
            identities: None,
            mode: None,
            contacts_created: None,
            contacts_updated: None,
            contacts_deleted: None,
        }
    }
}

/// An entry to write.
#[derive(Debug, Clone)]
pub struct NewEntry<'a> {
    /// What happened.
    pub action: AuditAction,
    /// Who acted.
    pub actor: AuditActor,
    /// The account it is about, or `None` when it is about no account.
    pub account_id: Option<i64>,
    /// That account's username; for a refused login, the username as typed.
    pub username: Option<&'a str>,
    /// `session_ended` and `login_refused`: how or why.
    pub reason: Option<AuditReason>,
    /// The app the request named.
    pub app: Option<&'a ConnectingApp>,
    /// Counts and names.
    pub details: Details,
}

impl<'a> NewEntry<'a> {
    /// An entry about no account, such as the owner opening registration.
    #[must_use]
    pub fn about_no_account(action: AuditAction, actor: AuditActor) -> Self {
        Self {
            action,
            actor,
            account_id: None,
            username: None,
            reason: None,
            app: None,
            details: Details::default(),
        }
    }

    /// An entry about `account`, with nothing else to say.
    #[must_use]
    pub fn about(action: AuditAction, actor: AuditActor, account: (i64, &'a str)) -> Self {
        Self {
            action,
            actor,
            account_id: Some(account.0),
            username: Some(account.1),
            reason: None,
            app: None,
            details: Details::default(),
        }
    }

    /// The same entry carrying `details`.
    #[must_use]
    pub fn with_details(mut self, details: Details) -> Self {
        self.details = details;
        self
    }
}

/// The time an entry is stamped with, written as `imports.started_at` is, so
/// entries and runs sort together.
fn now_text() -> String {
    Utc::now().to_rfc3339()
}

/// Write one entry and return its id.
///
/// # Errors
///
/// Returns an error when the insert fails.
pub async fn record(conn: &mut SqliteConnection, entry: &NewEntry<'_>) -> Result<i64> {
    insert(conn, entry, None, None).await
}

/// Write one entry about the account `account_id`, looking its username up.
/// Nothing is written when the account does not exist.
///
/// # Errors
///
/// Returns an error when the lookup or the insert fails.
pub async fn record_about(
    conn: &mut SqliteConnection,
    action: AuditAction,
    actor: AuditActor,
    account_id: i64,
    details: Details,
) -> Result<()> {
    let Some(username) = account_profile::username_for_account(conn, account_id).await? else {
        return Ok(());
    };
    record(
        conn,
        &NewEntry::about(action, actor, (account_id, &username)).with_details(details),
    )
    .await?;
    Ok(())
}

async fn insert(
    conn: &mut SqliteConnection,
    entry: &NewEntry<'_>,
    session_entry_id: Option<i64>,
    session_expires_at: Option<&str>,
) -> Result<i64> {
    let details = (entry.details != Details::default())
        .then(|| serde_json::to_string(&entry.details))
        .transpose()?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO audit_entries (
            at, action, actor, account_id, username, reason, app_kind, app_build,
            session_entry_id, session_expires_at, details
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
         RETURNING id",
    )
    .bind(now_text())
    .bind(entry.action.as_str())
    .bind(entry.actor.as_str())
    .bind(entry.account_id)
    .bind(entry.username)
    .bind(entry.reason.map(AuditReason::as_str))
    .bind(entry.app.map(|app| app.kind.as_str()))
    .bind(entry.app.map(|app| app.build.as_str()))
    .bind(session_entry_id)
    .bind(session_expires_at)
    .bind(details)
    .fetch_one(&mut *conn)
    .await
    .with_context(|| format!("record the Audit Trail entry {}", entry.action.as_str()))?;
    Ok(id)
}

/// The RFC 3339 form of a Unix-seconds expiry, as `audit_entries` stores it.
fn expiry_text(expires_at_unix: u64) -> String {
    let secs = i64::try_from(expires_at_unix).unwrap_or(i64::MAX);
    chrono::DateTime::from_timestamp(secs, 0)
        .unwrap_or(chrono::DateTime::<Utc>::MAX_UTC)
        .to_rfc3339()
}

/// Write the `logged_in` entry for a Session that expires at
/// `expires_at_unix`, and return its id for the session row to keep.
///
/// # Errors
///
/// Returns an error when the insert fails.
pub async fn record_login(
    conn: &mut SqliteConnection,
    account: (i64, &str),
    app: Option<&ConnectingApp>,
    expires_at_unix: u64,
) -> Result<i64> {
    let entry = NewEntry {
        app,
        ..NewEntry::about(
            AuditAction::LoggedIn,
            AuditActor::logged_in_as(account.0),
            account,
        )
    };
    insert(conn, &entry, None, Some(&expiry_text(expires_at_unix))).await
}

/// Move a `logged_in` entry's expiry, when a password change renews its
/// Session. The one change ever made to an entry: it keeps the entry's
/// account of when the Session runs out true, and says nothing new.
///
/// # Errors
///
/// Returns an error when the update fails.
pub async fn renew_login(
    conn: &mut SqliteConnection,
    login_entry_id: i64,
    expires_at_unix: u64,
) -> Result<()> {
    sqlx::query(
        "UPDATE audit_entries SET session_expires_at = $1 WHERE id = $2 AND action = 'logged_in'",
    )
    .bind(expiry_text(expires_at_unix))
    .bind(login_entry_id)
    .execute(&mut *conn)
    .await
    .context("renew the Audit Trail's login entry")?;
    Ok(())
}

/// Write the `session_ended` entry for the Session whose `logged_in` entry is
/// `login_entry_id`.
///
/// # Errors
///
/// Returns an error when the lookup or the insert fails.
pub async fn record_session_end(
    conn: &mut SqliteConnection,
    login_entry_id: i64,
    reason: AuditReason,
    actor: AuditActor,
) -> Result<()> {
    let login: Option<(Option<i64>, Option<String>)> =
        sqlx::query_as("SELECT account_id, username FROM audit_entries WHERE id = $1")
            .bind(login_entry_id)
            .fetch_optional(&mut *conn)
            .await?;
    let Some((account_id, username)) = login else {
        return Ok(());
    };
    let entry = NewEntry {
        action: AuditAction::SessionEnded,
        actor,
        account_id,
        username: username.as_deref(),
        reason: Some(reason),
        app: None,
        details: Details::default(),
    };
    insert(conn, &entry, Some(login_entry_id), None).await?;
    Ok(())
}

/// Write the entry for a refused login. `account` is the account the
/// username named, when it named one; otherwise the username is kept as
/// typed, cut to the username length limit, and belongs to no account.
///
/// # Errors
///
/// Returns an error when the insert fails.
pub async fn record_refused_login(
    conn: &mut SqliteConnection,
    typed_username: &str,
    account: Option<(i64, &str)>,
    reason: AuditReason,
    app: Option<&ConnectingApp>,
) -> Result<()> {
    let typed: String = typed_username
        .chars()
        .take(MAX_TYPED_USERNAME_CHARS)
        .collect();
    let entry = NewEntry {
        action: AuditAction::LoginRefused,
        actor: AuditActor::Anonymous,
        account_id: account.map(|(id, _)| id),
        username: Some(account.map_or(typed.as_str(), |(_, username)| username)),
        reason: Some(reason),
        app,
        details: Details::default(),
    };
    insert(conn, &entry, None, None).await?;
    Ok(())
}

/// Delete the refused logins as a username no account held that are older
/// than [`UNKNOWN_USERNAME_RETENTION_DAYS`]. Run when a login is handled, so
/// the record of them stays bounded with no sweeper. Nothing else in the
/// Audit Trail is ever deleted.
///
/// # Errors
///
/// Returns an error when the delete fails.
pub async fn trim_refused_logins(conn: &mut SqliteConnection) -> Result<u64> {
    let cutoff =
        (Utc::now() - chrono::Duration::days(UNKNOWN_USERNAME_RETENTION_DAYS)).to_rfc3339();
    let deleted = sqlx::query(
        "DELETE FROM audit_entries
         WHERE action = 'login_refused' AND reason = 'unknown_username' AND at < $1",
    )
    .bind(cutoff)
    .execute(&mut *conn)
    .await
    .context("trim refused logins for unknown usernames")?
    .rows_affected();
    Ok(deleted)
}

/// Record what started a run on its row: a Session and the app it named, or
/// an API token's label and hint as they are now.
///
/// # Errors
///
/// Returns an error when the update fails.
pub async fn record_run_credential(
    conn: &mut SqliteConnection,
    run: Run,
    run_id: i64,
    credential: &CredentialUsed,
) -> Result<()> {
    let (kind, app, label, hint) = match credential {
        CredentialUsed::Session(app) => ("session", app.as_ref(), None, None),
        CredentialUsed::ApiToken { label, hint } => {
            ("api_token", None, Some(label.as_str()), Some(hint.as_str()))
        }
    };
    let table = run.table();
    sqlx::query(&format!(
        "UPDATE {table} SET credential = $1, app_kind = $2, app_build = $3,
                api_token_label = $4, api_token_hint = $5
         WHERE id = $6"
    ))
    .bind(kind)
    .bind(app.map(|app| app.kind.as_str()))
    .bind(app.map(|app| app.build.as_str()))
    .bind(label)
    .bind(hint)
    .bind(run_id)
    .execute(&mut *conn)
    .await
    .with_context(|| format!("record what started {table} {run_id}"))?;
    Ok(())
}

/// The two kinds of run the Audit Trail reads from their own tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Run {
    /// An Import Run, in `imports`.
    Import,
    /// An Export Run, in `exports`.
    Export,
}

impl Run {
    /// The table the run lives in. A compile-time name, never a request's.
    fn table(self) -> &'static str {
        match self {
            Self::Import => "imports",
            Self::Export => "exports",
        }
    }
}

/// Make the account's record ready to outlive it, just before its row is
/// deleted, and write the `account_deleted` entry. Returns false, writing
/// nothing, when the account does not exist.
///
/// Its runs keep its username. A run still open is closed as `cancelled`, and
/// an Export Run's list of messages goes. What describes the person's
/// messages goes too: an Export Run's search text and picked ids, and an
/// Import Run's issues, form, staging folder, source details and the
/// addresses the backup sent from. The counts stay. The delete that follows
/// sets `account_id` NULL on the runs and entries.
///
/// # Errors
///
/// Returns an error when a statement fails.
pub async fn prepare_account_deletion(
    conn: &mut SqliteConnection,
    account_id: i64,
    actor: AuditActor,
) -> Result<bool> {
    let Some(username) = account_profile::username_for_account(conn, account_id).await? else {
        return Ok(false);
    };
    let now = now_text();
    sqlx::query(
        "UPDATE imports SET status = 'cancelled', finished_at = $2, stage = NULL
         WHERE account_id = $1 AND status = 'running'",
    )
    .bind(account_id)
    .bind(&now)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "DELETE FROM import_issues
         WHERE import_id IN (SELECT id FROM imports WHERE account_id = $1)",
    )
    .bind(account_id)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "UPDATE imports SET username = $2, form_json = NULL, staging_dir = NULL,
                source_fingerprint = NULL, source_identities = NULL, summary_json = NULL
         WHERE account_id = $1",
    )
    .bind(account_id)
    .bind(&username)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "UPDATE exports SET status = 'cancelled', finished_at = $2
         WHERE account_id = $1 AND status = 'running'",
    )
    .bind(account_id)
    .bind(&now)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "DELETE FROM export_messages
         WHERE export_id IN (SELECT id FROM exports WHERE account_id = $1)",
    )
    .bind(account_id)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "UPDATE exports SET username = $2, scope_query = NULL,
                scope_conversation_ids = NULL, scope_message_ids = NULL
         WHERE account_id = $1",
    )
    .bind(account_id)
    .bind(&username)
    .execute(&mut *conn)
    .await?;
    record(
        conn,
        &NewEntry::about(AuditAction::AccountDeleted, actor, (account_id, &username)),
    )
    .await?;
    Ok(true)
}

/// Which entries a read covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Every entry: the owner's Audit Trail.
    All,
    /// The entries about one account, whoever acted.
    Account(i64),
}

/// The union of the trail's sources as `(src, id, at, account_id)` rows.
/// `src` is `e` for an entry, `x` for a session that expired, `i` for an
/// Import Run and `o` for an Export Run.
const SOURCES_SQL: &str = "
    SELECT 'e' AS src, id, at, account_id FROM audit_entries
    UNION ALL
    SELECT 'x', l.id, l.session_expires_at, l.account_id FROM audit_entries l
     WHERE l.action = 'logged_in' AND l.session_expires_at <= $1
       AND NOT EXISTS (SELECT 1 FROM audit_entries e WHERE e.session_entry_id = l.id)
    UNION ALL
    SELECT 'i', id, started_at, account_id FROM imports
    UNION ALL
    SELECT 'o', id, started_at, account_id FROM exports";

/// One page of the Audit Trail, newest first, and how many entries it holds
/// in all.
///
/// # Errors
///
/// Returns an error when a statement fails or a stored row cannot be read.
pub async fn page(
    conn: &mut SqliteConnection,
    scope: Scope,
    limit: usize,
    offset: usize,
) -> Result<(Vec<AuditEntry>, u64)> {
    let now = now_text();
    let (filter, account) = match scope {
        Scope::All => ("", None),
        Scope::Account(id) => ("WHERE account_id = $2", Some(id)),
    };
    let count_sql = format!("SELECT COUNT(*) FROM ({SOURCES_SQL}) {filter}");
    let mut count = sqlx::query_scalar::<_, i64>(&count_sql).bind(&now);
    if let Some(id) = account {
        count = count.bind(id);
    }
    let total = count.fetch_one(&mut *conn).await?;

    let page_sql = format!(
        "SELECT src, id FROM ({SOURCES_SQL}) {filter}
         ORDER BY at DESC, src DESC, id DESC LIMIT {limit} OFFSET {offset}"
    );
    let mut rows = sqlx::query_as::<_, (String, i64)>(&page_sql).bind(&now);
    if let Some(id) = account {
        rows = rows.bind(id);
    }
    let rows = rows.fetch_all(&mut *conn).await?;

    let mut items = Vec::with_capacity(rows.len());
    for (src, id) in rows {
        let item = match src.as_str() {
            "e" => load_entry(conn, id).await?,
            "x" => load_expiry(conn, id).await?,
            "i" => load_import(conn, id).await?,
            _ => load_export(conn, id).await?,
        };
        if let Some(item) = item {
            items.push(item);
        }
    }
    Ok((items, u64::try_from(total).unwrap_or(0)))
}

/// An entry of `audit_entries` as the interface hands it out.
async fn load_entry(conn: &mut SqliteConnection, id: i64) -> Result<Option<AuditEntry>> {
    let row = sqlx::query(
        "SELECT id, at, action, actor, account_id, username, reason, app_kind, app_build,
                details
         FROM audit_entries WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(row) = row else { return Ok(None) };
    let details: Option<String> = row.try_get("details")?;
    let details: Details = match details {
        Some(text) => serde_json::from_str(&text).context("audit entry details are not JSON")?,
        None => Details::default(),
    };
    let reason: Option<String> = row.try_get("reason")?;
    let action: String = row.try_get("action")?;
    let actor: String = row.try_get("actor")?;
    let app_kind: Option<String> = row.try_get("app_kind")?;
    let app_build: Option<String> = row.try_get("app_build")?;
    let app = app_kind.as_deref().and_then(AppKind::parse);
    let action = AuditAction::parse(&action)
        .with_context(|| format!("audit entry {id} has an unknown action {action}"))?;
    Ok(Some(AuditEntry {
        reason: reason.as_deref().and_then(AuditReason::parse),
        app,
        app_version: app.and(app_build),
        api_token_label: details.api_token_label,
        api_token_hint: details.api_token_hint,
        permissions_added: details.permissions_added,
        permissions_removed: details.permissions_removed,
        conversations: details.conversations,
        attachments: details.attachments,
        contacts: details.contacts,
        identities: details.identities,
        mode: details.mode,
        contacts_created: details.contacts_created,
        contacts_updated: details.contacts_updated,
        contacts_deleted: details.contacts_deleted,
        ..AuditEntry::new(
            row.try_get("id")?,
            action,
            row.try_get("at")?,
            AuditActor::parse(&actor),
            row.try_get("account_id")?,
            row.try_get("username")?,
        )
    }))
}

/// The `session_ended` entry read, not stored, for a Session that ran out:
/// its `logged_in` entry's account, at its expiry.
async fn load_expiry(conn: &mut SqliteConnection, login_id: i64) -> Result<Option<AuditEntry>> {
    let row: Option<(i64, String, Option<i64>, Option<String>)> = sqlx::query_as(
        "SELECT id, session_expires_at, account_id, username FROM audit_entries WHERE id = $1",
    )
    .bind(login_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(|(id, at, account_id, username)| AuditEntry {
        reason: Some(AuditReason::Expired),
        ..AuditEntry::new(
            id,
            AuditAction::SessionEnded,
            at,
            AuditActor::Server,
            account_id,
            username,
        )
    }))
}

/// The account a run is about, and the username to show: the account's while
/// it exists, the one written on the run once it is deleted.
const RUN_ACCOUNT_SQL: &str =
    "r.account_id, COALESCE(a.username, r.username) AS username, r.credential, r.app_kind,
     r.app_build, r.api_token_label, r.api_token_hint";

/// Fill the fields every run has: whose it is and what started it.
fn run_entry(row: &sqlx::sqlite::SqliteRow, action: AuditAction) -> Result<AuditEntry> {
    let credential: Option<String> = row.try_get("credential")?;
    let credential = match credential.as_deref() {
        Some("session") => Some(RunCredential::Session),
        Some("api_token") => Some(RunCredential::ApiToken),
        _ => None,
    };
    let app_kind: Option<String> = row.try_get("app_kind")?;
    let app_build: Option<String> = row.try_get("app_build")?;
    let app = app_kind.as_deref().and_then(AppKind::parse);
    let actor = if credential.is_some() {
        let account_id: Option<i64> = row.try_get("account_id")?;
        account_id.map_or(AuditActor::Holder, AuditActor::logged_in_as)
    } else {
        AuditActor::Server
    };
    Ok(AuditEntry {
        app,
        app_version: app.and(app_build),
        credential,
        api_token_label: row.try_get("api_token_label")?,
        api_token_hint: row.try_get("api_token_hint")?,
        status: row.try_get("status")?,
        ..AuditEntry::new(
            row.try_get("id")?,
            action,
            row.try_get("started_at")?,
            actor,
            row.try_get("account_id")?,
            row.try_get("username")?,
        )
    })
}

/// An Import Run as an Audit Trail entry: its source, status and counts.
async fn load_import(conn: &mut SqliteConnection, id: i64) -> Result<Option<AuditEntry>> {
    let row = sqlx::query(&format!(
        "SELECT r.id, r.started_at, r.status, r.source, r.message_count, r.attachment_count,
                r.bytes_uploaded, {RUN_ACCOUNT_SQL}
         FROM imports r LEFT JOIN accounts a ON a.id = r.account_id
         WHERE r.id = $1"
    ))
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(row) = row else { return Ok(None) };
    Ok(Some(AuditEntry {
        source: row.try_get("source")?,
        messages: row.try_get("message_count")?,
        attachments: row.try_get("attachment_count")?,
        bytes: row.try_get("bytes_uploaded")?,
        ..run_entry(&row, AuditAction::ImportRun)?
    }))
}

/// An Export Run as an Audit Trail entry: the kind of scope and how much it
/// matched, never the query or the picked ids.
async fn load_export(conn: &mut SqliteConnection, id: i64) -> Result<Option<AuditEntry>> {
    let row = sqlx::query(&format!(
        "SELECT r.id, r.started_at, r.status, r.scope_kind, r.scope_list, r.message_count,
                r.conversation_count, r.attachment_count, r.total_bytes, {RUN_ACCOUNT_SQL}
         FROM exports r LEFT JOIN accounts a ON a.id = r.account_id
         WHERE r.id = $1"
    ))
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(row) = row else { return Ok(None) };
    Ok(Some(AuditEntry {
        scope_kind: row.try_get("scope_kind")?,
        scope_list: row.try_get("scope_list")?,
        messages: row.try_get("message_count")?,
        conversations: row.try_get("conversation_count")?,
        attachments: row.try_get("attachment_count")?,
        bytes: row.try_get("total_bytes")?,
        ..run_entry(&row, AuditAction::ExportRun)?
    }))
}

#[cfg(test)]
mod tests;
