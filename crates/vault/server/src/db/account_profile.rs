//! Account rows, profile fields, and message deletion.

use anyhow::{Context, Result, bail};
use message_ir::HandleType;
use sqlx::AnyConnection;

use crate::db::dialect;
use crate::db::engine::DbEngine;
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
    conn: &mut AnyConnection,
    account_id: &str,
) -> Result<AccountProfile> {
    let emails = query_account_strings(
        conn,
        "SELECT email FROM account_emails WHERE account_id = $1 ORDER BY email",
        account_id,
    )
    .await?;
    let phones = query_account_strings(
        conn,
        "SELECT h.normalized FROM handles h
         JOIN account_handles ah ON ah.handle_id = h.id
         WHERE ah.account_id = $1 AND h.handle_type = 'phone'
         ORDER BY h.normalized",
        account_id,
    )
    .await?;
    Ok(AccountProfile { emails, phones })
}

/// Run a one-column query bound to `account_id` and collect the strings.
async fn query_account_strings(
    conn: &mut AnyConnection,
    sql: &str,
    account_id: &str,
) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar::<_, String>(sql)
        .bind(account_id)
        .fetch_all(&mut *conn)
        .await?)
}

/// Ensure `accounts` row exists (stub username = id) for CLI imports.
pub async fn ensure_account_row(conn: &mut AnyConnection, account_id: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO accounts (id, username) VALUES ($1, $1)
         ON CONFLICT DO NOTHING",
    )
    .bind(account_id)
    .execute(&mut *conn)
    .await
    .with_context(|| format!("failed to ensure account row for {account_id}"))?;
    Ok(())
}

/// Ensure a `handles` row exists and link it to the account via `account_handles`.
/// Returns the handle id.
pub async fn link_account_handle(
    conn: &mut AnyConnection,
    account_id: &str,
    raw: &str,
    handle_type: HandleType,
) -> Result<i64> {
    link_account_handle_with_service(conn, account_id, raw, handle_type, None).await
}

