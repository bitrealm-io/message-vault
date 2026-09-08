//! Named CLI API tokens (`mv-api-…`); many per account, with per-token permissions.

use anyhow::{Context, Result};
use sqlx::AnyConnection;

use super::session_tokens::{generate_prefixed_token, hash_api_token, unix_secs_string};
use crate::db::dialect;
use crate::db::engine::DbEngine;
use crate::db::permissions::Permissions;

/// Metadata for one API token (never includes plaintext or hash).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiTokenRow {
    /// Token id (the secret itself is stored hashed, never in this row).
    pub id: i64,
    /// User-chosen label shown in Settings.
    pub label: String,
    /// What this token may do.
    pub permissions: Permissions,
    /// Masked secret for Settings, e.g. `mv-api-Sd..mE`.
    pub token_hint: String,
    /// Creation time as a Unix-seconds string.
    pub created_at: String,
    /// Unix-seconds string when the token was last used; `None` if never.
    pub last_accessed_at: Option<String>,
    /// Unix-seconds expiry; `None` means no expiry.
    pub expires_at: Option<String>,
    /// True when the token is disabled and rejects requests.
    pub disabled: bool,
}

/// Default API token lifetime when the client does not pass `expires_in_days` (365 days).
pub const DEFAULT_API_TOKEN_TTL_SECS: u64 = 365 * 24 * 60 * 60;

const API_TOKEN_PREFIX: &str = "mv-api-";
const LEGACY_APP_PASSWORD_PREFIX: &str = "mv-app-";
const HINT_HEAD: usize = 2;
const HINT_TAIL: usize = 2;

/// Mask a plaintext API token for list display (keeps `mv-api-` or legacy `mv-app-` + ends).
/// Format: `mv-api-xx..yy`.
pub fn mask_api_token(token: &str) -> String {
    let (prefix, secret) = if let Some(s) = token.strip_prefix(API_TOKEN_PREFIX) {
        (API_TOKEN_PREFIX, s)
    } else if let Some(s) = token.strip_prefix(LEGACY_APP_PASSWORD_PREFIX) {
        (LEGACY_APP_PASSWORD_PREFIX, s)
    } else {
        return format!("{API_TOKEN_PREFIX}..");
    };
    if secret.len() < HINT_HEAD + HINT_TAIL {
        return format!("{prefix}..");
    }
    let head = &secret[..HINT_HEAD];
    let tail = &secret[secret.len() - HINT_TAIL..];
    format!("{prefix}{head}..{tail}")
}

/// Account + permissions for a presented API token Bearer value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiTokenAuth {
    /// Account the token belongs to.
    pub account_id: i64,
    /// What this token may do (not yet intersected with its owner's grant).
    pub permissions: Permissions,
}

/// Label validation failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ApiTokenLabelError {
    /// The trimmed label is empty.
    #[error("label is required")]
    Required,
    /// The label is longer than 120 characters.
    #[error("label must be at most 120 characters")]
    TooLong,
}

