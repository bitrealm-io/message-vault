//! The two shell commands that reach the vault owner's credentials.
//!
//! Claiming a vault and getting back into one are different jobs, so they are
//! different commands: `create-owner` refuses a vault that already has an
//! owner, `reset-owner-password` refuses one that does not. Neither can be
//! mistaken for the other, so setting up a vault cannot silently overwrite a
//! live owner's password.
//!
//! A shell on the server is the right credential for both. Nothing inside the
//! product can reset the owner's password, because no account stands above
//! the owner. See `docs/adr/0008-the-vault-owner-holds-no-messages.md`.

use anyhow::{Result, bail};

use crate::db::account_profile;
use crate::open_vault::OpenVault;

/// Create the vault owner, claiming an unclaimed vault.
///
/// # Errors
///
/// Fails when the vault already has an owner, when the username is malformed
/// or taken, or when the password is empty.
pub async fn create_owner(vault: &OpenVault, username: &str, password: &str) -> Result<String> {
    let mut conn = vault.conn().await?;

    let username = match crate::credentials::require_valid_username(username) {
        Ok(username) => username,
        Err(e) => bail!("{e}"),
    };
    let hash = match crate::credentials::hash_owner_password(password) {
        Ok(hash) => hash,
        Err(e) => bail!("{e}"),
    };

    if account_profile::vault_is_claimed(&mut conn).await? {
        bail!(
            "this vault already has an owner; use `reset-owner-password` to set a new password for it"
        );
    }
    if let Err(e) = crate::credentials::require_username_free(&mut conn, &username).await {
        bail!("{e}");
    }

    account_profile::insert_account_at(
        &mut conn,
        account_profile::OWNER_ACCOUNT_ID,
        &username,
        Some(&hash),
        None,
    )
    .await?;

    Ok(username)
}

/// Set a new password for an existing vault owner and end their sessions.
///
/// Returns the owner's username, which is as easy to forget as the password
/// and just as unreachable from inside the product.
///
/// # Errors
///
/// Fails when the vault has no owner, or when the password is empty.
pub async fn reset_owner_password(vault: &OpenVault, password: &str) -> Result<String> {
    let mut conn = vault.conn().await?;

    let hash = match crate::credentials::hash_owner_password(password) {
        Ok(hash) => hash,
        Err(e) => bail!("{e}"),
    };
    if !account_profile::vault_is_claimed(&mut conn).await? {
        bail!("this vault has no owner yet; use `create-owner` to claim it");
    }

    account_profile::update_password_hash(
        &mut conn,
        account_profile::OWNER_ACCOUNT_ID,
        Some(&hash),
    )
    .await?;
    // The old password is gone, so every session it opened should be too.
    crate::db::session_tokens::revoke_account_sessions(
        &mut conn,
        account_profile::OWNER_ACCOUNT_ID,
    )
    .await?;

    let username =
        account_profile::username_for_account(&mut conn, account_profile::OWNER_ACCOUNT_ID)
            .await?
            .unwrap_or_else(|| account_profile::OWNER_ACCOUNT_ID.to_string());

    Ok(username)
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use super::*;
    use crate::test_support::{claim_vault_as_owner, get_status, login_status, test_vault};

    /// A session opened before the reset is refused after it, so whoever
    /// held the old password is signed out; the new password logs in and the
    /// old one does not.
    #[tokio::test]
    async fn resetting_the_owner_password_signs_out_the_sessions_it_opened() {
        let vault = test_vault().await;
        let state = vault.state.clone();
        let owner = claim_vault_as_owner(&state, "keeper", "hunter2hunter2").await;
        assert_eq!(
            get_status(&state, "/v1/session", &owner.token).await,
            StatusCode::OK
        );

        let shell = OpenVault {
            cfg: (*state.cfg).clone(),
            db: state.db.clone(),
        };
        let username = reset_owner_password(&shell, "a new owner password")
            .await
            .unwrap();
        assert_eq!(username, "keeper");

        assert_eq!(
            get_status(&state, "/v1/session", &owner.token).await,
            StatusCode::UNAUTHORIZED,
            "the session the old password opened is gone"
        );
        assert_eq!(
            login_status(&state, "keeper", "hunter2hunter2").await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            login_status(&state, "keeper", "a new owner password").await,
            StatusCode::CREATED
        );
    }
}
