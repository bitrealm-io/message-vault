use super::*;
use crate::db::handles::upsert_handle_row;

const ACCOUNT: i64 = 7;
const HEADER: &str = "contact_id,display_name,groups,service,handle_type,identity";

/// A database with the schema and one account, and a connection to it. The
/// pool and the directory are returned so they outlive the test body.
async fn account() -> (
    sqlx::pool::PoolConnection<sqlx::Sqlite>,
    sqlx::SqlitePool,
    tempfile::TempDir,
) {
    let (pool, dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    crate::db::schema::ensure_schema(&mut conn).await.unwrap();
    crate::db::account_profile::ensure_account_row(&mut conn, ACCOUNT)
        .await
        .unwrap();
    (conn, pool, dir)
}

/// A file from its data rows.
fn file(rows: &[&str]) -> String {
    let mut text = String::from(HEADER);
    for row in rows {
        text.push('\n');
        text.push_str(row);
    }
    text.push('\n');
    text
}

/// A contact as an import leaves it, holding the identities given as
/// `(service, handle_type, raw)`.
async fn imported(
    conn: &mut SqliteConnection,
    name: &str,
    identities: &[(&str, &str, &str)],
) -> i64 {
    let id = contacts::create_contact(conn, ACCOUNT, name, Origin::Import)
        .await
        .unwrap();
    for (service, handle_type, raw) in identities {
        let (handle_id, _) = upsert_handle_row(
            conn,
            ACCOUNT,
            raw,
            HandleType::parse(handle_type),
            Some(service),
        )
        .await
        .unwrap();
        contacts::link_handle_to_contact(conn, ACCOUNT, handle_id, id, Origin::Import)
            .await
            .unwrap();
    }
    id
}

/// Make `raw`, already an identity, the one other person in a one-to-one
/// conversation, so something cites it.
async fn in_a_conversation(conn: &mut SqliteConnection, raw: &str) {
    let handle_id: i64 =
        sqlx::query_scalar("SELECT id FROM handles WHERE account_id = $1 AND raw = $2")
            .bind(ACCOUNT)
            .bind(raw)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    let conversation: i64 = sqlx::query_scalar(
        "INSERT INTO conversations (account_id, chat_handle_id, conversation_type, source_file)
         VALUES ($1, $2, 'individual', 'c.jsonl') RETURNING id",
    )
    .bind(ACCOUNT)
    .bind(handle_id)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    sqlx::query("INSERT INTO participants (conversation_id, handle_id) VALUES ($1, $2)")
        .bind(conversation)
        .bind(handle_id)
        .execute(&mut *conn)
        .await
        .unwrap();
}

/// Put a contact in a Contact Group, creating the group.
async fn join_group(conn: &mut SqliteConnection, contact_id: i64, group: &str) {
    named_membership::set_membership(
        named_membership::group_spec(),
        conn,
        ACCOUNT,
        &[contact_id],
        group,
        true,
    )
    .await
    .unwrap();
}

/// Every contact as `(name, identities, groups)`, identities written
/// `service/handle_type/normalized`, all sorted.
async fn picture(conn: &mut SqliteConnection) -> Vec<(String, Vec<String>, Vec<String>)> {
    let ids: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, preferred_name FROM contacts WHERE account_id = $1 ORDER BY preferred_name, id",
    )
    .bind(ACCOUNT)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    let mut out = Vec::new();
    for (id, name) in ids {
        out.push((
            name,
            identities_of(conn, id).await,
            groups_of(conn, id).await,
        ));
    }
    out
}

async fn identities_of(conn: &mut SqliteConnection, contact_id: i64) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT h.service || '/' || h.handle_type || '/' || h.normalized
         FROM contact_handles ch JOIN handles h ON h.id = ch.handle_id
         WHERE ch.account_id = $1 AND ch.contact_id = $2 ORDER BY 1",
    )
    .bind(ACCOUNT)
    .bind(contact_id)
    .fetch_all(&mut *conn)
    .await
    .unwrap()
}

async fn groups_of(conn: &mut SqliteConnection, contact_id: i64) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT g.name FROM contact_group_members m JOIN contact_groups g ON g.id = m.group_id
         WHERE m.contact_id = $1 ORDER BY 1",
    )
    .bind(contact_id)
    .fetch_all(&mut *conn)
    .await
    .unwrap()
}

async fn name_of(conn: &mut SqliteConnection, contact_id: i64) -> Option<String> {
    sqlx::query_scalar("SELECT preferred_name FROM contacts WHERE id = $1")
        .bind(contact_id)
        .fetch_optional(&mut *conn)
        .await
        .unwrap()
}

async fn contact_named(conn: &mut SqliteConnection, name: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM contacts WHERE account_id = $1 AND preferred_name = $2")
        .bind(ACCOUNT)
        .bind(name)
        .fetch_one(&mut *conn)
        .await
        .unwrap()
}

