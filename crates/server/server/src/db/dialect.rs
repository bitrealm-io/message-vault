//! SQL fragments and statements that several queries share.

use std::io::{self, Write};
use std::time::Instant;

use sqlx::AnyConnection;

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
/// The `?` placeholder form is **only** for fragments consumed by the
/// [`crate::db::sql::renumber_placeholders`] pass, which rewrites `?` to the
/// right `$n`, so a fragment can be placed anywhere in a statement that
/// numbers its placeholders.
pub fn like_ci(column: &str) -> String {
    format!(r"lower({column}) LIKE lower(?) ESCAPE '\'")
}

/// Case-insensitive equality on a name column, folded with `lower()` on
/// both sides for the reason [`like_ci`] gives. `column` is the full column
/// expression (`name`, `ct.name`); the alias must stay INSIDE `lower()` —
/// `ct.lower(...)` parses as a schema-qualified function call. `placeholder`
/// is the placeholder text: `"?"` for renumber-pass fragments, `"$2"` for
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
async fn run_sql_warn(conn: &mut AnyConnection, statements: &[&str]) {
    for sql in statements {
        if let Err(err) = sqlx::query(sql).execute(&mut *conn).await {
            eprintln!("  sql:      warning: {sql} failed: {err}");
        }
    }
}

/// Refresh planner stats on committed import tables. Errors are warnings;
/// the caller still opens the promote transaction.
pub async fn analyze_import_tables(conn: &mut AnyConnection) {
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
pub async fn vacuum_import_tables(conn: &mut AnyConnection) {
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
