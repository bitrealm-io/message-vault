use super::*;
const ACCOUNT_A: i64 = 7;
const ACCOUNT_B: i64 = 8;

/// Insert a conversation owned by `account_id`, returning its id. Each
/// call creates its own chat handle so repeat calls for the same
/// account don't collide on `conversations`' `(account_id,
/// chat_handle_id)` uniqueness.
async fn insert_conversation(conn: &mut AnyConnection, account_id: i64) -> i64 {
    sqlx::query(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES ($1, '+15555550100', '+15555550100', 'phone', 'phone')",
    )
    .bind(account_id)
    .execute(&mut *conn)
    .await
    .unwrap();
    let handle_id: i64 =
        sqlx::query_scalar("SELECT id FROM handles WHERE account_id = $1 ORDER BY id DESC LIMIT 1")
            .bind(account_id)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO conversations (
            account_id, chat_handle_id, conversation_type, source_file
         ) VALUES ($1, $2, 'individual', 'c.jsonl')",
    )
    .bind(account_id)
    .bind(handle_id)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query_scalar("SELECT id FROM conversations WHERE chat_handle_id = $1")
        .bind(handle_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap()
}

/// Insert a contact owned by `account_id`, returning its id.
async fn insert_contact(conn: &mut AnyConnection, account_id: i64) -> i64 {
    sqlx::query("INSERT INTO contacts (account_id, preferred_name) VALUES ($1, 'Pat')")
        .bind(account_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query_scalar("SELECT id FROM contacts WHERE account_id = $1")
        .bind(account_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap()
}

async fn trashed_conversation_count(conn: &mut AnyConnection, account_id: i64) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM trashed_conversations WHERE account_id = $1")
        .bind(account_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap()
}

async fn trashed_contact_count(conn: &mut AnyConnection, account_id: i64) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM trashed_contacts WHERE account_id = $1")
        .bind(account_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap()
}

#[tokio::test]
async fn trash_conversation_marks_an_owned_row() {
    let vault = crate::test_support::test_vault().await;
    for account in [ACCOUNT_A, ACCOUNT_B] {
        vault.account_with_id(account, &account.to_string()).await;
    }
    let mut conn = vault.conn().await;
    let id = insert_conversation(&mut conn, ACCOUNT_A).await;

    assert!(
        move_to_trash(&mut conn, ACCOUNT_A, Trashable::Conversation(id))
            .await
            .unwrap()
    );
    assert_eq!(trashed_conversation_count(&mut conn, ACCOUNT_A).await, 1);
}

#[tokio::test]
async fn trash_conversation_twice_stays_one_row() {
    let vault = crate::test_support::test_vault().await;
    for account in [ACCOUNT_A, ACCOUNT_B] {
        vault.account_with_id(account, &account.to_string()).await;
    }
    let mut conn = vault.conn().await;
    let id = insert_conversation(&mut conn, ACCOUNT_A).await;

    assert!(
        move_to_trash(&mut conn, ACCOUNT_A, Trashable::Conversation(id))
            .await
            .unwrap()
    );
    assert!(
        move_to_trash(&mut conn, ACCOUNT_A, Trashable::Conversation(id))
            .await
            .unwrap()
    );
    assert_eq!(trashed_conversation_count(&mut conn, ACCOUNT_A).await, 1);
}

#[tokio::test]
async fn restore_conversation_removes_the_marker() {
    let vault = crate::test_support::test_vault().await;
    for account in [ACCOUNT_A, ACCOUNT_B] {
        vault.account_with_id(account, &account.to_string()).await;
    }
    let mut conn = vault.conn().await;
    let id = insert_conversation(&mut conn, ACCOUNT_A).await;
    move_to_trash(&mut conn, ACCOUNT_A, Trashable::Conversation(id))
        .await
        .unwrap();

    assert!(
        restore(&mut conn, ACCOUNT_A, Trashable::Conversation(id))
            .await
            .unwrap()
    );
    assert_eq!(trashed_conversation_count(&mut conn, ACCOUNT_A).await, 0);
}

#[tokio::test]
async fn restore_conversation_not_trashed_is_a_noop() {
    let vault = crate::test_support::test_vault().await;
    for account in [ACCOUNT_A, ACCOUNT_B] {
        vault.account_with_id(account, &account.to_string()).await;
    }
    let mut conn = vault.conn().await;
    let id = insert_conversation(&mut conn, ACCOUNT_A).await;

    assert!(
        restore(&mut conn, ACCOUNT_A, Trashable::Conversation(id))
            .await
            .unwrap()
    );
    assert_eq!(trashed_conversation_count(&mut conn, ACCOUNT_A).await, 0);
}

#[tokio::test]
async fn conversation_operations_refuse_another_accounts_id() {
    let vault = crate::test_support::test_vault().await;
    for account in [ACCOUNT_A, ACCOUNT_B] {
        vault.account_with_id(account, &account.to_string()).await;
    }
    let mut conn = vault.conn().await;
    let id = insert_conversation(&mut conn, ACCOUNT_A).await;

    assert!(
        !move_to_trash(&mut conn, ACCOUNT_B, Trashable::Conversation(id))
            .await
            .unwrap()
    );
    assert_eq!(trashed_conversation_count(&mut conn, ACCOUNT_A).await, 0);
    assert_eq!(trashed_conversation_count(&mut conn, ACCOUNT_B).await, 0);

    // Trash it as its rightful owner, then confirm B still can't restore it.
    move_to_trash(&mut conn, ACCOUNT_A, Trashable::Conversation(id))
        .await
        .unwrap();
    assert!(
        !restore(&mut conn, ACCOUNT_B, Trashable::Conversation(id))
            .await
            .unwrap()
    );
    assert_eq!(trashed_conversation_count(&mut conn, ACCOUNT_A).await, 1);
}

#[tokio::test]
async fn trash_contact_marks_an_owned_row() {
    let vault = crate::test_support::test_vault().await;
    for account in [ACCOUNT_A, ACCOUNT_B] {
        vault.account_with_id(account, &account.to_string()).await;
    }
    let mut conn = vault.conn().await;
    let id = insert_contact(&mut conn, ACCOUNT_A).await;

    assert!(
        move_to_trash(&mut conn, ACCOUNT_A, Trashable::Contact(id))
            .await
            .unwrap()
    );
    assert_eq!(trashed_contact_count(&mut conn, ACCOUNT_A).await, 1);
}

#[tokio::test]
async fn trash_contact_twice_stays_one_row() {
    let vault = crate::test_support::test_vault().await;
    for account in [ACCOUNT_A, ACCOUNT_B] {
        vault.account_with_id(account, &account.to_string()).await;
    }
    let mut conn = vault.conn().await;
    let id = insert_contact(&mut conn, ACCOUNT_A).await;

    assert!(
        move_to_trash(&mut conn, ACCOUNT_A, Trashable::Contact(id))
            .await
            .unwrap()
    );
    assert!(
        move_to_trash(&mut conn, ACCOUNT_A, Trashable::Contact(id))
            .await
            .unwrap()
    );
    assert_eq!(trashed_contact_count(&mut conn, ACCOUNT_A).await, 1);
}

#[tokio::test]
async fn restore_contact_removes_the_marker() {
    let vault = crate::test_support::test_vault().await;
    for account in [ACCOUNT_A, ACCOUNT_B] {
        vault.account_with_id(account, &account.to_string()).await;
    }
    let mut conn = vault.conn().await;
    let id = insert_contact(&mut conn, ACCOUNT_A).await;
    move_to_trash(&mut conn, ACCOUNT_A, Trashable::Contact(id))
        .await
        .unwrap();

    assert!(
        restore(&mut conn, ACCOUNT_A, Trashable::Contact(id))
            .await
            .unwrap()
    );
    assert_eq!(trashed_contact_count(&mut conn, ACCOUNT_A).await, 0);
}

#[tokio::test]
async fn restore_contact_not_trashed_is_a_noop() {
    let vault = crate::test_support::test_vault().await;
    for account in [ACCOUNT_A, ACCOUNT_B] {
        vault.account_with_id(account, &account.to_string()).await;
    }
    let mut conn = vault.conn().await;
    let id = insert_contact(&mut conn, ACCOUNT_A).await;

    assert!(
        restore(&mut conn, ACCOUNT_A, Trashable::Contact(id))
            .await
            .unwrap()
    );
    assert_eq!(trashed_contact_count(&mut conn, ACCOUNT_A).await, 0);
}

#[tokio::test]
async fn contact_operations_refuse_another_accounts_id() {
    let vault = crate::test_support::test_vault().await;
    for account in [ACCOUNT_A, ACCOUNT_B] {
        vault.account_with_id(account, &account.to_string()).await;
    }
    let mut conn = vault.conn().await;
    let id = insert_contact(&mut conn, ACCOUNT_A).await;

    assert!(
        !move_to_trash(&mut conn, ACCOUNT_B, Trashable::Contact(id))
            .await
            .unwrap()
    );
    assert_eq!(trashed_contact_count(&mut conn, ACCOUNT_A).await, 0);
    assert_eq!(trashed_contact_count(&mut conn, ACCOUNT_B).await, 0);

    move_to_trash(&mut conn, ACCOUNT_A, Trashable::Contact(id))
        .await
        .unwrap();
    assert!(
        !restore(&mut conn, ACCOUNT_B, Trashable::Contact(id))
            .await
            .unwrap()
    );
    assert_eq!(trashed_contact_count(&mut conn, ACCOUNT_A).await, 1);
}

#[tokio::test]
async fn purge_account_clears_only_that_accounts_trash() {
    let vault = crate::test_support::test_vault().await;
    for account in [ACCOUNT_A, ACCOUNT_B] {
        vault.account_with_id(account, &account.to_string()).await;
    }
    let mut conn = vault.conn().await;
    let conv_a = insert_conversation(&mut conn, ACCOUNT_A).await;
    let contact_a = insert_contact(&mut conn, ACCOUNT_A).await;
    let conv_b = insert_conversation(&mut conn, ACCOUNT_B).await;
    let contact_b = insert_contact(&mut conn, ACCOUNT_B).await;
    move_to_trash(&mut conn, ACCOUNT_A, Trashable::Conversation(conv_a))
        .await
        .unwrap();
    move_to_trash(&mut conn, ACCOUNT_A, Trashable::Contact(contact_a))
        .await
        .unwrap();
    move_to_trash(&mut conn, ACCOUNT_B, Trashable::Conversation(conv_b))
        .await
        .unwrap();
    move_to_trash(&mut conn, ACCOUNT_B, Trashable::Contact(contact_b))
        .await
        .unwrap();

    purge_account(&mut conn, ACCOUNT_A).await.unwrap();

    assert_eq!(trashed_conversation_count(&mut conn, ACCOUNT_A).await, 0);
    assert_eq!(trashed_contact_count(&mut conn, ACCOUNT_A).await, 0);
    assert_eq!(trashed_conversation_count(&mut conn, ACCOUNT_B).await, 1);
    assert_eq!(trashed_contact_count(&mut conn, ACCOUNT_B).await, 1);
}

// ── Permanent deletion ──────────────────────────────────────────────────────

/// Insert a conversation owned by `account_id` on its own handle `raw`,
/// returning its id. Unlike [`insert_conversation`], the handle is the
/// caller's, so a test can put two conversations on two different people.
async fn insert_conversation_on(conn: &mut AnyConnection, account_id: i64, raw: &str) -> i64 {
    let handle_id: i64 = sqlx::query_scalar(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES ($1, $2, $2, 'phone', 'phone') RETURNING id",
    )
    .bind(account_id)
    .bind(raw)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    sqlx::query_scalar(
        "INSERT INTO conversations (
            account_id, chat_handle_id, conversation_type, source_file
         ) VALUES ($1, $2, 'individual', 'c.jsonl') RETURNING id",
    )
    .bind(account_id)
    .bind(handle_id)
    .fetch_one(&mut *conn)
    .await
    .unwrap()
}

/// Insert one `imessage` message into `conversation_id`, returning its id.
async fn insert_message(
    conn: &mut AnyConnection,
    account_id: i64,
    conversation_id: i64,
    sort_order: i64,
) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO messages (
            conversation_id, account_id, source, timestamp, is_from_me, sort_order, body
         ) VALUES ($1, $2, 'imessage', '2020-01-01T00:00:00Z', 1, $3, 'hi') RETURNING id",
    )
    .bind(conversation_id)
    .bind(account_id)
    .bind(sort_order)
    .fetch_one(&mut *conn)
    .await
    .unwrap()
}

