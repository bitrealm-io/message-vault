use super::*;

const TEST_ACCOUNT_ID: &str = "00000000-0000-0000-0000-000000000042";

#[test]
fn email_detection() {
    assert!(is_email_handle("a@b.com"));
    assert!(!is_email_handle("+15551234567"));
    assert_eq!(
        phone_handles_only(&[
            "+15551234567".into(),
            "a@b.com".into(),
            "+15559876543".into()
        ]),
        vec![
            ("+15551234567".to_string(), None),
            ("+15559876543".to_string(), None)
        ]
    );
}

/// One table for the naming rule ADR-0006 sets, read at the seam that
/// enforces it rather than through the three callers that used to carry
/// their own copy of it.
#[tokio::test]
async fn who_may_name_a_contact() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    crate::db::schema::ensure_vault_schema(&mut conn)
        .await
        .unwrap();
    sqlx::query("INSERT INTO accounts (id, username) VALUES ($1, 't')")
        .bind(TEST_ACCOUNT_ID)
        .execute(&mut *conn)
        .await
        .unwrap();

    let name_of = async |conn: &mut AnyConnection, id: i64| -> String {
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

    // The person outranks both, and nothing renames the row afterwards.
    assert!(
        propose_name(&mut conn, TEST_ACCOUNT_ID, nameless, "Bob S", Origin::User)
            .await
            .unwrap()
    );
    for by in [Origin::Import, Origin::AddressBook] {
        assert!(
            !propose_name(&mut conn, TEST_ACCOUNT_ID, nameless, "Someone Else", by)
                .await
                .unwrap(),
            "{by:?} must not rename a contact the person named"
        );
    }
    assert_eq!(name_of(&mut conn, nameless).await, "Bob S");

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

#[tokio::test]
async fn trunk_zero_phone_is_flagged_with_note() {
    let (pool, dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    crate::db::schema::ensure_vault_schema(&mut conn)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO accounts (id, username, preferred_name)
         VALUES ($1, 't', 'T')",
    )
    .bind(TEST_ACCOUNT_ID)
    .execute(&mut *conn)
    .await
    .unwrap();
    let vcf_path = dir.path().join("contacts.vcf");
    std::fs::write(
        &vcf_path,
        "BEGIN:VCARD\nVERSION:3.0\nFN:UK Peer\nN:Peer;UK;;;\nTEL:020 7946 0000\nEND:VCARD\n",
    )
    .unwrap();

    let stats = load_contacts_if_needed(&mut conn, Some(&vcf_path), true, TEST_ACCOUNT_ID)
        .await
        .unwrap();
    assert_eq!(stats.phones, 1);
    assert_eq!(stats.phones_needing_review, 1);

    // Guarded policy: normalized mirrors the digits (no fabricated
    // +02079460000) and the handles row carries a review note.
    let (normalized, note): (String, Option<String>) = sqlx::query_as(
        "SELECT normalized, normalized_note FROM handles
         WHERE account_id = $1 AND handle_type = 'phone'",
    )
    .bind(TEST_ACCOUNT_ID)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(normalized, "02079460000");
    assert!(
        note.as_deref().is_some(),
        "trunk-zero phone must carry a review note"
    );
}

#[test]
fn accepts_vcard_csv_and_vcf_but_rejects_vault_csv() {
    let dir = std::env::temp_dir().join(format!(
        "mv-contacts-fmt-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let vcard_csv = dir.join("contacts.csv");
    std::fs::write(
        &vcard_csv,
        "First Name,Last Name,Mobile Phone\nAda,Lovelace,+15551234567\n",
    )
    .unwrap();
    assert_eq!(
        contacts_file_format(&vcard_csv).unwrap(),
        ContactsFormat::VcardCsv
    );

    // Vault's own export CSV (phones/first_name/last_name) is not an address book.
    let vault_export = dir.join("vault-export.csv");
    std::fs::write(
        &vault_export,
        "phones,first_name,last_name,label_1\n+15551234567,Ada,Lovelace,Family\n",
    )
    .unwrap();
    assert!(contacts_file_format(&vault_export).is_err());

    let vcf = dir.join("book.vcf");
    std::fs::write(
        &vcf,
        "BEGIN:VCARD\nVERSION:3.0\nFN:Ada Lovelace\nTEL:+15551234567\nEND:VCARD\n",
    )
    .unwrap();
    assert_eq!(contacts_file_format(&vcf).unwrap(), ContactsFormat::Vcf);

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn loads_vcard_csv_into_sqlite() {
    let (pool, dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    crate::db::schema::ensure_vault_schema(&mut conn)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO accounts (id, username, preferred_name)
         VALUES ($1, 't', 'T')",
    )
    .bind(TEST_ACCOUNT_ID)
    .execute(&mut *conn)
    .await
    .unwrap();
    let csv_path = dir.path().join("contacts.csv");
    std::fs::write(
        &csv_path,
        "First Name,Middle Name,Last Name,Mobile Phone,Home Phone\n\
         Ada,Augusta,Lovelace,+15551234567,+15559876543\n\
         NoPhone,,,+\n",
    )
    .unwrap();

    let stats = load_contacts_if_needed(&mut conn, Some(&csv_path), true, TEST_ACCOUNT_ID)
        .await
        .unwrap();
    assert_eq!(stats.contacts, 1);
    assert_eq!(stats.phones, 2);

    let name: String =
        sqlx::query_scalar("SELECT preferred_name FROM contacts WHERE account_id = $1")
            .bind(TEST_ACCOUNT_ID)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(name, "Ada Augusta Lovelace");
}

#[tokio::test]
async fn loads_vcf_into_sqlite() {
    let (pool, dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    crate::db::schema::ensure_vault_schema(&mut conn)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO accounts (id, username, preferred_name)
         VALUES ($1, 't', 'T')",
    )
    .bind(TEST_ACCOUNT_ID)
    .execute(&mut *conn)
    .await
    .unwrap();
    let vcf_path = dir.path().join("contacts.vcf");
    std::fs::write(
        &vcf_path,
        "BEGIN:VCARD\nVERSION:3.0\nFN:Ada Augusta Lovelace\nN:Lovelace;Ada;Augusta;;\nTEL:+15551234567\nCATEGORIES:Family\nEND:VCARD\n\
         BEGIN:VCARD\nVERSION:3.0\nFN:Ada Duplicate\nN:Duplicate;Ada;;;\nTEL:+15551234567\nTEL:+15559876543\nCATEGORIES:Work\nEND:VCARD\n\
         BEGIN:VCARD\nVERSION:3.0\nFN:Mononym\nN:;Mononym;;;\nTEL:+15557654321\nCATEGORIES:Friends\nEND:VCARD\n",
    )
    .unwrap();

    let stats = load_contacts_if_needed(&mut conn, Some(&vcf_path), true, TEST_ACCOUNT_ID)
        .await
        .unwrap();
    assert_eq!(stats.contacts, 2);
    assert_eq!(stats.phones, 3);
    // An address book never creates Contact Groups: those belong to the
    // person, and a CATEGORIES line is not one of theirs (#322).
    let groups: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM contact_groups WHERE account_id = $1")
            .bind(TEST_ACCOUNT_ID)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(groups, 0);

    let preferred_name: String = sqlx::query_scalar(
        "SELECT c.preferred_name FROM contacts c
         JOIN contact_handles ch ON ch.contact_id = c.id
         JOIN handles h ON h.id = ch.handle_id
         WHERE c.account_id = $1 AND h.normalized = '+15551234567'",
    )
    .bind(TEST_ACCOUNT_ID)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(preferred_name, "Ada Augusta Lovelace");

    let groups: Vec<String> = sqlx::query_scalar(
        "SELECT cg.name FROM contact_groups cg
         JOIN contact_group_members m ON m.group_id = cg.id
         WHERE cg.account_id = $1 ORDER BY cg.name",
    )
    .bind(TEST_ACCOUNT_ID)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert!(
        groups.is_empty(),
        "the address book must not create Contact Groups: {groups:?}"
    );
}
