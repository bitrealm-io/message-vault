use super::*;
use crate::db::schema;

const TEST_ACCOUNT: i64 = 7;

/// Insert an account, one conversation, and one participant on `handle`
/// whose backup name is `name_alias`. Returns (conversation_id, handle_id).
async fn seed(
    conn: &mut sqlx::SqliteConnection,
    handle: &str,
    name_alias: Option<&str>,
) -> (i64, i64) {
    schema::ensure_schema(conn).await.unwrap();
    crate::db::account_profile::ensure_account_row(conn, TEST_ACCOUNT)
        .await
        .unwrap();
    let handle_id: i64 = sqlx::query_scalar(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES ($1, $2, $2, 'phone', 'imessage') RETURNING id",
    )
    .bind(TEST_ACCOUNT)
    .bind(handle)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let conversation_id: i64 = sqlx::query_scalar(
        "INSERT INTO conversations
             (account_id, chat_handle_id, conversation_type, source_file)
         VALUES ($1, $2, 'individual', 'c.jsonl') RETURNING id",
    )
    .bind(TEST_ACCOUNT)
    .bind(handle_id)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO participants (conversation_id, handle_id, name_alias)
         VALUES ($1, $2, $3)",
    )
    .bind(conversation_id)
    .bind(handle_id)
    .bind(name_alias)
    .execute(&mut *conn)
    .await
    .unwrap();
    (conversation_id, handle_id)
}

async fn link(conn: &mut sqlx::SqliteConnection, handle_id: i64, preferred_name: &str) -> i64 {
    let contact_id: i64 = sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name) VALUES ($1, $2) RETURNING id",
    )
    .bind(TEST_ACCOUNT)
    .bind(preferred_name)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO contact_handles (account_id, handle_id, contact_id)
         VALUES ($1, $2, $3)",
    )
    .bind(TEST_ACCOUNT)
    .bind(handle_id)
    .bind(contact_id)
    .execute(&mut *conn)
    .await
    .unwrap();
    contact_id
}

/// Insert a participant the source named with no address on
/// `conversation_id`, the way an import does: an identity of type `other`
/// holding the name, on a fresh contact carrying the name. Returns the
/// contact id.
async fn seed_address_less(
    conn: &mut sqlx::SqliteConnection,
    conversation_id: i64,
    name_alias: &str,
) -> i64 {
    let handle_id: i64 = sqlx::query_scalar(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES ($1, $2, $2, 'other', 'phone') RETURNING id",
    )
    .bind(TEST_ACCOUNT)
    .bind(name_alias)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let contact_id = link(conn, handle_id, name_alias).await;
    sqlx::query(
        "INSERT INTO participants (conversation_id, handle_id, name_alias)
         VALUES ($1, $2, $3)",
    )
    .bind(conversation_id)
    .bind(handle_id)
    .bind(name_alias)
    .execute(&mut *conn)
    .await
    .unwrap();
    contact_id
}

#[tokio::test]
async fn contact_name_wins_over_the_backup_name() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    let (conversation_id, handle_id) = seed(&mut conn, "+15555550100", Some("Bobby")).await;
    let contact_id = link(&mut conn, handle_id, "Robert Smith").await;

    let loaded = load_for_conversations(&mut conn, &[conversation_id])
        .await
        .unwrap();
    let p = &loaded[&conversation_id][0];
    assert_eq!(p.name, "Robert Smith");
    assert_eq!(p.identity, Some("+15555550100".to_string()));
    assert_eq!(p.service, Some("imessage".to_string()));
    assert_eq!(p.contact_id, Some(contact_id));
}

#[tokio::test]
async fn backup_name_shows_when_the_contact_has_none() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    let (conversation_id, handle_id) = seed(&mut conn, "+15555550200", Some("Bobby")).await;
    link(&mut conn, handle_id, "   ").await;

    let loaded = load_for_conversations(&mut conn, &[conversation_id])
        .await
        .unwrap();
    assert_eq!(loaded[&conversation_id][0].name, "Bobby");
}

#[tokio::test]
async fn the_handle_shows_when_nothing_names_the_person() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    let (conversation_id, _handle_id) = seed(&mut conn, "+15555550300", None).await;

    let loaded = load_for_conversations(&mut conn, &[conversation_id])
        .await
        .unwrap();
    let p = &loaded[&conversation_id][0];
    assert_eq!(p.name, "+15555550300");
    assert_eq!(p.contact_id, None);
}