/// The sentences of a refused load.
async fn refused(conn: &mut SqliteConnection, text: &str, mode: LoadMode) -> Vec<String> {
    match load(conn, ACCOUNT, text, mode).await {
        Err(LoadError::Refused(reasons)) => reasons,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

async fn loaded(conn: &mut SqliteConnection, text: &str, mode: LoadMode) -> CreateContactsResponse {
    load(conn, ACCOUNT, text, mode)
        .await
        .unwrap_or_else(|e| panic!("load failed: {e}"))
}

// --- The rows of one contact ---

#[tokio::test]
async fn rows_of_one_contact_that_disagree_on_the_name_are_refused_with_the_row() {
    let (mut conn, _pool, _dir) = account().await;
    let text = file(&[
        "ada,Ada Lovelace,,phone,phone,+15555550100",
        "ada,Ada L,,phone,email,ada@example.com",
    ]);
    let reasons = refused(&mut conn, &text, LoadMode::Append).await;
    assert_eq!(reasons.len(), 1, "{reasons:?}");
    assert!(reasons[0].starts_with("row 3: display_name"), "{reasons:?}");
    assert!(
        reasons[0].contains("\"Ada Lovelace\" on row 2"),
        "{reasons:?}"
    );
}

#[tokio::test]
async fn rows_of_one_contact_that_disagree_on_groups_are_refused_with_the_row() {
    let (mut conn, _pool, _dir) = account().await;
    let text = file(&[
        "ada,Ada,Family;Work,phone,phone,+15555550100",
        "ada,Ada,Family,phone,email,ada@example.com",
    ]);
    let reasons = refused(&mut conn, &text, LoadMode::Append).await;
    assert_eq!(reasons.len(), 1, "{reasons:?}");
    assert!(reasons[0].starts_with("row 3: groups"), "{reasons:?}");
}

/// The name and the groups describe the contact, so they need writing once:
/// a blank cell on a later row agrees with anything, and the same groups in
/// another order or case are the same groups.
#[tokio::test]
async fn a_blank_name_or_groups_cell_agrees_with_the_rows_that_fill_it() {
    let (mut conn, _pool, _dir) = account().await;
    let text = file(&[
        "ada,Ada,Family;Work,phone,phone,+15555550100",
        "ada,,,phone,email,ada@example.com",
        "ada,Ada,work; family,whatsapp,phone,+15555550100",
    ]);
    let counts = loaded(&mut conn, &text, LoadMode::Append).await;
    assert_eq!(counts.contacts_created, 1);
    assert_eq!(
        picture(&mut conn).await,
        vec![(
            "Ada".to_string(),
            vec![
                "phone/email/ada@example.com".to_string(),
                "phone/phone/+15555550100".to_string(),
                "whatsapp/phone/+15555550100".to_string(),
            ],
            vec!["Family".to_string(), "Work".to_string()],
        )]
    );
}

// --- How an identity is keyed ---

/// A phone gets the key every other part of the server gives it: E.164 when
/// the number says its country or is a US number, and the digits as written
/// otherwise, never an invented `+0…`.
#[tokio::test]
async fn a_phone_is_stored_under_the_key_an_import_gives_the_same_number() {
    let (mut conn, _pool, _dir) = account().await;
    let text = file(&[
        "a,Ada,,phone,phone,(555) 555-0100",
        "b,Bao,,phone,phone,+65 9555 0100",
        "c,Cy,,phone,phone,020 7946 0000",
    ]);
    loaded(&mut conn, &text, LoadMode::Append).await;
    let keys: Vec<String> = sqlx::query_scalar(
        "SELECT normalized FROM handles WHERE account_id = $1 ORDER BY normalized",
    )
    .bind(ACCOUNT)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(keys, ["+15555550100", "+6595550100", "02079460000"]);
    for (raw, key) in [
        ("(555) 555-0100", "+15555550100"),
        ("+65 9555 0100", "+6595550100"),
        ("020 7946 0000", "02079460000"),
    ] {
        assert_eq!(
            phone::normalize_typed_handle(raw, HandleType::Phone).0,
            key,
            "the load and the import must agree on {raw}"
        );
    }
}

#[tokio::test]
async fn a_phone_that_cannot_be_keyed_is_refused_with_its_row() {
    let (mut conn, _pool, _dir) = account().await;
    let text = file(&[
        "a,Ada,,phone,phone,+15555550100",
        "b,Bao,,phone,phone,call me",
        "c,Cy,,phone,phone,555-0100 x99",
        "d,Di,,phone,phone,12",
    ]);
    let reasons = refused(&mut conn, &text, LoadMode::Append).await;
    assert_eq!(reasons.len(), 3, "{reasons:?}");
    assert!(reasons[0].starts_with("row 3: \"call me\" is not a phone number"));
    assert!(reasons[1].starts_with("row 4: \"555-0100 x99\""));
    assert!(reasons[2].starts_with("row 5: \"12\""));
}

#[tokio::test]
async fn an_email_is_lowercased() {
    let (mut conn, _pool, _dir) = account().await;
    let text = file(&["a,Ada,,phone,email,Ada.Lovelace@Example.COM"]);
    loaded(&mut conn, &text, LoadMode::Append).await;
    let id = contact_named(&mut conn, "Ada").await;
    assert_eq!(
        identities_of(&mut conn, id).await,
        ["phone/email/ada.lovelace@example.com"]
    );
}

#[tokio::test]
async fn an_email_without_one_at_sign_and_text_on_both_sides_is_refused_with_its_row() {
    let (mut conn, _pool, _dir) = account().await;
    let text = file(&[
        "a,Ada,,phone,email,ada.example.com",
        "b,Bao,,phone,email,@example.com",
        "c,Cy,,phone,email,cy@",
        "d,Di,,phone,email,di@home@example.com",
        "e,Ed,,phone,email,ed@example.com",
    ]);
    let reasons = refused(&mut conn, &text, LoadMode::Append).await;
    assert_eq!(reasons.len(), 4, "{reasons:?}");
    for (reason, row) in reasons.iter().zip(2..) {
        assert!(
            reason.starts_with(&format!("row {row}: ")) && reason.contains("not an email address"),
            "{reason}"
        );
    }
}

#[tokio::test]
async fn an_unknown_service_is_refused_with_its_row() {
    let (mut conn, _pool, _dir) = account().await;
    let text = file(&[
        "a,Ada,,phone,phone,+15555550100",
        "b,Bao,,imessage,phone,+15555550101",
    ]);
    let reasons = refused(&mut conn, &text, LoadMode::Append).await;
    assert_eq!(reasons.len(), 1, "{reasons:?}");
    assert!(
        reasons[0].starts_with("row 3: service \"imessage\""),
        "{reasons:?}"
    );
}

#[tokio::test]
async fn an_unknown_handle_type_is_refused_with_its_row() {
    let (mut conn, _pool, _dir) = account().await;
    let text = file(&["a,Ada,,phone,mobile,+15555550100"]);
    let reasons = refused(&mut conn, &text, LoadMode::Append).await;
    assert_eq!(reasons.len(), 1, "{reasons:?}");
    assert!(
        reasons[0].starts_with("row 2: handle_type \"mobile\""),
        "{reasons:?}"
    );
}

#[tokio::test]
async fn the_same_identity_under_two_ids_is_refused_with_both_rows() {
    let (mut conn, _pool, _dir) = account().await;
    // Row 3 writes the number another way; it is the same key.
    let text = file(&[
        "a,Ada,,phone,phone,+15555550100",
        "b,Bao,,phone,phone,(555) 555-0100",
    ]);
    let reasons = refused(&mut conn, &text, LoadMode::Append).await;
    assert_eq!(reasons.len(), 1, "{reasons:?}");
    assert!(
        reasons[0].starts_with("row 3: +15555550100 is also on row 2"),
        "{reasons:?}"
    );

    // Two rows with a blank id are two contacts, so they collide the same way.
    let text = file(&[
        ",Ada,,phone,phone,+15555550100",
        ",Bao,,phone,phone,+15555550100",
    ]);
    let reasons = refused(&mut conn, &text, LoadMode::Append).await;
    assert!(reasons[0].starts_with("row 3: "), "{reasons:?}");
}

// --- Moving an identity ---

/// The cleanup the file exists for. An import met one number on the phone
/// platform (iMessage and SMS share that `handles` row) and again on
/// WhatsApp, and made one nameless contact holding both. The file gives both
/// to a contact it names. The holder has other identities and no name, so
/// each move is free, and the holder, left with nothing, is deleted.
#[tokio::test]
async fn identities_move_from_an_unknown_holding_one_number_on_two_services() {
    let (mut conn, _pool, _dir) = account().await;
    let unknown = imported(
        &mut conn,
        "",
        &[
            ("phone", "phone", "+15555550100"),
            ("whatsapp", "phone", "+15555550100"),
        ],
    )
    .await;
    let text = file(&[
        "ada,Ada,,phone,phone,+15555550100",
        "ada,Ada,,whatsapp,phone,+15555550100",
    ]);
    let counts = loaded(&mut conn, &text, LoadMode::Edit).await;
    assert_eq!(
        counts,
        CreateContactsResponse {
            contacts_created: 1,
            contacts_deleted: 1,
            identities_moved: 2,
            ..CreateContactsResponse::default()
        }
    );
    assert_eq!(
        name_of(&mut conn, unknown).await,
        None,
        "the holder is gone"
    );
    assert_eq!(
        picture(&mut conn).await,
        vec![(
            "Ada".to_string(),
            vec![
                "phone/phone/+15555550100".to_string(),
                "whatsapp/phone/+15555550100".to_string(),
            ],
            vec![],
        )]
    );
}

/// The Unknown keeps what the file did not ask for: taking one of its two
/// identities is allowed, and it stays, still nameless, with the other.
#[tokio::test]
async fn an_unknown_holder_keeps_the_identity_the_file_did_not_take() {
    let (mut conn, _pool, _dir) = account().await;
    let unknown = imported(
        &mut conn,
        "",
        &[
            ("phone", "phone", "+15555550100"),
            ("whatsapp", "phone", "+15555550100"),
        ],
    )
    .await;
    let text = file(&["ada,Ada,,phone,phone,+15555550100"]);
    let counts = loaded(&mut conn, &text, LoadMode::Append).await;
    assert_eq!(counts.identities_moved, 1);
    assert_eq!(counts.contacts_deleted, 0);
    assert_eq!(
        identities_of(&mut conn, unknown).await,
        ["whatsapp/phone/+15555550100"]
    );
}

#[tokio::test]
async fn an_identity_moves_between_two_contacts_that_are_both_in_the_file() {
    let (mut conn, _pool, _dir) = account().await;
    let ada = imported(&mut conn, "Ada", &[("phone", "phone", "+15555550100")]).await;
    let bao = imported(
        &mut conn,
        "Bao",
        &[
            ("phone", "phone", "+15555550101"),
            ("phone", "email", "ada@example.com"),
        ],
    )
    .await;
    // The email was linked to the wrong person; the sheet moves the row.
    for (mode, first) in [(LoadMode::Append, ada), (LoadMode::Edit, bao)] {
        let (second, second_name) = if first == ada {
            (bao, "Bao")
        } else {
            (ada, "Ada")
        };
        let first_name = if first == ada { "Ada" } else { "Bao" };
        let phone_of = |id: i64| {
            if id == ada {
                "+15555550100"
            } else {
                "+15555550101"
            }
        };
        let mut rows = vec![
            format!("{first},{first_name},,phone,phone,{}", phone_of(first)),
            format!("{second},{second_name},,phone,phone,{}", phone_of(second)),
        ];
        rows.push(format!("{ada},Ada,,phone,email,ada@example.com"));
        let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
        let counts = loaded(&mut conn, &file(&rows), mode).await;
        assert_eq!(
            identities_of(&mut conn, ada).await,
            ["phone/email/ada@example.com", "phone/phone/+15555550100"],
            "{mode:?}"
        );
        assert_eq!(
            identities_of(&mut conn, bao).await,
            ["phone/phone/+15555550101"],
            "{mode:?}"
        );
        if mode == LoadMode::Append {
            assert_eq!(
                counts,
                CreateContactsResponse {
                    contacts_updated: 2,
                    identities_moved: 1,
                    ..CreateContactsResponse::default()
                },
                "one move, whichever contact the file lists first"
            );
        } else {
            assert_eq!(
                counts,
                CreateContactsResponse::default(),
                "already as the file says"
            );
        }
    }
}

#[tokio::test]
async fn an_identity_of_a_named_contact_outside_the_file_is_refused_naming_both_contacts() {
    let (mut conn, _pool, _dir) = account().await;
    let ada = imported(&mut conn, "Ada", &[("phone", "phone", "+15555550100")]).await;
    let bao = imported(&mut conn, "Bao", &[("phone", "phone", "+15555550101")]).await;
    let text = file(&[
        &format!("{bao},Bao,,phone,phone,+15555550101"),
        &format!("{bao},Bao,,phone,phone,+15555550100"),
    ]);
    for mode in [LoadMode::Append, LoadMode::Edit] {
        let reasons = refused(&mut conn, &text, mode).await;
        assert_eq!(reasons.len(), 1, "{reasons:?}");
        let reason = &reasons[0];
        assert!(
            reason.starts_with("row 3: +15555550100 belongs to"),
            "{reason}"
        );
        assert!(
            reason.contains(&format!("\"Ada\" (contact {ada})")),
            "{reason}"
        );
        assert!(
            reason.contains(&format!("\"Bao\" (contact {bao})")),
            "{reason}"
        );
    }
    assert_eq!(
        identities_of(&mut conn, ada).await,
        ["phone/phone/+15555550100"],
        "nothing moved"
    );
}

/// A contact in the Trash cannot be added to the file, because its id reads
/// as unknown text. The refusal says to restore it or delete it for good.
#[tokio::test]
async fn an_identity_of_a_trashed_named_contact_is_refused_saying_it_is_in_the_trash() {
    let (mut conn, _pool, _dir) = account().await;
    let ada = imported(&mut conn, "Ada", &[("phone", "phone", "+15555550100")]).await;
    let bob = imported(&mut conn, "Bob", &[("phone", "phone", "+15555550142")]).await;
    sqlx::query("INSERT INTO trashed_contacts (account_id, contact_id) VALUES ($1, $2)")
        .bind(ACCOUNT)
        .bind(bob)
        .execute(&mut *conn)
        .await
        .unwrap();
    let expected = format!(
        "row 2: +15555550142 belongs to \"Bob\" (contact {bob}), which is in the Trash, \
         so it cannot move to \"Ada\" (contact {ada}); restore \"Bob\" and add it to the file, \
         or delete \"Bob\" for good, to move it"
    );
    let without_bob = file(&[&format!("{ada},Ada,,phone,phone,+15555550142")]);
    let with_bob = file(&[
        &format!("{ada},Ada,,phone,phone,+15555550142"),
        &format!("{bob},Bob,,,,"),
    ]);
    for text in [without_bob, with_bob] {
        for mode in [LoadMode::Append, LoadMode::Edit] {
            assert_eq!(
                refused(&mut conn, &text, mode).await,
                std::slice::from_ref(&expected)
            );
        }
    }
    assert_eq!(
        identities_of(&mut conn, bob).await,
        ["phone/phone/+15555550142"],
        "nothing moved"
    );
}

// --- Which contact a row speaks for ---

#[tokio::test]
async fn a_blank_id_creates_a_contact_for_each_row() {
    let (mut conn, _pool, _dir) = account().await;
    let text = file(&[
        ",Ada,,phone,phone,+15555550100",
        ",Ada,,phone,email,ada@example.com",
    ]);
    let counts = loaded(&mut conn, &text, LoadMode::Append).await;
    assert_eq!(counts.contacts_created, 2);
    assert_eq!(counts.identities_added, 2);
    assert_eq!(
        picture(&mut conn).await,
        vec![
            (
                "Ada".to_string(),
                vec!["phone/phone/+15555550100".to_string()],
                vec![]
            ),
            (
                "Ada".to_string(),
                vec!["phone/email/ada@example.com".to_string()],
                vec![]
            ),
        ]
    );
}

/// Text that is no contact's id is a key of the person's choosing: its rows
/// are one new contact. A number no contact has is such text too.
#[tokio::test]
async fn rows_sharing_an_unknown_id_are_one_new_contact() {
    let (mut conn, _pool, _dir) = account().await;
    let text = file(&[
        "new-1,Ada,,phone,phone,+15555550100",
        "new-1,,,phone,email,ada@example.com",
        "9001,Bao,,phone,phone,+15555550101",
        "9001,Bao,,phone,email,bao@example.com",
    ]);
    let counts = loaded(&mut conn, &text, LoadMode::Append).await;
    assert_eq!(counts.contacts_created, 2);
    assert_eq!(counts.identities_added, 4);
    let ada = contact_named(&mut conn, "Ada").await;
    assert_eq!(identities_of(&mut conn, ada).await.len(), 2);
    let bao = contact_named(&mut conn, "Bao").await;
    assert_ne!(bao, 9001, "the text is a grouping key, not an id to take");
    assert_eq!(identities_of(&mut conn, bao).await.len(), 2);
}

/// A loaded name replaces the one the contact carried, whoever supplied it:
/// the file is the person typing. A blank name says nothing and leaves the
/// name alone.
#[tokio::test]
async fn a_known_id_renames_the_contact() {
    let (mut conn, _pool, _dir) = account().await;
    let by_import = imported(&mut conn, "ada l", &[("phone", "phone", "+15555550100")]).await;
    let typed = contacts::create_contact(&mut conn, ACCOUNT, "Bao", Origin::User)
        .await
        .unwrap();
    let kept = imported(&mut conn, "Cy", &[("phone", "phone", "+15555550102")]).await;
    let text = file(&[
        &format!("{by_import},Ada Lovelace,,phone,phone,+15555550100"),
        &format!("{typed},Bao Nguyen,,,,"),
        &format!("{kept},,,phone,phone,+15555550102"),
    ]);
    let counts = loaded(&mut conn, &text, LoadMode::Append).await;
    assert_eq!(
        counts,
        CreateContactsResponse {
            contacts_updated: 2,
            ..CreateContactsResponse::default()
        }
    );
    assert_eq!(
        name_of(&mut conn, by_import).await.as_deref(),
        Some("Ada Lovelace")
    );
    assert_eq!(
        name_of(&mut conn, typed).await.as_deref(),
        Some("Bao Nguyen")
    );
    assert_eq!(name_of(&mut conn, kept).await.as_deref(), Some("Cy"));
}

/// A name the load gave is the person's, so a later import that spells it
/// another way does not take it back.
#[tokio::test]
async fn an_import_does_not_rename_a_contact_a_load_named() {
    let (mut conn, _pool, _dir) = account().await;
    let unknown = imported(&mut conn, "", &[("phone", "phone", "+15555550100")]).await;
    let text = file(&[&format!("{unknown},Ada Lovelace,,phone,phone,+15555550100")]);
    loaded(&mut conn, &text, LoadMode::Append).await;
    let renamed = contacts::propose_name(&mut conn, ACCOUNT, unknown, "ADA", Origin::Import)
        .await
        .unwrap();
    assert!(!renamed);
    assert_eq!(
        name_of(&mut conn, unknown).await.as_deref(),
        Some("Ada Lovelace")
    );
}

// --- Append and Edit ---

#[tokio::test]
async fn append_adds_identities_and_memberships_and_removes_nothing() {
    let (mut conn, _pool, _dir) = account().await;
    let ada = imported(
        &mut conn,
        "Ada",
        &[
            ("phone", "phone", "+15555550100"),
            ("phone", "phone", "+15555550109"),
        ],
    )
    .await;
    join_group(&mut conn, ada, "Family").await;
    let text = file(&[
        &format!("{ada},Ada,Work,phone,phone,+15555550100"),
        &format!("{ada},Ada,Work,phone,email,ada@example.com"),
    ]);
    let counts = loaded(&mut conn, &text, LoadMode::Append).await;
    assert_eq!(
        counts,
        CreateContactsResponse {
            contacts_updated: 1,
            identities_added: 1,
            groups_created: 1,
            ..CreateContactsResponse::default()
        }
    );
    assert_eq!(
        identities_of(&mut conn, ada).await,
        [
            "phone/email/ada@example.com",
            "phone/phone/+15555550100",
            "phone/phone/+15555550109"
        ]
    );
    assert_eq!(groups_of(&mut conn, ada).await, ["Family", "Work"]);
}

#[tokio::test]
async fn edit_removes_the_identities_and_memberships_the_rows_do_not_list() {
    let (mut conn, _pool, _dir) = account().await;
    let ada = imported(
        &mut conn,
        "Ada",
        &[
            ("phone", "phone", "+15555550100"),
            ("phone", "phone", "+15555550109"),
        ],
    )
    .await;
    join_group(&mut conn, ada, "Family").await;
    in_a_conversation(&mut conn, "+15555550109").await;
    let text = file(&[
        &format!("{ada},Ada,Work,phone,phone,+15555550100"),
        &format!("{ada},Ada,Work,phone,email,ada@example.com"),
    ]);
    let counts = loaded(&mut conn, &text, LoadMode::Edit).await;
    assert_eq!(
        counts,
        CreateContactsResponse {
            contacts_updated: 1,
            identities_added: 1,
            identities_removed: 1,
            groups_created: 1,
            ..CreateContactsResponse::default()
        }
    );
    assert_eq!(
        identities_of(&mut conn, ada).await,
        ["phone/email/ada@example.com", "phone/phone/+15555550100"]
    );
    assert_eq!(groups_of(&mut conn, ada).await, ["Work"]);
    // The identity is off the contact and, because a conversation cites it,
    // on a new contact with no name: the person is Unknown for it again.
    let holder: Vec<String> = sqlx::query_scalar(
        "SELECT ct.preferred_name FROM handles h
         JOIN contact_handles ch ON ch.handle_id = h.id
         JOIN contacts ct ON ct.id = ch.contact_id
         WHERE h.account_id = $1 AND h.normalized = '+15555550109'",
    )
    .bind(ACCOUNT)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(holder, [String::new()]);
    crate::test_support::assert_every_person_is_on_a_contact(&mut conn, "an Edit load").await;
    // The Contact Group it left is not deleted either.
    let family: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM contact_groups WHERE name = 'Family'")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(family, 1);
}

/// An identity Edit takes off a contact that nothing else uses is deleted,
/// because on no contact it would appear in no list.
#[tokio::test]
async fn edit_deletes_an_unlisted_identity_nothing_uses() {
    let (mut conn, _pool, _dir) = account().await;
    let ada = imported(
        &mut conn,
        "Ada",
        &[
            ("phone", "phone", "+15555550100"),
            ("phone", "phone", "+15555550109"),
        ],
    )
    .await;
    let text = file(&[&format!("{ada},Ada,,phone,phone,+15555550100")]);
    let counts = loaded(&mut conn, &text, LoadMode::Edit).await;
    assert_eq!(counts.identities_removed, 1);
    let left: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM handles WHERE account_id = $1 AND normalized = '+15555550109'",
    )
    .bind(ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(left, 0);
}

/// A group name is matched to a Contact Group ignoring case, and one that
/// matches nothing is created once, however many contacts list it.
#[tokio::test]
async fn a_group_name_matching_no_contact_group_creates_one() {
    let (mut conn, _pool, _dir) = account().await;
    let ada = imported(&mut conn, "Ada", &[("phone", "phone", "+15555550100")]).await;
    join_group(&mut conn, ada, "Family").await;
    let text = file(&[
        "b,Bao,family;Chess Club,phone,phone,+15555550101",
        "c,Cy,Chess Club,phone,phone,+15555550102",
    ]);
    let counts = loaded(&mut conn, &text, LoadMode::Append).await;
    assert_eq!(counts.groups_created, 1);
    let groups: Vec<String> =
        sqlx::query_scalar("SELECT name FROM contact_groups WHERE account_id = $1 ORDER BY name")
            .bind(ACCOUNT)
            .fetch_all(&mut *conn)
            .await
            .unwrap();
    assert_eq!(groups, ["Chess Club", "Family"]);
    let bao = contact_named(&mut conn, "Bao").await;
    assert_eq!(groups_of(&mut conn, bao).await, ["Chess Club", "Family"]);
}

/// Unknown is computed, and the other reserved names are screens: a file
/// cannot create a Contact Group the product would not let a person create.
#[tokio::test]
async fn a_reserved_group_name_is_refused_with_its_row() {
    let (mut conn, _pool, _dir) = account().await;
    let text = file(&["a,Ada,Family;Unknown,phone,phone,+15555550100"]);
    let reasons = refused(&mut conn, &text, LoadMode::Append).await;
    assert_eq!(reasons.len(), 1, "{reasons:?}");
    assert!(
        reasons[0].starts_with("row 2: Contact Group \"Unknown\""),
        "{reasons:?}"
    );
}

#[tokio::test]
async fn a_nameless_contact_edit_leaves_with_no_identity_is_deleted() {
    let (mut conn, _pool, _dir) = account().await;
    let unknown = imported(&mut conn, "", &[("phone", "phone", "+15555550100")]).await;
    let named = imported(&mut conn, "Bao", &[("phone", "phone", "+15555550101")]).await;
    // Both contacts stay in the file with their identity rows taken out.
    let text = file(&[&format!("{unknown},,,,,"), &format!("{named},Bao,,,,")]);

    // Append removes nothing, so nothing is deleted.
    let counts = loaded(&mut conn, &text, LoadMode::Append).await;
    assert_eq!(counts, CreateContactsResponse::default());

    let counts = loaded(&mut conn, &text, LoadMode::Edit).await;
    assert_eq!(
        counts,
        CreateContactsResponse {
            contacts_updated: 1,
            contacts_deleted: 1,
            identities_removed: 2,
            ..CreateContactsResponse::default()
        }
    );
    assert_eq!(name_of(&mut conn, unknown).await, None);
    // A contact with a name is never deleted, whatever it is left holding.
    assert_eq!(name_of(&mut conn, named).await.as_deref(), Some("Bao"));
    assert!(identities_of(&mut conn, named).await.is_empty());
}

#[tokio::test]
async fn a_contact_absent_from_the_file_is_untouched_in_both_modes() {
    let (mut conn, _pool, _dir) = account().await;
    let ada = imported(&mut conn, "Ada", &[("phone", "phone", "+15555550100")]).await;
    let bao = imported(
        &mut conn,
        "Bao",
        &[
            ("phone", "phone", "+15555550101"),
            ("phone", "email", "bao@example.com"),
        ],
    )
    .await;
    join_group(&mut conn, bao, "Family").await;
    let stamp = "2001-01-01 00:00:00";
    sqlx::query("UPDATE contacts SET last_modified = $1")
        .bind(stamp)
        .execute(&mut *conn)
        .await
        .unwrap();
    let text = file(&[&format!("{ada},Ada Lovelace,,phone,phone,+15555550100")]);
    for mode in [LoadMode::Append, LoadMode::Edit] {
        loaded(&mut conn, &text, mode).await;
        assert_eq!(name_of(&mut conn, bao).await.as_deref(), Some("Bao"));
        assert_eq!(identities_of(&mut conn, bao).await.len(), 2, "{mode:?}");
        assert_eq!(groups_of(&mut conn, bao).await, ["Family"], "{mode:?}");
        let modified: String =
            sqlx::query_scalar("SELECT last_modified FROM contacts WHERE id = $1")
                .bind(bao)
                .fetch_one(&mut *conn)
                .await
                .unwrap();
        assert_eq!(modified, stamp, "{mode:?}");
    }
}

/// What a load makes is marked as the address book's; what an import made
/// and the load only moved becomes the address book's link to an import's
/// identity.
#[tokio::test]
async fn the_rows_a_load_makes_carry_the_address_book_origin() {
    let (mut conn, _pool, _dir) = account().await;
    imported(&mut conn, "", &[("phone", "phone", "+15555550100")]).await;
    let text = file(&[
        "a,Ada,,phone,phone,+15555550100",
        "a,Ada,,phone,email,ada@example.com",
    ]);
    loaded(&mut conn, &text, LoadMode::Append).await;
    let rows: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT h.normalized, h.origin, ch.origin, c.origin
         FROM handles h
         JOIN contact_handles ch ON ch.handle_id = h.id
         JOIN contacts c ON c.id = ch.contact_id
         WHERE h.account_id = $1 ORDER BY h.normalized",
    )
    .bind(ACCOUNT)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    let rows: Vec<(&str, &str, &str, &str)> = rows
        .iter()
        .map(|(a, b, c, d)| (a.as_str(), b.as_str(), c.as_str(), d.as_str()))
        .collect();
    assert_eq!(
        rows,
        [
            ("+15555550100", "import", "address_book", "address_book"),
            (
                "ada@example.com",
                "address_book",
                "address_book",
                "address_book"
            ),
        ]
    );
    let notes: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM handles WHERE account_id = $1 AND normalized_note IS NOT NULL
           AND origin = 'address_book'",
    )
    .bind(ACCOUNT)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(notes, 0, "a load stores nothing as needing a look");
}

