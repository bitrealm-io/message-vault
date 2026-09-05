use super::*;

#[tokio::test]
async fn create_trims_and_defaults_to_manual() {
    let vault = crate::test_support::test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000e1", "alice")
        .await;
    let mut conn = vault.conn().await;
    let made = create(
        &mut conn,
        &account,
        "  Work team  ",
        "  service:whatsapp kind:group  ",
        SavedSearchKind::Manual,
    )
    .await
    .unwrap();
    assert_eq!(made.name, "Work team");
    assert_eq!(made.query, "service:whatsapp kind:group");
    assert_eq!(made.kind, "manual");
}

#[tokio::test]
async fn list_is_alphabetical_not_insertion_order() {
    let vault = crate::test_support::test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000e1", "alice")
        .await;
    let mut conn = vault.conn().await;
    for name in ["zeta", "Alpha", "middle"] {
        create(
            &mut conn,
            &account,
            name,
            "kind:group",
            SavedSearchKind::Manual,
        )
        .await
        .unwrap();
    }
    let names: Vec<String> = list(&mut conn, &account)
        .await
        .unwrap()
        .into_iter()
        .map(|s| s.name)
        .collect();
    assert_eq!(names, vec!["Alpha", "middle", "zeta"]);
}

#[tokio::test]
async fn names_collide_case_insensitively_within_an_account() {
    let vault = crate::test_support::test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000e1", "alice")
        .await;
    let mut conn = vault.conn().await;
    create(
        &mut conn,
        &account,
        "Family",
        "kind:group",
        SavedSearchKind::Manual,
    )
    .await
    .unwrap();
    let err = create(
        &mut conn,
        &account,
        "family",
        "kind:direct",
        SavedSearchKind::Manual,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, SavedSearchError::Conflict(_)), "got {err:?}");
}

#[tokio::test]
async fn saved_searches_are_scoped_per_account() {
    let vault = crate::test_support::test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000e1", "alice")
        .await;
    let mut conn = vault.conn().await;
    let other = "00000000-0000-4000-8000-0000000000e2".to_string();
    sqlx::query("INSERT INTO accounts (id, username) VALUES ($1, 'bob')")
        .bind(&other)
        .execute(&mut *conn)
        .await
        .unwrap();

    let mine = create(
        &mut conn,
        &account,
        "Family",
        "kind:group",
        SavedSearchKind::Manual,
    )
    .await
    .unwrap();
    // The same name is free for another account.
    create(
        &mut conn,
        &other,
        "Family",
        "kind:direct",
        SavedSearchKind::Manual,
    )
    .await
    .unwrap();

    assert_eq!(list(&mut conn, &other).await.unwrap().len(), 1);
    // One account cannot read or delete another's row by id.
    assert!(get(&mut conn, &other, mine.id).await.unwrap().is_none());
    let err = delete(&mut conn, &other, mine.id).await.unwrap_err();
    assert!(matches!(err, SavedSearchError::NotFound(_)), "got {err:?}");
}

#[tokio::test]
async fn update_replaces_both_fields_and_keeps_id_and_kind() {
    let vault = crate::test_support::test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000e1", "alice")
        .await;
    let mut conn = vault.conn().await;
    let made = create_for_import(&mut conn, &account, 7, "imessage", "2026-08-30")
        .await
        .unwrap();
    let edited = update(&mut conn, &account, made.id, "Renamed", "kind:direct")
        .await
        .unwrap();
    assert_eq!(edited.id, made.id);
    assert_eq!(edited.name, "Renamed");
    assert_eq!(edited.query, "kind:direct");
    assert_eq!(edited.kind, "import", "kind records how a row was born");
}

#[tokio::test]
async fn update_allows_a_row_to_keep_or_recase_its_own_name() {
    let vault = crate::test_support::test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000e1", "alice")
        .await;
    let mut conn = vault.conn().await;
    let made = create(
        &mut conn,
        &account,
        "Family",
        "kind:group",
        SavedSearchKind::Manual,
    )
    .await
    .unwrap();
    let same = update(&mut conn, &account, made.id, "Family", "kind:direct")
        .await
        .unwrap();
    assert_eq!(same.query, "kind:direct");
    let recased = update(&mut conn, &account, made.id, "FAMILY", "kind:direct")
        .await
        .unwrap();
    assert_eq!(recased.name, "FAMILY");
}

