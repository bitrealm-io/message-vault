//! Settings that belong to the whole vault rather than to one account.
//!
//! One row, at id 1. A vault that has never been written to has no row at
//! all, which reads the same as a row of defaults — `public_registration`
//! off — so nothing has to seed it.

use anyhow::Result;
use sqlx::AnyConnection;

use crate::db::schema;

/// The vault's settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VaultSettings {
    /// Anyone reaching the vault may create their own account. Off unless the
    /// vault owner turns it on.
    pub public_registration: bool,
}

impl Default for VaultSettings {
    /// What an unwritten vault reads as: nobody signs themselves up.
    fn default() -> Self {
        Self {
            public_registration: false,
        }
    }
}

/// Read the vault's settings, or the defaults when nothing has been written.
pub async fn load(conn: &mut AnyConnection) -> Result<VaultSettings> {
    schema::ensure_accounts_schema(conn).await?;
    let row: Option<(i64,)> =
        sqlx::query_as("SELECT public_registration FROM vault_settings WHERE id = 1")
            .fetch_optional(&mut *conn)
            .await?;
    Ok(
        row.map_or_else(VaultSettings::default, |(public_registration,)| {
            VaultSettings {
                public_registration: public_registration != 0,
            }
        }),
    )
}

/// Turn public registration on or off, creating the settings row if this is
/// the first thing ever written to it.
pub async fn set_public_registration(conn: &mut AnyConnection, enabled: bool) -> Result<()> {
    schema::ensure_accounts_schema(conn).await?;
    sqlx::query(
        "INSERT INTO vault_settings (id, public_registration) VALUES (1, $1)
         ON CONFLICT(id) DO UPDATE SET public_registration = excluded.public_registration",
    )
    .bind(i32::from(enabled))
    .execute(&mut *conn)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_unwritten_vault_reads_as_closed() {
        let (pool, _dir) = crate::db::engine::test_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        assert!(
            !load(&mut conn).await.unwrap().public_registration,
            "a vault nobody has configured admits nobody"
        );
    }

    #[tokio::test]
    async fn the_setting_round_trips_and_can_be_turned_back_off() {
        let (pool, _dir) = crate::db::engine::test_pool().await;
        let mut conn = pool.acquire().await.unwrap();

        set_public_registration(&mut conn, true).await.unwrap();
        assert!(load(&mut conn).await.unwrap().public_registration);

        // The second write must update the one row, not fail on its primary key.
        set_public_registration(&mut conn, false).await.unwrap();
        assert!(!load(&mut conn).await.unwrap().public_registration);

        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM vault_settings")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
        assert_eq!(rows, 1, "the vault has one settings record");
    }
}
