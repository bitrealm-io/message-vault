use super::*;

async fn insert_contact(conn: &mut SqliteConnection, account: i64, name: &str) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name) VALUES ($1, $2) RETURNING id",
    )
    .bind(account)
    .bind(name)
    .fetch_one(&mut *conn)
    .await
    .unwrap()
}

/// Insert a one-to-one conversation for `account`, answering its id.
async fn insert_conversation(conn: &mut SqliteConnection, account: i64, phone: &str) -> i64 {
    let handle: i64 = sqlx::query_scalar(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES ($1, $2, $2, 'phone', 'phone') RETURNING id",
    )
    .bind(account)
    .bind(phone)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    sqlx::query_scalar(
        "INSERT INTO conversations (account_id, chat_handle_id, conversation_type, source_file)
         VALUES ($1, $2, 'individual', 'seed.jsonl') RETURNING id",
    )
    .bind(account)
    .bind(handle)
    .fetch_one(&mut *conn)
    .await
    .unwrap()
}

/// Insert one row a set of `spec`'s kind can hold, answering its id.
async fn insert_member(
    spec: &MembershipSpec,
    conn: &mut SqliteConnection,
    account: i64,
    name: &str,
) -> i64 {
    if spec.member_table == "contacts" {
        insert_contact(conn, account, name).await
    } else {
        insert_conversation(conn, account, &format!("+1555{account}{name}")).await
    }
}

/// Every membership row of `spec`'s kind, across all accounts.
async fn all_membership_rows(spec: &MembershipSpec, conn: &mut SqliteConnection) -> Vec<i64> {
    let sql = format!(
        "SELECT {mc} FROM {mt} ORDER BY {mc}",
        mc = spec.member_column,
        mt = spec.members_table,
    );
    sqlx::query_scalar(&sql)
        .fetch_all(&mut *conn)
        .await
        .unwrap()
}

/// The contact's `last_modified`.
async fn last_modified(conn: &mut SqliteConnection, contact: i64) -> String {
    sqlx::query_scalar("SELECT last_modified FROM contacts WHERE id = $1")
        .bind(contact)
        .fetch_one(&mut *conn)
        .await
        .unwrap()
}

/// A `last_modified` no hook writes, so a change from it shows a touch.
const OLD: &str = "2000-01-01 00:00:00";

/// Set every contact of `account` to [`OLD`].
async fn age_contacts(conn: &mut SqliteConnection, account: i64) {
    sqlx::query("UPDATE contacts SET last_modified = $1 WHERE account_id = $2")
        .bind(OLD)
        .bind(account)
        .execute(&mut *conn)
        .await
        .unwrap();
}

#[tokio::test]
async fn reserved_names_rejected_with_exact_messages() {
    let fixture = crate::test_support::test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    let err = create_set(tag_spec(), &mut conn, account, "Trash")
        .await
        .unwrap_err();
    match err {
        MembershipError::BadRequest(msg) => assert_eq!(msg, "\"Trash\" is a reserved Message Tag"),
        other => panic!("expected BadRequest, got {other:?}"),
    }
    let err = create_set(group_spec(), &mut conn, account, "Trash")
        .await
        .unwrap_err();
    match err {
        MembershipError::BadRequest(msg) => assert_eq!(msg, "Trash is a reserved Contact Group"),
        other => panic!("expected BadRequest, got {other:?}"),
    }
    let err = create_set(group_spec(), &mut conn, account, "Group Chats")
        .await
        .unwrap_err();
    match err {
        MembershipError::BadRequest(msg) => {
            assert_eq!(msg, "\"Group Chats\" is a reserved Contact Group");
        }
        other => panic!("expected BadRequest, got {other:?}"),
    }
}

