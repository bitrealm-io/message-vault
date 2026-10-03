//! Insert a row under a name its account has free.
//!
//! `table` has `account_id` and `name` columns and `UNIQUE(account_id,
//! name)`: `contact_groups`, `message_tags` or `saved_searches`. `columns` are
//! the row's other columns with the text each one gets. Table and column names
//! go into the statement as written, so they come from the code, never from a
//! request.
//!
//! A name is taken when the account has a row under it in any letter case
//! (`lower(name) = lower($2)`, with the Unicode `lower()` of
//! [`crate::db::sqlite_functions`]), which is how a name a person types is
//! compared in all three tables. The name is claimed by the insert itself,
//! not by a lookup before it: one statement checks the name and writes the
//! row, so nothing can take the name in between, and a name that is taken
//! inserts nothing instead of failing. `ON CONFLICT DO NOTHING` covers the
//! exact name through the table's own `UNIQUE(account_id, name)`.

use sqlx::SqliteConnection;

/// Insert a row under `name` and answer its id, or `None` when the account
/// already has a row under `name` in any letter case.
///
/// # Errors
///
/// Returns an error when the insert fails.
pub async fn insert_if_name_free(
    conn: &mut SqliteConnection,
    table: &str,
    account_id: i64,
    name: &str,
    columns: &[(&str, &str)],
) -> sqlx::Result<Option<i64>> {
    let sql = insert_sql(table, columns);
    insert(conn, &sql, account_id, name, columns).await
}

/// Insert a row for an Import Run's shortcut under the first free name and
/// answer its id and that name: `base`, or `base` with " 2", " 3", … up to
/// " 999". `None` when all 999 are taken.
///
/// # Errors
///
/// Returns an error when an insert fails.
pub async fn insert_under_free_name(
    conn: &mut SqliteConnection,
    table: &str,
    account_id: i64,
    base: &str,
    columns: &[(&str, &str)],
) -> sqlx::Result<Option<(i64, String)>> {
    let sql = insert_sql(table, columns);
    for n in 1..1000 {
        let name = if n == 1 {
            base.to_string()
        } else {
            format!("{base} {n}")
        };
        if let Some(id) = insert(conn, &sql, account_id, &name, columns).await? {
            return Ok(Some((id, name)));
        }
    }
    Ok(None)
}

/// The statement that inserts a row under a free name: `$1` is the account,
/// `$2` the name, and the `columns` follow from `$3`.
fn insert_sql(table: &str, columns: &[(&str, &str)]) -> String {
    let mut names = String::new();
    let mut placeholders = String::new();
    for (i, (column, _)) in columns.iter().enumerate() {
        names.push_str(", ");
        names.push_str(column);
        placeholders.push_str(&format!(", ${}", i + 3));
    }
    format!(
        "INSERT INTO {table} (account_id, name{names})
         SELECT $1, $2{placeholders}
         WHERE NOT EXISTS (
             SELECT 1 FROM {table} WHERE account_id = $1 AND lower(name) = lower($2)
         )
         ON CONFLICT DO NOTHING
         RETURNING id",
    )
}

/// Run [`insert_sql`]'s statement for one name.
async fn insert(
    conn: &mut SqliteConnection,
    sql: &str,
    account_id: i64,
    name: &str,
    columns: &[(&str, &str)],
) -> sqlx::Result<Option<i64>> {
    let mut insert = sqlx::query_scalar(sql).bind(account_id).bind(name);
    for (_, value) in columns {
        insert = insert.bind(*value);
    }
    insert.fetch_optional(&mut *conn).await
}