/// A backup that recorded the thread's address and nothing about who was
/// in it leaves no participants rows, but the database may still have a name
/// for the person on the other end.
#[tokio::test]
async fn the_chat_handle_takes_the_contact_name_and_id() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    let (conversation_id, handle_id) = seed(&mut conn, "+15555550500", None).await;
    sqlx::query("DELETE FROM participants WHERE conversation_id = $1")
        .bind(conversation_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    let contact_id = link(&mut conn, handle_id, "Robert Smith").await;

    let loaded = load_for_conversations(&mut conn, &[conversation_id])
        .await
        .unwrap();
    let loaded = loaded.get(&conversation_id).expect("chat-handle fallback");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].name, "Robert Smith");
    assert_eq!(loaded[0].identity, Some("+15555550500".to_string()));
    assert_eq!(loaded[0].service, Some("imessage".to_string()));
    assert_eq!(loaded[0].contact_id, Some(contact_id));
}

/// With nothing naming them, the handle stands in, and there is no
/// contact drawer to open.
#[tokio::test]
async fn the_chat_handle_falls_back_to_itself() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    let (conversation_id, _handle_id) = seed(&mut conn, "+15555550600", None).await;
    sqlx::query("DELETE FROM participants WHERE conversation_id = $1")
        .bind(conversation_id)
        .execute(&mut *conn)
        .await
        .unwrap();

    let loaded = load_for_conversations(&mut conn, &[conversation_id])
        .await
        .unwrap();
    let loaded = loaded.get(&conversation_id).expect("chat-handle fallback");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].name, "+15555550600");
    assert_eq!(loaded[0].contact_id, None);
}

/// A group's chat id names the conversation, not a person
/// (`docs/architecture/contacts-identities-and-messages.md`, "A group
/// conversation is not a person"), so a group with no participants rows has
/// no participants rather than one named `chat1000000005`.
#[tokio::test]
async fn a_group_with_no_participants_rows_has_no_participants() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    let (conversation_id, _handle_id) = seed(&mut conn, "chat1000000005", None).await;
    sqlx::query("UPDATE conversations SET conversation_type = 'group' WHERE id = $1")
        .bind(conversation_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query("DELETE FROM participants WHERE conversation_id = $1")
        .bind(conversation_id)
        .execute(&mut *conn)
        .await
        .unwrap();

    let loaded = load_for_conversations(&mut conn, &[conversation_id])
        .await
        .unwrap();
    assert!(
        loaded.get(&conversation_id).is_none_or(Vec::is_empty),
        "the group's chat id is not a participant: {loaded:?}"
    );
}

/// A participant the source named with no address is in their conversation
/// under the name, with the identity of type `other` that holds it and the
/// contact that identity is on.
#[tokio::test]
async fn an_address_less_participant_appears_in_their_conversation() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    let (conversation_id, _handle_id) = seed(&mut conn, "+15555550700", None).await;
    sqlx::query("DELETE FROM participants WHERE conversation_id = $1")
        .bind(conversation_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    let contact_id = seed_address_less(&mut conn, conversation_id, "Sarah Vale").await;

    let loaded = load_for_conversations(&mut conn, &[conversation_id])
        .await
        .unwrap();
    let p = &loaded[&conversation_id][0];
    assert_eq!(p.name, "Sarah Vale");
    assert_eq!(p.identity, Some("Sarah Vale".to_string()));
    assert_eq!(p.service, Some("phone".to_string()));
    assert_eq!(p.contact_id, Some(contact_id));
}

/// A conversation can hold both kinds of participant at once — one with
/// an address, one without — and both come back in participant-id order,
/// so a conversation's roster is stable regardless of which shape each
/// member is.
#[tokio::test]
async fn addressed_and_address_less_participants_both_return_in_id_order() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    let (conversation_id, _handle_id) = seed(&mut conn, "+15555550800", Some("Bobby")).await;
    let address_less_contact = seed_address_less(&mut conn, conversation_id, "Sarah Vale").await;

    let loaded = load_for_conversations(&mut conn, &[conversation_id])
        .await
        .unwrap();
    let participants = &loaded[&conversation_id];
    assert_eq!(participants.len(), 2);
    assert_eq!(participants[0].name, "Bobby");
    assert_eq!(participants[0].identity, Some("+15555550800".to_string()));
    assert_eq!(participants[1].name, "Sarah Vale");
    assert_eq!(participants[1].identity, Some("Sarah Vale".to_string()));
    assert_eq!(participants[1].contact_id, Some(address_less_contact));
}