/// Two requests create one name at once, and the second reads the name free
/// before the first has committed. The same spelling broke the `UNIQUE`
/// constraint and answered `500`; another letter case got past it and both
/// were stored. Either way the second must be refused as taken.
#[tokio::test]
async fn a_name_created_meanwhile_is_taken_in_any_letter_case() {
    for second in ["Work", "work"] {
        let fixture = crate::test_support::test_fixture().await;
        let account = fixture.account_with_id(101, "alice").await;
        let mut other_conn = fixture.conn().await;
        let mut other = crate::db::begin_write(&mut other_conn).await.unwrap();
        create_set(tag_spec(), &mut other, account, "Work")
            .await
            .unwrap();
        let mut conn = fixture.conn().await;
        let err = crate::db::write_tx::commit_during(
            other,
            create_set(tag_spec(), &mut conn, account, second),
        )
        .await
        .unwrap_err();

        assert!(
            matches!(err, MembershipError::Conflict(_)),
            "{second}: {err:?}"
        );
        let names: Vec<String> = list_sets(tag_spec(), &mut conn, account)
            .await
            .unwrap()
            .into_iter()
            .map(|(_, name)| name)
            .collect();
        assert_eq!(names, ["Work"], "{second}");
    }
}

// Unknown and No group are computed from contact state and searched as
// `group:unknown` and `group:none`. A stored Contact Group under either name
// would put two things with one name in the left panel.
#[tokio::test]
async fn a_contact_group_cannot_take_a_computed_groups_name() {
    let fixture = crate::test_support::test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    let (family_id, _) = create_set(group_spec(), &mut conn, account, "Family")
        .await
        .unwrap();
    for name in ["Unknown", "unknown", "UNKNOWN", "none", "None", "NONE"] {
        let expected = format!("\"{name}\" is a reserved Contact Group");
        match create_set(group_spec(), &mut conn, account, name)
            .await
            .unwrap_err()
        {
            MembershipError::BadRequest(msg) => assert_eq!(msg, expected),
            other => panic!("create {name}: expected BadRequest, got {other:?}"),
        }
        match rename_set(group_spec(), &mut conn, account, family_id, name)
            .await
            .unwrap_err()
        {
            MembershipError::BadRequest(msg) => assert_eq!(msg, expected),
            other => panic!("rename to {name}: expected BadRequest, got {other:?}"),
        }
    }
    // `tag:` has no Unknown, so a Message Tag can take that name.
    create_set(tag_spec(), &mut conn, account, "Unknown")
        .await
        .unwrap();
}

