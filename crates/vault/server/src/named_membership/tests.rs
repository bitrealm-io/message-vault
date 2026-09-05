use super::*;

async fn insert_contact(conn: &mut AnyConnection, account: &str, name: &str) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name) VALUES ($1, $2) RETURNING id",
    )
    .bind(account)
    .bind(name)
    .fetch_one(&mut *conn)
    .await
    .unwrap()
}

#[tokio::test]
async fn reserved_names_rejected_with_exact_messages() {
    let vault = crate::test_support::test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000d9", "alice")
        .await;
    let mut conn = vault.conn().await;
    let err = create_set(tag_spec(), &mut conn, &account, "Trash")
        .await
        .unwrap_err();
    match err {
        MembershipError::BadRequest(msg) => assert_eq!(msg, "\"Trash\" is a reserved tag"),
        other => panic!("expected BadRequest, got {other:?}"),
    }
    let err = create_set(group_spec(), &mut conn, &account, "Trash")
        .await
        .unwrap_err();
    match err {
        MembershipError::BadRequest(msg) => assert_eq!(msg, "Trash is a reserved group"),
        other => panic!("expected BadRequest, got {other:?}"),
    }
    let err = create_set(group_spec(), &mut conn, &account, "Group Chats")
        .await
        .unwrap_err();
    match err {
        MembershipError::BadRequest(msg) => {
            assert_eq!(msg, "Group Messages is a reserved name");
        }
        other => panic!("expected BadRequest, got {other:?}"),
    }
}

#[tokio::test]
async fn names_over_max_len_rejected() {
    let vault = crate::test_support::test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000d9", "alice")
        .await;
    let mut conn = vault.conn().await;
    let long = "x".repeat(MAX_NAME_LEN + 1);
    let err = create_set(tag_spec(), &mut conn, &account, &long)
        .await
        .unwrap_err();
    match err {
        MembershipError::BadRequest(msg) => {
            assert_eq!(
                msg,
                format!("name must be at most {MAX_NAME_LEN} characters")
            );
        }
        other => panic!("expected BadRequest, got {other:?}"),
    }
}

#[tokio::test]
async fn create_set_refuses_an_empty_name() {
    let vault = crate::test_support::test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000d9", "alice")
        .await;
    let mut conn = vault.conn().await;
    let err = create_set(group_spec(), &mut conn, &account, "   ")
        .await
        .unwrap_err();
    assert!(matches!(err, MembershipError::BadRequest(_)));
}

#[tokio::test]
async fn rename_set_refuses_an_empty_or_over_long_name() {
    let vault = crate::test_support::test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000d9", "alice")
        .await;
    let mut conn = vault.conn().await;
    let (id, _) = create_set(group_spec(), &mut conn, &account, "Family")
        .await
        .unwrap();

    let err = rename_set(group_spec(), &mut conn, &account, id, "   ")
        .await
        .unwrap_err();
    assert!(matches!(err, MembershipError::BadRequest(_)));

    let long = "x".repeat(MAX_NAME_LEN + 1);
    let err = rename_set(group_spec(), &mut conn, &account, id, &long)
        .await
        .unwrap_err();
    assert!(matches!(err, MembershipError::BadRequest(_)));
}

#[tokio::test]
async fn on_change_hook_runs_on_membership_change() {
    let vault = crate::test_support::test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000d9", "alice")
        .await;
    let mut conn = vault.conn().await;
    sqlx::query("INSERT INTO contacts (account_id, preferred_name) VALUES ($1, 'Ada')")
        .bind(&account)
        .execute(&mut *conn)
        .await
        .unwrap();
    let contact_id: i64 = sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name) VALUES ($1, 'Ada') RETURNING id",
    )
    .bind(&account)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    sqlx::query("UPDATE contacts SET last_modified = '2000-01-01 00:00:00' WHERE id = $1")
        .bind(contact_id)
        .execute(&mut *conn)
        .await
        .unwrap();

    assert_eq!(
        set_membership(
            group_spec(),
            &mut conn,
            &account,
            &[contact_id],
            "Family",
            true
        )
        .await
        .unwrap(),
        1
    );
    let after: String = sqlx::query_scalar("SELECT last_modified FROM contacts WHERE id = $1")
        .bind(contact_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_ne!(
        after, "2000-01-01 00:00:00",
        "group change must touch the contact"
    );
}

