//! Account rows, profile fields, and message deletion.

use anyhow::{Context, Result, bail};
use message_ir::{HandleService, HandleType};
use sqlx::SqliteConnection;

use crate::db::begin_write;
use crate::db::handles::{normalize_handle, upsert_handle_row};
use crate::db::schema;

/// Contact points linked to an account, for profile display.
#[derive(Debug, Clone)]
pub struct AccountProfile {
    /// Email addresses linked to the account.
    pub emails: Vec<String>,
    /// Phone handles linked to the account.
    pub phones: Vec<String>,
}

/// Load the email and phone handles linked to an account. Both default to empty
/// when nothing is linked.
pub async fn load_account_profile(
    conn: &mut SqliteConnection,
    account_id: i64,
) -> Result<AccountProfile> {
    let emails = account_handle_addresses(conn, account_id, HandleType::Email).await?;
    let phones = account_handle_addresses(conn, account_id, HandleType::Phone).await?;
    Ok(AccountProfile { emails, phones })
}

/// The normalized addresses of the account's handles of `handle_type`, A to
/// Z, from `account_handles`.
async fn account_handle_addresses(
    conn: &mut SqliteConnection,
    account_id: i64,
    handle_type: HandleType,
) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar::<_, String>(
        "SELECT h.normalized FROM handles h
         JOIN account_handles ah ON ah.handle_id = h.id
         WHERE ah.account_id = $1 AND h.handle_type = $2
         ORDER BY h.normalized",
    )
    .bind(account_id)
    .bind(handle_type.as_str())
    .fetch_all(&mut *conn)
    .await?)
}

/// Ensure an `accounts` row exists at `account_id`, with the id as its stub
/// username. The demo reset and the tests use it to place a row at a chosen
/// id, and the import and export paths call it before they write for an
/// account; new accounts are made by [`insert_account`] and
/// [`insert_account_at`].
pub async fn ensure_account_row(conn: &mut SqliteConnection, account_id: i64) -> Result<()> {
    sqlx::query(
        "INSERT INTO accounts (id, username) VALUES ($1, $2)
         ON CONFLICT DO NOTHING",
    )
    .bind(account_id)
    .bind(account_id.to_string())
    .execute(&mut *conn)
    .await
    .with_context(|| format!("failed to ensure account row for {account_id}"))?;
    Ok(())
}

/// Ensure a `handles` row exists and link it to the account via `account_handles`.
/// Returns the handle id.
pub async fn link_account_handle(
    conn: &mut SqliteConnection,
    account_id: i64,
    raw: &str,
    handle_type: HandleType,
) -> Result<i64> {
    link_account_handle_with_service(conn, account_id, raw, handle_type, None).await
}

/// Like [`link_account_handle`], recording a platform `service`
/// (`phone` | `whatsapp`). Missing/`None` defaults to `phone`.
pub async fn link_account_handle_with_service(
    conn: &mut SqliteConnection,
    account_id: i64,
    raw: &str,
    handle_type: HandleType,
    service: Option<&str>,
) -> Result<i64> {
    let (handle_id, _) = upsert_handle_row(conn, account_id, raw, handle_type, service).await?;
    sqlx::query(
        "INSERT INTO account_handles (account_id, handle_id) VALUES ($1, $2)
         ON CONFLICT DO NOTHING",
    )
    .bind(account_id)
    .bind(handle_id)
    .execute(&mut *conn)
    .await?;
    Ok(handle_id)
}

/// The id of the account whose username is `username`, compared without
/// regard to case. `None` when no account has that username.
///
/// Login and the username-free check use this, never
/// [`lookup_account_ref`]: a username is what a person types, and an account
/// whose username happens to be digits must not be mistaken for an id.
pub async fn lookup_account_by_username(
    conn: &mut SqliteConnection,
    username: &str,
) -> Result<Option<i64>> {
    let username = username.trim();
    if username.is_empty() {
        return Ok(None);
    }
    schema::ensure_accounts_schema(conn).await?;
    let by_user: Option<i64> =
        sqlx::query_scalar("SELECT id FROM accounts WHERE username = $1 COLLATE NOCASE")
            .bind(username)
            .fetch_optional(&mut *conn)
            .await?;
    Ok(by_user)
}

/// Look up an existing account by id or by username (case-insensitive), for
/// a command line's `--account`. A reference that parses as an integer is
/// tried as an id first. `None` when no row matches.
pub async fn lookup_account_ref(
    conn: &mut SqliteConnection,
    account_ref: &str,
) -> Result<Option<i64>> {
    let account_ref = account_ref.trim();
    if let Ok(id) = account_ref.parse::<i64>() {
        schema::ensure_accounts_schema(conn).await?;
        let by_id: Option<i64> = sqlx::query_scalar("SELECT id FROM accounts WHERE id = $1")
            .bind(id)
            .fetch_optional(&mut *conn)
            .await?;
        if by_id.is_some() {
            return Ok(by_id);
        }
    }
    lookup_account_by_username(conn, account_ref).await
}