/// Failures from creating or renaming an API token: a typed label error, or
/// any other database error.
#[derive(Debug, thiserror::Error)]
pub enum ApiTokenMutationError {
    /// The label failed validation.
    #[error(transparent)]
    InvalidLabel(#[from] ApiTokenLabelError),
    /// Any other database failure.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl From<sqlx::Error> for ApiTokenMutationError {
    fn from(e: sqlx::Error) -> Self {
        Self::Other(anyhow::Error::new(e))
    }
}

/// Generate a new API token (`mv-api-` + 32 alphanumeric characters).
///
/// # Errors
///
/// Returns an error when random bytes cannot be generated.
pub fn generate_api_token() -> Result<String> {
    generate_prefixed_token("mv-api-")
}

/// Look up which account owns this API token Bearer value.
/// On a successful match, updates `last_accessed_at`. Expired or disabled
/// tokens are rejected.
///
/// # Errors
///
/// Returns an error when the lookup or last-accessed update fails.
pub async fn lookup_account_for_api_token(
    conn: &mut AnyConnection,
    token: &str,
) -> Result<Option<ApiTokenAuth>> {
    let token_hash = hash_api_token(token);
    let row: Option<(i64, i64, i64, i64, Option<String>, i64)> = sqlx::query_as(
        "SELECT account_id, can_import, can_export, can_delete, expires_at, disabled
         FROM account_api_tokens WHERE token_hash = $1",
    )
    .bind(token_hash.as_str())
    .fetch_optional(&mut *conn)
    .await?;
    match row {
        Some((account_id, can_import, can_export, can_delete, expires_at, disabled)) => {
            if disabled != 0 {
                return Ok(None);
            }
            if let Some(exp) = expires_at.as_deref() {
                let exp_secs = exp.parse::<u64>().unwrap_or(0);
                let now = unix_secs_string().parse::<u64>().unwrap_or(0);
                let expired = exp_secs == 0 || exp_secs <= now;
                if expired {
                    return Ok(None);
                }
            }
            sqlx::query(
                "UPDATE account_api_tokens SET last_accessed_at = $1 WHERE token_hash = $2",
            )
            .bind(unix_secs_string())
            .bind(token_hash)
            .execute(&mut *conn)
            .await
            .with_context(|| "update API token last_accessed_at")?;
            Ok(Some(ApiTokenAuth {
                account_id,
                permissions: Permissions::from_ints(can_import, can_export, can_delete),
            }))
        }
        None => Ok(None),
    }
}

/// A freshly created API token, including the plaintext secret.
///
/// This is the only place the plaintext `token` exists; everything else
/// stores or returns the hash and the masked hint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedApiToken {
    /// Token id.
    pub id: i64,
    /// The validated (trimmed) label as stored.
    pub label: String,
    /// What this token may do.
    pub permissions: Permissions,
    /// Creation time as a Unix-seconds string.
    pub created_at: String,
    /// Unix-seconds expiry; `None` means no expiry.
    pub expires_at: Option<String>,
    /// The plaintext secret (`mv-api-…`), shown to the caller exactly once.
    pub token: String,
}

/// Create a named API token.
///
/// Returns `ApiTokenMutationError::InvalidLabel` when the label is empty
/// or longer than 120 characters, and `Other` for database failures.
pub async fn create_api_token(
    conn: &mut AnyConnection,
    account_id: i64,
    label: &str,
    permissions: Permissions,
    expires_in_days: Option<u64>,
) -> Result<CreatedApiToken, ApiTokenMutationError> {
    let label = validate_api_token_label(label)?;
    let token = generate_api_token()?;
    let token_hash = hash_api_token(&token);
    let token_hint = mask_api_token(&token);
    let created_at = unix_secs_string();
    let expires_at = api_token_expiry(expires_in_days, &created_at);
    let label_owned = label.to_string();
    let id: i64 = sqlx::query_scalar(
        r"
        INSERT INTO account_api_tokens
            (account_id, label, token_hash, can_import, can_export, can_delete, token_hint, created_at, expires_at, disabled)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 0)
        RETURNING id
        ",
    )
    .bind(account_id)
    .bind(label_owned.as_str())
    .bind(token_hash.as_str())
    .bind(permissions.import as i32)
    .bind(permissions.export as i32)
    .bind(permissions.delete as i32)
    .bind(token_hint.as_str())
    .bind(created_at.as_str())
    .bind(expires_at.as_deref())
    .fetch_one(&mut *conn)
    .await
    .with_context(|| format!("insert API token for {account_id}"))?;
    Ok(CreatedApiToken {
        id,
        label: label_owned,
        permissions,
        created_at,
        expires_at,
        token,
    })
}

/// Raw row for [`list_api_tokens`] before disabled/expiry mapping into
/// [`ApiTokenRow`].
type ApiTokenRowRaw = (
    i64,
    String,
    i64,
    i64,
    i64,
    String,
    String,
    Option<String>,
    Option<String>,
    i64,
);