#[tokio::test]
async fn create_and_list_sets_answer_ids_and_names_a_to_z() {
    let vault = crate::test_support::test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000d9", "alice")
        .await;
    let mut conn = vault.conn().await;
    let (work_id, work) = create_set(group_spec(), &mut conn, &account, " Work ")
        .await
        .unwrap();
    assert_eq!(work, "Work");
    let (family_id, _) = create_set(group_spec(), &mut conn, &account, "Family")
        .await
        .unwrap();
    assert_ne!(work_id, family_id);
    assert_eq!(
        list_sets(group_spec(), &mut conn, &account).await.unwrap(),
        vec![
            (family_id, "Family".to_string()),
            (work_id, "Work".to_string())
        ]
    );
    assert_eq!(
        get_set(group_spec(), &mut conn, &account, work_id)
            .await
            .unwrap(),
        (work_id, "Work".to_string())
    );
}

#[tokio::test]
async fn create_set_refuses_duplicates_and_reserved_names() {
    let vault = crate::test_support::test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000d9", "alice")
        .await;
    let mut conn = vault.conn().await;
    create_set(group_spec(), &mut conn, &account, "Family")
        .await
        .unwrap();
    let err = create_set(group_spec(), &mut conn, &account, "family")
        .await
        .unwrap_err();
    assert!(matches!(err, MembershipError::Conflict(_)));
    let err = create_set(group_spec(), &mut conn, &account, "Trash")
        .await
        .unwrap_err();
    assert!(matches!(err, MembershipError::BadRequest(_)));
}

#[tokio::test]
async fn rename_set_allows_a_case_change_and_refuses_another_sets_name() {
    let vault = crate::test_support::test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000d9", "alice")
        .await;
    let mut conn = vault.conn().await;
    let (family_id, _) = create_set(group_spec(), &mut conn, &account, "Family")
        .await
        .unwrap();
    let (work_id, _) = create_set(group_spec(), &mut conn, &account, "Work")
        .await
        .unwrap();
    assert_eq!(
        rename_set(group_spec(), &mut conn, &account, family_id, "FAMILY")
            .await
            .unwrap(),
        "FAMILY"
    );
    let err = rename_set(group_spec(), &mut conn, &account, work_id, "family")
        .await
        .unwrap_err();
    assert!(matches!(err, MembershipError::Conflict(_)));
    let err = rename_set(group_spec(), &mut conn, &account, 999_999, "Anything")
        .await
        .unwrap_err();
    assert!(matches!(err, MembershipError::NotFound(_)));
}

#[tokio::test]
async fn delete_set_drops_its_memberships_and_refuses_an_unknown_id() {
    let vault = crate::test_support::test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000d9", "alice")
        .await;
    let mut conn = vault.conn().await;
    let a = insert_contact(&mut conn, &account, "Ada").await;
    let (id, _) = create_set(group_spec(), &mut conn, &account, "Family")
        .await
        .unwrap();
    patch_members(group_spec(), &mut conn, &account, id, &[a], &[])
        .await
        .unwrap();
    delete_set(group_spec(), &mut conn, &account, id)
        .await
        .unwrap();
    assert!(
        list_sets(group_spec(), &mut conn, &account)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        names_for_item(group_spec(), &mut conn, &account, a)
            .await
            .unwrap()
            .is_empty()
    );
    let err = delete_set(group_spec(), &mut conn, &account, id)
        .await
        .unwrap_err();
    assert!(matches!(err, MembershipError::NotFound(_)));
}

#[tokio::test]
async fn patch_members_adds_and_removes_in_one_call() {
    let vault = crate::test_support::test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000d9", "alice")
        .await;
    let mut conn = vault.conn().await;
    let a = insert_contact(&mut conn, &account, "Ada").await;
    let b = insert_contact(&mut conn, &account, "Ben").await;
    let (id, _) = create_set(group_spec(), &mut conn, &account, "Family")
        .await
        .unwrap();
    assert_eq!(
        patch_members(group_spec(), &mut conn, &account, id, &[a, b, b], &[])
            .await
            .unwrap(),
        (2, 0)
    );
    assert_eq!(
        list_member_ids_of(group_spec(), &mut conn, &account, id)
            .await
            .unwrap(),
        vec![a, b]
    );
    assert_eq!(
        patch_members(group_spec(), &mut conn, &account, id, &[a], &[b])
            .await
            .unwrap(),
        (0, 1)
    );
    assert_eq!(
        list_member_ids_of(group_spec(), &mut conn, &account, id)
            .await
            .unwrap(),
        vec![a]
    );
    let err = patch_members(group_spec(), &mut conn, &account, id, &[], &[])
        .await
        .unwrap_err();
    assert!(matches!(err, MembershipError::BadRequest(_)));
}