/// Attach a stored file to `message_id`: the original under `sha`, and a
/// derivative under `derived` when given.
async fn insert_attachment(
    conn: &mut AnyConnection,
    message_id: i64,
    sha: &str,
    derived: Option<&str>,
) {
    sqlx::query(
        "INSERT INTO attachments (
            message_id, sha256, assets_path, derived_sha256, derived_assets_path
         ) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(message_id)
    .bind(sha)
    .bind(format!("{}/{sha}.jpg", &sha[..2]))
    .bind(derived)
    .bind(derived.map(|d| format!("{}/{d}.jpg", &d[..2])))
    .execute(&mut *conn)
    .await
    .unwrap();
}

async fn count(conn: &mut AnyConnection, sql: &str, id: i64) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(&mut *conn)
        .await
        .unwrap()
}

/// 64 hex digits, distinct per `tag`, the length a real SHA-256 has.
fn sha(tag: char) -> String {
    std::iter::repeat_n(tag, 64).collect()
}

#[tokio::test]
async fn delete_trashed_conversation_removes_it_and_its_messages() {
    let vault = crate::test_support::test_vault().await;
    vault.account_with_id(ACCOUNT_A, "a").await;
    let mut conn = vault.conn().await;
    let id = insert_conversation_on(&mut conn, ACCOUNT_A, "+15550001").await;
    insert_message(&mut conn, ACCOUNT_A, id, 0).await;
    insert_message(&mut conn, ACCOUNT_A, id, 1).await;
    move_to_trash(&mut conn, ACCOUNT_A, Trashable::Conversation(id))
        .await
        .unwrap();

    let outcome = delete_trashed(&mut conn, ACCOUNT_A, Trashable::Conversation(id))
        .await
        .unwrap();

    assert_eq!(outcome, DeleteOutcome::Deleted(Vec::new()));
    assert_eq!(
        count(
            &mut conn,
            "SELECT COUNT(*) FROM conversations WHERE id = $1",
            id
        )
        .await,
        0
    );
    assert_eq!(
        count(
            &mut conn,
            "SELECT COUNT(*) FROM messages WHERE conversation_id = $1",
            id
        )
        .await,
        0
    );
    assert_eq!(trashed_conversation_count(&mut conn, ACCOUNT_A).await, 0);
}