// --- Refusing whole ---

#[tokio::test]
async fn a_refused_load_names_every_bad_row_and_writes_nothing() {
    let (mut conn, _pool, _dir) = account().await;
    let ada = imported(&mut conn, "Ada", &[("phone", "phone", "+15555550100")]).await;
    let before = picture(&mut conn).await;
    let text = file(&[
        &format!("{ada},Ada Lovelace,New Group,phone,phone,+15555550100"),
        "b,Bao,,phone,phone,+15555550101",
        "c,Cy,,sms,phone,+15555550102",
        "d,Di,,phone,email,nobody",
    ]);
    for mode in [LoadMode::Append, LoadMode::Edit] {
        let reasons = refused(&mut conn, &text, mode).await;
        assert_eq!(reasons.len(), 2, "{reasons:?}");
        assert!(reasons[0].starts_with("row 4: "), "{reasons:?}");
        assert!(reasons[1].starts_with("row 5: "), "{reasons:?}");
        assert_eq!(
            picture(&mut conn).await,
            before,
            "the good rows did not go in"
        );
        let groups: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM contact_groups")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
        assert_eq!(groups, 0);
    }
}

#[tokio::test]
async fn a_file_without_the_six_columns_is_refused() {
    let (mut conn, _pool, _dir) = account().await;
    let reasons = refused(
        &mut conn,
        "First Name,Last Name,Phone\nAda,Lovelace,+15555550100\n",
        LoadMode::Append,
    )
    .await;
    assert_eq!(reasons.len(), 1);
    assert!(
        reasons[0].contains("the header is missing contact_id"),
        "{reasons:?}"
    );
}

