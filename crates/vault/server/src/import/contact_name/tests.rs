use super::*;
use crate::db::schema;

const TEST_ACCOUNT: &str = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";

#[tokio::test]
async fn an_import_creates_the_contact_with_the_backup_name() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    schema::ensure_vault_schema(&mut conn).await.unwrap();
    crate::db::account_profile::ensure_account_row(&mut conn, TEST_ACCOUNT)
        .await
        .unwrap();
    let handle_id: i64 = sqlx::query_scalar(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES ($1, '+15555550700', '+15555550700', 'phone', 'imessage') RETURNING id",
    )
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();

    let mut stats = ImportStats::default();
    let contact_id =
        ensure_contact_for_handle(&mut conn, TEST_ACCOUNT, handle_id, Some("Ada"), &mut stats)
            .await
            .unwrap();

    let (name, origin): (String, String) =
        sqlx::query_as("SELECT preferred_name, origin FROM contacts WHERE id = $1")
            .bind(contact_id)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(name, "Ada");
    assert_eq!(origin, "import");
}

#[tokio::test]
async fn a_later_backup_names_a_contact_an_earlier_one_left_nameless() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    schema::ensure_vault_schema(&mut conn).await.unwrap();
    crate::db::account_profile::ensure_account_row(&mut conn, TEST_ACCOUNT)
        .await
        .unwrap();
    let handle_id: i64 = sqlx::query_scalar(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES ($1, '+15555550800', '+15555550800', 'phone', 'imessage') RETURNING id",
    )
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();

    let mut stats = ImportStats::default();
    let first = ensure_contact_for_handle(&mut conn, TEST_ACCOUNT, handle_id, None, &mut stats)
        .await
        .unwrap();
    let second =
        ensure_contact_for_handle(&mut conn, TEST_ACCOUNT, handle_id, Some("Ada"), &mut stats)
            .await
            .unwrap();
    assert_eq!(first, second, "the same handle keeps the same contact");

    let name: String = sqlx::query_scalar("SELECT preferred_name FROM contacts WHERE id = $1")
        .bind(first)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(name, "Ada");
}

/// Trash sets a person aside; it does not make them absent. A re-import
/// that meets their handle attaches to the trashed contact and leaves it
/// where the person put it, rather than minting a second contact whose
/// handle link would then collide with the first (#328).
#[tokio::test]
async fn an_import_reuses_a_trashed_contact_and_leaves_it_trashed() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    schema::ensure_vault_schema(&mut conn).await.unwrap();
    crate::db::account_profile::ensure_account_row(&mut conn, TEST_ACCOUNT)
        .await
        .unwrap();
    let handle_id: i64 = sqlx::query_scalar(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES ($1, '+15555550950', '+15555550950', 'phone', 'imessage') RETURNING id",
    )
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let mut stats = ImportStats::default();
    let first =
        ensure_contact_for_handle(&mut conn, TEST_ACCOUNT, handle_id, Some("Ada"), &mut stats)
            .await
            .unwrap();
    sqlx::query("INSERT INTO trashed_contacts (account_id, contact_id) VALUES ($1, $2)")
        .bind(TEST_ACCOUNT)
        .bind(first)
        .execute(&mut *conn)
        .await
        .unwrap();

    let second =
        ensure_contact_for_handle(&mut conn, TEST_ACCOUNT, handle_id, Some("Ada"), &mut stats)
            .await
            .unwrap();

    assert_eq!(
        first, second,
        "the trashed contact is reused, not duplicated"
    );
    let contacts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM contacts WHERE account_id = $1")
        .bind(TEST_ACCOUNT)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(contacts, 1);
    let still_trashed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM trashed_contacts WHERE account_id = $1 AND contact_id = $2",
    )
    .bind(TEST_ACCOUNT)
    .bind(first)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        still_trashed, 1,
        "an import does not restore what the person set aside"
    );
}