#[tokio::test]
async fn delete_conversation_not_in_the_trash_is_refused_and_changes_nothing() {
    let vault = crate::test_support::test_vault().await;
    vault.account_with_id(ACCOUNT_A, "a").await;
    let mut conn = vault.conn().await;
    let id = insert_conversation_on(&mut conn, ACCOUNT_A, "+15550001").await;
    insert_message(&mut conn, ACCOUNT_A, id, 0).await;

    let outcome = delete_trashed(&mut conn, ACCOUNT_A, Trashable::Conversation(id))
        .await
        .unwrap();

    assert_eq!(outcome, DeleteOutcome::NotTrashed);
    assert_eq!(
        count(
            &mut conn,
            "SELECT COUNT(*) FROM messages WHERE conversation_id = $1",
            id
        )
        .await,
        1
    );
}

#[tokio::test]
async fn delete_refuses_another_accounts_conversation_even_when_trashed() {
    let vault = crate::test_support::test_vault().await;
    for account in [ACCOUNT_A, ACCOUNT_B] {
        vault.account_with_id(account, &account.to_string()).await;
    }
    let mut conn = vault.conn().await;
    let id = insert_conversation_on(&mut conn, ACCOUNT_A, "+15550001").await;
    move_to_trash(&mut conn, ACCOUNT_A, Trashable::Conversation(id))
        .await
        .unwrap();

    let outcome = delete_trashed(&mut conn, ACCOUNT_B, Trashable::Conversation(id))
        .await
        .unwrap();

    assert_eq!(outcome, DeleteOutcome::NotOwned);
    assert_eq!(
        count(
            &mut conn,
            "SELECT COUNT(*) FROM conversations WHERE id = $1",
            id
        )
        .await,
        1
    );
}