#[tokio::test]
async fn a_new_contact_with_no_name_and_no_identity_is_refused_with_its_row() {
    let (mut conn, _pool, _dir) = account().await;
    let text = file(&["a,Ada,,phone,phone,+15555550100", "b,,Family,,,"]);
    let reasons = refused(&mut conn, &text, LoadMode::Append).await;
    assert_eq!(
        reasons,
        ["row 3: a new contact needs a display_name or an identity"]
    );
}

/// A row with more fields than the header is malformed: a name holding a
/// comma without quotes would otherwise move every later cell one column to
/// the right.
#[tokio::test]
async fn a_row_with_more_fields_than_the_header_is_refused_with_its_row() {
    let (mut conn, _pool, _dir) = account().await;
    let ada = imported(
        &mut conn,
        "Ada Lovelace",
        &[("phone", "phone", "+15555550110")],
    )
    .await;
    join_group(&mut conn, ada, "Family").await;
    let before = picture(&mut conn).await;
    let text = file(&[&format!("{ada},Lovelace, Ada,,,,")]);
    for mode in [LoadMode::Append, LoadMode::Edit] {
        let reasons = refused(&mut conn, &text, mode).await;
        assert_eq!(
            reasons,
            [
                "row 2: has 7 fields where the header has 6; put a cell that holds a comma in double quotes"
            ]
        );
        assert_eq!(picture(&mut conn).await, before, "the load wrote");
    }
}