/// List API tokens for an account (no secrets).
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn list_api_tokens(
    conn: &mut AnyConnection,
    account_id: i64,
) -> Result<Vec<ApiTokenRow>> {
    // `COLLATE NOCASE` is SQLite-only; Postgres lowercases the label instead.
    let order_by = if dialect::engine_of(conn) == DbEngine::Postgres {
        "ORDER BY created_at DESC, lower(label)"
    } else {
        "ORDER BY created_at DESC, label COLLATE NOCASE"
    };
    let rows: Vec<ApiTokenRowRaw> = sqlx::query_as(&format!(
        "SELECT id, label, can_import, can_export, can_delete, token_hint, created_at, last_accessed_at, expires_at, disabled
         FROM account_api_tokens
         WHERE account_id = $1
         {order_by}"
    ))
    .bind(account_id)
    .fetch_all(&mut *conn)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for (
        id,
        label,
        can_import,
        can_export,
        can_delete,
        token_hint,
        created_at,
        last_accessed_at,
        expires_at,
        disabled,
    ) in rows
    {
        out.push(ApiTokenRow {
            id,
            label,
            permissions: Permissions::from_ints(can_import, can_export, can_delete),
            token_hint,
            created_at,
            last_accessed_at,
            expires_at,
            disabled: disabled != 0,
        });
    }
    Ok(out)
}

/// Delete one API token if it belongs to the account.
///
/// # Errors
///
/// Returns an error when the delete statement fails.
pub async fn delete_api_token(conn: &mut AnyConnection, account_id: i64, id: i64) -> Result<bool> {
    let n = sqlx::query("DELETE FROM account_api_tokens WHERE id = $1 AND account_id = $2")
        .bind(id)
        .bind(account_id)
        .execute(&mut *conn)
        .await
        .with_context(|| format!("delete API token {id} for {account_id}"))?
        .rows_affected();
    Ok(n > 0)
}

/// Delete every named API token belonging to an account.
///
/// # Errors
///
/// Returns an error when the delete statement fails.
pub async fn delete_all_api_tokens(conn: &mut AnyConnection, account_id: i64) -> Result<u64> {
    let deleted = sqlx::query("DELETE FROM account_api_tokens WHERE account_id = $1")
        .bind(account_id)
        .execute(&mut *conn)
        .await
        .with_context(|| format!("delete all API tokens for {account_id}"))?
        .rows_affected();
    Ok(deleted)
}

/// Rename an API token label if it belongs to the account.
///
/// Returns `ApiTokenMutationError::InvalidLabel` when the label is empty
/// or longer than 120 characters, and `Other` for database failures.
pub async fn update_api_token_label(
    conn: &mut AnyConnection,
    account_id: i64,
    id: i64,
    label: &str,
) -> Result<bool, ApiTokenMutationError> {
    let label = validate_api_token_label(label)?;
    let n =
        sqlx::query("UPDATE account_api_tokens SET label = $1 WHERE id = $2 AND account_id = $3")
            .bind(label)
            .bind(id)
            .bind(account_id)
            .execute(&mut *conn)
            .await
            .with_context(|| format!("rename API token {id} for {account_id}"))?
            .rows_affected();
    Ok(n > 0)
}

/// Expiry timestamp `expires_in_days` after `created_at`; `Some(0)` means the caller asked for no expiry.
fn api_token_expiry(expires_in_days: Option<u64>, created_at: &str) -> Option<String> {
    let now = created_at.parse::<u64>().unwrap_or(0);
    match expires_in_days {
        Some(0) => None, // caller asked for no expiry
        Some(days) => Some(format!(
            "{}",
            now.saturating_add(days.saturating_mul(86_400))
        )),
        None => Some(format!(
            "{}",
            now.saturating_add(DEFAULT_API_TOKEN_TTL_SECS)
        )),
    }
}