/// The module's founding guarantee — naming a Contact renames them in
/// every conversation at once — holds for a participant the source named
/// with no address too, even though nothing ever rewrites
/// `participants.name_alias` after import (ADR-0006 keeps it as the
/// backup's own record): the rename reaches them through their identity.
#[tokio::test]
async fn renaming_the_contact_renames_an_address_less_participant_too() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    let (conversation_id, _handle_id) = seed(&mut conn, "+15555550900", None).await;
    sqlx::query("DELETE FROM participants WHERE conversation_id = $1")
        .bind(conversation_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    let contact_id = seed_address_less(&mut conn, conversation_id, "Sarah Vale").await;

    sqlx::query("UPDATE contacts SET preferred_name = 'Sarah Connor' WHERE id = $1")
        .bind(contact_id)
        .execute(&mut *conn)
        .await
        .unwrap();

    let loaded = load_for_conversations(&mut conn, &[conversation_id])
        .await
        .unwrap();
    assert_eq!(
        loaded[&conversation_id][0].name, "Sarah Connor",
        "the Contact's new name reaches an address-less participant"
    );
}

async fn trash_contact(conn: &mut sqlx::SqliteConnection, contact_id: i64) {
    let owned = crate::db::trash::move_to_trash(
        conn,
        TEST_ACCOUNT,
        crate::db::trash::Trashable::Contact(contact_id),
    )
    .await
    .unwrap();
    assert!(owned);
}

async fn restore_contact(conn: &mut sqlx::SqliteConnection, contact_id: i64) {
    let owned = crate::db::trash::restore(
        conn,
        TEST_ACCOUNT,
        crate::db::trash::Trashable::Contact(contact_id),
    )
    .await
    .unwrap();
    assert!(owned);
}

/// A trashed contact is not one of a conversation's contacts (CONTEXT.md,
/// "Trash"): its participant is named as if no contact held the identity, and
/// carries no contact id, because `GET /v1/contacts/{id}` answers 404 for it.
/// Restoring the contact brings the name and the id back.
#[tokio::test]
async fn a_trashed_contact_neither_names_nor_links_its_participant() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    let (conversation_id, handle_id) = seed(&mut conn, "+15555550123", Some("Bobby")).await;
    let (bare_conversation, bare_handle) = seed(&mut conn, "+15555550124", None).await;
    let contact_id = link(&mut conn, handle_id, "Ada").await;
    let bare_contact = link(&mut conn, bare_handle, "Grace").await;
    trash_contact(&mut conn, contact_id).await;
    trash_contact(&mut conn, bare_contact).await;

    let loaded = load_for_conversations(&mut conn, &[conversation_id, bare_conversation])
        .await
        .unwrap();
    let p = &loaded[&conversation_id][0];
    assert_eq!(p.name, "Bobby");
    assert_eq!(p.contact_id, None);
    let p = &loaded[&bare_conversation][0];
    assert_eq!(p.name, "+15555550124");
    assert_eq!(p.contact_id, None);

    restore_contact(&mut conn, contact_id).await;
    let loaded = load_for_conversations(&mut conn, &[conversation_id])
        .await
        .unwrap();
    let p = &loaded[&conversation_id][0];
    assert_eq!(p.name, "Ada");
    assert_eq!(p.contact_id, Some(contact_id));
}

/// The same for a participant the source named with no address: the
/// backup's name stays, the link goes.
#[tokio::test]
async fn a_trashed_contact_does_not_link_an_address_less_participant() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    let (conversation_id, _handle_id) = seed(&mut conn, "+15555550125", None).await;
    sqlx::query("DELETE FROM participants WHERE conversation_id = $1")
        .bind(conversation_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    let contact_id = seed_address_less(&mut conn, conversation_id, "Sarah Vale").await;
    sqlx::query("UPDATE contacts SET preferred_name = 'Sarah Connor' WHERE id = $1")
        .bind(contact_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    trash_contact(&mut conn, contact_id).await;

    let loaded = load_for_conversations(&mut conn, &[conversation_id])
        .await
        .unwrap();
    let p = &loaded[&conversation_id][0];
    assert_eq!(p.name, "Sarah Vale");
    assert_eq!(p.contact_id, None);
}

/// The chat-handle fallback, for a conversation with no participants rows,
/// leaves a trashed contact out the same way.
#[tokio::test]
async fn the_chat_handle_ignores_a_trashed_contact() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    let (conversation_id, handle_id) = seed(&mut conn, "+15555550126", None).await;
    sqlx::query("DELETE FROM participants WHERE conversation_id = $1")
        .bind(conversation_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    let contact_id = link(&mut conn, handle_id, "Ada").await;
    trash_contact(&mut conn, contact_id).await;

    let loaded = load_for_conversations(&mut conn, &[conversation_id])
        .await
        .unwrap();
    let p = &loaded[&conversation_id][0];
    assert_eq!(p.name, "+15555550126");
    assert_eq!(p.contact_id, None);
}
