use super::*;

const TEST_ACCOUNT_ID: i64 = 7;

#[tokio::test]
async fn who_may_name_a_contact() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    crate::db::schema::ensure_schema(&mut conn).await.unwrap();
    sqlx::query("INSERT INTO accounts (id, username) VALUES ($1, 't')")
        .bind(TEST_ACCOUNT_ID)
        .execute(&mut *conn)
        .await
        .unwrap();

    let name_of = async |conn: &mut SqliteConnection, id: i64| -> String {
        sqlx::query_scalar("SELECT preferred_name FROM contacts WHERE id = $1")
            .bind(id)
            .fetch_one(&mut *conn)
            .await
            .unwrap()
    };

    // An import names a contact no earlier import managed to name, and
    // leaves the first spelling alone after that.
    let nameless = create_contact(&mut conn, TEST_ACCOUNT_ID, "", Origin::Import)
        .await
        .unwrap();
    assert!(
        propose_name(
            &mut conn,
            TEST_ACCOUNT_ID,
            nameless,
            "Bobby",
            Origin::Import
        )
        .await
        .unwrap()
    );
    assert!(
        !propose_name(&mut conn, TEST_ACCOUNT_ID, nameless, "Bob", Origin::Import)
            .await
            .unwrap()
    );
    assert_eq!(name_of(&mut conn, nameless).await, "Bobby");

    // An address book outranks an import.
    assert!(
        propose_name(
            &mut conn,
            TEST_ACCOUNT_ID,
            nameless,
            "Robert Smith",
            Origin::AddressBook
        )
        .await
        .unwrap()
    );
    assert_eq!(name_of(&mut conn, nameless).await, "Robert Smith");

    // The person typing outranks an import, which never renames the row
    // afterwards.
    assert!(
        propose_name(&mut conn, TEST_ACCOUNT_ID, nameless, "Bob S", Origin::User)
            .await
            .unwrap()
    );
    assert!(
        !propose_name(
            &mut conn,
            TEST_ACCOUNT_ID,
            nameless,
            "Someone Else",
            Origin::Import
        )
        .await
        .unwrap(),
        "an import must not rename a contact the person named"
    );
    assert_eq!(name_of(&mut conn, nameless).await, "Bob S");

    // An address book is the person typing in a spreadsheet, so it renames a
    // typed name too, and the name it already has is no change.
    assert!(
        !propose_name(
            &mut conn,
            TEST_ACCOUNT_ID,
            nameless,
            "Bob S",
            Origin::AddressBook
        )
        .await
        .unwrap()
    );
    assert!(
        propose_name(
            &mut conn,
            TEST_ACCOUNT_ID,
            nameless,
            "Bob Smith",
            Origin::AddressBook
        )
        .await
        .unwrap()
    );
    assert_eq!(name_of(&mut conn, nameless).await, "Bob Smith");

    // An empty name says nothing about who someone is.
    let blank = create_contact(&mut conn, TEST_ACCOUNT_ID, "", Origin::Import)
        .await
        .unwrap();
    assert!(
        !propose_name(&mut conn, TEST_ACCOUNT_ID, blank, "   ", Origin::User)
            .await
            .unwrap()
    );
    assert_eq!(name_of(&mut conn, blank).await, "");
}
