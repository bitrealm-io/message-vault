//! `ANALYZE` and `VACUUM`: the statements that keep the database file fast
//! and small after an import. A failure is printed as a warning, never
//! returned.

use std::io::{self, Write};
use std::time::Instant;

use sqlx::SqliteConnection;

/// Run each statement, printing a warning instead of failing when one errors.
async fn run_sql_warn(conn: &mut SqliteConnection, statements: &[&str]) {
    for sql in statements {
        if let Err(err) = sqlx::query(sql).execute(&mut *conn).await {
            eprintln!("  sql:      warning: {sql} failed: {err}");
        }
    }
}

/// Refresh planner stats on the committed tables promote writes. Errors are
/// warnings; the caller still opens the promote transaction.
pub async fn analyze_import_tables(conn: &mut SqliteConnection) {
    let started = Instant::now();
    run_sql_warn(
        conn,
        &[
            "ANALYZE messages",
            "ANALYZE attachments",
            "ANALYZE tapbacks",
        ],
    )
    .await;
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