#[tokio::test]
async fn update_rejects_a_name_another_row_already_uses() {
    let vault = crate::test_support::test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000e1", "alice")
        .await;
    let mut conn = vault.conn().await;
    create(
        &mut conn,
        &account,
        "Family",
        "kind:group",
        SavedSearchKind::Manual,
    )
    .await
    .unwrap();
    let second = create(
        &mut conn,
        &account,
        "Work",
        "kind:direct",
        SavedSearchKind::Manual,
    )
    .await
    .unwrap();
    let err = update(&mut conn, &account, second.id, "family", "kind:direct")
        .await
        .unwrap_err();
    assert!(matches!(err, SavedSearchError::Conflict(_)), "got {err:?}");
}

#[tokio::test]
async fn empty_name_or_query_is_rejected() {
    let vault = crate::test_support::test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000e1", "alice")
        .await;
    let mut conn = vault.conn().await;
    let err = create(
        &mut conn,
        &account,
        "   ",
        "kind:group",
        SavedSearchKind::Manual,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, SavedSearchError::BadRequest(_)),
        "got {err:?}"
    );
    let err = create(&mut conn, &account, "Name", "   ", SavedSearchKind::Manual)
        .await
        .unwrap_err();
    assert!(
        matches!(err, SavedSearchError::BadRequest(_)),
        "got {err:?}"
    );
}

#[tokio::test]
async fn names_over_max_len_are_rejected() {
    let vault = crate::test_support::test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000e1", "alice")
        .await;
    let mut conn = vault.conn().await;
    let long = "a".repeat(MAX_NAME_LEN + 1);
    let err = create(
        &mut conn,
        &account,
        &long,
        "kind:group",
        SavedSearchKind::Manual,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, SavedSearchError::BadRequest(_)),
        "got {err:?}"
    );
}

#[tokio::test]
async fn any_query_string_is_stored_verbatim() {
    let vault = crate::test_support::test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000e1", "alice")
        .await;
    let mut conn = vault.conn().await;
    // Nonsense in both grammars. The vault stores it anyway: the two
    // parsers disagree about what is legal, so nothing validates here.
    let made = create(
        &mut conn,
        &account,
        "Nonsense",
        "from:bob service:discord",
        SavedSearchKind::Manual,
    )
    .await
    .unwrap();
    assert_eq!(made.query, "from:bob service:discord");
}

#[tokio::test]
async fn import_saved_search_is_named_and_marked() {
    let vault = crate::test_support::test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000e1", "alice")
        .await;
    let mut conn = vault.conn().await;
    let made = create_for_import(&mut conn, &account, 42, "imessage", "2026-08-30")
        .await
        .unwrap();
    assert_eq!(made.name, "Import imessage 2026-08-30");
    assert_eq!(made.query, "import:42");
    assert_eq!(made.kind, "import");
}

#[tokio::test]
async fn repeat_imports_on_one_day_get_numbered_names() {
    let vault = crate::test_support::test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000e1", "alice")
        .await;
    let mut conn = vault.conn().await;
    let first = create_for_import(&mut conn, &account, 1, "imessage", "2026-08-30")
        .await
        .unwrap();
    let second = create_for_import(&mut conn, &account, 2, "imessage", "2026-08-30")
        .await
        .unwrap();
    let third = create_for_import(&mut conn, &account, 3, "imessage", "2026-08-30")
        .await
        .unwrap();
    assert_eq!(first.name, "Import imessage 2026-08-30");
    assert_eq!(second.name, "Import imessage 2026-08-30 2");
    assert_eq!(third.name, "Import imessage 2026-08-30 3");
    assert_eq!(third.query, "import:3");
}

#[tokio::test]
async fn deleting_a_saved_search_leaves_the_import_record() {
    let vault = crate::test_support::test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000e1", "alice")
        .await;
    let mut conn = vault.conn().await;
    sqlx::query(
        "INSERT INTO vault_imports
         (id, account_id, source, mode, status, started_at, message_count)
         VALUES (99, $1, 'imessage', 'append', 'completed', '2026-08-30T00:00:00Z', 12)",
    )
    .bind(&account)
    .execute(&mut *conn)
    .await
    .unwrap();

    let made = create_for_import(&mut conn, &account, 99, "imessage", "2026-08-30")
        .await
        .unwrap();
    delete(&mut conn, &account, made.id).await.unwrap();

    assert!(list(&mut conn, &account).await.unwrap().is_empty());
    let still_there: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM vault_imports WHERE id = 99")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(
        still_there, 1,
        "deleting the shortcut must not touch the import record"
    );
}