/// Trim a token label and reject empty or over-long ones.
fn validate_api_token_label(label: &str) -> Result<&str, ApiTokenLabelError> {
    let label = label.trim();
    if label.is_empty() {
        return Err(ApiTokenLabelError::Required);
    }
    if label.len() > 120 {
        return Err(ApiTokenLabelError::TooLong);
    }
    Ok(label)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn create_list_lookup_delete() {
        let vault = crate::test_support::test_vault().await;
        let account_id = vault.account_with_id(101, "alice").await;
        let mut conn = vault.conn().await;
        let created = create_api_token(
            &mut conn,
            account_id,
            " laptop CLI ",
            Permissions {
                import: false,
                export: true,
                delete: false,
            },
            None,
        )
        .await
        .unwrap();
        let id = created.id;
        let token = created.token;
        assert!(token.starts_with("mv-api-"));
        assert_eq!(
            created.permissions,
            Permissions {
                import: false,
                export: true,
                delete: false
            }
        );
        assert_eq!(
            mask_api_token("mv-api-Sd1abcdefghijklmnopqrsmtuvwxyZmE"),
            "mv-api-Sd..mE"
        );
        assert_eq!(
            mask_api_token("mv-app-Sd1abcdefghijklmnopqrsmtuvwxyZmE"),
            "mv-app-Sd..mE"
        );

        let listed = list_api_tokens(&mut conn, account_id).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, id);
        assert_eq!(listed[0].label, "laptop CLI");
        assert_eq!(
            listed[0].permissions,
            Permissions {
                import: false,
                export: true,
                delete: false
            }
        );
        assert_eq!(listed[0].token_hint, mask_api_token(&token));
        assert!(listed[0].last_accessed_at.is_none());

        let auth = lookup_account_for_api_token(&mut conn, &token)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(auth.account_id, account_id);
        assert_eq!(
            auth.permissions,
            Permissions {
                import: false,
                export: true,
                delete: false
            }
        );

        let listed_after = list_api_tokens(&mut conn, account_id).await.unwrap();
        assert!(listed_after[0].last_accessed_at.is_some());

        assert!(
            lookup_account_for_api_token(&mut conn, "mv-api-nope")
                .await
                .unwrap()
                .is_none()
        );

        assert!(delete_api_token(&mut conn, account_id, id).await.unwrap());
        assert!(
            list_api_tokens(&mut conn, account_id)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            lookup_account_for_api_token(&mut conn, &token)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn empty_label_rejected() {
        let vault = crate::test_support::test_vault().await;
        let account_id = vault.account_with_id(101, "alice").await;
        let mut conn = vault.conn().await;
        assert!(
            create_api_token(&mut conn, account_id, "  ", Permissions::all(), None)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn rename_label() {
        let vault = crate::test_support::test_vault().await;
        let account_id = vault.account_with_id(101, "alice").await;
        let mut conn = vault.conn().await;
        let id = create_api_token(&mut conn, account_id, "old name", Permissions::all(), None)
            .await
            .unwrap()
            .id;
        assert!(
            update_api_token_label(&mut conn, account_id, id, " new name ")
                .await
                .unwrap()
        );
        let listed = list_api_tokens(&mut conn, account_id).await.unwrap();
        assert_eq!(listed[0].label, "new name");
        assert!(
            update_api_token_label(&mut conn, account_id, id, "  ")
                .await
                .is_err()
        );
        assert!(
            !update_api_token_label(&mut conn, 102, id, "stolen")
                .await
                .unwrap()
        );
        assert_eq!(
            list_api_tokens(&mut conn, account_id).await.unwrap()[0].label,
            "new name"
        );
    }

    #[tokio::test]
    async fn label_validation_errors_are_typed() {
        let vault = crate::test_support::test_vault().await;
        let account_id = vault.account_with_id(101, "alice").await;
        let mut conn = vault.conn().await;

        let err = create_api_token(&mut conn, account_id, "  ", Permissions::all(), None)
            .await
            .unwrap_err();
        match err {
            ApiTokenMutationError::InvalidLabel(label_err) => {
                assert_eq!(label_err.to_string(), "label is required");
            }
            other => panic!("expected InvalidLabel, got {other:?}"),
        }

        let err = create_api_token(
            &mut conn,
            account_id,
            &"x".repeat(121),
            Permissions::all(),
            None,
        )
        .await
        .unwrap_err();
        match err {
            ApiTokenMutationError::InvalidLabel(label_err) => {
                assert_eq!(
                    label_err.to_string(),
                    "label must be at most 120 characters"
                );
            }
            other => panic!("expected InvalidLabel, got {other:?}"),
        }
    }
}