// The search reads `tag:none`, quoted or not, as "no tag", so a Message Tag
// stored under that name would list the untagged conversations on its page.
#[tokio::test]
async fn a_message_tag_cannot_be_named_none() {
    let fixture = crate::test_support::test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    let (holiday_id, _) = create_set(tag_spec(), &mut conn, account, "Holiday")
        .await
        .unwrap();
    for name in ["none", "None", "NONE"] {
        let expected = format!("\"{name}\" is a reserved Message Tag");
        match create_set(tag_spec(), &mut conn, account, name)
            .await
            .unwrap_err()
        {
            MembershipError::BadRequest(msg) => assert_eq!(msg, expected),
            other => panic!("create {name}: expected BadRequest, got {other:?}"),
        }
        match rename_set(tag_spec(), &mut conn, account, holiday_id, name)
            .await
            .unwrap_err()
        {
            MembershipError::BadRequest(msg) => assert_eq!(msg, expected),
            other => panic!("rename to {name}: expected BadRequest, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn names_over_max_len_rejected() {
    let fixture = crate::test_support::test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    let long = "x".repeat(MAX_NAME_LEN + 1);
    let err = create_set(tag_spec(), &mut conn, account, &long)
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

// S4-1: the address book lists a contact's Contact Groups in one `groups`
// cell, separated by `;`. A group named `Work; 2024` would come back from an
// export as the two groups `Work` and `2024`, so the name is refused.
#[tokio::test]
async fn a_contact_group_name_cannot_hold_a_semicolon() {
    let fixture = crate::test_support::test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    let expected =
        "name can't hold \";\", because the address book separates Contact Group names with it";
    match create_set(group_spec(), &mut conn, account, "Work; 2024")
        .await
        .unwrap_err()
    {
        MembershipError::BadRequest(msg) => assert_eq!(msg, expected),
        other => panic!("create: expected BadRequest, got {other:?}"),
    }
    let (work_id, _) = create_set(group_spec(), &mut conn, account, "Work")
        .await
        .unwrap();
    match rename_set(group_spec(), &mut conn, account, work_id, "Work; 2024")
        .await
        .unwrap_err()
    {
        MembershipError::BadRequest(msg) => assert_eq!(msg, expected),
        other => panic!("rename: expected BadRequest, got {other:?}"),
    }
    assert_eq!(
        check_name(group_spec(), "Work; 2024").unwrap_err(),
        expected
    );
    // Message Tags are not in the address book, so a tag keeps the name.
    create_set(tag_spec(), &mut conn, account, "Work; 2024")
        .await
        .unwrap();
}

#[tokio::test]
async fn create_set_refuses_an_empty_name() {
    let fixture = crate::test_support::test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    let err = create_set(group_spec(), &mut conn, account, "   ")
        .await
        .unwrap_err();
    assert!(matches!(err, MembershipError::BadRequest(_)));
}

#[tokio::test]
async fn rename_set_refuses_an_empty_or_over_long_name() {
    let fixture = crate::test_support::test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    let (id, _) = create_set(group_spec(), &mut conn, account, "Family")
        .await
        .unwrap();

    let err = rename_set(group_spec(), &mut conn, account, id, "   ")
        .await
        .unwrap_err();
    assert!(matches!(err, MembershipError::BadRequest(_)));

    let long = "x".repeat(MAX_NAME_LEN + 1);
    let err = rename_set(group_spec(), &mut conn, account, id, &long)
        .await
        .unwrap_err();
    assert!(matches!(err, MembershipError::BadRequest(_)));
}

// The routes change a Contact Group's members through `patch_members`, so the
// hook that touches the member contact is checked on that path, after an add
// and after a remove.
#[tokio::test]
async fn patch_members_touches_the_member_contact_on_add_and_on_remove() {
    let fixture = crate::test_support::test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    let ada = insert_contact(&mut conn, account, "Ada").await;
    let (id, _) = create_set(group_spec(), &mut conn, account, "Family")
        .await
        .unwrap();

    age_contacts(&mut conn, account).await;
    assert_eq!(
        patch_members(group_spec(), &mut conn, account, id, &[ada], &[])
            .await
            .unwrap(),
        (1, 0)
    );
    assert_ne!(
        last_modified(&mut conn, ada).await,
        OLD,
        "adding a contact to a group must touch the contact"
    );

    age_contacts(&mut conn, account).await;
    assert_eq!(
        patch_members(group_spec(), &mut conn, account, id, &[], &[ada])
            .await
            .unwrap(),
        (0, 1)
    );
    assert_ne!(
        last_modified(&mut conn, ada).await,
        OLD,
        "taking a contact out of a group must touch the contact"
    );
}

#[tokio::test]
async fn create_and_list_sets_answer_ids_and_names_a_to_z() {
    let fixture = crate::test_support::test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    let (work_id, work) = create_set(group_spec(), &mut conn, account, " Work ")
        .await
        .unwrap();
    assert_eq!(work, "Work");
    let (family_id, _) = create_set(group_spec(), &mut conn, account, "Family")
        .await
        .unwrap();
    assert_ne!(work_id, family_id);
    assert_eq!(
        list_sets(group_spec(), &mut conn, account).await.unwrap(),
        vec![
            (family_id, "Family".to_string()),
            (work_id, "Work".to_string())
        ]
    );
    assert_eq!(
        get_set(group_spec(), &mut conn, account, work_id)
            .await
            .unwrap(),
        (work_id, "Work".to_string())
    );
}

#[tokio::test]
async fn create_set_refuses_duplicates_and_reserved_names() {
    let fixture = crate::test_support::test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    create_set(group_spec(), &mut conn, account, "Family")
        .await
        .unwrap();
    let err = create_set(group_spec(), &mut conn, account, "family")
        .await
        .unwrap_err();
    assert!(matches!(err, MembershipError::Conflict(_)));
    let err = create_set(group_spec(), &mut conn, account, "Trash")
        .await
        .unwrap_err();
    assert!(matches!(err, MembershipError::BadRequest(_)));
}

#[tokio::test]
async fn rename_set_allows_a_case_change_and_refuses_another_sets_name() {
    let fixture = crate::test_support::test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    let (family_id, _) = create_set(group_spec(), &mut conn, account, "Family")
        .await
        .unwrap();
    let (work_id, _) = create_set(group_spec(), &mut conn, account, "Work")
        .await
        .unwrap();
    assert_eq!(
        rename_set(group_spec(), &mut conn, account, family_id, "FAMILY")
            .await
            .unwrap(),
        "FAMILY"
    );
    let err = rename_set(group_spec(), &mut conn, account, work_id, "family")
        .await
        .unwrap_err();
    assert!(matches!(err, MembershipError::Conflict(_)));
    let err = rename_set(group_spec(), &mut conn, account, 999_999, "Anything")
        .await
        .unwrap_err();
    assert!(matches!(err, MembershipError::NotFound(_)));
}

#[tokio::test]
async fn delete_set_drops_its_memberships_and_refuses_an_unknown_id() {
    let fixture = crate::test_support::test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    let a = insert_contact(&mut conn, account, "Ada").await;
    let (id, _) = create_set(group_spec(), &mut conn, account, "Family")
        .await
        .unwrap();
    patch_members(group_spec(), &mut conn, account, id, &[a], &[])
        .await
        .unwrap();
    delete_set(group_spec(), &mut conn, account, id)
        .await
        .unwrap();
    assert!(
        list_sets(group_spec(), &mut conn, account)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        names_for_item(group_spec(), &mut conn, account, a)
            .await
            .unwrap()
            .is_empty()
    );
    let err = delete_set(group_spec(), &mut conn, account, id)
        .await
        .unwrap_err();
    assert!(matches!(err, MembershipError::NotFound(_)));
}

// Deleting a Contact Group changes each member's groups, so it touches each
// member as taking the member out by hand does, and leaves other contacts be.
#[tokio::test]
async fn delete_set_touches_each_member_and_no_other_contact() {
    let fixture = crate::test_support::test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    let ada = insert_contact(&mut conn, account, "Ada").await;
    let ben = insert_contact(&mut conn, account, "Ben").await;
    let cy = insert_contact(&mut conn, account, "Cy").await;
    let (id, _) = create_set(group_spec(), &mut conn, account, "Family")
        .await
        .unwrap();
    patch_members(group_spec(), &mut conn, account, id, &[ada, ben], &[])
        .await
        .unwrap();
    age_contacts(&mut conn, account).await;

    delete_set(group_spec(), &mut conn, account, id)
        .await
        .unwrap();

    for (name, contact, moved) in [("Ada", ada, true), ("Ben", ben, true), ("Cy", cy, false)] {
        let after = last_modified(&mut conn, contact).await;
        assert_eq!(after != OLD, moved, "{name}: last_modified is {after}");
    }
}

#[tokio::test]
async fn patch_members_adds_and_removes_in_one_call() {
    let fixture = crate::test_support::test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    let a = insert_contact(&mut conn, account, "Ada").await;
    let b = insert_contact(&mut conn, account, "Ben").await;
    let (id, _) = create_set(group_spec(), &mut conn, account, "Family")
        .await
        .unwrap();
    assert_eq!(
        patch_members(group_spec(), &mut conn, account, id, &[a, b, b], &[])
            .await
            .unwrap(),
        (2, 0)
    );
    assert_eq!(
        list_member_ids_of(group_spec(), &mut conn, account, id)
            .await
            .unwrap(),
        vec![a, b]
    );
    assert_eq!(
        patch_members(group_spec(), &mut conn, account, id, &[a], &[b])
            .await
            .unwrap(),
        (0, 1)
    );
    assert_eq!(
        list_member_ids_of(group_spec(), &mut conn, account, id)
            .await
            .unwrap(),
        vec![a]
    );
    let err = patch_members(group_spec(), &mut conn, account, id, &[], &[])
        .await
        .unwrap_err();
    assert!(matches!(err, MembershipError::BadRequest(_)));
}

// Bob's contact and Bob's conversation exist, so only the account condition
// in `member_exists` keeps Alice from putting them in her set. One bad id in
// `add` refuses the whole call, so Alice's own member is not written either.
#[tokio::test]
async fn patch_members_with_a_foreign_member_writes_nothing() {
    for spec in [group_spec(), tag_spec()] {
        let fixture = crate::test_support::test_fixture().await;
        let alice = fixture.account_with_id(101, "alice").await;
        let bob = fixture.account_with_id(102, "bob").await;
        let mut conn = fixture.conn().await;
        let alices = insert_member(spec, &mut conn, alice, "Ada").await;
        let bobs = insert_member(spec, &mut conn, bob, "Ben").await;
        let (alice_set, _) = create_set(spec, &mut conn, alice, "Family").await.unwrap();
        let (bob_set, _) = create_set(spec, &mut conn, bob, "Family").await.unwrap();

        let err = patch_members(spec, &mut conn, alice, alice_set, &[alices, bobs], &[])
            .await
            .unwrap_err();
        match err {
            MembershipError::BadRequest(msg) => {
                assert_eq!(msg, format!("{} {bobs} not found", spec.member_label));
            }
            other => panic!("{}: expected BadRequest, got {other:?}", spec.label),
        }
        assert!(
            list_member_ids_of(spec, &mut conn, alice, alice_set)
                .await
                .unwrap()
                .is_empty(),
            "{}: Alice's set must stay empty",
            spec.label
        );
        assert!(
            list_member_ids_of(spec, &mut conn, bob, bob_set)
                .await
                .unwrap()
                .is_empty(),
            "{}: Bob's set must stay empty",
            spec.label
        );
        assert!(
            all_membership_rows(spec, &mut conn).await.is_empty(),
            "{}: no membership row may be written for either account",
            spec.label
        );
    }
}

/// An id to remove that names no member is ignored: the set is already in
/// the state the caller asked for, and `removed` counts only deleted rows.
#[tokio::test]
async fn patch_members_ignores_an_unknown_id_in_remove() {
    let fixture = crate::test_support::test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    let a = insert_contact(&mut conn, account, "Ada").await;
    let b = insert_contact(&mut conn, account, "Ben").await;
    let (id, _) = create_set(group_spec(), &mut conn, account, "Family")
        .await
        .unwrap();
    patch_members(group_spec(), &mut conn, account, id, &[a], &[])
        .await
        .unwrap();
    // A contact that exists but is not a member, and an id that names no
    // row at all: both are ignored, and nothing else changes.
    assert_eq!(
        patch_members(group_spec(), &mut conn, account, id, &[], &[b, 999_999])
            .await
            .unwrap(),
        (0, 0)
    );
    assert_eq!(
        list_member_ids_of(group_spec(), &mut conn, account, id)
            .await
            .unwrap(),
        vec![a]
    );
    // Mixed with a real member, only that one counts.
    assert_eq!(
        patch_members(group_spec(), &mut conn, account, id, &[], &[a, 999_999])
            .await
            .unwrap(),
        (0, 1)
    );
    assert!(
        list_member_ids_of(group_spec(), &mut conn, account, id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn another_accounts_set_is_not_found() {
    let fixture = crate::test_support::test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    let other = 102_i64;
    sqlx::query("INSERT INTO accounts (id, username) VALUES ($1, 'bob')")
        .bind(other)
        .execute(&mut *conn)
        .await
        .unwrap();
    let (id, _) = create_set(tag_spec(), &mut conn, other, "Holiday")
        .await
        .unwrap();
    let err = get_set(tag_spec(), &mut conn, account, id)
        .await
        .unwrap_err();
    assert!(matches!(err, MembershipError::NotFound(_)));
    assert!(
        list_sets(tag_spec(), &mut conn, account)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn get_set_does_not_find_a_reserved_name_leftover() {
    let fixture = crate::test_support::test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    // create_set and rename_set both refuse reserved names, so the only
    // way a reserved-name row exists is a leftover from before that
    // check existed (or a direct insert, as here).
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO contact_groups (account_id, name) VALUES ($1, 'Trash') RETURNING id",
    )
    .bind(account)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let err = get_set(group_spec(), &mut conn, account, id)
        .await
        .unwrap_err();
    assert!(matches!(err, MembershipError::NotFound(_)));
}

thread_local! {
    /// The member ids [`record_hook`] was called with on this thread.
    static HOOK_CALLS: std::cell::RefCell<Vec<i64>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// A change hook that records each call, so a test can count them. A second
/// `touch_contact` writes the `last_modified` the first one wrote, so that
/// column cannot show a hook that ran twice.
fn record_hook<'a>(
    _conn: &'a mut SqliteConnection,
    _account_id: i64,
    member_id: i64,
) -> Pin<Box<dyn Future<Output = AnyResult<()>> + Send + 'a>> {
    HOOK_CALLS.with_borrow_mut(|calls| calls.push(member_id));
    Box::pin(async { Ok(()) })
}

/// The calls [`record_hook`] recorded since the last take, emptying the record.
fn take_hook_calls() -> Vec<i64> {
    HOOK_CALLS.with_borrow_mut(std::mem::take)
}

#[tokio::test]
async fn patch_members_an_id_in_both_add_and_remove_nets_to_removed() {
    let fixture = crate::test_support::test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    let spec = MembershipSpec {
        on_change: Some(record_hook),
        ..*group_spec()
    };
    let a = insert_contact(&mut conn, account, "Ada").await;
    let (id, _) = create_set(&spec, &mut conn, account, "Family")
        .await
        .unwrap();
    take_hook_calls();

    assert_eq!(
        patch_members(&spec, &mut conn, account, id, &[a], &[])
            .await
            .unwrap(),
        (1, 0)
    );
    assert_eq!(take_hook_calls(), vec![a], "an add runs the hook once");

    // Already a member: add and remove the same id nets to "removed",
    // and the change hook fires once, not twice.
    assert_eq!(
        patch_members(&spec, &mut conn, account, id, &[a], &[a])
            .await
            .unwrap(),
        (0, 1)
    );
    assert_eq!(
        take_hook_calls(),
        vec![a],
        "a net remove runs the hook once"
    );
    assert!(
        list_member_ids_of(&spec, &mut conn, account, id)
            .await
            .unwrap()
            .is_empty()
    );

    // Never a member: add and remove the same id changes nothing, so the
    // hook does not run.
    let b = insert_contact(&mut conn, account, "Ben").await;
    assert_eq!(
        patch_members(&spec, &mut conn, account, id, &[b], &[b])
            .await
            .unwrap(),
        (0, 0)
    );
    assert_eq!(take_hook_calls(), Vec::<i64>::new(), "no change, no hook");
    assert!(
        list_member_ids_of(&spec, &mut conn, account, id)
            .await
            .unwrap()
            .is_empty()
    );
}

/// Tests across the server fill groups by name through `set_membership`.
#[tokio::test]
async fn set_membership_by_name_still_creates_and_fills_a_group() {
    let fixture = crate::test_support::test_fixture().await;
    let account = fixture.account_with_id(101, "alice").await;
    let mut conn = fixture.conn().await;
    let a = insert_contact(&mut conn, account, "Ada").await;
    assert_eq!(
        set_membership(group_spec(), &mut conn, account, &[a], "Family", true)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        names_for_item(group_spec(), &mut conn, account, a)
            .await
            .unwrap(),
        vec!["Family"]
    );
}