#[tokio::test]
async fn delete_reports_only_the_files_no_remaining_message_uses() {
    let vault = crate::test_support::test_vault().await;
    vault.account_with_id(ACCOUNT_A, "a").await;
    let mut conn = vault.conn().await;
    let shared = sha('a');
    let only_here = sha('b');
    let derived = sha('c');
    let staged = sha('d');

    // The conversation to delete: one message with the shared file, one with
    // a file only it holds (plus a derivative), one with a file an import in
    // progress has also staged.
    let doomed = insert_conversation_on(&mut conn, ACCOUNT_A, "+15550001").await;
    let m1 = insert_message(&mut conn, ACCOUNT_A, doomed, 0).await;
    insert_attachment(&mut conn, m1, &shared, None).await;
    let m2 = insert_message(&mut conn, ACCOUNT_A, doomed, 1).await;
    insert_attachment(&mut conn, m2, &only_here, Some(&derived)).await;
    // The same file twice in one conversation is reported once.
    insert_attachment(&mut conn, m2, &only_here, Some(&derived)).await;
    let m3 = insert_message(&mut conn, ACCOUNT_A, doomed, 2).await;
    insert_attachment(&mut conn, m3, &staged, None).await;

    // Another conversation still points at the shared file.
    let kept = insert_conversation_on(&mut conn, ACCOUNT_A, "+15550002").await;
    let k1 = insert_message(&mut conn, ACCOUNT_A, kept, 0).await;
    insert_attachment(&mut conn, k1, &shared, None).await;

    // And staging holds the fourth, mid-import.
    let staging_conversation: i64 = sqlx::query_scalar(
        "INSERT INTO staging_conversations (
            account_id, chat_handle_id, conversation_type, source_file
         ) VALUES ($1, 1, 'individual', 's.jsonl') RETURNING id",
    )
    .bind(ACCOUNT_A)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let staging_message: i64 = sqlx::query_scalar(
        "INSERT INTO staging_messages (
            conversation_id, account_id, source, timestamp, is_from_me, sort_order
         ) VALUES ($1, $2, 'imessage', '2020-01-01T00:00:00Z', 1, 0) RETURNING id",
    )
    .bind(staging_conversation)
    .bind(ACCOUNT_A)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO staging_attachments (message_id, sha256, assets_path) VALUES ($1, $2, 'x')",
    )
    .bind(staging_message)
    .bind(&staged)
    .execute(&mut *conn)
    .await
    .unwrap();

    move_to_trash(&mut conn, ACCOUNT_A, Trashable::Conversation(doomed))
        .await
        .unwrap();
    let outcome = delete_trashed(&mut conn, ACCOUNT_A, Trashable::Conversation(doomed))
        .await
        .unwrap();

    assert_eq!(
        outcome,
        DeleteOutcome::Deleted(vec![
            OrphanedFile::Original {
                source: "imessage".into(),
                sha256: only_here.clone(),
                assets_path: format!("bb/{only_here}.jpg"),
            },
            OrphanedFile::Derived {
                source: "imessage".into(),
                assets_path: format!("cc/{derived}.jpg"),
            },
        ]),
        "the shared file and the staged file must not be reported"
    );
    assert_eq!(
        count(
            &mut conn,
            "SELECT COUNT(*) FROM attachments a JOIN messages m ON m.id = a.message_id
             WHERE m.conversation_id = $1",
            kept
        )
        .await,
        1,
        "the other conversation's attachment row survives"
    );
}