/// Resolve a command line's `--account` to `accounts.id`. Accepts an id or a
/// username; anything else is an error naming the reference.
pub async fn resolve_account_ref(conn: &mut SqliteConnection, account_ref: &str) -> Result<i64> {
    let account_ref = account_ref.trim();
    if account_ref.is_empty() {
        bail!("account is empty");
    }
    if let Some(id) = lookup_account_ref(conn, account_ref).await? {
        return Ok(id);
    }
    bail!("account not found: {account_ref} (use an existing username or account id)");
}

/// Username for an account id, if the row exists.
pub async fn username_for_account(
    conn: &mut SqliteConnection,
    account_id: i64,
) -> Result<Option<String>> {
    schema::ensure_accounts_schema(conn).await?;
    let name: Option<String> = sqlx::query_scalar("SELECT username FROM accounts WHERE id = $1")
        .bind(account_id)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(name)
}

/// Load the argon2 password hash for an account id, if set.
///
/// `None` when the row is missing or its `password_hash` is NULL (NULL or
/// empty means passwordless login).
pub async fn load_password_hash(
    conn: &mut SqliteConnection,
    account_id: i64,
) -> Result<Option<String>> {
    let hash: Option<Option<String>> =
        sqlx::query_scalar("SELECT password_hash FROM accounts WHERE id = $1")
            .bind(account_id)
            .fetch_optional(&mut *conn)
            .await?;
    Ok(hash.flatten())
}

/// Replace the argon2 password hash for an account.
pub async fn update_password_hash(
    conn: &mut SqliteConnection,
    account_id: i64,
    password_hash: Option<&str>,
) -> Result<()> {
    sqlx::query("UPDATE accounts SET password_hash = $1 WHERE id = $2")
        .bind(password_hash)
        .bind(account_id)
        .execute(&mut *conn)
        .await
        .with_context(|| format!("update password hash for {account_id}"))?;
    Ok(())
}

/// Record that the account logged in just now. Called by every route that
/// opens a Session for a person: login, claiming the server, and
/// registering. Rotating a token on a password change is not a login.
pub async fn record_login(conn: &mut SqliteConnection, account_id: i64) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    sqlx::query("UPDATE accounts SET last_login_at = $1 WHERE id = $2")
        .bind(now)
        .bind(account_id)
        .execute(&mut *conn)
        .await
        .with_context(|| format!("record login for {account_id}"))?;
    Ok(())
}

/// When the account last logged in, as stored, or `None` if it never has.
pub async fn load_last_login(
    conn: &mut SqliteConnection,
    account_id: i64,
) -> Result<Option<String>> {
    let at: Option<Option<String>> =
        sqlx::query_scalar("SELECT last_login_at FROM accounts WHERE id = $1")
            .bind(account_id)
            .fetch_optional(&mut *conn)
            .await?;
    Ok(at.flatten())
}

/// Permanently delete an account, `actor` acting. Its data rows are removed
/// by ON DELETE CASCADE (messages, conversations, contacts,
/// `account_handles/emails/api_tokens`). Its Audit Trail stays: entries and
/// runs are unlinked, keep its username, and gain an `account_deleted` entry
/// (`docs/adr/0020-the-audit-trail-outlives-the-account.md`). One
/// transaction, so the record and the deletion land together. Returns
/// whether there was an account to delete.
pub async fn delete_account(
    conn: &mut SqliteConnection,
    account_id: i64,
    actor: crate::db::audit_trail::AuditActor,
) -> Result<bool> {
    let mut tx = begin_write(conn).await?;
    let existed =
        crate::db::audit_trail::prepare_account_deletion(&mut tx, account_id, actor).await?;
    sqlx::query("DELETE FROM accounts WHERE id = $1")
        .bind(account_id)
        .execute(&mut *tx)
        .await
        .with_context(|| format!("delete account {account_id}"))?;
    tx.commit().await?;
    Ok(existed)
}

/// Stable id for the seeded demo account (`reset-demo`).
pub const DEMO_ACCOUNT_ID: i64 = 2;

/// What the Demo Account may do, known from its id and never read from its
/// row: export, and neither import nor delete for good
/// (`docs/adr/0016-the-demo-account-is-fixed-not-configured.md`).
/// [`load_account_auth`] answers it for the Demo Account, so the profile and
/// the owner's account list report what the guards, which refuse the Demo
/// Account by its id, enforce: a row changed by hand cannot make a screen
/// offer what the server refuses.
pub const DEMO_ACCOUNT_PERMISSIONS: crate::db::permissions::Permissions =
    crate::db::permissions::Permissions {
        import: false,
        export: true,
        delete: false,
    };

