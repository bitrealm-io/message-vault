use super::*;
use crate::db::schema;

const TEST_ACCOUNT: i64 = 7;

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

/// A trashed contact stays set aside until an import meets one of its
/// handles. Then the backup wins (ADR-0013): the trashed contact goes, name,
/// Contact Group memberships and every handle link with it, and the import
/// makes a fresh contact carrying only what the backup said. A handle the
/// trashed contact had that the backup did not mention belongs to nobody
/// afterwards.
#[tokio::test]
async fn an_import_replaces_a_trashed_contact_with_a_fresh_one() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    schema::ensure_vault_schema(&mut conn).await.unwrap();
    crate::db::account_profile::ensure_account_row(&mut conn, TEST_ACCOUNT)
        .await
        .unwrap();
    let met = insert_handle(&mut conn, "+15555550950", "imessage").await;
    let unmentioned = insert_handle(&mut conn, "+15555550951", "imessage").await;
    let mut stats = ImportStats::default();
    let old = ensure_contact_for_handle(&mut conn, TEST_ACCOUNT, met, Some("Ada"), &mut stats)
        .await
        .unwrap();
    // The person then curated the contact: a second handle, a name of their
    // own, a group. None of it survives the trash.
    crate::db::contacts::link_handle_to_contact(
        &mut conn,
        TEST_ACCOUNT,
        unmentioned,
        old,
        crate::db::contacts::Origin::User,
    )
    .await
    .unwrap();
    sqlx::query("UPDATE contacts SET preferred_name = 'Ada (work)', origin = 'user' WHERE id = $1")
        .bind(old)
        .execute(&mut *conn)
        .await
        .unwrap();
    let group_id: i64 = sqlx::query_scalar(
        "INSERT INTO contact_groups (account_id, name) VALUES ($1, 'Colleagues') RETURNING id",
    )
    .bind(TEST_ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    sqlx::query("INSERT INTO contact_group_members (contact_id, group_id) VALUES ($1, $2)")
        .bind(old)
        .bind(group_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query("INSERT INTO trashed_contacts (account_id, contact_id) VALUES ($1, $2)")
        .bind(TEST_ACCOUNT)
        .bind(old)
        .execute(&mut *conn)
        .await
        .unwrap();

    let fresh = ensure_contact_for_handle(
        &mut conn,
        TEST_ACCOUNT,
        met,
        Some("Ada Lovelace"),
        &mut stats,
    )
    .await
    .unwrap();

    // SQLite may hand the fresh row the id the deleted one had, so the ids
    // say nothing; what the row holds does.
    assert_eq!(stats.contacts_created, 2, "a fresh contact was created");
    let contacts: Vec<(i64, String, String)> =
        sqlx::query_as("SELECT id, preferred_name, origin FROM contacts WHERE account_id = $1")
            .bind(TEST_ACCOUNT)
            .fetch_all(&mut *conn)
            .await
            .unwrap();
    assert_eq!(
        contacts,
        vec![(fresh, "Ada Lovelace".to_string(), "import".to_string())],
        "only the fresh contact remains, named by the backup"
    );
    assert_eq!(
        count(
            &mut conn,
            "SELECT COUNT(*) FROM trashed_contacts WHERE account_id = $1",
            TEST_ACCOUNT
        )
        .await,
        0,
        "the trash marker goes with the contact"
    );
    assert_eq!(
        count(
            &mut conn,
            "SELECT COUNT(*) FROM contact_group_members WHERE contact_id = $1",
            old
        )
        .await,
        0,
        "and so do the group memberships"
    );
    assert_eq!(
        crate::db::contacts::contact_id_for_handle(&mut conn, TEST_ACCOUNT, met)
            .await
            .unwrap(),
        Some(fresh)
    );
    assert_eq!(
        crate::db::contacts::contact_id_for_handle(&mut conn, TEST_ACCOUNT, unmentioned)
            .await
            .unwrap(),
        None,
        "a handle the backup did not mention belongs to nobody"
    );
}

/// One number is one person on every service. When the fresh contact is
/// made, the same number on another service — a sibling handle the trashed
/// contact had — comes along, rather than turning Unknown.
#[tokio::test]
async fn a_fresh_contact_takes_the_number_on_every_service() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    schema::ensure_vault_schema(&mut conn).await.unwrap();
    crate::db::account_profile::ensure_account_row(&mut conn, TEST_ACCOUNT)
        .await
        .unwrap();
    let on_whatsapp = insert_handle(&mut conn, "+15555550960", "whatsapp").await;
    let on_phone = insert_handle(&mut conn, "+15555550960", "phone").await;
    let mut stats = ImportStats::default();
    let old = ensure_contact_for_handle(
        &mut conn,
        TEST_ACCOUNT,
        on_whatsapp,
        Some("Ada"),
        &mut stats,
    )
    .await
    .unwrap();
    sqlx::query("INSERT INTO trashed_contacts (account_id, contact_id) VALUES ($1, $2)")
        .bind(TEST_ACCOUNT)
        .bind(old)
        .execute(&mut *conn)
        .await
        .unwrap();

    let fresh =
        ensure_contact_for_handle(&mut conn, TEST_ACCOUNT, on_phone, Some("Ada"), &mut stats)
            .await
            .unwrap();

    assert_eq!(stats.contacts_created, 2, "a fresh contact was created");
    for handle in [on_whatsapp, on_phone] {
        assert_eq!(
            crate::db::contacts::contact_id_for_handle(&mut conn, TEST_ACCOUNT, handle)
                .await
                .unwrap(),
            Some(fresh),
            "both services' handles are on the fresh contact"
        );
    }
    assert_eq!(
        count(
            &mut conn,
            "SELECT COUNT(*) FROM contacts WHERE account_id = $1",
            TEST_ACCOUNT
        )
        .await,
        1
    );
}

async fn insert_handle(conn: &mut sqlx::AnyConnection, raw: &str, service: &str) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES ($1, $2, $2, 'phone', $3) RETURNING id",
    )
    .bind(TEST_ACCOUNT)
    .bind(raw)
    .bind(service)
    .fetch_one(&mut *conn)
    .await
    .unwrap()
}

/// `sql` is a `SELECT COUNT(*)` with one `$1` bind, `id`.
async fn count(conn: &mut sqlx::AnyConnection, sql: &str, id: i64) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(&mut *conn)
        .await
        .unwrap()
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