/// A contact named by the person, in a Contact Group, with one conversation
/// on its handle; returns `(contact_id, conversation_id)`.
async fn insert_named_contact_in_a_conversation(
    conn: &mut AnyConnection,
    account_id: i64,
    raw: &str,
) -> (i64, i64) {
    let contact_id: i64 = sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name, origin)
         VALUES ($1, 'Pat', 'user') RETURNING id",
    )
    .bind(account_id)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let conversation_id = insert_conversation_on(conn, account_id, raw).await;
    let handle_id: i64 =
        sqlx::query_scalar("SELECT chat_handle_id FROM conversations WHERE id = $1")
            .bind(conversation_id)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO contact_handles (account_id, handle_id, contact_id, origin)
         VALUES ($1, $2, $3, 'user')",
    )
    .bind(account_id)
    .bind(handle_id)
    .bind(contact_id)
    .execute(&mut *conn)
    .await
    .unwrap();
    insert_message(conn, account_id, conversation_id, 0).await;
    let group_id: i64 = sqlx::query_scalar(
        "INSERT INTO contact_groups (account_id, name) VALUES ($1, $2) RETURNING id",
    )
    .bind(account_id)
    .bind(format!("Family {raw}"))
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    sqlx::query("INSERT INTO contact_group_members (contact_id, group_id) VALUES ($1, $2)")
        .bind(contact_id)
        .bind(group_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    (contact_id, conversation_id)
}