/// Like [`link_account_handle`], recording a platform `service`
/// (`phone` | `whatsapp`). Missing/`None` defaults to `phone`.
pub async fn link_account_handle_with_service(
    conn: &mut AnyConnection,
    account_id: &str,
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

/// True for the 8-4-4-4-12 hex shape of a UUID.
fn looks_like_uuid(s: &str) -> bool {
    let s = s.trim();
    if s.len() != 36 {
        return false;
    }
    let b = s.as_bytes();
    if b[8] != b'-' || b[13] != b'-' || b[18] != b'-' || b[23] != b'-' {
        return false;
    }
    s.chars()
        .enumerate()
        .all(|(i, c)| matches!(i, 8 | 13 | 18 | 23) || c.is_ascii_hexdigit())
}

/// Look up an existing account by UUID or username (case-insensitive).
/// Returns `None` when no row matches (does not create stubs).
pub async fn lookup_account_ref(
    conn: &mut AnyConnection,
    account_ref: &str,
) -> Result<Option<String>> {
    let account_ref = account_ref.trim();
    if account_ref.is_empty() {
        return Ok(None);
    }
    schema::ensure_accounts_schema(conn).await?;

    let by_id: Option<String> = sqlx::query_scalar("SELECT id FROM accounts WHERE id = $1")
        .bind(account_ref)
        .fetch_optional(&mut *conn)
        .await?;
    if by_id.is_some() {
        return Ok(by_id);
    }

    // `COLLATE NOCASE` is SQLite-only; Postgres lowercases both sides (the
    // CI index from the schema is on `lower(username)`).
    let by_user: Option<String> = if dialect::engine_of(conn) == DbEngine::Postgres {
        sqlx::query_scalar("SELECT id FROM accounts WHERE lower(username) = lower($1)")
            .bind(account_ref)
            .fetch_optional(&mut *conn)
            .await?
    } else {
        sqlx::query_scalar("SELECT id FROM accounts WHERE username = $1 COLLATE NOCASE")
            .bind(account_ref)
            .fetch_optional(&mut *conn)
            .await?
    };
    Ok(by_user)
}

/// Resolve an account reference to `accounts.id` for import.
///
/// Accepts UUID or username. Unknown usernames error. Unknown UUID-shaped
/// values are returned as-is so CLI import can still stub-create the row.
pub async fn resolve_account_ref(conn: &mut AnyConnection, account_ref: &str) -> Result<String> {
    let account_ref = account_ref.trim();
    if account_ref.is_empty() {
        bail!("account is empty");
    }
    if let Some(id) = lookup_account_ref(conn, account_ref).await? {
        return Ok(id);
    }
    if looks_like_uuid(account_ref) {
        return Ok(account_ref.to_string());
    }
    bail!("account not found: {account_ref} (use an existing username or account UUID)");
}

/// Username for an account id, if the row exists.
pub async fn username_for_account(
    conn: &mut AnyConnection,
    account_id: &str,
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
/// Outer `Option` is "row missing"; inner is the nullable `password_hash`
/// column (NULL/empty means passwordless login).
pub async fn load_password_hash(
    conn: &mut AnyConnection,
    account_id: &str,
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
    conn: &mut AnyConnection,
    account_id: &str,
    password_hash: &str,
) -> Result<()> {
    sqlx::query("UPDATE accounts SET password_hash = $1 WHERE id = $2")
        .bind(password_hash)
        .bind(account_id)
        .execute(&mut *conn)
        .await
        .with_context(|| format!("update password hash for {account_id}"))?;
    Ok(())
}

/// Permanently delete an account. All dependent rows are removed by
/// ON DELETE CASCADE (messages, conversations, contacts, `vault_imports`,
/// `account_handles/emails/api_tokens`).
pub async fn delete_account(conn: &mut AnyConnection, account_id: &str) -> Result<()> {
    sqlx::query("DELETE FROM accounts WHERE id = $1")
        .bind(account_id)
        .execute(&mut *conn)
        .await
        .with_context(|| format!("delete account {account_id}"))?;
    Ok(())
}

/// Stable id for the seeded demo account (`reset-demo`).
pub const DEMO_ACCOUNT_ID: &str = "00000000-0000-0000-0000-00000000d001";

/// True when `account_id` is the seeded demo account.
pub fn is_demo_account(account_id: &str) -> bool {
    account_id == DEMO_ACCOUNT_ID
}

/// Stable id for the vault owner. A vault has one owner or none, and this id
/// is what makes "one" structural: there is no flag to set, no second owner to
/// create, and nothing to promote. A vault holding no row at this id is
/// unclaimed. See `docs/adr/0008-the-vault-owner-holds-no-messages.md`.
pub const OWNER_ACCOUNT_ID: &str = "00000000-0000-0000-0000-00000000a001";

/// True when `account_id` is the vault owner.
pub fn is_vault_owner(account_id: &str) -> bool {
    account_id == OWNER_ACCOUNT_ID
}

/// True when this vault has an owner: the vault is claimed.
pub async fn vault_is_claimed(conn: &mut AnyConnection) -> Result<bool> {
    schema::ensure_accounts_schema(conn).await?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts WHERE id = $1")
        .bind(OWNER_ACCOUNT_ID)
        .fetch_one(&mut *conn)
        .await?;
    Ok(count > 0)
}

/// An account's disabled flag, the two things its holder still owes, and its
/// permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccountAuth {
    /// May not sign in; existing sessions are refused.
    pub disabled: bool,
    /// The vault owner chose this password; the holder must replace it.
    pub must_change_password: bool,
    /// The holder has not set up their profile; they must before going on.
    pub must_set_up_profile: bool,
    /// What this account may do.
    pub permissions: crate::db::permissions::Permissions,
}

/// Load one account's authorization row. `None` when the account is gone.
pub async fn load_account_auth(
    conn: &mut AnyConnection,
    account_id: &str,
) -> Result<Option<AccountAuth>> {
    schema::ensure_accounts_schema(conn).await?;
    let row: Option<(i64, i64, i64, i64, i64, i64)> = sqlx::query_as(
        "SELECT disabled, must_change_password, must_set_up_profile,
                can_import, can_export, can_delete
         FROM accounts WHERE id = $1",
    )
    .bind(account_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(
        |(disabled, must_change, must_set_up, import, export, delete)| AccountAuth {
            disabled: disabled != 0,
            must_change_password: must_change != 0,
            must_set_up_profile: must_set_up != 0,
            permissions: crate::db::permissions::Permissions::from_ints(import, export, delete),
        },
    ))
}

/// Mark an account so its holder must replace the password the vault owner
/// chose for it, or clear the mark once they have.
pub async fn set_must_change_password(
    conn: &mut AnyConnection,
    account_id: &str,
    must_change: bool,
) -> Result<()> {
    schema::ensure_accounts_schema(conn).await?;
    sqlx::query("UPDATE accounts SET must_change_password = $1 WHERE id = $2")
        .bind(i32::from(must_change))
        .bind(account_id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Mark an account as still owing profile setup, or clear the mark once its
/// holder has saved one.
///
/// The vault says whether setup is owed, rather than each client deciding for
/// itself from an empty-looking profile. A rule the client owns is a rule that
/// drifts, and the answer has to survive cleared site data and a second
/// browser.
pub async fn set_must_set_up_profile(
    conn: &mut AnyConnection,
    account_id: &str,
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
/// attachments, participants, tapbacks), staging rows, and trash markers.
/// Contacts, groups, login details, and import tokens are retained.
pub async fn delete_all_messages_for_account(
    conn: &mut AnyConnection,
    account_id: &str,
) -> Result<DeletedMessagesStats> {
    schema::ensure_vault_schema(conn).await?;
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
    .fetch_one(&mut *conn)
    .await?;
    let conversations = sqlx::query("DELETE FROM conversations WHERE account_id = $1")
        .bind(account_id)
        .execute(&mut *conn)
        .await
        .with_context(|| format!("delete conversations for {account_id}"))?
        .rows_affected();
    sqlx::query("DELETE FROM staging_conversations WHERE account_id = $1")
        .bind(account_id)
        .execute(&mut *conn)
        .await
        .with_context(|| format!("delete staging conversations for {account_id}"))?;
    crate::db::trash::purge_account(conn, account_id)
        .await
        .with_context(|| format!("purge trash markers for {account_id}"))?;
    Ok(DeletedMessagesStats {
        conversations,
        attachments: u64::try_from(attachment_count).unwrap_or(0),
    })
}

/// The account's IANA time zone. UTC when the row is missing or the stored
/// name is not one chrono-tz knows, so a bad value degrades to Greenwich
/// rather than to an error on every list.
pub async fn load_time_zone(conn: &mut AnyConnection, account_id: &str) -> Result<chrono_tz::Tz> {
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
    conn: &mut AnyConnection,
    account_id: &str,
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
    conn: &mut AnyConnection,
    account_id: &str,
) -> Result<(chrono_tz::Tz, chrono::NaiveDate)> {
    let zone = load_time_zone(conn, account_id).await?;
    Ok((zone, crate::search::today_in(zone)))
}

/// Load the `preferred_name` for an account, if set.
pub async fn load_preferred_name(
    conn: &mut AnyConnection,
    account_id: &str,
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

/// Insert a new account row. All fields except id and username are optional.
/// The new account gets every permission (`Permissions::all()`); narrow it
/// afterward if needed.
pub async fn insert_account(
    conn: &mut AnyConnection,
    id: &str,
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
    .with_context(|| format!("insert account {username}"))?;
    Ok(())
}

/// Ensure a phone handle is linked to the account via `account_handles`.
pub async fn upsert_account_phone(
    conn: &mut AnyConnection,
    account_id: &str,
    phone: &str,
) -> Result<()> {
    link_account_handle(conn, account_id, phone, HandleType::Phone).await?;
    Ok(())
}

/// Upsert an `account_emails` row.
pub async fn upsert_account_email(
    conn: &mut AnyConnection,
    account_id: &str,
    email: &str,
    is_primary: bool,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO account_emails (account_id, email, is_primary) VALUES ($1, $2, $3)
         ON CONFLICT DO NOTHING",
    )
    .bind(account_id)
    .bind(email)
    .bind(is_primary as i32)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Unlink a handle from the account profile (`account_handles`).
///
/// For emails, also removes the matching `account_emails` row. The underlying
/// `handles` row is left in place so conversation history stays intact.
pub async fn unlink_account_handle(
    conn: &mut AnyConnection,
    account_id: &str,
    raw: &str,
    handle_type: HandleType,
) -> Result<bool> {
    let (normalized, _) = normalize_handle(raw, handle_type);
    let handle_id: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM handles
         WHERE account_id = $1 AND normalized = $2 AND handle_type = $3
         ORDER BY CASE service WHEN 'phone' THEN 0 WHEN 'whatsapp' THEN 1 ELSE 2 END
         LIMIT 1",
    )
    .bind(account_id)
    .bind(normalized.as_str())
    .bind(handle_type.as_str())
    .fetch_optional(&mut *conn)
    .await?;
    let Some(handle_id) = handle_id else {
        if matches!(handle_type, HandleType::Email) {
            let n = sqlx::query("DELETE FROM account_emails WHERE account_id = $1 AND email = $2")
                .bind(account_id)
                .bind(normalized.as_str())
                .execute(&mut *conn)
                .await?
                .rows_affected();
            return Ok(n > 0);
        }
        return Ok(false);
    };

    let removed =
        sqlx::query("DELETE FROM account_handles WHERE account_id = $1 AND handle_id = $2")
            .bind(account_id)
            .bind(handle_id)
            .execute(&mut *conn)
            .await?
            .rows_affected();
    if matches!(handle_type, HandleType::Email) {
        sqlx::query("DELETE FROM account_emails WHERE account_id = $1 AND email = $2")
            .bind(account_id)
            .bind(normalized.as_str())
            .execute(&mut *conn)
            .await?;
    }
    Ok(removed > 0)
}

/// Open the vault at `target` and resolve `account_ref` (username or UUID)
/// to an account UUID. Used by CLI commands that take `--account`.
///
/// # Errors
///
/// Returns an error when the database cannot be opened or the account does
/// not exist.
pub async fn resolve_account_ref_at(
    target: crate::db::engine::DbTarget<'_>,
    account_ref: &str,
) -> Result<String> {
    let pool = target.open().await?;
    let mut conn = pool.acquire().await?;
    resolve_account_ref(&mut conn, account_ref).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACCOUNT_ID: &str = "00000000-0000-4000-8000-000000000001";

    #[tokio::test]
    async fn resolve_by_username_case_insensitive() {
        let vault = crate::test_support::test_vault().await;
        vault.account_with_id(ACCOUNT_ID, "Alice").await;
        let mut conn = vault.conn().await;
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
    async fn resolve_by_uuid() {
        let vault = crate::test_support::test_vault().await;
        vault.account_with_id(ACCOUNT_ID, "Alice").await;
        let mut conn = vault.conn().await;
        assert_eq!(
            resolve_account_ref(&mut conn, ACCOUNT_ID).await.unwrap(),
            ACCOUNT_ID
        );
    }

    #[tokio::test]
    async fn unknown_username_errors() {
        let vault = crate::test_support::test_vault().await;
        vault.account_with_id(ACCOUNT_ID, "Alice").await;
        let mut conn = vault.conn().await;
        let err = resolve_account_ref(&mut conn, "nobody")
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("not found"), "{err}");
    }

    #[tokio::test]
    async fn unknown_uuid_passthrough() {
        let vault = crate::test_support::test_vault().await;
        vault.account_with_id(ACCOUNT_ID, "Alice").await;
        let mut conn = vault.conn().await;
        let id = "11111111-1111-4111-8111-111111111111";
        assert_eq!(resolve_account_ref(&mut conn, id).await.unwrap(), id);
    }

    #[tokio::test]
    async fn username_for_account_works() {
        let vault = crate::test_support::test_vault().await;
        vault.account_with_id(ACCOUNT_ID, "Alice").await;
        let mut conn = vault.conn().await;
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
        let vault = crate::test_support::test_vault().await;
        vault.account_with_id(ACCOUNT_ID, "Alice").await;
        let mut conn = vault.conn().await;
        let hash = load_password_hash(&mut conn, ACCOUNT_ID).await.unwrap();
        assert_eq!(hash, None);
    }

    #[tokio::test]
    async fn load_password_hash_returns_set_value() {
        let vault = crate::test_support::test_vault().await;
        vault.account_with_id(ACCOUNT_ID, "Alice").await;
        let mut conn = vault.conn().await;
        update_password_hash(&mut conn, ACCOUNT_ID, "$argon2id$example")
            .await
            .unwrap();
        let hash = load_password_hash(&mut conn, ACCOUNT_ID).await.unwrap();
        assert_eq!(hash.as_deref(), Some("$argon2id$example"));
    }

    #[tokio::test]
    async fn load_profile_returns_linked_handles_and_preferred_name() {
        let vault = crate::test_support::test_vault().await;
        vault.account_with_id(ACCOUNT_ID, "Alice").await;
        let mut conn = vault.conn().await;
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
        let vault = crate::test_support::test_vault().await;
        vault.account_with_id(ACCOUNT_ID, "Alice").await;
        let mut conn = vault.conn().await;
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

    #[tokio::test]
    async fn delete_all_messages_keeps_account_and_contacts() {
        let vault = crate::test_support::test_vault().await;
        vault.account_with_id(ACCOUNT_ID, "Alice").await;
        let mut conn = vault.conn().await;
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

        let stats = delete_all_messages_for_account(&mut conn, ACCOUNT_ID)
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