/// A spreadsheet can drop the empty cells at the end of a row. No column
/// moves, so the missing cells read as blank.
#[tokio::test]
async fn a_row_with_fewer_fields_than_the_header_reads_the_missing_cells_as_blank() {
    let (mut conn, _pool, _dir) = account().await;
    let text = file(&["a,Ada", "a,,,phone,phone,+15555550100"]);
    let counts = loaded(&mut conn, &text, LoadMode::Append).await;
    assert_eq!(counts.contacts_created, 1);
    let ada = contact_named(&mut conn, "Ada").await;
    assert_eq!(
        identities_of(&mut conn, ada).await,
        ["phone/phone/+15555550100"]
    );
}

/// A spreadsheet saves a byte-order mark, its own column order, and empty
/// rows at the end. None of them is an error.
#[tokio::test]
async fn a_file_a_spreadsheet_saved_loads() {
    let (mut conn, _pool, _dir) = account().await;
    let text = "\u{feff}display_name,contact_id,identity,service,handle_type,groups\r\n\
                \"Lovelace, Ada\",a,+15555550100,phone,phone,Family\r\n\
                ,,,,,\r\n";
    let counts = loaded(&mut conn, text, LoadMode::Append).await;
    assert_eq!(counts.contacts_created, 1);
    let ada = contact_named(&mut conn, "Lovelace, Ada").await;
    assert_eq!(
        identities_of(&mut conn, ada).await,
        ["phone/phone/+15555550100"]
    );
}