/// The Demo Account's username. It stays reserved while the Demo Account is
/// absent, so no other account can take it and block the next build.
pub const DEMO_USERNAME: &str = "demo";

/// True when `account_id` is the seeded demo account.
pub fn is_demo_account(account_id: i64) -> bool {
    account_id == DEMO_ACCOUNT_ID
}

/// Stable id for the owner. There is one owner or none, and this id
/// is what makes "one" structural: there is no flag to set, no second owner to
/// create, and nothing to promote. A database holding no row at this id is
/// unclaimed. See `docs/adr/0008-the-owner-holds-no-messages.md`.
pub const OWNER_ACCOUNT_ID: i64 = 1;

/// True when `account_id` is the owner.
pub fn is_server_owner(account_id: i64) -> bool {
    account_id == OWNER_ACCOUNT_ID
}

/// True when there is an owner: this Message Crate is claimed.
pub async fn is_claimed(conn: &mut SqliteConnection) -> Result<bool> {
    schema::ensure_accounts_schema(conn).await?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts WHERE id = $1")
        .bind(OWNER_ACCOUNT_ID)
        .fetch_one(&mut *conn)
        .await?;
    Ok(count > 0)
}

/// An account's disabled flag, whether its holder still owes profile setup,
/// and its permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccountAuth {
    /// May not log in; existing sessions are refused.
    pub disabled: bool,
    /// The holder has not set up their profile; they must before going on.
    pub must_set_up_profile: bool,
    /// What this account may do.
    pub permissions: crate::db::permissions::Permissions,
}

/// Load one account's authorization row. `None` when the account is gone.
pub async fn load_account_auth(
    conn: &mut SqliteConnection,
    account_id: i64,
) -> Result<Option<AccountAuth>> {
    schema::ensure_accounts_schema(conn).await?;
    let row: Option<(i64, i64, i64, i64, i64)> = sqlx::query_as(
        "SELECT disabled, must_set_up_profile, can_import, can_export, can_delete
         FROM accounts WHERE id = $1",
    )
    .bind(account_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(
        |(disabled, must_set_up, import, export, delete)| AccountAuth {
            disabled: disabled != 0,
            must_set_up_profile: must_set_up != 0,
            permissions: if is_demo_account(account_id) {
                DEMO_ACCOUNT_PERMISSIONS
            } else {
                crate::db::permissions::Permissions::from_ints(import, export, delete)
            },
        },
    ))
}

