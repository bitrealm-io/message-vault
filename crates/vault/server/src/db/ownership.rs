//! One answer to "does this account own this row?".
//!
//! Every route that takes a row id from the path has to decide between 404
//! and going ahead, and the decision is the same one each time: the row
//! exists *and* its `account_id` is the caller's. Asking it here rather than
//! inline keeps the two cases — an id that does not exist, and another
//! account's id — indistinguishable from outside, which is what makes them
//! both a 404 rather than one a 404 and the other a 403.

use sqlx::AnyConnection;

/// True when `account_id` owns a `conversations` row with this id.
///
/// # Errors
///
/// Returns a database error when the query fails.
pub async fn owns_conversation(
    conn: &mut AnyConnection,
    account_id: i64,
    conversation_id: i64,
) -> Result<bool, sqlx::Error> {
    owns_row(conn, "conversations", account_id, conversation_id).await
}

/// True when `account_id` owns a `contacts` row with this id.
///
/// # Errors
///
/// Returns a database error when the query fails.
pub async fn owns_contact(
    conn: &mut AnyConnection,
    account_id: i64,
    contact_id: i64,
) -> Result<bool, sqlx::Error> {
    owns_row(conn, "contacts", account_id, contact_id).await
}

/// The shared query. `table` is a fixed literal chosen by the caller above,
/// never anything from the request, so no part of a request reaches the SQL
/// text. `SELECT 1` with `fetch_optional` rather than `COUNT(*)` because the
/// answer is a yes or no; `id` is the primary key of both tables, so there is
/// never a second row to stop at.
async fn owns_row(
    conn: &mut AnyConnection,
    table: &'static str,
    account_id: i64,
    id: i64,
) -> Result<bool, sqlx::Error> {
    let found: Option<i64> = sqlx::query_scalar(&format!(
        "SELECT 1 FROM {table} WHERE id = $1 AND account_id = $2"
    ))
    .bind(id)
    .bind(account_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(found.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema;

    const ACCOUNT_A: i64 = 7;
    const ACCOUNT_B: i64 = 8;

    async fn setup() -> (sqlx::AnyPool, tempfile::TempDir) {
        let (pool, dir) = crate::db::engine::test_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        schema::ensure_vault_schema(&mut conn).await.unwrap();
        for account in [ACCOUNT_A, ACCOUNT_B] {
            sqlx::query("INSERT INTO accounts (id, username) VALUES ($1, $1)")
                .bind(account)
                .execute(&mut *conn)
                .await
                .unwrap();
        }
        (pool, dir)
    }

    async fn insert_contact(conn: &mut AnyConnection, account_id: i64) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO contacts (account_id, preferred_name, origin)
             VALUES ($1, 'Ada', 'user') RETURNING id",
        )
        .bind(account_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn a_contact_is_owned_by_its_own_account_only() {
        let (pool, _dir) = setup().await;
        let mut conn = pool.acquire().await.unwrap();
        let id = insert_contact(&mut conn, ACCOUNT_A).await;
        assert!(owns_contact(&mut conn, ACCOUNT_A, id).await.unwrap());
        assert!(!owns_contact(&mut conn, ACCOUNT_B, id).await.unwrap());
    }

    #[tokio::test]
    async fn an_id_that_does_not_exist_is_not_owned() {
        let (pool, _dir) = setup().await;
        let mut conn = pool.acquire().await.unwrap();
        assert!(!owns_contact(&mut conn, ACCOUNT_A, 4321).await.unwrap());
        assert!(!owns_conversation(&mut conn, ACCOUNT_A, 4321).await.unwrap());
    }
}