async fn contact_row(conn: &mut AnyConnection, contact_id: i64) -> Option<(String, String)> {
    sqlx::query_as("SELECT preferred_name, origin FROM contacts WHERE id = $1")
        .bind(contact_id)
        .fetch_optional(&mut *conn)
        .await
        .unwrap()
}

#[tokio::test]
async fn delete_trashed_contact_makes_it_unknown_and_leaves_its_conversations() {
    let vault = crate::test_support::test_vault().await;
    vault.account_with_id(ACCOUNT_A, "a").await;
    let mut conn = vault.conn().await;
    let (contact_id, conversation_id) =
        insert_named_contact_in_a_conversation(&mut conn, ACCOUNT_A, "+15550001").await;
    move_to_trash(&mut conn, ACCOUNT_A, Trashable::Contact(contact_id))
        .await
        .unwrap();

    let outcome = delete_trashed(&mut conn, ACCOUNT_A, Trashable::Contact(contact_id))
        .await
        .unwrap();

    assert_eq!(outcome, DeleteOutcome::Deleted(Vec::new()));
    assert_eq!(
        contact_row(&mut conn, contact_id).await,
        Some((String::new(), "import".into())),
        "the row stays, nameless and an import's again"
    );
    assert_eq!(
        count(
            &mut conn,
            "SELECT COUNT(*) FROM contact_handles WHERE contact_id = $1",
            contact_id
        )
        .await,
        1,
        "the handle stays linked"
    );
    assert_eq!(
        count(
            &mut conn,
            "SELECT COUNT(*) FROM contact_group_members WHERE contact_id = $1",
            contact_id
        )
        .await,
        0,
        "group memberships go"
    );
    assert_eq!(trashed_contact_count(&mut conn, ACCOUNT_A).await, 0);
    assert_eq!(
        count(
            &mut conn,
            "SELECT COUNT(*) FROM messages WHERE conversation_id = $1",
            conversation_id
        )
        .await,
        1,
        "the conversation and its messages are untouched"
    );
}