// --- Export ---

#[tokio::test]
async fn export_writes_one_row_per_identity_with_the_contact_repeated() {
    let (mut conn, _pool, _dir) = account().await;
    let ada = imported(
        &mut conn,
        "Ada",
        &[
            ("phone", "phone", "+15555550100"),
            ("whatsapp", "phone", "+15555550100"),
            ("phone", "email", "ada@example.com"),
        ],
    )
    .await;
    join_group(&mut conn, ada, "Work").await;
    join_group(&mut conn, ada, "Family").await;
    let unknown = imported(&mut conn, "", &[("phone", "phone", "+15555550101")]).await;
    let no_identity = contacts::create_contact(&mut conn, ACCOUNT, "Cy", Origin::User)
        .await
        .unwrap();
    let trashed = imported(&mut conn, "Di", &[("phone", "phone", "+15555550103")]).await;
    sqlx::query("INSERT INTO trashed_contacts (account_id, contact_id) VALUES ($1, $2)")
        .bind(ACCOUNT)
        .bind(trashed)
        .execute(&mut *conn)
        .await
        .unwrap();

    let written = export_csv(&mut conn, ACCOUNT, None).await.unwrap();
    assert_eq!((written.contacts, written.identities), (3, 4));
    let text = written.csv;
    assert_eq!(
        text,
        format!(
            "{HEADER}\n\
             {unknown},,,phone,phone,'+15555550101\n\
             {ada},Ada,Family;Work,phone,email,ada@example.com\n\
             {ada},Ada,Family;Work,phone,phone,'+15555550100\n\
             {ada},Ada,Family;Work,whatsapp,phone,'+15555550100\n\
             {no_identity},Cy,,,,\n"
        )
    );

    let only: HashSet<i64> = [unknown].into();
    let written = export_csv(&mut conn, ACCOUNT, Some(&only)).await.unwrap();
    assert_eq!((written.contacts, written.identities), (1, 1));
    assert_eq!(
        written.csv,
        format!("{HEADER}\n{unknown},,,phone,phone,'+15555550101\n")
    );
}

/// The file Export writes says what the account holds, so loading it
/// straight back changes nothing in either mode: no count, no row, and no
/// contact's last-modified time. The account here holds what imports leave:
/// a number an import could not make E.164, an identity that is not
/// phone-shaped under the phone type, an Unknown, and a contact with no
/// identity.
#[tokio::test]
async fn an_export_loaded_straight_back_changes_nothing() {
    let (mut conn, _pool, _dir) = account().await;
    let ada = imported(
        &mut conn,
        "Ada",
        &[
            ("phone", "phone", "+15555550100"),
            ("whatsapp", "phone", "+15555550100"),
            ("phone", "email", "Ada@Example.com"),
        ],
    )
    .await;
    join_group(&mut conn, ada, "Family").await;
    join_group(&mut conn, ada, "Chess Club").await;
    imported(&mut conn, "", &[("phone", "phone", "020 7946 0000")]).await;
    imported(&mut conn, "Bank", &[("phone", "phone", "*611")]).await;
    imported(&mut conn, "Sam", &[("phone", "username", "@sam")]).await;
    contacts::create_contact(&mut conn, ACCOUNT, "Cy", Origin::User)
        .await
        .unwrap();
    let stamp = "2001-01-01 00:00:00";
    sqlx::query("UPDATE contacts SET last_modified = $1")
        .bind(stamp)
        .execute(&mut *conn)
        .await
        .unwrap();

    let before = picture(&mut conn).await;
    let text = export_csv(&mut conn, ACCOUNT, None).await.unwrap().csv;
    for mode in [LoadMode::Append, LoadMode::Edit] {
        let counts = loaded(&mut conn, &text, mode).await;
        assert_eq!(counts, CreateContactsResponse::default(), "{mode:?}");
        assert_eq!(picture(&mut conn).await, before, "{mode:?}");
        let touched: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM contacts WHERE last_modified <> $1")
                .bind(stamp)
                .fetch_one(&mut *conn)
                .await
                .unwrap();
        assert_eq!(touched, 0, "{mode:?}");
        assert_eq!(
            export_csv(&mut conn, ACCOUNT, None).await.unwrap().csv,
            text,
            "{mode:?}"
        );
    }
}

/// An identity Edit takes off a contact goes from the database too when the
/// file was the only thing that ever knew it, so loads leave no rows nothing
/// can reach.
#[tokio::test]
async fn an_identity_only_a_file_knew_goes_when_edit_takes_it_off() {
    let (mut conn, _pool, _dir) = account().await;
    loaded(
        &mut conn,
        &file(&[
            "a,Ada,,phone,phone,+15555550100",
            "a,Ada,,phone,email,old@example.com",
        ]),
        LoadMode::Append,
    )
    .await;
    let ada = contact_named(&mut conn, "Ada").await;
    let counts = loaded(
        &mut conn,
        &file(&[&format!("{ada},Ada,,phone,phone,+15555550100")]),
        LoadMode::Edit,
    )
    .await;
    assert_eq!(counts.identities_removed, 1);
    let keys: Vec<String> =
        sqlx::query_scalar("SELECT normalized FROM handles WHERE account_id = $1")
            .bind(ACCOUNT)
            .fetch_all(&mut *conn)
            .await
            .unwrap();
    assert_eq!(keys, ["+15555550100"]);
}

