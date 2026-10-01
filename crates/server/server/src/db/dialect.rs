//! SQL fragments and statements that several queries share.

use std::io::{self, Write};
use std::time::Instant;

use sqlx::SqliteConnection;

/// `column` contains a `LIKE` pattern, case-insensitively: both sides go
/// through `lower()`, so a non-ASCII capital folds the same way an ASCII
/// one does. SQLite's own `lower()` folds only ASCII, so the server
/// registers a Unicode one on every connection
/// ([`crate::db::sqlite_functions`]). `COLLATE NOCASE` is not used, because
/// it folds only ASCII.
///
/// `\` is the escape character. SQLite has none unless told, so the clause
/// names it. The caller escapes the text it binds.
///
/// The fragment binds with `?`, so it belongs in a statement whose every
/// placeholder is `?`. A statement that numbers its placeholders (`$1`)
/// takes no `?` fragment, because mixing the two forms binds values out of
/// position.
pub fn like_ci(column: &str) -> String {
    format!(r"lower({column}) LIKE lower(?) ESCAPE '\'")
}

/// Case-insensitive equality on a name column, folded with `lower()` on
/// both sides for the reason [`like_ci`] gives. `column` is the full column
/// expression (`name`, `ct.name`); the alias must stay INSIDE `lower()` —
/// `ct.lower(...)` parses as a schema-qualified function call. `placeholder`
/// is the placeholder text: `"?"` in a statement that binds with `?`, `"$2"` in
/// hand-numbered SQL.
pub fn name_eq_ci(column: &str, placeholder: &str) -> String {
    format!("lower({column}) = lower({placeholder})")
}

/// Case-insensitive A–Z `ORDER BY` on a name column, matching [`name_eq_ci`].
/// `column` is the full column expression (`name`, `n.name`); append further
/// sort keys with a leading comma.
pub fn order_by_name_ci(column: &str) -> String {
    format!("ORDER BY {}", name_ci_expr(column))
}

/// The case-folded form of a name column for an `ORDER BY`, so a caller can
/// put its own direction after it: `lower(name)`.
pub fn name_ci_expr(column: &str) -> String {
    format!("lower({column})")
}

/// Insert a row for an Import Run's shortcut under the first free name and
/// answer its id and that name: `base`, or `base` with " 2", " 3", … up to
/// " 999". `None` when all 999 are taken.
///
/// `table` has `account_id` and `name` columns and `UNIQUE(account_id,
/// name)`: `contact_groups` or `saved_searches`. `columns` are the row's
/// other columns with the text each one gets. Table and column names go
/// into the statement as written, so they come from the code, never from a
/// request.
///
/// A name is taken when the account has a row under it in any letter case
/// ([`name_eq_ci`]), which is how a name a person types is compared in both
/// tables. Each candidate is claimed by the insert itself, not by a lookup
/// before it: one statement checks the name and writes the row, so nothing
/// can take the name in between, and a name that is taken inserts nothing
/// instead of failing. `ON CONFLICT DO NOTHING` covers the exact name
/// through the table's own `UNIQUE(account_id, name)`.
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
    let mut names = String::new();
    let mut placeholders = String::new();
    for (i, (column, _)) in columns.iter().enumerate() {
        names.push_str(", ");
        names.push_str(column);
        // $1 is the account and $2 the name.
        placeholders.push_str(&format!(", ${}", i + 3));
    }
    let sql = format!(
        "INSERT INTO {table} (account_id, name{names})
         SELECT $1, $2{placeholders}
         WHERE NOT EXISTS (
             SELECT 1 FROM {table} WHERE account_id = $1 AND {name_taken}
         )
         ON CONFLICT DO NOTHING
         RETURNING id",
        name_taken = name_eq_ci("name", "$2"),
    );
    for n in 1..1000 {
        let name = if n == 1 {
            base.to_string()
        } else {
            format!("{base} {n}")
        };
        let mut insert = sqlx::query_scalar(&sql).bind(account_id).bind(&name);
        for (_, value) in columns {
            insert = insert.bind(*value);
        }
        let id: Option<i64> = insert.fetch_optional(&mut *conn).await?;
        if let Some(id) = id {
            return Ok(Some((id, name)));
        }
    }
    Ok(None)
}

/// The statement that begins a write transaction. IMMEDIATE takes the write
/// lock at once, so overlapping writers wait on the busy timeout instead of
/// failing at their first write.
pub const BEGIN_IMMEDIATE_SQL: &str = "BEGIN IMMEDIATE TRANSACTION";

/// Planner refresh for the tables promote writes.
pub fn analyze_import_tables_sql() -> &'static [&'static str] {
    &[
        "ANALYZE messages",
        "ANALYZE attachments",
        "ANALYZE tapbacks",
    ]
}

/// Run each statement, printing a warning instead of failing when one errors.
async fn run_sql_warn(conn: &mut SqliteConnection, statements: &[&str]) {
    for sql in statements {
        if let Err(err) = sqlx::query(sql).execute(&mut *conn).await {
            eprintln!("  sql:      warning: {sql} failed: {err}");
        }
    }
}

/// Refresh planner stats on committed import tables. Errors are warnings;
/// the caller still opens the promote transaction.
pub async fn analyze_import_tables(conn: &mut SqliteConnection) {
    let started = Instant::now();
    run_sql_warn(conn, analyze_import_tables_sql()).await;
    println!(
        "  sql:      analyze messages, attachments, tapbacks ({:.1}s)",
        started.elapsed().as_secs_f64()
    );
    let _ = io::stdout().flush();
}

/// Reclaim the space the demo import freed: `VACUUM` rewrites the whole
/// file. Errors are warnings; `reset-demo` still succeeds.
pub async fn vacuum_import_tables(conn: &mut SqliteConnection) {
    let started = Instant::now();
    run_sql_warn(conn, &["VACUUM"]).await;
    println!(
        "  sql:      vacuum database ({:.1}s)",
        started.elapsed().as_secs_f64()
    );
    let _ = io::stdout().flush();
}

/// Aggregate many values into one column with U+001F separators (the format
/// the export pipeline expects).
pub fn group_concat_unit_separator(col: &str) -> String {
    format!("GROUP_CONCAT({col}, char(31))")
}