#[tokio::test]
async fn patch_members_with_a_foreign_member_writes_nothing() {
    let vault = crate::test_support::test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000d9", "alice")
        .await;
    let mut conn = vault.conn().await;
    let a = insert_contact(&mut conn, &account, "Ada").await;
    let (id, _) = create_set(group_spec(), &mut conn, &account, "Family")
        .await
        .unwrap();
    let err = patch_members(group_spec(), &mut conn, &account, id, &[a, 999_999], &[])
        .await
        .unwrap_err();
    assert!(matches!(err, MembershipError::NotFound(_)));
    assert!(
        list_member_ids_of(group_spec(), &mut conn, &account, id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn another_accounts_set_is_not_found() {
    let vault = crate::test_support::test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000d9", "alice")
        .await;
    let mut conn = vault.conn().await;
    let other = "00000000-0000-4000-8000-0000000000ca";
    sqlx::query("INSERT INTO accounts (id, username) VALUES ($1, 'bob')")
        .bind(other)
        .execute(&mut *conn)
        .await
        .unwrap();
    let (id, _) = create_set(tag_spec(), &mut conn, other, "Holiday")
        .await
        .unwrap();
    let err = get_set(tag_spec(), &mut conn, &account, id)
        .await
        .unwrap_err();
    assert!(matches!(err, MembershipError::NotFound(_)));
    assert!(
        list_sets(tag_spec(), &mut conn, &account)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn get_set_does_not_find_a_reserved_name_leftover() {
    let vault = crate::test_support::test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000d9", "alice")
        .await;
    let mut conn = vault.conn().await;
    // create_set and rename_set both refuse reserved names, so the only
    // way a reserved-name row exists is a leftover from before that
    // check existed (or a direct insert, as here).
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO contact_groups (account_id, name) VALUES ($1, 'Trash') RETURNING id",
    )
    .bind(&account)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let err = get_set(group_spec(), &mut conn, &account, id)
        .await
        .unwrap_err();
    assert!(matches!(err, MembershipError::NotFound(_)));
}

#[tokio::test]
async fn patch_members_an_id_in_both_add_and_remove_nets_to_removed() {
    let vault = crate::test_support::test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000d9", "alice")
        .await;
    let mut conn = vault.conn().await;
    let a = insert_contact(&mut conn, &account, "Ada").await;
    let (id, _) = create_set(group_spec(), &mut conn, &account, "Family")
        .await
        .unwrap();

    // Already a member: add and remove the same id nets to "removed",
    // and the change hook fires once, not twice.
    patch_members(group_spec(), &mut conn, &account, id, &[a], &[])
        .await
        .unwrap();
    assert_eq!(
        patch_members(group_spec(), &mut conn, &account, id, &[a], &[a])
            .await
            .unwrap(),
        (0, 1)
    );
    assert!(
        list_member_ids_of(group_spec(), &mut conn, &account, id)
            .await
            .unwrap()
            .is_empty()
    );

    // Never a member: add and remove the same id changes nothing.
    let b = insert_contact(&mut conn, &account, "Ben").await;
    assert_eq!(
        patch_members(group_spec(), &mut conn, &account, id, &[b], &[b])
            .await
            .unwrap(),
        (0, 0)
    );
    assert!(
        list_member_ids_of(group_spec(), &mut conn, &account, id)
            .await
            .unwrap()
            .is_empty()
    );
}

/// The import path still fills groups by name through `set_membership`.
#[tokio::test]
async fn set_membership_by_name_still_creates_and_fills_a_group() {
    let vault = crate::test_support::test_vault().await;
    let account = vault
        .account_with_id("00000000-0000-4000-8000-0000000000d9", "alice")
        .await;
    let mut conn = vault.conn().await;
    let a = insert_contact(&mut conn, &account, "Ada").await;
    assert_eq!(
        set_membership(group_spec(), &mut conn, &account, &[a], "Family", true)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        names_for_item(group_spec(), &mut conn, &account, a)
            .await
            .unwrap(),
        vec!["Family"]
    );
}