#[tokio::test]
async fn delete_contact_not_in_the_trash_is_refused_and_keeps_the_name() {
    let vault = crate::test_support::test_vault().await;
    vault.account_with_id(ACCOUNT_A, "a").await;
    let mut conn = vault.conn().await;
    let (contact_id, _) =
        insert_named_contact_in_a_conversation(&mut conn, ACCOUNT_A, "+15550001").await;

    let outcome = delete_trashed(&mut conn, ACCOUNT_A, Trashable::Contact(contact_id))
        .await
        .unwrap();

    assert_eq!(outcome, DeleteOutcome::NotTrashed);
    assert_eq!(
        contact_row(&mut conn, contact_id).await,
        Some(("Pat".into(), "user".into()))
    );
}

#[tokio::test]
async fn empty_trash_takes_everything_trashed_and_only_that() {
    let vault = crate::test_support::test_vault().await;
    for account in [ACCOUNT_A, ACCOUNT_B] {
        vault.account_with_id(account, &account.to_string()).await;
    }
    let mut conn = vault.conn().await;
    let trashed_conversation = insert_conversation_on(&mut conn, ACCOUNT_A, "+15550001").await;
    let m = insert_message(&mut conn, ACCOUNT_A, trashed_conversation, 0).await;
    insert_attachment(&mut conn, m, &sha('a'), None).await;
    let kept_conversation = insert_conversation_on(&mut conn, ACCOUNT_A, "+15550002").await;
    let (trashed_contact, _) =
        insert_named_contact_in_a_conversation(&mut conn, ACCOUNT_A, "+15550003").await;
    let (kept_contact, _) =
        insert_named_contact_in_a_conversation(&mut conn, ACCOUNT_A, "+15550004").await;
    let other_accounts = insert_conversation_on(&mut conn, ACCOUNT_B, "+15550005").await;
    for (account, target) in [
        (ACCOUNT_A, Trashable::Conversation(trashed_conversation)),
        (ACCOUNT_A, Trashable::Contact(trashed_contact)),
        (ACCOUNT_B, Trashable::Conversation(other_accounts)),
    ] {
        move_to_trash(&mut conn, account, target).await.unwrap();
    }
    // A marker whose conversation is already gone is cleared too.
    sqlx::query(
        "INSERT INTO trashed_conversations (account_id, conversation_id) VALUES ($1, 999999)",
    )
    .bind(ACCOUNT_A)
    .execute(&mut *conn)
    .await
    .unwrap();

    let orphaned = empty_trash(&mut conn, ACCOUNT_A).await.unwrap();

    assert_eq!(
        orphaned,
        vec![OrphanedFile::Original {
            source: "imessage".into(),
            sha256: sha('a'),
            assets_path: format!("aa/{}.jpg", sha('a')),
        }]
    );
    for (id, expected) in [
        (trashed_conversation, 0),
        (kept_conversation, 1),
        (other_accounts, 1),
    ] {
        assert_eq!(
            count(
                &mut conn,
                "SELECT COUNT(*) FROM conversations WHERE id = $1",
                id
            )
            .await,
            expected,
            "conversation {id}"
        );
    }
    assert_eq!(
        contact_row(&mut conn, trashed_contact).await,
        Some((String::new(), "import".into()))
    );
    assert_eq!(
        contact_row(&mut conn, kept_contact).await,
        Some(("Pat".into(), "user".into()))
    );
    assert_eq!(trashed_conversation_count(&mut conn, ACCOUNT_A).await, 0);
    assert_eq!(trashed_contact_count(&mut conn, ACCOUNT_A).await, 0);
    assert_eq!(
        trashed_conversation_count(&mut conn, ACCOUNT_B).await,
        1,
        "another account's trash is not emptied"
    );
}

#[tokio::test]
async fn empty_trash_on_an_empty_trash_is_a_noop() {
    let vault = crate::test_support::test_vault().await;
    vault.account_with_id(ACCOUNT_A, "a").await;
    let mut conn = vault.conn().await;
    let id = insert_conversation_on(&mut conn, ACCOUNT_A, "+15550001").await;

    assert_eq!(empty_trash(&mut conn, ACCOUNT_A).await.unwrap(), Vec::new());
    assert_eq!(
        count(
            &mut conn,
            "SELECT COUNT(*) FROM conversations WHERE id = $1",
            id
        )
        .await,
        1
    );
}
