//! How much an account, or the whole vault, holds: row counts and the bytes
//! its attachments take. Every number here describes message data without
//! being it, which is what lets the vault owner read them
//! (`docs/adr/0008-the-vault-owner-holds-no-messages.md`, "What the owner
//! may see"). The per-account and vault-wide figures come from the same
//! queries with and without an account filter, so the total on Owner Home
//! cannot drift from the numbers on an account's Storage tab.

use anyhow::Result;
use sqlx::AnyConnection;

/// Which rows a count covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// One account's rows.
    Account(i64),
    /// Every account's rows.
    Vault,
}

/// Run `SELECT {select} FROM {from}` over the rows `scope` names, where
/// `account_column` is the column that holds the owning account.
async fn scalar(
    conn: &mut AnyConnection,
    select: &str,
    from: &str,
    account_column: &str,
    scope: Scope,
) -> Result<i64> {
    let n: i64 = match scope {
        Scope::Account(account_id) => {
            sqlx::query_scalar(&format!(
                "SELECT {select} FROM {from} WHERE {account_column} = $1"
            ))
            .bind(account_id)
            .fetch_one(&mut *conn)
            .await?
        }
        Scope::Vault => {
            sqlx::query_scalar(&format!("SELECT {select} FROM {from}"))
                .fetch_one(&mut *conn)
                .await?
        }
    };
    Ok(n)
}

/// Messages held.
pub async fn message_count(conn: &mut AnyConnection, scope: Scope) -> Result<i64> {
    scalar(conn, "COUNT(*)", "messages", "account_id", scope).await
}

/// Conversations held.
pub async fn conversation_count(conn: &mut AnyConnection, scope: Scope) -> Result<i64> {
    scalar(conn, "COUNT(*)", "conversations", "account_id", scope).await
}

/// Contacts held.
pub async fn contact_count(conn: &mut AnyConnection, scope: Scope) -> Result<i64> {
    scalar(conn, "COUNT(*)", "contacts", "account_id", scope).await
}

const ATTACHMENTS_FROM: &str = "attachments a JOIN messages m ON m.id = a.message_id";

/// Attachment rows held.
pub async fn attachment_count(conn: &mut AnyConnection, scope: Scope) -> Result<i64> {
    scalar(conn, "COUNT(*)", ATTACHMENTS_FROM, "m.account_id", scope).await
}

/// Bytes the attachments take, by their original `size_bytes`.
pub async fn attachment_bytes(conn: &mut AnyConnection, scope: Scope) -> Result<i64> {
    scalar(
        conn,
        "COALESCE(SUM(a.size_bytes), 0)",
        ATTACHMENTS_FROM,
        "m.account_id",
        scope,
    )
    .await
}