// --- A phone number whose `+` a spreadsheet dropped ---

/// S4-6: a number whose `+` a spreadsheet dropped is the same identity, and
/// Edit does not take the real one off the contact.
#[tokio::test]
async fn s4_6_a_number_without_its_plus_keeps_the_real_identity() {
    let (mut conn, _pool, _dir) = account().await;
    let ada = imported(&mut conn, "Ada", &[("phone", "phone", "+6595550100")]).await;
    let before = identities_of(&mut conn, ada).await;
    let row = format!("{ada},Ada,,phone,phone,6595550100");
    let _ = load(&mut conn, ACCOUNT, &file(&[&row]), LoadMode::Edit).await;
    assert_eq!(identities_of(&mut conn, ada).await, before);
}

/// A person editing in a spreadsheet does not see the `+` go, so the load
/// names the row it read with the `+` back, and changes nothing.
#[tokio::test]
async fn a_number_read_with_its_plus_back_is_named_in_the_result() {
    let (mut conn, _pool, _dir) = account().await;
    let ada = imported(&mut conn, "Ada", &[("phone", "phone", "+6595550100")]).await;
    let counts = loaded(
        &mut conn,
        &file(&[&format!("{ada},Ada,,phone,phone,6595550100")]),
        LoadMode::Edit,
    )
    .await;
    assert_eq!(
        counts,
        CreateContactsResponse {
            notes: vec![format!(
                "row 2: 6595550100 has no +, so it was read as +6595550100, \
                 which \"Ada\" (contact {ada}) holds"
            )],
            ..CreateContactsResponse::default()
        }
    );
}

/// When the contact holds both readings of a number without `+`, the row
/// cannot say which it means, so the load is refused with both keys.
#[tokio::test]
async fn a_number_without_plus_whose_contact_holds_both_readings_is_refused() {
    let (mut conn, _pool, _dir) = account().await;
    let ada = imported(
        &mut conn,
        "Ada",
        &[
            ("phone", "phone", "+6595550100"),
            ("phone", "phone", "+16595550100"),
        ],
    )
    .await;
    let reasons = refused(
        &mut conn,
        &file(&[&format!("{ada},Ada,,phone,phone,6595550100")]),
        LoadMode::Edit,
    )
    .await;
    assert_eq!(reasons.len(), 1, "{reasons:?}");
    assert!(reasons[0].starts_with("row 2: "), "{reasons:?}");
    assert!(reasons[0].contains("+6595550100"), "{reasons:?}");
    assert!(reasons[0].contains("+16595550100"), "{reasons:?}");
}

/// Only the row's own contact is looked at, so a dropped `+` never puts
/// another person's number on this contact. The value is keyed by the phone
/// rule, and the result says it became a new identity.
#[tokio::test]
async fn a_number_without_plus_never_takes_another_contacts_identity() {
    let (mut conn, _pool, _dir) = account().await;
    let bob = imported(&mut conn, "Bob", &[("phone", "phone", "+6595550100")]).await;
    let ada = imported(&mut conn, "Ada", &[]).await;
    let counts = loaded(
        &mut conn,
        &file(&[&format!("{ada},Ada,,phone,phone,6595550100")]),
        LoadMode::Append,
    )
    .await;
    assert_eq!(
        identities_of(&mut conn, bob).await,
        ["phone/phone/+6595550100"]
    );
    assert_eq!(
        identities_of(&mut conn, ada).await,
        ["phone/phone/+16595550100"]
    );
    assert_eq!(
        counts.notes,
        ["row 2: 6595550100 has no +, so it became the new identity +16595550100"]
    );
}

/// A number that is not ten digits once its `+` is gone becomes an identity
/// of bare digits, and the result says so.
#[tokio::test]
async fn a_new_identity_of_bare_digits_is_named_in_the_result() {
    let (mut conn, _pool, _dir) = account().await;
    let counts = loaded(
        &mut conn,
        &file(&["new,Ada,,phone,phone,447700900123"]),
        LoadMode::Append,
    )
    .await;
    let ada = contact_named(&mut conn, "Ada").await;
    assert_eq!(
        identities_of(&mut conn, ada).await,
        ["phone/phone/447700900123"]
    );
    assert_eq!(
        counts.notes,
        ["row 2: 447700900123 has no +, so it became the new identity 447700900123"]
    );
}

/// A ten-digit US number written without `+` whose `+1` key the account
/// holds is that identity, as before, and needs no word in the result.
#[tokio::test]
async fn a_us_number_without_plus_still_loads_as_its_plus_one_identity() {
    let (mut conn, _pool, _dir) = account().await;
    let ada = imported(&mut conn, "Ada", &[("phone", "phone", "+15555550100")]).await;
    let counts = loaded(
        &mut conn,
        &file(&[&format!("{ada},Ada,,phone,phone,555-555-0100")]),
        LoadMode::Edit,
    )
    .await;
    assert_eq!(counts, CreateContactsResponse::default());
    assert_eq!(
        identities_of(&mut conn, ada).await,
        ["phone/phone/+15555550100"]
    );
}

// --- Cells a spreadsheet would read as a formula ---

/// A spreadsheet runs a cell that starts with `=`, `+`, `-`, `@`, a tab or
/// a carriage return as a formula, so Export writes a `'` before each such
/// cell in every column, and a spreadsheet shows the cell as text.
#[tokio::test]
async fn export_writes_a_quote_before_a_cell_a_spreadsheet_would_run() {
    let (mut conn, _pool, _dir) = account().await;
    let name = "=HYPERLINK(\"http://example.com/?\"&B2,\"Ada\")";
    let ada = imported(
        &mut conn,
        name,
        &[
            ("phone", "phone", "+15555550100"),
            ("phone", "username", "@ada"),
            ("phone", "other", "-ada"),
        ],
    )
    .await;
    join_group(&mut conn, ada, "+Work").await;

    let text = export_csv(&mut conn, ACCOUNT, None).await.unwrap().csv;
    let cell = "\"'=HYPERLINK(\"\"http://example.com/?\"\"&B2,\"\"Ada\"\")\"";
    assert_eq!(
        text,
        format!(
            "{HEADER}\n\
             {ada},{cell},'+Work,phone,other,'-ada\n\
             {ada},{cell},'+Work,phone,phone,'+15555550100\n\
             {ada},{cell},'+Work,phone,username,'@ada\n"
        )
    );
}

/// The `'` Export writes is taken off again, so the file loaded straight
/// back changes nothing. A name that itself starts with `'` and then one of
/// those characters gets a second `'`, so it comes back with its own.
#[tokio::test]
async fn a_quoted_cell_loaded_straight_back_changes_nothing() {
    let (mut conn, _pool, _dir) = account().await;
    let ada = imported(
        &mut conn,
        "=HYPERLINK(\"http://example.com/?\"&B2,\"Ada\")",
        &[("phone", "phone", "+15555550100")],
    )
    .await;
    join_group(&mut conn, ada, "-Family").await;
    imported(&mut conn, "'=1+1", &[("phone", "username", "@bo")]).await;
    let stamp = "2001-01-01 00:00:00";
    sqlx::query("UPDATE contacts SET last_modified = $1")
        .bind(stamp)
        .execute(&mut *conn)
        .await
        .unwrap();

    let before = picture(&mut conn).await;
    let text = export_csv(&mut conn, ACCOUNT, None).await.unwrap().csv;
    for mode in [LoadMode::Append, LoadMode::Edit] {
        let counts = loaded(&mut conn, &text, mode).await;
        assert_eq!(counts, CreateContactsResponse::default(), "{mode:?}");
        assert_eq!(picture(&mut conn).await, before, "{mode:?}");
        assert_eq!(
            export_csv(&mut conn, ACCOUNT, None).await.unwrap().csv,
            text,
            "{mode:?}"
        );
    }
}

