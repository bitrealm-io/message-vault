//! Settings that belong to the whole server rather than to one account.
//!
//! One row, at id 1. A database that has never been written to has no row at
//! all, which reads the same as a row of defaults — `public_registration`
//! off, `asset_max_bytes` at 512 MiB — so nothing has to seed it.

use anyhow::Result;
use sqlx::SqliteConnection;

use crate::db::schema;

/// What the attachment size limit reads as until the owner sets it: 512 MiB.
pub const DEFAULT_ASSET_MAX_BYTES: u64 = 512 * 1024 * 1024;

/// The server's settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerSettings {
    /// Anyone reaching the server may create their own account. Off unless the
    /// owner turns it on.
    pub public_registration: bool,
    /// The attachment size limit: the largest asset the server accepts, as one
    /// `PUT` or as the declared total of a multipart upload, in bytes. This
    /// row is the only place it lives.
    pub asset_max_bytes: u64,
}

impl Default for ServerSettings {
    /// What an unwritten database reads as: nobody signs themselves up, and
    /// an attachment may be up to 512 MiB.
    fn default() -> Self {
        Self {
            public_registration: false,
            asset_max_bytes: DEFAULT_ASSET_MAX_BYTES,
        }
    }
}

/// Read the server's settings, or the defaults when nothing has been written.
pub async fn load(conn: &mut SqliteConnection) -> Result<ServerSettings> {
    schema::ensure_accounts_schema(conn).await?;
    let row: Option<(i64, i64)> = sqlx::query_as(
        "SELECT public_registration, asset_max_bytes FROM server_settings WHERE id = 1",
    )
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map_or_else(
        ServerSettings::default,
        |(public_registration, asset_max_bytes)| ServerSettings {
            public_registration: public_registration != 0,
            // The column is only ever written from a `u64` that fits.
            asset_max_bytes: u64::try_from(asset_max_bytes).unwrap_or(0),
        },
    ))
}

/// Set the attachment size limit, creating the settings row if this is the
/// first thing ever written to it. The caller has checked the number against
/// the part size; this stores what it is given.
///
/// # Errors
///
/// Returns an error when `bytes` does not fit the column, or the write fails.
pub async fn set_asset_max_bytes(conn: &mut SqliteConnection, bytes: u64) -> Result<()> {
    schema::ensure_accounts_schema(conn).await?;
    let bytes = i64::try_from(bytes)?;
    sqlx::query(
        "INSERT INTO server_settings (id, asset_max_bytes) VALUES (1, $1)
         ON CONFLICT(id) DO UPDATE SET asset_max_bytes = excluded.asset_max_bytes",
    )
    .bind(bytes)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Turn public registration on or off, creating the settings row if this is
/// the first thing ever written to it.
pub async fn set_public_registration(conn: &mut SqliteConnection, enabled: bool) -> Result<()> {
    schema::ensure_accounts_schema(conn).await?;
    sqlx::query(
        "INSERT INTO server_settings (id, public_registration) VALUES (1, $1)
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
    async fn an_unwritten_database_reads_as_closed() {
        let (pool, _dir) = crate::db::engine::test_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        assert!(
            !load(&mut conn).await.unwrap().public_registration,
            "a server nobody has configured admits nobody"
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

        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM server_settings")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
        assert_eq!(rows, 1, "the database has one settings record");
    }

    /// Each setting is written alone, so writing one must leave the other as
    /// it was, whichever of the two created the row. The limit is above
    /// 2 GiB, which a 32-bit column would not hold.
    #[tokio::test]
    async fn the_limit_and_registration_are_written_independently() {
        let (pool, _dir) = crate::db::engine::test_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        assert_eq!(
            load(&mut conn).await.unwrap().asset_max_bytes,
            DEFAULT_ASSET_MAX_BYTES
        );

        // Registration creates the row; the limit still reads as the default.
        set_public_registration(&mut conn, true).await.unwrap();
        assert_eq!(
            load(&mut conn).await.unwrap().asset_max_bytes,
            DEFAULT_ASSET_MAX_BYTES
        );

        let four_gib = 4 * 1024 * 1024 * 1024;
        set_asset_max_bytes(&mut conn, four_gib).await.unwrap();
        let settings = load(&mut conn).await.unwrap();
        assert_eq!(settings.asset_max_bytes, four_gib);
        assert!(settings.public_registration, "setting the limit kept it");

        set_public_registration(&mut conn, false).await.unwrap();
        assert_eq!(load(&mut conn).await.unwrap().asset_max_bytes, four_gib);
    }
}