/// Mark an account as still owing profile setup, or clear the mark once its
/// holder has saved one.
///
/// The server says whether setup is owed, rather than each client deciding for
/// itself from an empty-looking profile. A rule the client owns is a rule that
/// drifts, and the answer has to survive cleared site data and a second
/// browser.
pub async fn set_must_set_up_profile(
    conn: &mut SqliteConnection,
    account_id: i64,
    must_set_up: bool,
) -> Result<()> {
    schema::ensure_accounts_schema(conn).await?;
    sqlx::query("UPDATE accounts SET must_set_up_profile = $1 WHERE id = $2")
        .bind(i32::from(must_set_up))
        .bind(account_id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Counts from deleting one account's messages.
#[derive(Debug, Clone, Copy)]
pub struct DeletedMessagesStats {
    /// Conversations deleted (cascade removes their messages).
    pub conversations: u64,
    /// Attachment rows deleted (files on disk are removed by the caller).
    pub attachments: u64,
}

/// Permanently delete one account's conversations (cascades to messages,
/// attachments, participants, tapbacks), staging rows, and trash markers,
/// in one write transaction: all of it is deleted or none of it.
/// Contacts, groups, login details, and import tokens are retained.
pub async fn delete_all_messages_for_account(
    conn: &mut SqliteConnection,
    account_id: i64,
    actor: crate::db::audit_trail::AuditActor,
) -> Result<DeletedMessagesStats> {
    schema::ensure_schema(conn).await?;
    let mut tx = begin_write(conn).await?;
    let attachment_count: i64 = sqlx::query_scalar(
        r"
        SELECT COUNT(*)
        FROM attachments a
        JOIN messages m ON m.id = a.message_id
        JOIN conversations c ON c.id = m.conversation_id
        WHERE c.account_id = $1
        ",
    )
    .bind(account_id)
    .fetch_one(&mut *tx)
    .await?;
    let conversations = sqlx::query("DELETE FROM conversations WHERE account_id = $1")
        .bind(account_id)
        .execute(&mut *tx)
        .await
        .with_context(|| format!("delete conversations for {account_id}"))?
        .rows_affected();
    sqlx::query("DELETE FROM staging_conversations WHERE account_id = $1")
        .bind(account_id)
        .execute(&mut *tx)
        .await
        .with_context(|| format!("delete staging conversations for {account_id}"))?;
    crate::db::trash::purge_account(&mut tx, account_id)
        .await
        .with_context(|| format!("purge trash markers for {account_id}"))?;
    // Recorded in the delete's own transaction, so the two land together.
    crate::db::audit_trail::record_about(
        &mut tx,
        crate::db::audit_trail::AuditAction::MessagesDeleted,
        actor,
        account_id,
        crate::db::audit_trail::Details {
            conversations: Some(i64::try_from(conversations).unwrap_or(i64::MAX)),
            attachments: Some(attachment_count),
            ..crate::db::audit_trail::Details::default()
        },
    )
    .await?;
    tx.commit().await?;
    Ok(DeletedMessagesStats {
        conversations,
        attachments: u64::try_from(attachment_count).unwrap_or(0),
    })
}

/// The account's IANA time zone. UTC when the row is missing or the stored
/// name is not one chrono-tz knows, so a bad value degrades to Greenwich
/// rather than to an error on every list.
pub async fn load_time_zone(conn: &mut SqliteConnection, account_id: i64) -> Result<chrono_tz::Tz> {
    let name: Option<String> = sqlx::query_scalar("SELECT time_zone FROM accounts WHERE id = $1")
        .bind(account_id)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(name
        .and_then(|n| n.trim().parse::<chrono_tz::Tz>().ok())
        .unwrap_or(chrono_tz::UTC))
}

/// Store the account's time zone.
pub async fn set_time_zone(
    conn: &mut SqliteConnection,
    account_id: i64,
    zone: chrono_tz::Tz,
) -> Result<()> {
    sqlx::query("UPDATE accounts SET time_zone = $1 WHERE id = $2")
        .bind(zone.name())
        .bind(account_id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// The account's zone and today's date in it: what every search compile and
/// every year boundary needs.
pub async fn account_clock(
    conn: &mut SqliteConnection,
    account_id: i64,
) -> Result<(chrono_tz::Tz, chrono::NaiveDate)> {
    let zone = load_time_zone(conn, account_id).await?;
    Ok((zone, crate::search::today_in(zone)))
}

/// Load the `preferred_name` for an account, if set.
pub async fn load_preferred_name(
    conn: &mut SqliteConnection,
    account_id: i64,
) -> Result<Option<String>> {
    let name: Option<Option<String>> =
        sqlx::query_scalar("SELECT preferred_name FROM accounts WHERE id = $1")
            .bind(account_id)
            .fetch_optional(&mut *conn)
            .await?;
    Ok(name
        .flatten()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty()))
}

/// Set or clear an account's `preferred_name`.
///
/// # Errors
///
/// Returns an error when the statement fails.
pub async fn set_preferred_name(
    conn: &mut SqliteConnection,
    account_id: i64,
    name: Option<&str>,
) -> Result<()> {
    sqlx::query("UPDATE accounts SET preferred_name = $1 WHERE id = $2")
        .bind(name)
        .bind(account_id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// The flags only the owner sets on an account. `None` leaves a flag
/// as it is.
#[derive(Debug, Clone, Copy, Default)]
pub struct AccountFlags {
    /// May not log in.
    pub disabled: Option<bool>,
    /// May call the import endpoints.
    pub can_import: Option<bool>,
    /// May call the export endpoints.
    pub can_export: Option<bool>,
    /// May destroy message data.
    pub can_delete: Option<bool>,
}

/// Write the flags `flags` names onto an account.
///
/// # Errors
///
/// Returns an error when a statement fails.
pub async fn set_account_flags(
    conn: &mut SqliteConnection,
    account_id: i64,
    flags: AccountFlags,
) -> Result<()> {
    // One statement, so the flags change together or not at all. A flag
    // left out binds NULL and keeps the stored value.
    sqlx::query(
        "UPDATE accounts SET
             disabled = COALESCE($1, disabled),
             can_import = COALESCE($2, can_import),
             can_export = COALESCE($3, can_export),
             can_delete = COALESCE($4, can_delete)
         WHERE id = $5",
    )
    .bind(flags.disabled.map(i32::from))
    .bind(flags.can_import.map(i32::from))
    .bind(flags.can_export.map(i32::from))
    .bind(flags.can_delete.map(i32::from))
    .bind(account_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// How many accounts the server holds.
///
/// # Errors
///
/// Returns an error when the statement fails.
pub async fn count_accounts(conn: &mut SqliteConnection) -> Result<i64> {
    Ok(sqlx::query_scalar("SELECT COUNT(*) FROM accounts")
        .fetch_one(&mut *conn)
        .await?)
}

/// One page of account ids: the owner's first, then the rest by
/// username.
///
/// # Errors
///
/// Returns an error when the statement fails.
pub async fn account_ids_page(
    conn: &mut SqliteConnection,
    limit: usize,
    offset: usize,
) -> Result<Vec<i64>> {
    Ok(sqlx::query_scalar(
        "SELECT id FROM accounts \
         ORDER BY CASE WHEN id = $1 THEN 0 ELSE 1 END, username LIMIT $2 OFFSET $3",
    )
    .bind(OWNER_ACCOUNT_ID)
    .bind(limit as i64)
    .bind(i64::try_from(offset)?)
    .fetch_all(&mut *conn)
    .await?)
}

/// Insert a new account row at the next generated id. All fields except
/// username are optional.
/// The new account gets every permission (`Permissions::all()`); narrow it
/// afterward if needed.
pub async fn insert_account(
    conn: &mut SqliteConnection,
    username: &str,
    password_hash: Option<&str>,
    preferred_name: Option<&str>,
) -> Result<i64> {
    schema::ensure_accounts_schema(conn).await?;
    // The id is chosen here rather than by the database's own generator,
    // because that generator would hand out 1 on an empty table, and 1 is
    // the owner. Ids below `FIRST_GENERATED_ACCOUNT_ID` belong to the accounts
    // the server makes itself; every other account takes the next id above
    // both that floor and the highest id the table has ever held. The column
    // is AUTOINCREMENT, so `sqlite_sequence` keeps that high mark after the
    // row is deleted: a deleted account's id never passes to a new account,
    // which would otherwise inherit any folder of files the delete left.
    let highest: Option<i64> =
        sqlx::query_scalar("SELECT seq FROM sqlite_sequence WHERE name = 'accounts'")
            .fetch_optional(&mut *conn)
            .await?;
    let id = highest.map_or(FIRST_GENERATED_ACCOUNT_ID, |highest| {
        highest.max(FIRST_GENERATED_ACCOUNT_ID - 1) + 1
    });
    insert_account_at(conn, id, username, password_hash, preferred_name).await?;
    Ok(id)
}

/// The first id [`insert_account`] hands out. Everything below it is reserved
/// for accounts the server makes itself: [`OWNER_ACCOUNT_ID`] and
/// [`DEMO_ACCOUNT_ID`].
pub const FIRST_GENERATED_ACCOUNT_ID: i64 = 100;

/// Insert an account at a fixed id: the owner at [`OWNER_ACCOUNT_ID`], and a
/// test's chosen row.
pub async fn insert_account_at(
    conn: &mut SqliteConnection,
    id: i64,
    username: &str,
    password_hash: Option<&str>,
    preferred_name: Option<&str>,
) -> Result<()> {
    schema::ensure_accounts_schema(conn).await?;
    sqlx::query(
        "INSERT INTO accounts (id, username, password_hash, preferred_name) VALUES ($1, $2, $3, $4)",
    )
    .bind(id)
    .bind(username)
    .bind(password_hash)
    .bind(preferred_name)
    .execute(&mut *conn)
    .await
    .with_context(|| format!("insert account {username} at id {id}"))?;
    Ok(())
}

/// Ensure a phone handle is linked to the account via `account_handles`.
pub async fn upsert_account_phone(
    conn: &mut SqliteConnection,
    account_id: i64,
    phone: &str,
) -> Result<()> {
    link_account_handle(conn, account_id, phone, HandleType::Phone).await?;
    Ok(())
}

/// Unlink one of the account's identities (`account_handles`): the linked
/// handle with this address and, for a phone number, this `service`. One
/// number can be linked twice, as a Text message identity and as a WhatsApp
/// identity, and removing one leaves the other.
///
/// An email address is one identity whatever service its `handles` row
/// records, so `service` is ignored for one. The `handles` row itself stays,
/// so conversation history stays intact. True when anything was unlinked.
pub async fn unlink_account_handle(
    conn: &mut SqliteConnection,
    account_id: i64,
    raw: &str,
    handle_type: HandleType,
    service: HandleService,
) -> Result<bool> {
    let (normalized, _) = normalize_handle(raw, handle_type);
    let is_email = matches!(handle_type, HandleType::Email);
    let service = (!is_email).then_some(service.as_str());
    let removed = sqlx::query(
        "DELETE FROM account_handles
         WHERE account_id = $1 AND handle_id IN (
             SELECT id FROM handles
             WHERE account_id = $1 AND normalized = $2 AND handle_type = $3
               AND ($4 IS NULL OR service = $4))",
    )
    .bind(account_id)
    .bind(normalized.as_str())
    .bind(handle_type.as_str())
    .bind(service)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    Ok(removed > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACCOUNT_ID: i64 = 7;

    /// An account's email addresses are its email identities: one linked in
    /// `account_handles` is in the profile's `emails`, with nothing else to
    /// write (#1027).
    #[tokio::test]
    async fn the_profile_lists_the_email_identities() {
        let fixture = crate::test_support::test_fixture().await;
        fixture.account_with_id(ACCOUNT_ID, "Alice").await;
        let mut conn = fixture.conn().await;
        link_account_handle(
            &mut conn,
            ACCOUNT_ID,
            "Alice@Example.com",
            HandleType::Email,
        )
        .await
        .unwrap();
        link_account_handle(&mut conn, ACCOUNT_ID, "+15555550100", HandleType::Phone)
            .await
            .unwrap();

        let profile = load_account_profile(&mut conn, ACCOUNT_ID).await.unwrap();

        assert_eq!(profile.emails, vec!["alice@example.com".to_string()]);
        assert_eq!(profile.phones, vec!["+15555550100".to_string()]);
    }

    #[tokio::test]
    async fn resolve_by_username_case_insensitive() {
        let fixture = crate::test_support::test_fixture().await;
        fixture.account_with_id(ACCOUNT_ID, "Alice").await;
        let mut conn = fixture.conn().await;
        assert_eq!(
            resolve_account_ref(&mut conn, "alice").await.unwrap(),
            ACCOUNT_ID
        );
        assert_eq!(
            resolve_account_ref(&mut conn, "ALICE").await.unwrap(),
            ACCOUNT_ID
        );
    }

    #[tokio::test]
    async fn resolve_by_id() {
        let fixture = crate::test_support::test_fixture().await;
        fixture.account_with_id(ACCOUNT_ID, "Alice").await;
        let mut conn = fixture.conn().await;
        assert_eq!(
            resolve_account_ref(&mut conn, &ACCOUNT_ID.to_string())
                .await
                .unwrap(),
            ACCOUNT_ID
        );
    }

    #[tokio::test]
    async fn a_username_made_of_digits_is_a_username_at_login() {
        let fixture = crate::test_support::test_fixture().await;
        fixture.account_with_id(ACCOUNT_ID, "Alice").await;
        let digits = fixture.account("7").await;
        let mut conn = fixture.conn().await;
        // `--account 7` names the id; logging in as "7" names the username.
        assert_eq!(
            resolve_account_ref(&mut conn, "7").await.unwrap(),
            ACCOUNT_ID
        );
        assert_eq!(
            lookup_account_by_username(&mut conn, "7").await.unwrap(),
            Some(digits)
        );
    }

    #[tokio::test]
    async fn generated_ids_start_above_the_reserved_range_and_climb() {
        let fixture = crate::test_support::test_fixture().await;
        let mut conn = fixture.conn().await;
        // On an empty table the first generated id is the floor, never 1.
        let first = insert_account(&mut conn, "alice", None, None)
            .await
            .unwrap();
        assert_eq!(first, FIRST_GENERATED_ACCOUNT_ID);
        // The owner and the demo account still fit below it afterwards.
        insert_account_at(&mut conn, OWNER_ACCOUNT_ID, "owner", None, None)
            .await
            .unwrap();
        insert_account_at(&mut conn, DEMO_ACCOUNT_ID, "demo", None, None)
            .await
            .unwrap();
        let next = insert_account(&mut conn, "bob", None, None).await.unwrap();
        assert_eq!(next, first + 1);
        // A fixed id above the floor is never handed out twice.
        insert_account_at(&mut conn, 500, "carol", None, None)
            .await
            .unwrap();
        assert_eq!(
            insert_account(&mut conn, "dave", None, None).await.unwrap(),
            501
        );
    }

    /// A deleted account's id is never handed out again, because a folder of
    /// its files can outlive the row and the next account must not inherit it.
    #[tokio::test]
    async fn the_id_of_a_deleted_account_is_not_handed_out_again() {
        let fixture = crate::test_support::test_fixture().await;
        let mut conn = fixture.conn().await;
        let alice = insert_account(&mut conn, "alice", None, None)
            .await
            .unwrap();
        let bob = insert_account(&mut conn, "bob", None, None).await.unwrap();
        delete_account(&mut conn, bob, crate::db::audit_trail::AuditActor::Owner)
            .await
            .unwrap();
        let carol = insert_account(&mut conn, "carol", None, None)
            .await
            .unwrap();
        assert!(carol > bob, "carol got {carol}, after bob's {bob}");
        // With every generated account gone, the next id still climbs.
        delete_account(&mut conn, carol, crate::db::audit_trail::AuditActor::Owner)
            .await
            .unwrap();
        delete_account(&mut conn, alice, crate::db::audit_trail::AuditActor::Owner)
            .await
            .unwrap();
        let dave = insert_account(&mut conn, "dave", None, None).await.unwrap();
        assert!(dave > carol, "dave got {dave}, after carol's {carol}");
    }

    #[tokio::test]
    async fn unknown_username_errors() {
        let fixture = crate::test_support::test_fixture().await;
        fixture.account_with_id(ACCOUNT_ID, "Alice").await;
        let mut conn = fixture.conn().await;
        let err = resolve_account_ref(&mut conn, "nobody")
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("not found"), "{err}");
    }

    #[tokio::test]
    async fn an_unknown_id_is_not_found() {
        let fixture = crate::test_support::test_fixture().await;
        fixture.account_with_id(ACCOUNT_ID, "Alice").await;
        let mut conn = fixture.conn().await;
        let err = resolve_account_ref(&mut conn, "4321")
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("not found"), "{err}");
    }

    #[tokio::test]
    async fn username_for_account_works() {
        let fixture = crate::test_support::test_fixture().await;
        fixture.account_with_id(ACCOUNT_ID, "Alice").await;
        let mut conn = fixture.conn().await;
        assert_eq!(
            username_for_account(&mut conn, ACCOUNT_ID)
                .await
                .unwrap()
                .as_deref(),
            Some("Alice")
        );
    }

    #[tokio::test]
    async fn load_password_hash_returns_none_when_null() {
        // Demo (and any passwordless account) stores password_hash as SQL NULL.
        // Reading that column must not fail with "Invalid column type Null".
        let fixture = crate::test_support::test_fixture().await;
        fixture.account_with_id(ACCOUNT_ID, "Alice").await;
        let mut conn = fixture.conn().await;
        let hash = load_password_hash(&mut conn, ACCOUNT_ID).await.unwrap();
        assert_eq!(hash, None);
    }

    #[tokio::test]
    async fn load_password_hash_returns_set_value() {
        let fixture = crate::test_support::test_fixture().await;
        fixture.account_with_id(ACCOUNT_ID, "Alice").await;
        let mut conn = fixture.conn().await;
        update_password_hash(&mut conn, ACCOUNT_ID, Some("$argon2id$example"))
            .await
            .unwrap();
        let hash = load_password_hash(&mut conn, ACCOUNT_ID).await.unwrap();
        assert_eq!(hash.as_deref(), Some("$argon2id$example"));
    }

    #[tokio::test]
    async fn load_profile_returns_linked_handles_and_preferred_name() {
        let fixture = crate::test_support::test_fixture().await;
        fixture.account_with_id(ACCOUNT_ID, "Alice").await;
        let mut conn = fixture.conn().await;
        let empty = load_account_profile(&mut conn, ACCOUNT_ID).await.unwrap();
        assert!(empty.phones.is_empty());
        assert!(empty.emails.is_empty());
        assert_eq!(
            load_preferred_name(&mut conn, ACCOUNT_ID).await.unwrap(),
            None
        );

        sqlx::query("UPDATE accounts SET preferred_name = 'MB' WHERE id = $1")
            .bind(ACCOUNT_ID)
            .execute(&mut *conn)
            .await
            .unwrap();
        link_account_handle(&mut conn, ACCOUNT_ID, "+15555550100", HandleType::Phone)
            .await
            .unwrap();
        let loaded = load_account_profile(&mut conn, ACCOUNT_ID).await.unwrap();
        assert_eq!(loaded.phones, vec!["+15555550100".to_string()]);
        assert_eq!(
            load_preferred_name(&mut conn, ACCOUNT_ID).await.unwrap(),
            Some("MB".to_string())
        );
    }

    #[tokio::test]
    async fn link_account_handle_normalizes_and_dedupes() {
        let fixture = crate::test_support::test_fixture().await;
        fixture.account_with_id(ACCOUNT_ID, "Alice").await;
        let mut conn = fixture.conn().await;
        let a = link_account_handle(
            &mut conn,
            ACCOUNT_ID,
            "+1 (555) 555-0100",
            HandleType::Phone,
        )
        .await
        .unwrap();
        // Same normalized value with a different raw form reuses the handle row.
        let b = link_account_handle(&mut conn, ACCOUNT_ID, "+15555550100", HandleType::Phone)
            .await
            .unwrap();
        assert_eq!(a, b);
        let normalized: String = sqlx::query_scalar("SELECT normalized FROM handles WHERE id = $1")
            .bind(a)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
        assert_eq!(normalized, "+15555550100");
        let linked: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM account_handles WHERE account_id = $1")
                .bind(ACCOUNT_ID)
                .fetch_one(&mut *conn)
                .await
                .unwrap();
        assert_eq!(linked, 1);
        // Email handles are lowercased and stored separately by type.
        let email = link_account_handle(&mut conn, ACCOUNT_ID, "ME@EXAMPLE.com", HandleType::Email)
            .await
            .unwrap();
        let linked_ids: Vec<i64> =
            sqlx::query_scalar("SELECT handle_id FROM account_handles WHERE account_id = $1")
                .bind(ACCOUNT_ID)
                .fetch_all(&mut *conn)
                .await
                .unwrap();
        assert_eq!(linked_ids.len(), 2);
        assert!(linked_ids.contains(&email));
    }

    /// The delete is one transaction: when a statement after the
    /// conversations' delete fails, the conversations are still there.
    #[tokio::test]
    async fn a_failed_delete_of_all_messages_leaves_the_conversations() {
        let fixture = crate::test_support::test_fixture().await;
        fixture.account_with_id(ACCOUNT_ID, "Alice").await;
        let mut conn = fixture.conn().await;
        let handle_id =
            link_account_handle(&mut conn, ACCOUNT_ID, "+15555550100", HandleType::Phone)
                .await
                .unwrap();
        sqlx::query(
            "INSERT INTO conversations (account_id, chat_handle_id, conversation_type, source_file)
             VALUES ($1, $2, 'individual', 'c.jsonl')",
        )
        .bind(ACCOUNT_ID)
        .bind(handle_id)
        .execute(&mut *conn)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO staging_conversations (
                account_id, chat_handle_id, conversation_type, source_file
             ) VALUES ($1, $2, 'individual', 'c.jsonl')",
        )
        .bind(ACCOUNT_ID)
        .bind(handle_id)
        .execute(&mut *conn)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TEMP TRIGGER fail_staging_delete BEFORE DELETE ON staging_conversations
             BEGIN SELECT RAISE(ABORT, 'staging delete fails'); END",
        )
        .execute(&mut *conn)
        .await
        .unwrap();

        let result = delete_all_messages_for_account(
            &mut conn,
            ACCOUNT_ID,
            crate::db::audit_trail::AuditActor::Holder,
        )
        .await;

        assert!(result.is_err(), "the staging delete was made to fail");
        let left: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM conversations WHERE account_id = $1")
                .bind(ACCOUNT_ID)
                .fetch_one(&mut *conn)
                .await
                .unwrap();
        assert_eq!(left, 1, "the conversations' delete was rolled back");
    }

    #[tokio::test]
    async fn delete_all_messages_keeps_account_and_contacts() {
        let fixture = crate::test_support::test_fixture().await;
        fixture.account_with_id(ACCOUNT_ID, "Alice").await;
        let mut conn = fixture.conn().await;
        let handle_id =
            link_account_handle(&mut conn, ACCOUNT_ID, "+15555550100", HandleType::Phone)
                .await
                .unwrap();
        sqlx::query("INSERT INTO contacts (account_id, preferred_name) VALUES ($1, 'Pat')")
            .bind(ACCOUNT_ID)
            .execute(&mut *conn)
            .await
            .unwrap();
        let contact_id: i64 = sqlx::query_scalar("SELECT id FROM contacts WHERE account_id = $1")
            .bind(ACCOUNT_ID)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO conversations (
                id, account_id, chat_handle_id, conversation_type, source_file
             ) VALUES (1, $1, $2, 'individual', 'c.jsonl')",
        )
        .bind(ACCOUNT_ID)
        .bind(handle_id)
        .execute(&mut *conn)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO messages (
                conversation_id, account_id, source, timestamp, is_from_me, sort_order, body
             ) VALUES (1, $1, 'imessage', '2020-01-01T00:00:00Z', 1, 0, 'hi')",
        )
        .bind(ACCOUNT_ID)
        .execute(&mut *conn)
        .await
        .unwrap();
        let msg_id: i64 = sqlx::query_scalar("SELECT id FROM messages WHERE account_id = $1")
            .bind(ACCOUNT_ID)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO attachments (message_id, path, original_name, mime_type)
             VALUES ($1, 'a.jpg', 'a.jpg', 'image/jpeg')",
        )
        .bind(msg_id)
        .execute(&mut *conn)
        .await
        .unwrap();

        let stats = delete_all_messages_for_account(
            &mut conn,
            ACCOUNT_ID,
            crate::db::audit_trail::AuditActor::Holder,
        )
        .await
        .unwrap();
        assert_eq!(stats.conversations, 1);
        assert_eq!(stats.attachments, 1);
        let remaining_msgs: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE account_id = $1")
                .bind(ACCOUNT_ID)
                .fetch_one(&mut *conn)
                .await
                .unwrap();
        assert_eq!(remaining_msgs, 0);
        let contacts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM contacts WHERE id = $1")
            .bind(contact_id)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
        assert_eq!(contacts, 1);
        assert!(
            username_for_account(&mut conn, ACCOUNT_ID)
                .await
                .unwrap()
                .is_some()
        );
    }
}
