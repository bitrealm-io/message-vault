use super::*;
use crate::test_support::test_fixture;

/// A refused login keeps the username as typed, cut to the username length
/// limit, so a stranger cannot write an entry of any length.
#[tokio::test]
async fn a_refused_username_is_cut_to_the_username_limit() {
    let fixture = test_fixture().await;
    let mut conn = fixture.conn().await;
    let typed = "x".repeat(MAX_TYPED_USERNAME_CHARS + 50);
    record_refused_login(&mut conn, &typed, None, AuditReason::UnknownUsername, None)
        .await
        .unwrap();
    let (items, total) = page(&mut conn, Scope::All, 10, 0).await.unwrap();
    assert_eq!(total, 1);
    assert_eq!(
        items[0].username.as_deref().map(str::len),
        Some(MAX_TYPED_USERNAME_CHARS)
    );
}

/// Deleting an account keeps its Import Runs and their counts, and drops what
/// describes the person's messages: the run's issues, its form, its staging
/// folder and the addresses the backup sent from. A run still open is closed.
#[tokio::test]
async fn deleting_an_account_keeps_an_import_runs_counts_and_drops_its_details() {
    let fixture = test_fixture().await;
    let account = fixture.account("alice").await;
    let mut conn = fixture.conn().await;
    let import_id: i64 = sqlx::query_scalar(
        "INSERT INTO imports (account_id, source, mode, status, started_at, message_count,
                              staging_dir, form_json, source_identities)
         VALUES ($1, 'imessage', 'append', 'running', '2026-10-01T00:00:00+00:00', 12,
                 '/home/alice/staging', '{}', '[\"+15555550123\"]')
         RETURNING id",
    )
    .bind(account)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO import_issues (import_id, kind, step, item, reason, created_at)
         VALUES ($1, 'skip', 'parse', 'chat-with-bob.txt', 'unreadable', 'now')",
    )
    .bind(import_id)
    .execute(&mut *conn)
    .await
    .unwrap();

    account_profile::delete_account(&mut conn, account, AuditActor::Owner)
        .await
        .unwrap();

    let row: (
        Option<i64>,
        Option<String>,
        String,
        i64,
        Option<String>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT account_id, username, status, message_count, staging_dir, source_identities
             FROM imports WHERE id = $1",
    )
    .bind(import_id)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        row,
        (
            None,
            Some("alice".into()),
            "cancelled".into(),
            12,
            None,
            None
        )
    );
    let issues: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM import_issues")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(issues, 0);
}

/// A password change renews the Session, and the login's expiry moves with
/// it, so a live Session never reads as expired.
#[tokio::test]
async fn a_renewed_session_does_not_read_as_expired() {
    let fixture = test_fixture().await;
    let account = fixture.account("alice").await;
    let mut conn = fixture.conn().await;
    crate::db::session_tokens::open_session(&mut conn, account, "alice", None)
        .await
        .unwrap();
    sqlx::query("UPDATE audit_entries SET session_expires_at = '2000-01-01T00:00:00+00:00'")
        .execute(&mut *conn)
        .await
        .unwrap();
    crate::db::session_tokens::rotate_account_session_token(&mut conn, account)
        .await
        .unwrap();
    let (items, _) = page(&mut conn, Scope::Account(account), 10, 0)
        .await
        .unwrap();
    let actions: Vec<AuditAction> = items.iter().map(|item| item.action).collect();
    assert_eq!(actions, [AuditAction::LoggedIn]);
}
