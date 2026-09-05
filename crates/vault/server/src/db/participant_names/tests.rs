use super::*;
use crate::db::schema;

const TEST_ACCOUNT: &str = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";

/// Insert an account, one conversation, and one participant on `handle`
/// whose backup name is `name_alias`. Returns (conversation_id, handle_id).
async fn seed(
    conn: &mut sqlx::AnyConnection,
    handle: &str,
    name_alias: Option<&str>,
) -> (i64, i64) {
    schema::ensure_vault_schema(conn).await.unwrap();
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

async fn link(conn: &mut sqlx::AnyConnection, handle_id: i64, preferred_name: &str) -> i64 {
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

/// Insert an address-less participant on `conversation_id`: `handle_id
/// IS NULL`, bound to a fresh contact carrying `name_alias`. This is the
/// row shape `resolve_name_only_participant` produces — the contact link
/// lives on `participants.contact_id` directly, since there is no handle
/// for `contact_handles` to key on. Returns the contact id.
async fn seed_address_less(
    conn: &mut sqlx::AnyConnection,
    conversation_id: i64,
    name_alias: &str,
) -> i64 {
    let contact_id: i64 = sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name) VALUES ($1, $2) RETURNING id",
    )
    .bind(TEST_ACCOUNT)
    .bind(name_alias)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO participants (conversation_id, handle_id, contact_id, name_alias)
         VALUES ($1, NULL, $2, $3)",
    )
    .bind(conversation_id)
    .bind(contact_id)
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
    assert_eq!(p.handle, Some("+15555550100".to_string()));
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
/// in it leaves no participants rows, but the vault may still have a name
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
    assert_eq!(loaded[0].handle, Some("+15555550500".to_string()));
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

/// `participants.contact_id` is not consulted: only the link in
/// `contact_handles` names someone, so naming a Contact renames them in
/// every conversation at once.
#[tokio::test]
async fn a_participant_contact_id_does_not_name_anyone() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    let (conversation_id, handle_id) = seed(&mut conn, "+15555550400", Some("Bobby")).await;
    let stranger: i64 = sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name) VALUES ($1, 'Wrong') RETURNING id",
    )
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    sqlx::query("UPDATE participants SET contact_id = $1 WHERE handle_id = $2")
        .bind(stranger)
        .bind(handle_id)
        .execute(&mut *conn)
        .await
        .unwrap();

    let loaded = load_for_conversations(&mut conn, &[conversation_id])
        .await
        .unwrap();
    let p = &loaded[&conversation_id][0];
    assert_eq!(p.name, "Bobby");
    assert_eq!(p.contact_id, None);
}

/// `resolve_name_only_participant` binds a name-only participant straight
/// to a contact with `handle_id IS NULL`; the `INNER JOIN handles` this
/// module used to have dropped that row from every conversation it
/// belongs to. This pins that a `LEFT JOIN` brings it back, carrying the
/// name from `p.name_alias` (the naming rule's second clause — no handle
/// means no `h.raw` fallback and no contact to consult via
/// `contact_handles`), no handle, no service, and the contact bound
/// directly on the participant row, since that is the only place an
/// address-less participant's contact link is recorded.
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
    assert_eq!(p.handle, None);
    assert_eq!(p.service, None);
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
    assert_eq!(participants[0].handle, Some("+15555550800".to_string()));
    assert_eq!(participants[1].name, "Sarah Vale");
    assert_eq!(participants[1].handle, None);
    assert_eq!(participants[1].service, None);
    assert_eq!(participants[1].contact_id, Some(address_less_contact));
}

/// The module's founding guarantee — naming a Contact renames them in
/// every conversation at once — has to hold for a handle-less
/// participant too, even though nothing ever rewrites
/// `participants.name_alias` after import (ADR-0006 keeps it as the
/// backup's own record). The only way a rename can reach them is if the
/// `contacts` join keys on `p.contact_id` for this case, exactly as the
/// `contact_id` column already does.
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
        "the Contact's new name never reaches an address-less participant \
         unless the contacts join keys on p.contact_id for them"
    );
}