#[tokio::test]
async fn a_second_spelling_does_not_rename_anyone() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    schema::ensure_vault_schema(&mut conn).await.unwrap();
    crate::db::account_profile::ensure_account_row(&mut conn, TEST_ACCOUNT)
        .await
        .unwrap();
    let handle_id: i64 = sqlx::query_scalar(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES ($1, '+15555550900', '+15555550900', 'phone', 'imessage') RETURNING id",
    )
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();

    let mut stats = ImportStats::default();
    let contact_id = ensure_contact_for_handle(
        &mut conn,
        TEST_ACCOUNT,
        handle_id,
        Some("Ada Lovelace"),
        &mut stats,
    )
    .await
    .unwrap();
    ensure_contact_for_handle(
        &mut conn,
        TEST_ACCOUNT,
        handle_id,
        Some("ada l"),
        &mut stats,
    )
    .await
    .unwrap();

    let name: String = sqlx::query_scalar("SELECT preferred_name FROM contacts WHERE id = $1")
        .bind(contact_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(name, "Ada Lovelace", "first backup wins");
}

/// A name the person typed carries `origin = 'user'` and outranks any
/// backup, however many imports later run.
#[tokio::test]
async fn an_import_does_not_overwrite_a_name_the_person_typed() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    schema::ensure_vault_schema(&mut conn).await.unwrap();
    crate::db::account_profile::ensure_account_row(&mut conn, TEST_ACCOUNT)
        .await
        .unwrap();
    let handle_id: i64 = sqlx::query_scalar(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES ($1, '+15555551000', '+15555551000', 'phone', 'imessage') RETURNING id",
    )
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let contact_id = crate::db::contacts::create_contact(
        &mut conn,
        TEST_ACCOUNT,
        "",
        crate::db::contacts::Origin::User,
    )
    .await
    .unwrap();
    crate::db::contacts::link_handle_to_contact(
        &mut conn,
        TEST_ACCOUNT,
        handle_id,
        contact_id,
        crate::db::contacts::Origin::User,
    )
    .await
    .unwrap();

    let mut stats = ImportStats::default();
    ensure_contact_for_handle(&mut conn, TEST_ACCOUNT, handle_id, Some("Ada"), &mut stats)
        .await
        .unwrap();

    let name: String = sqlx::query_scalar("SELECT preferred_name FROM contacts WHERE id = $1")
        .bind(contact_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(name, "", "the person's contact is not the import's to name");
}

#[tokio::test]
async fn sibling_contact_link_bumps_last_modified_only_on_insert() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    schema::ensure_vault_schema(&mut conn).await.unwrap();
    crate::db::account_profile::ensure_account_row(&mut conn, TEST_ACCOUNT)
        .await
        .unwrap();

    let contact_id: i64 = sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name) VALUES ($1, 'Pat') RETURNING id",
    )
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let phone_id: i64 = sqlx::query_scalar(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES ($1, '+15555550100', '+15555550100', 'phone', 'phone') RETURNING id",
    )
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO contact_handles (account_id, handle_id, contact_id)
         VALUES ($1, $2, $3)",
    )
    .bind(TEST_ACCOUNT)
    .bind(phone_id)
    .bind(contact_id)
    .execute(&mut *conn)
    .await
    .unwrap();

    let wa_id: i64 = sqlx::query_scalar(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES ($1, '+15555550100', '+15555550100', 'phone', 'whatsapp') RETURNING id",
    )
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();

    const OLD: &str = "2000-01-01 00:00:00";
    sqlx::query("UPDATE contacts SET last_modified = $1 WHERE id = $2")
        .bind(OLD)
        .bind(contact_id)
        .execute(&mut *conn)
        .await
        .unwrap();

    let linked = ensure_sibling_contact_link(&mut conn, TEST_ACCOUNT, wa_id)
        .await
        .unwrap()
        .expect("sibling link");
    assert_eq!(linked, contact_id);
    let after_insert: String =
        sqlx::query_scalar("SELECT last_modified FROM contacts WHERE id = $1")
            .bind(contact_id)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_ne!(after_insert, OLD);

    sqlx::query("UPDATE contacts SET last_modified = $1 WHERE id = $2")
        .bind(OLD)
        .bind(contact_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    let again = ensure_sibling_contact_link(&mut conn, TEST_ACCOUNT, wa_id)
        .await
        .unwrap()
        .expect("already linked");
    assert_eq!(again, contact_id);
    let after_noop: String = sqlx::query_scalar("SELECT last_modified FROM contacts WHERE id = $1")
        .bind(contact_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(after_noop, OLD);
}
