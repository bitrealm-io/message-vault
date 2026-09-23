//! How much an account, or the whole vault, holds: row counts, the bytes its
//! attachments take, and what the database itself takes on disk. Every
//! number here describes message data without being it, which is what lets
//! the vault owner read them
//! (`docs/adr/0008-the-vault-owner-holds-no-messages.md`, "What the owner
//! may see"). The per-account and vault-wide figures come from the same
//! queries with and without an account filter, so the total on Owner Home
//! cannot drift from the numbers on an account's Storage tab.
//!
//! The database sizes are measured, on both engines, never estimated. The
//! one estimate is each account's share of message storage, which is the
//! measured messages figure split by the account's share of text.

use anyhow::Result;
use sqlx::AnyConnection;

use super::account_profile::OWNER_ACCOUNT_ID;
use super::dialect::engine_of;
use super::engine::DbEngine;

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

/// One account's share of the messages held, as the vault owner's Dashboard
/// lists it: an id, a username and numbers, nothing that names a person or a
/// conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountText {
    pub account_id: i64,
    pub username: String,
    /// Messages the account holds.
    pub message_count: i64,
    /// Bytes of message text: the length of every body and subject, added up.
    pub text_bytes: i64,
}

/// Bytes the database takes, without attachment files on disk. SQLite: the
/// file's pages; Postgres: the whole database.
pub async fn database_bytes(conn: &mut AnyConnection) -> Result<i64> {
    let sql = match engine_of(conn) {
        DbEngine::Sqlite => {
            "SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()"
        }
        DbEngine::Postgres => "SELECT pg_database_size(current_database())",
    };
    let n: i64 = sqlx::query_scalar(sql).fetch_one(&mut *conn).await?;
    Ok(n)
}

/// Bytes the full-text search index takes. On SQLite it is the four shadow
/// tables behind `messages_fts`, measured through `dbstat`, which the bundled
/// library is compiled with (`SQLITE_ENABLE_DBSTAT_VTAB`). On Postgres it is
/// the `search_tsv` column on `messages` plus its GIN index.
pub async fn fts_bytes(conn: &mut AnyConnection) -> Result<i64> {
    let sql = match engine_of(conn) {
        DbEngine::Sqlite => {
            "SELECT COALESCE(SUM(pgsize), 0) FROM dbstat \
             WHERE name IN ('messages_fts_data', 'messages_fts_idx', \
                            'messages_fts_docsize', 'messages_fts_config')"
        }
        DbEngine::Postgres => {
            "SELECT COALESCE(SUM(pg_column_size(search_tsv)), 0)::bigint \
             + pg_relation_size('ix_messages_search_tsv') FROM messages"
        }
    };
    let n: i64 = sqlx::query_scalar(sql).fetch_one(&mut *conn).await?;
    Ok(n)
}

/// Bytes the `messages` table and its indexes take, without the full-text
/// search index, so the figure means the same thing on both engines. On
/// SQLite the FTS index is separate tables, so the table's `dbstat` pages are
/// the answer. On Postgres the FTS vector is a column on the table, so
/// `fts_bytes`, which the caller has already measured, is subtracted; the
/// measurement scans every message, so it is not repeated here.
pub async fn messages_bytes(conn: &mut AnyConnection, fts_bytes: i64) -> Result<i64> {
    match engine_of(conn) {
        DbEngine::Sqlite => {
            let n: i64 = sqlx::query_scalar(
                "SELECT COALESCE(SUM(pgsize), 0) FROM dbstat \
                 WHERE name = 'messages' \
                    OR name IN (SELECT name FROM sqlite_master \
                                WHERE type = 'index' AND tbl_name = 'messages')",
            )
            .fetch_one(&mut *conn)
            .await?;
            Ok(n)
        }
        DbEngine::Postgres => {
            let total: i64 = sqlx::query_scalar("SELECT pg_total_relation_size('messages')")
                .fetch_one(&mut *conn)
                .await?;
            Ok((total - fts_bytes).max(0))
        }
    }
}

/// Every account with its message count and text bytes, the owner first and
/// then by username: the order the User Accounts table uses. An account with
/// no messages is listed with zeros. Text is counted in bytes, not
/// characters: `LENGTH` on either engine counts characters, so SQLite reads
/// the text as a blob and Postgres uses `octet_length`.
pub async fn text_by_account(conn: &mut AnyConnection) -> Result<Vec<AccountText>> {
    let bytes_of = match engine_of(conn) {
        DbEngine::Sqlite => |column: &str| format!("COALESCE(LENGTH(CAST({column} AS BLOB)), 0)"),
        DbEngine::Postgres => |column: &str| format!("COALESCE(octet_length({column}), 0)"),
    };
    let rows: Vec<(i64, String, i64, i64)> = sqlx::query_as(&format!(
        "SELECT a.id, a.username, COUNT(m.id), COALESCE(SUM({} + {}), 0) \
         FROM accounts a LEFT JOIN messages m ON m.account_id = a.id \
         GROUP BY a.id, a.username \
         ORDER BY CASE WHEN a.id = $1 THEN 0 ELSE 1 END, a.username",
        bytes_of("m.body"),
        bytes_of("m.subject"),
    ))
    .bind(OWNER_ACCOUNT_ID)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(account_id, username, message_count, text_bytes)| AccountText {
                account_id,
                username,
                message_count,
                text_bytes,
            },
        )
        .collect())
}

/// Split `messages_bytes` across the accounts by each one's share of the
/// text, in the accounts' order. The shares add up to `messages_bytes`
/// exactly: every share rounds down and the last account with any text takes
/// what is left. An account with no text gets zero, and when no account has
/// any text, every share is zero.
pub fn split_by_text(messages_bytes: i64, accounts: &[AccountText]) -> Vec<i64> {
    let total_text: i64 = accounts.iter().map(|a| a.text_bytes).sum();
    if total_text <= 0 {
        return vec![0; accounts.len()];
    }
    let mut shares: Vec<i64> = accounts
        .iter()
        .map(|a| {
            // Widened for the product. The share is at most messages_bytes,
            // because text_bytes is at most total_text, so it fits an i64.
            let share =
                i128::from(messages_bytes) * i128::from(a.text_bytes) / i128::from(total_text);
            i64::try_from(share).expect("a share is at most messages_bytes")
        })
        .collect();
    if let Some(last) = accounts.iter().rposition(|a| a.text_bytes > 0) {
        let given: i64 = shares.iter().sum();
        shares[last] += messages_bytes - given;
    }
    shares
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(id: i64, text_bytes: i64) -> AccountText {
        AccountText {
            account_id: id,
            username: format!("u{id}"),
            message_count: 0,
            text_bytes,
        }
    }

    #[test]
    fn shares_add_up_and_the_last_account_with_text_takes_the_rounding() {
        let accounts = [
            account(1, 0),
            account(2, 1),
            account(3, 1),
            account(4, 1),
            account(5, 0),
        ];
        assert_eq!(split_by_text(100, &accounts), vec![0, 33, 33, 34, 0]);
    }

    #[test]
    fn no_text_means_every_share_is_zero() {
        let accounts = [account(1, 0), account(2, 0)];
        assert_eq!(split_by_text(4096, &accounts), vec![0, 0]);
        assert_eq!(split_by_text(0, &[]), Vec::<i64>::new());
    }
}
