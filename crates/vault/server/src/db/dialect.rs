//! SQL dialect helpers for queries that cannot be written portably.

use std::io::{self, Write};
use std::time::Instant;

use sqlx::AnyConnection;

use crate::db::engine::DbEngine;

/// Case-insensitive substring match fragment (`%term%` patterns).
///
/// Both engines name `\` as the escape character. Postgres already treats a
/// backslash that way by default and SQLite has no escape character unless
/// told, so without the clause the two disagree on any pattern holding a
/// backslash. The caller escapes the text it binds.
///
/// The `?` placeholder form is **only** for fragments consumed by the
/// [`crate::db::sql::renumber_placeholders`] pass, which rewrites `?` to the
/// right `$n`; nothing else may use it — sqlx Any does no client-side
/// placeholder rewriting, so a bare `?` is invalid on Postgres.
pub fn like_ci(engine: DbEngine) -> &'static str {
    match engine {
        DbEngine::Sqlite => r"LIKE ? COLLATE NOCASE ESCAPE '\'",
        DbEngine::Postgres => r"ILIKE ? ESCAPE '\'",
    }
}

/// Case-insensitive equality on a name column (`COLLATE NOCASE` is invalid
/// Postgres SQL; Postgres lower()s both sides). `column` is the full column
/// expression (`name`, `ct.name`); the alias must stay INSIDE `lower()` —
/// `ct.lower(...)` parses as a schema-qualified function call. `placeholder`
/// is the placeholder text: `"?"` for renumber-pass fragments, `"$2"` for
/// hand-numbered SQL.
pub fn name_eq_ci(engine: DbEngine, column: &str, placeholder: &str) -> String {
    match engine {
        DbEngine::Sqlite => format!("{column} = {placeholder} COLLATE NOCASE"),
        DbEngine::Postgres => format!("lower({column}) = lower({placeholder})"),
    }
}

/// Case-insensitive A–Z `ORDER BY` on a name column, matching [`name_eq_ci`].
/// `column` is the full column expression (`name`, `n.name`); append further
/// sort keys with a leading comma.
pub fn order_by_name_ci(engine: DbEngine, column: &str) -> String {
    format!("ORDER BY {}", name_ci_expr(engine, column))
}

/// The case-folded form of a name column for an `ORDER BY`, so a caller can
/// put its own direction after it: `name COLLATE NOCASE` on SQLite,
/// `lower(name)` on Postgres.
pub fn name_ci_expr(engine: DbEngine, column: &str) -> String {
    match engine {
        DbEngine::Sqlite => format!("{column} COLLATE NOCASE"),
        DbEngine::Postgres => format!("lower({column})"),
    }
}

/// Engine for a live connection, for db-module code that has no `DbEngine` in scope.
pub fn engine_of(conn: &sqlx::AnyConnection) -> DbEngine {
    if conn.backend_name() == "PostgreSQL" {
        DbEngine::Postgres
    } else {
        DbEngine::Sqlite
    }
}

/// The transaction-begin statement for each engine. SQLite uses IMMEDIATE so
/// overlapping writers fail fast instead of deadlocking; Postgres has no
/// equivalent statement-level mode and uses a plain BEGIN.
pub fn begin_immediate_sql(engine: DbEngine) -> &'static str {
    match engine {
        DbEngine::Sqlite => "BEGIN IMMEDIATE TRANSACTION",
        DbEngine::Postgres => "BEGIN",
    }
}

/// Planner refresh for the tables promote writes. Same statements on both
/// engines (`ANALYZE <table>` is valid on SQLite and Postgres).
pub fn analyze_import_tables_sql() -> &'static [&'static str] {
    &[
        "ANALYZE messages",
        "ANALYZE attachments",
        "ANALYZE tapbacks",
    ]
}

/// Compact after `--reset-demo` finishes. Postgres vacuums the three written
/// tables. SQLite `VACUUM` rewrites the whole file.
pub fn vacuum_after_demo_sql(engine: DbEngine) -> &'static [&'static str] {
    match engine {
        DbEngine::Postgres => &["VACUUM messages", "VACUUM attachments", "VACUUM tapbacks"],
        DbEngine::Sqlite => &["VACUUM"],
    }
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

/// Reclaim dead row versions after the demo inbox is fully imported.
/// Errors are warnings; `reset-demo` still succeeds.
pub async fn vacuum_import_tables(conn: &mut AnyConnection) {
    let started = Instant::now();
    let engine = engine_of(conn);
    run_sql_warn(conn, vacuum_after_demo_sql(engine)).await;
    let what = match engine {
        DbEngine::Postgres => "messages, attachments, tapbacks",
        DbEngine::Sqlite => "database",
    };
    println!(
        "  sql:      vacuum {what} ({:.1}s)",
        started.elapsed().as_secs_f64()
    );
    let _ = io::stdout().flush();
}

/// Aggregate many values into one column with U+001F separators (the format
/// the export pipeline expects). SQLite uses `GROUP_CONCAT`, Postgres `string_agg`.
pub fn group_concat_unit_separator(engine: DbEngine, col: &str) -> String {
    match engine {
        DbEngine::Sqlite => format!("GROUP_CONCAT({col}, char(31))"),
        DbEngine::Postgres => format!("string_agg({col}, chr(31))"),
    }
}