/// A spreadsheet may keep the `'` when it saves or drop it. The load reads
/// the cell the same either way.
#[tokio::test]
async fn a_cell_loads_the_same_with_or_without_its_quote() {
    let (mut conn, _pool, _dir) = account().await;
    let counts = loaded(
        &mut conn,
        &file(&[
            ",'=Ada,'+Work,phone,phone,'+15555550100",
            ",=Bo,+Work,phone,phone,+15555550101",
        ]),
        LoadMode::Append,
    )
    .await;
    assert_eq!(counts.contacts_created, 2);
    let ada = contact_named(&mut conn, "=Ada").await;
    assert_eq!(
        identities_of(&mut conn, ada).await,
        ["phone/phone/+15555550100"]
    );
    assert_eq!(groups_of(&mut conn, ada).await, ["+Work"]);
    let bo = contact_named(&mut conn, "=Bo").await;
    assert_eq!(groups_of(&mut conn, bo).await, ["+Work"]);
}

/// Only one `'`, and only before one of those characters, is taken off: a
/// name that starts with `'` for its own sake keeps it.
#[tokio::test]
async fn a_quote_before_other_text_is_part_of_the_cell() {
    let (mut conn, _pool, _dir) = account().await;
    loaded(
        &mut conn,
        &file(&[
            ",'Twas,,phone,phone,+15555550100",
            ",''=Cy,,phone,phone,+15555550101",
        ]),
        LoadMode::Append,
    )
    .await;
    contact_named(&mut conn, "'Twas").await;
    contact_named(&mut conn, "'=Cy").await;
}

/// Every character a spreadsheet reads as the start of a formula gets the
/// `'`, and the load takes exactly that `'` off again.
#[test]
fn every_formula_start_is_quoted_and_read_back() {
    for cell in ["=1", "+1", "-1", "@a", "\ta", "\ra", "'=1", "''+1"] {
        let written = written_cell(cell);
        assert_eq!(written, format!("'{cell}"), "{cell:?}");
        assert_eq!(read_cell(&written), cell, "{cell:?}");
    }
    for cell in ["", "Ada", "'Twas", "a=1", "15555550100"] {
        assert_eq!(written_cell(cell), cell, "{cell:?}");
        assert_eq!(read_cell(cell), cell, "{cell:?}");
    }
}

/// LibreOffice Calc 26.2 opens the export with every `'` shown in its cell,
/// and saves it back with every text cell in double quotes and the `'`
/// kept. That file loads back without a change.
#[tokio::test]
async fn an_export_libreoffice_saved_again_changes_nothing() {
    let (mut conn, _pool, _dir) = account().await;
    let ada = imported(
        &mut conn,
        "=HYPERLINK(\"http://example.com/?\"&B2,\"Ada\")",
        &[
            ("phone", "phone", "+15555550100"),
            ("phone", "username", "@ada"),
        ],
    )
    .await;
    join_group(&mut conn, ada, "+Work").await;
    let before = picture(&mut conn).await;
    let text = export_csv(&mut conn, ACCOUNT, None).await.unwrap().csv;

    let mut reader = csv::Reader::from_reader(text.as_bytes());
    let mut writer = csv::WriterBuilder::new()
        .quote_style(csv::QuoteStyle::NonNumeric)
        .from_writer(Vec::new());
    writer.write_record(reader.headers().unwrap()).unwrap();
    for record in reader.records() {
        writer.write_record(&record.unwrap()).unwrap();
    }
    let saved = String::from_utf8(writer.into_inner().unwrap()).unwrap();
    assert!(saved.contains(&format!("{ada},\"'=HYPERLINK(")), "{saved}");
    assert!(saved.contains(",\"'+15555550100\""), "{saved}");

    for mode in [LoadMode::Append, LoadMode::Edit] {
        let counts = loaded(&mut conn, &saved, mode).await;
        assert_eq!(counts, CreateContactsResponse::default(), "{mode:?}");
        assert_eq!(picture(&mut conn).await, before, "{mode:?}");
    }
}

// --- Another write at the same time ---

/// An import that commits while a load reads the account's contacts made the
/// load's first write fail with `SQLITE_BUSY_SNAPSHOT`, and the load answered
/// `500`. The load waits for the other write instead and then runs.
#[tokio::test]
async fn a_write_that_commits_while_the_load_reads_does_not_fail_it() {
    let (mut conn, pool, _dir) = account().await;
    let mut other_conn = pool.acquire().await.unwrap();
    let mut other = crate::db::begin_write(&mut other_conn).await.unwrap();
    crate::db::account_profile::ensure_account_row(&mut other, ACCOUNT + 1)
        .await
        .unwrap();
    let text = file(&["ada,Ada,Family,phone,phone,+15555550100"]);
    let counts = crate::db::write_tx::commit_during(
        other,
        load(&mut conn, ACCOUNT, &text, LoadMode::Append),
    )
    .await
    .unwrap_or_else(|e| panic!("load failed: {e}"));
    assert_eq!(counts.contacts_created, 1);
}

// --- Rewriting a file's new contacts onto the nameless contacts ---

/// The `contact_id` cell of each data row of a file.
fn ids_of(text: &str) -> Vec<String> {
    contact_ids_of(text)
}

#[tokio::test]
async fn a_new_contact_whose_identity_a_nameless_contact_holds_names_it_in_place() {
    let (mut conn, _pool, _dir) = account().await;
    let unknown = imported(&mut conn, "", &[("phone", "phone", "+15555550150")]).await;

    let text = rewrite_ids_to_nameless(
        &mut conn,
        ACCOUNT,
        &file(&["abc,Alice,,phone,phone,+15555550150"]),
    )
    .await
    .unwrap();
    assert_eq!(ids_of(&text), [unknown.to_string()]);

    let counts = loaded(&mut conn, &text, LoadMode::Append).await;
    assert_eq!((counts.contacts_updated, counts.contacts_created), (1, 0));
    assert_eq!(name_of(&mut conn, unknown).await.as_deref(), Some("Alice"));
}

#[tokio::test]
async fn a_new_contact_whose_rows_read_otherwise_under_the_nameless_contacts_id_stays_new() {
    let (mut conn, _pool, _dir) = account().await;
    imported(&mut conn, "", &[("phone", "phone", "+6595550100")]).await;
    // As a new contact, the second row is another number; under the
    // nameless contact's id it would read as the first row's `+6595550100`.
    let original = file(&[
        "c1,Carol,,phone,phone,+6595550100",
        "c1,,,phone,phone,6595550100",
    ]);

    let text = rewrite_ids_to_nameless(&mut conn, ACCOUNT, &original)
        .await
        .unwrap();
    assert_eq!(ids_of(&text), ["c1", "c1"]);
}
