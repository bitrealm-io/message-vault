//! Tests at the module's interface: seed a SQLite vault, compile a query
//! for a list, run it, and assert which ids come back.

use chrono::NaiveDate;
use sqlx::AnyConnection;

use super::{CompileRequest, ListKind, QueryError, compile};
use crate::db::dialect::engine_of;
use crate::db::engine::DbEngine;
use crate::db::sql::{bind_args, renumber_placeholders};

pub(crate) const ACCOUNT: i64 = 7;
pub(crate) const OTHER_ACCOUNT: i64 = 8;

pub(crate) fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 9, 2).unwrap()
}

/// The clock the route tests hand the lists: UTC, on the fixed day above.
pub(crate) fn clock() -> (chrono_tz::Tz, NaiveDate) {
    (chrono_tz::UTC, today())
}

/// Ids the fixture created, so a test can assert on them by name.
#[derive(Debug, Default)]
pub(crate) struct Fixture {
    // contacts
    pub ana: i64,
    pub bo: i64,
    pub cy: i64,
    pub jane: i64,
    pub sam: i64,
    pub nameless: i64,
    // handles
    pub me_handle: i64,
    pub ana_handle: i64,
    pub bo_handle: i64,
    pub jane_handle: i64,
    pub sam_handle: i64,
    pub nameless_handle: i64,
    // conversations
    pub ana_direct: i64,
    pub bo_direct: i64,
    pub jane_direct: i64,
    pub sam_direct: i64,
    pub archive_group: i64,
    pub big_group: i64,
    pub trashed_conv: i64,
    // messages
    pub ana_2018: i64,
    pub ana_2021: i64,
    pub bo_2023: i64,
    pub jane_avocado_from_me: i64,
    pub jane_guac_from_me: i64,
    pub jane_avocado_to_me: i64,
    pub sam_avocado_from_me: i64,
    pub jane_2018: i64,
    pub feb_big_jpeg: i64,
    pub feb_small_jpeg: i64,
    pub feb_pdf: i64,
    pub may_big_jpeg: i64,
    pub big_group_msg: i64,
    pub archive_msg: i64,
    pub trashed_msg: i64,
    // a conversation whose only message a later import marked as a
    // duplicate: invisible to an ordinary query, findable only by `import:`.
    pub dup_only_conv: i64,
    pub dup_only_msg: i64,
    // named sets
    pub family: i64,
    pub archive: i64,
}

pub(crate) async fn handle(
    conn: &mut AnyConnection,
    account: i64,
    raw: &str,
    service: &str,
) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES ($1, $2, $2, 'phone', $3) RETURNING id",
    )
    .bind(account)
    .bind(raw)
    .bind(service)
    .fetch_one(&mut *conn)
    .await
    .unwrap()
}

pub(crate) async fn contact(
    conn: &mut AnyConnection,
    account: i64,
    name: &str,
    handles: &[i64],
) -> i64 {
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name) VALUES ($1, $2) RETURNING id",
    )
    .bind(account)
    .bind(name)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    for h in handles {
        sqlx::query(
            "INSERT INTO contact_handles (account_id, handle_id, contact_id) VALUES ($1, $2, $3)",
        )
        .bind(account)
        .bind(h)
        .bind(id)
        .execute(&mut *conn)
        .await
        .unwrap();
    }
    id
}

/// A conversation whose chat handle is `chat` and whose participants are the
/// given handles, each linked to its contact when one exists.
pub(crate) async fn conversation(
    conn: &mut AnyConnection,
    account: i64,
    chat: i64,
    kind: &str,
    title: Option<&str>,
    participants: &[i64],
) -> i64 {
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO conversations (account_id, chat_handle_id, conversation_type, group_title, source_file)
         VALUES ($1, $2, $3, $4, 'seed.jsonl') RETURNING id",
    )
    .bind(account)
    .bind(chat)
    .bind(kind)
    .bind(title)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    for h in participants {
        let contact_id: Option<i64> = sqlx::query_scalar(
            "SELECT contact_id FROM contact_handles WHERE account_id = $1 AND handle_id = $2",
        )
        .bind(account)
        .bind(h)
        .fetch_optional(&mut *conn)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO participants (conversation_id, handle_id, contact_id) VALUES ($1, $2, $3)",
        )
        .bind(id)
        .bind(h)
        .bind(contact_id)
        .execute(&mut *conn)
        .await
        .unwrap();
    }
    id
}

/// A participant the source named but gave no address for: `handle_id` is
/// NULL and `name_alias` carries who they are.
pub(crate) async fn named_participant(conn: &mut AnyConnection, conversation: i64, alias: &str) {
    sqlx::query(
        "INSERT INTO participants (conversation_id, handle_id, contact_id, name_alias)
         VALUES ($1, NULL, NULL, $2)",
    )
    .bind(conversation)
    .bind(alias)
    .execute(&mut *conn)
    .await
    .unwrap();
}

pub(crate) struct Msg<'a> {
    pub conversation: i64,
    pub timestamp: &'a str,
    pub from_me: bool,
    pub sender: Option<i64>,
    pub body: Option<&'a str>,
    pub subject: Option<&'a str>,
    pub source: &'a str,
    pub service: &'a str,
}

pub(crate) fn msg<'a>(
    conversation: i64,
    timestamp: &'a str,
    from_me: bool,
    sender: Option<i64>,
    body: &'a str,
) -> Msg<'a> {
    Msg {
        conversation,
        timestamp,
        from_me,
        sender,
        body: Some(body),
        subject: None,
        source: "imessage",
        service: "imessage",
    }
}

pub(crate) async fn message(conn: &mut AnyConnection, account: i64, m: Msg<'_>) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO messages (conversation_id, account_id, source, timestamp, is_from_me,
                               sender_handle_id, service, subject, body, sort_order)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 0) RETURNING id",
    )
    .bind(m.conversation)
    .bind(account)
    .bind(m.source)
    .bind(m.timestamp)
    .bind(i64::from(m.from_me))
    .bind(m.sender)
    .bind(m.service)
    .bind(m.subject)
    .bind(m.body)
    .fetch_one(&mut *conn)
    .await
    .unwrap()
}

pub(crate) async fn attachment(
    conn: &mut AnyConnection,
    message: i64,
    name: &str,
    mime: &str,
    size: i64,
) {
    sqlx::query(
        "INSERT INTO attachments (message_id, original_name, mime_type, size_bytes)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(message)
    .bind(name)
    .bind(mime)
    .bind(size)
    .execute(&mut *conn)
    .await
    .unwrap();
}

pub(crate) async fn group(
    conn: &mut AnyConnection,
    account: i64,
    name: &str,
    members: &[i64],
) -> i64 {
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO contact_groups (account_id, name) VALUES ($1, $2) RETURNING id",
    )
    .bind(account)
    .bind(name)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    for c in members {
        sqlx::query("INSERT INTO contact_group_members (contact_id, group_id) VALUES ($1, $2)")
            .bind(c)
            .bind(id)
            .execute(&mut *conn)
            .await
            .unwrap();
    }
    id
}

pub(crate) async fn tag(
    conn: &mut AnyConnection,
    account: i64,
    name: &str,
    conversations: &[i64],
) -> i64 {
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO message_tags (account_id, name) VALUES ($1, $2) RETURNING id",
    )
    .bind(account)
    .bind(name)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    for c in conversations {
        sqlx::query("INSERT INTO message_tag_members (conversation_id, tag_id) VALUES ($1, $2)")
            .bind(c)
            .bind(id)
            .execute(&mut *conn)
            .await
            .unwrap();
    }
    id
}

/// A vault with two accounts and every row the spec's cases need.
pub(crate) async fn seeded() -> (sqlx::AnyPool, tempfile::TempDir, Fixture) {
    let (pool, dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    crate::db::schema::ensure_vault_schema(&mut conn)
        .await
        .unwrap();
    for (id, name) in [(ACCOUNT, "alice"), (OTHER_ACCOUNT, "bob")] {
        sqlx::query("INSERT INTO accounts (id, username) VALUES ($1, $2)")
            .bind(id)
            .bind(name)
            .execute(&mut *conn)
            .await
            .unwrap();
    }
    let a = ACCOUNT;
    let mut f = Fixture {
        me_handle: handle(&mut conn, a, "+15550000", "imessage").await,
        ..Fixture::default()
    };

    sqlx::query("INSERT INTO account_handles (account_id, handle_id) VALUES ($1, $2)")
        .bind(a)
        .bind(f.me_handle)
        .execute(&mut *conn)
        .await
        .unwrap();
    f.ana_handle = handle(&mut conn, a, "+15550001", "imessage").await;
    f.bo_handle = handle(&mut conn, a, "+15550002", "sms").await;
    f.jane_handle = handle(&mut conn, a, "jane.doe@gmail.com", "imessage").await;
    f.sam_handle = handle(&mut conn, a, "sam@icloud.com", "imessage").await;
    f.nameless_handle = handle(&mut conn, a, "+15550009", "sms").await;
    let cy_handle = handle(&mut conn, a, "+15550003", "whatsapp").await;

    f.ana = contact(&mut conn, a, "Ana", &[f.ana_handle]).await;
    f.bo = contact(&mut conn, a, "Bo", &[f.bo_handle]).await;
    f.cy = contact(&mut conn, a, "Cy", &[cy_handle]).await;
    f.jane = contact(&mut conn, a, "Jane Doe", &[f.jane_handle]).await;
    f.sam = contact(&mut conn, a, "Sam", &[f.sam_handle]).await;
    f.nameless = contact(&mut conn, a, "", &[f.nameless_handle]).await;

    f.ana_direct = conversation(
        &mut conn,
        a,
        f.ana_handle,
        "individual",
        None,
        &[f.ana_handle],
    )
    .await;
    f.bo_direct = conversation(
        &mut conn,
        a,
        f.bo_handle,
        "individual",
        None,
        &[f.bo_handle],
    )
    .await;
    f.jane_direct = conversation(
        &mut conn,
        a,
        f.jane_handle,
        "individual",
        None,
        &[f.jane_handle],
    )
    .await;
    f.sam_direct = conversation(
        &mut conn,
        a,
        f.sam_handle,
        "individual",
        None,
        &[f.sam_handle],
    )
    .await;
    let archive_chat = handle(&mut conn, a, "chat100", "imessage").await;
    f.archive_group = conversation(
        &mut conn,
        a,
        archive_chat,
        "group",
        Some("Old Times"),
        &[f.ana_handle, f.bo_handle, f.sam_handle],
    )
    .await;
    // The source named this one and gave no address for them.
    named_participant(&mut conn, f.archive_group, "Robin").await;
    let big_chat = handle(&mut conn, a, "chat200", "imessage").await;
    f.big_group = conversation(
        &mut conn,
        a,
        big_chat,
        "group",
        Some("Book Club"),
        &[f.ana_handle, f.bo_handle, f.jane_handle, f.sam_handle],
    )
    .await;
    let trashed_chat = handle(&mut conn, a, "chat300", "imessage").await;
    f.trashed_conv = conversation(
        &mut conn,
        a,
        trashed_chat,
        "group",
        Some("Gone"),
        &[f.ana_handle, f.bo_handle],
    )
    .await;
    sqlx::query("INSERT INTO trashed_conversations (account_id, conversation_id) VALUES ($1, $2)")
        .bind(a)
        .bind(f.trashed_conv)
        .execute(&mut *conn)
        .await
        .unwrap();

    f.ana_2018 = message(
        &mut conn,
        a,
        msg(
            f.ana_direct,
            "2018-03-01T10:00:00Z",
            false,
            Some(f.ana_handle),
            "hello from ana",
        ),
    )
    .await;
    f.ana_2021 = message(
        &mut conn,
        a,
        msg(f.ana_direct, "2021-05-01T10:00:00Z", true, None, "hi ana"),
    )
    .await;
    f.bo_2023 = message(
        &mut conn,
        a,
        Msg {
            service: "sms",
            ..msg(
                f.bo_direct,
                "2023-01-01T10:00:00Z",
                false,
                Some(f.bo_handle),
                "bo here",
            )
        },
    )
    .await;
    f.jane_avocado_from_me = message(
        &mut conn,
        a,
        msg(
            f.jane_direct,
            "2024-02-10T10:00:00Z",
            true,
            None,
            "want some avocado",
        ),
    )
    .await;
    f.jane_guac_from_me = message(
        &mut conn,
        a,
        msg(
            f.jane_direct,
            "2024-02-11T10:00:00Z",
            true,
            None,
            "guacamole night at mine",
        ),
    )
    .await;
    f.jane_avocado_to_me = message(
        &mut conn,
        a,
        msg(
            f.jane_direct,
            "2024-02-12T10:00:00Z",
            false,
            Some(f.jane_handle),
            "avocado toast?",
        ),
    )
    .await;
    f.sam_avocado_from_me = message(
        &mut conn,
        a,
        msg(
            f.sam_direct,
            "2024-02-13T10:00:00Z",
            true,
            None,
            "avocado again",
        ),
    )
    .await;
    f.jane_2018 = message(
        &mut conn,
        a,
        msg(
            f.jane_direct,
            "2018-06-01T10:00:00Z",
            false,
            Some(f.jane_handle),
            "first hello",
        ),
    )
    .await;
    f.feb_big_jpeg = message(
        &mut conn,
        a,
        msg(
            f.jane_direct,
            "2024-02-20T10:00:00Z",
            false,
            Some(f.jane_handle),
            "photo",
        ),
    )
    .await;
    attachment(
        &mut conn,
        f.feb_big_jpeg,
        "beach.jpg",
        "image/jpeg",
        900 * 1024,
    )
    .await;
    f.feb_small_jpeg = message(
        &mut conn,
        a,
        msg(
            f.jane_direct,
            "2024-02-21T10:00:00Z",
            false,
            Some(f.jane_handle),
            "small photo",
        ),
    )
    .await;
    attachment(
        &mut conn,
        f.feb_small_jpeg,
        "thumb.jpg",
        "image/jpeg",
        100 * 1024,
    )
    .await;
    f.feb_pdf = message(
        &mut conn,
        a,
        msg(
            f.jane_direct,
            "2024-02-22T10:00:00Z",
            false,
            Some(f.jane_handle),
            "the document",
        ),
    )
    .await;
    attachment(
        &mut conn,
        f.feb_pdf,
        "notes.pdf",
        "application/pdf",
        2 * 1024 * 1024,
    )
    .await;
    f.may_big_jpeg = message(
        &mut conn,
        a,
        msg(
            f.jane_direct,
            "2024-05-20T10:00:00Z",
            false,
            Some(f.jane_handle),
            "later photo",
        ),
    )
    .await;
    attachment(
        &mut conn,
        f.may_big_jpeg,
        "hike.jpg",
        "image/jpeg",
        900 * 1024,
    )
    .await;
    f.big_group_msg = message(
        &mut conn,
        a,
        Msg {
            subject: Some("Dinner plans"),
            ..msg(
                f.big_group,
                "2024-03-01T10:00:00Z",
                false,
                Some(f.sam_handle),
                "who is in",
            )
        },
    )
    .await;
    f.archive_msg = message(
        &mut conn,
        a,
        Msg {
            source: "whatsapp",
            service: "whatsapp",
            ..msg(
                f.archive_group,
                "2019-01-01T10:00:00Z",
                false,
                Some(f.bo_handle),
                "old",
            )
        },
    )
    .await;
    f.trashed_msg = message(
        &mut conn,
        a,
        msg(
            f.trashed_conv,
            "2019-02-01T10:00:00Z",
            false,
            Some(f.bo_handle),
            "gone",
        ),
    )
    .await;

    // A conversation whose only message a later import superseded: marked a
    // duplicate, with no other kept copy in its conversation. The handle
    // links to no contact, so this adds no contact-level count.
    let dup_only_handle = handle(&mut conn, a, "+15550098", "sms").await;
    f.dup_only_conv = conversation(
        &mut conn,
        a,
        dup_only_handle,
        "individual",
        None,
        &[dup_only_handle],
    )
    .await;
    f.dup_only_msg = message(
        &mut conn,
        a,
        msg(
            f.dup_only_conv,
            "2025-01-01T10:00:00Z",
            false,
            Some(dup_only_handle),
            "resent copy",
        ),
    )
    .await;
    sqlx::query("UPDATE messages SET duplicate_of = $1 WHERE id = $2")
        .bind(f.dup_only_msg)
        .bind(f.dup_only_msg)
        .execute(&mut *conn)
        .await
        .unwrap();

    f.family = group(&mut conn, a, "Family", &[f.ana]).await;
    f.archive = tag(&mut conn, a, "Archive", &[f.archive_group]).await;

    // The other account has one contact and one message that must never show.
    let other_handle = handle(&mut conn, OTHER_ACCOUNT, "+15559999", "imessage").await;
    contact(&mut conn, OTHER_ACCOUNT, "Ana", &[other_handle]).await;
    let other_conv = conversation(
        &mut conn,
        OTHER_ACCOUNT,
        other_handle,
        "individual",
        None,
        &[other_handle],
    )
    .await;
    message(
        &mut conn,
        OTHER_ACCOUNT,
        msg(
            other_conv,
            "2024-02-10T10:00:00Z",
            false,
            Some(other_handle),
            "avocado",
        ),
    )
    .await;

    drop(conn);
    (pool, dir, f)
}

/// Compile `q` for `list` and return the matching ids, ascending.
pub(crate) async fn run(conn: &mut AnyConnection, list: ListKind, q: &str) -> Vec<i64> {
    let f = compile(CompileRequest {
        list,
        query: q,
        account_id: ACCOUNT,
        engine: engine_of(conn),
        today: today(),
        zone: chrono_tz::UTC,
    })
    .unwrap_or_else(|e| panic!("{q:?} on {list:?}: {}", e.message));
    let table = match list {
        ListKind::Contacts => "contacts",
        ListKind::Conversations => "conversations",
        ListKind::Messages => "messages",
    };
    let alias = list.base_alias();
    let sql = renumber_placeholders(&format!(
        "SELECT {alias}.id FROM {table} {alias} WHERE {} ORDER BY {alias}.id",
        f.where_sql()
    ));
    let rows: Vec<i64> = sqlx::query_scalar_with(&sql, bind_args(f.params()))
        .fetch_all(&mut *conn)
        .await
        .unwrap_or_else(|e| panic!("{q:?} on {list:?}: {e}\n{sql}"));
    rows
}

pub(crate) fn sorted(mut ids: Vec<i64>) -> Vec<i64> {
    ids.sort_unstable();
    ids
}

/// Compile `q` for `list` expecting a refusal.
pub(crate) fn err(list: ListKind, q: &str) -> QueryError {
    compile(CompileRequest {
        list,
        query: q,
        account_id: ACCOUNT,
        engine: DbEngine::Sqlite,
        today: today(),
        zone: chrono_tz::UTC,
    })
    .expect_err("expected a refusal")
}

mod free_text {
    use super::*;

    #[tokio::test]
    async fn empty_query_returns_the_account_rows_minus_trash() {
        let (pool, _dir, f) = seeded().await;
        let mut conn = pool.acquire().await.unwrap();
        assert_eq!(
            run(&mut conn, ListKind::Contacts, "").await,
            sorted(vec![f.ana, f.bo, f.cy, f.jane, f.sam, f.nameless])
        );
        let convs = run(&mut conn, ListKind::Conversations, "").await;
        assert!(!convs.contains(&f.trashed_conv));
        assert_eq!(convs.len(), 6);
        let msgs = run(&mut conn, ListKind::Messages, "").await;
        assert!(msgs.contains(&f.ana_2018));
        assert!(!msgs.contains(&f.trashed_msg));
        assert!(
            msgs.iter().all(|id| *id <= f.trashed_msg),
            "other account's message leaked"
        );
    }

    #[tokio::test]
    async fn bare_text_is_the_rows_own_text() {
        let (pool, _dir, f) = seeded().await;
        let mut conn = pool.acquire().await.unwrap();
        // Contacts: name or handle.
        assert_eq!(run(&mut conn, ListKind::Contacts, "ana").await, vec![f.ana]);
        assert_eq!(
            run(&mut conn, ListKind::Contacts, "gmail").await,
            vec![f.jane]
        );
        // Conversations: title, or a participant's name or handle.
        assert_eq!(
            run(&mut conn, ListKind::Conversations, "book").await,
            vec![f.big_group]
        );
        assert_eq!(
            run(&mut conn, ListKind::Conversations, "jane").await,
            sorted(vec![f.jane_direct, f.big_group])
        );
        // Messages: body, subject, attachment names, through the full-text index.
        assert_eq!(
            run(&mut conn, ListKind::Messages, "avocado").await,
            sorted(vec![
                f.jane_avocado_from_me,
                f.jane_avocado_to_me,
                f.sam_avocado_from_me
            ])
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "dinner").await,
            vec![f.big_group_msg]
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "beach").await,
            vec![f.feb_big_jpeg]
        );
        // A person's name is not message text.
        assert_eq!(
            run(&mut conn, ListKind::Messages, "jane").await,
            Vec::<i64>::new()
        );
    }

    #[tokio::test]
    async fn phrases_prefixes_negation_and_or() {
        let (pool, _dir, f) = seeded().await;
        let mut conn = pool.acquire().await.unwrap();
        assert_eq!(
            run(&mut conn, ListKind::Messages, "\"guacamole night\"").await,
            vec![f.jane_guac_from_me]
        );
        assert_eq!(run(&mut conn, ListKind::Messages, "avoc*").await.len(), 3);
        assert_eq!(
            run(&mut conn, ListKind::Messages, "avocado -toast").await,
            sorted(vec![f.jane_avocado_from_me, f.sam_avocado_from_me])
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "toast or guacamole").await,
            sorted(vec![f.jane_guac_from_me, f.jane_avocado_to_me])
        );
        assert_eq!(
            run(
                &mut conn,
                ListKind::Messages,
                "(toast or guacamole) avocado"
            )
            .await,
            vec![f.jane_avocado_to_me]
        );
    }

    #[tokio::test]
    async fn free_text_finds_part_of_an_attachment_file_name() {
        let (pool, _dir, f) = seeded().await;
        let mut conn = pool.acquire().await.unwrap();
        // "each" sits inside "beach.jpg" but is not a word the full-text
        // index holds, so only the file-name match can find it. Postgres
        // reads a whole file name as one token, so without this leg a
        // search for part of a file name finds nothing there at all.
        assert_eq!(
            run(&mut conn, ListKind::Messages, "each").await,
            vec![f.feb_big_jpeg]
        );
        // A prefix and a phrase go through the same file-name match.
        assert_eq!(
            run(&mut conn, ListKind::Messages, "hike*").await,
            vec![f.may_big_jpeg]
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "\"notes.pdf\"").await,
            vec![f.feb_pdf]
        );
        // On Conversations free text is the title and the people, by
        // design, so the thread is reached by the file name's own word.
        assert_eq!(
            run(&mut conn, ListKind::Conversations, "filename:each").await,
            vec![f.jane_direct]
        );
    }

    #[tokio::test]
    async fn a_participant_the_source_only_named_is_still_searchable() {
        let (pool, _dir, f) = seeded().await;
        let mut conn = pool.acquire().await.unwrap();
        // "Robin" is in Old Times with no address of their own.
        assert_eq!(
            run(&mut conn, ListKind::Conversations, "robin").await,
            vec![f.archive_group]
        );
    }

    #[tokio::test]
    async fn a_prefix_matches_any_word_that_starts_with_it() {
        let (pool, _dir, f) = seeded().await;
        let mut conn = pool.acquire().await.unwrap();
        // The prefix starts Jane Doe's second word, not her first.
        let by_prefix = run(&mut conn, ListKind::Contacts, "doe*").await;
        assert_eq!(by_prefix, vec![f.jane]);
        // A prefix never loses a row the bare term found.
        assert_eq!(run(&mut conn, ListKind::Contacts, "doe").await, by_prefix);
        // And the same on a conversation, through the participant's name.
        assert_eq!(
            run(&mut conn, ListKind::Conversations, "doe*").await,
            sorted(vec![f.jane_direct, f.big_group])
        );
    }

    #[test]
    fn compile_is_deterministic_and_pure() {
        let req = || CompileRequest {
            list: ListKind::Messages,
            query: "avocado -toast",
            account_id: ACCOUNT,
            engine: DbEngine::Postgres,
            today: today(),
            zone: chrono_tz::UTC,
        };
        let a = compile(req()).unwrap();
        let b = compile(req()).unwrap();
        assert_eq!(a.where_sql(), b.where_sql());
        assert_eq!(a.params(), b.params());
    }

    #[test]
    fn a_fragment_mentions_only_its_base_alias_and_binds_in_order() {
        let f = compile(CompileRequest {
            list: ListKind::Contacts,
            query: "ana",
            account_id: ACCOUNT,
            engine: DbEngine::Sqlite,
            today: today(),
            zone: chrono_tz::UTC,
        })
        .unwrap();
        assert!(f.where_sql().starts_with("(ct.account_id = ?"));
        assert_eq!(f.where_sql().matches('?').count(), f.params().len());
    }

    /// Free text on Messages is two legs, the index and the file name, and
    /// each binds its own value; Postgres is the engine where the two spell
    /// themselves differently, so check the binding there.
    #[test]
    fn free_text_on_messages_binds_every_placeholder_on_postgres() {
        for query in ["avocado", "avoc*", "\"two words\""] {
            let f = compile(CompileRequest {
                list: ListKind::Messages,
                query,
                account_id: ACCOUNT,
                engine: DbEngine::Postgres,
                today: today(),
                zone: chrono_tz::UTC,
            })
            .unwrap();
            assert_eq!(
                f.where_sql().matches('?').count(),
                f.params().len(),
                "{query}"
            );
            assert!(
                !f.where_sql().contains("COLLATE NOCASE"),
                "{query}: SQLite collation leaked into Postgres SQL"
            );
        }
    }
}

mod text_words {
    use super::*;

    #[tokio::test]
    async fn name_and_handle_on_contacts() {
        let (pool, _dir, f) = seeded().await;
        let mut conn = pool.acquire().await.unwrap();
        assert_eq!(
            run(&mut conn, ListKind::Contacts, "name:jane").await,
            vec![f.jane]
        );
        assert_eq!(
            run(&mut conn, ListKind::Contacts, "name:none").await,
            vec![f.nameless]
        );
        assert_eq!(
            run(&mut conn, ListKind::Contacts, "name:any").await,
            sorted(vec![f.ana, f.bo, f.cy, f.jane, f.sam])
        );
        assert_eq!(
            run(&mut conn, ListKind::Contacts, "handle:gmail").await,
            vec![f.jane]
        );
        assert_eq!(
            run(&mut conn, ListKind::Contacts, "handle:+1555*")
                .await
                .len(),
            4
        );
        assert_eq!(
            run(&mut conn, ListKind::Contacts, "handle:none").await,
            Vec::<i64>::new()
        );
    }

    #[tokio::test]
    async fn name_handle_and_title_on_conversations() {
        let (pool, _dir, f) = seeded().await;
        let mut conn = pool.acquire().await.unwrap();
        assert_eq!(
            run(&mut conn, ListKind::Conversations, "name:jane").await,
            sorted(vec![f.jane_direct, f.big_group])
        );
        assert_eq!(
            run(&mut conn, ListKind::Conversations, "handle:icloud").await,
            sorted(vec![f.sam_direct, f.archive_group, f.big_group])
        );
        assert_eq!(
            run(&mut conn, ListKind::Conversations, "title:book").await,
            vec![f.big_group]
        );
        assert_eq!(
            run(&mut conn, ListKind::Conversations, "title:none").await,
            sorted(vec![f.ana_direct, f.bo_direct, f.jane_direct, f.sam_direct])
        );
    }

    /// A handle put on a Contact after the messages were imported.
    /// `participants.contact_id` is written once at import and never updated,
    /// so it still says nothing about this person while `contact_handles`
    /// says who they are. Search has to reach the Contact the same way the
    /// naming query does, or the conversation list shows "Robert Smith" and
    /// `name:"Robert Smith"` finds nothing.
    #[tokio::test]
    async fn name_finds_a_contact_linked_after_the_import() {
        let (pool, _dir, _f) = seeded().await;
        let mut conn = pool.acquire().await.unwrap();
        let late_handle = handle(&mut conn, ACCOUNT, "+15550777", "imessage").await;
        let conv = conversation(
            &mut conn,
            ACCOUNT,
            late_handle,
            "individual",
            None,
            &[late_handle],
        )
        .await;
        message(
            &mut conn,
            ACCOUNT,
            msg(conv, "2024-01-01T00:00:00Z", false, Some(late_handle), "hi"),
        )
        .await;
        // Only now does the handle go onto a Contact, the way linking a
        // handle, merging two contacts, or an address book adopting one does.
        contact(&mut conn, ACCOUNT, "Robert Smith", &[late_handle]).await;

        assert_eq!(
            run(&mut conn, ListKind::Conversations, "name:\"Robert Smith\"").await,
            vec![conv]
        );
        assert_eq!(
            run(&mut conn, ListKind::Conversations, "\"Robert Smith\"").await,
            vec![conv]
        );
        assert_eq!(
            run(&mut conn, ListKind::Conversations, "with:\"Robert Smith\"").await,
            vec![conv]
        );
    }

    #[tokio::test]
    async fn body_subject_and_filename() {
        let (pool, _dir, f) = seeded().await;
        let mut conn = pool.acquire().await.unwrap();
        assert_eq!(
            run(&mut conn, ListKind::Messages, "body:toast").await,
            vec![f.jane_avocado_to_me]
        );
        assert_eq!(
            run(&mut conn, ListKind::Conversations, "body:toast").await,
            vec![f.jane_direct]
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "subject:dinner").await,
            vec![f.big_group_msg]
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "subject:any").await,
            vec![f.big_group_msg]
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "filename:beach*").await,
            vec![f.feb_big_jpeg]
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "filename:.jpg").await,
            sorted(vec![f.feb_big_jpeg, f.feb_small_jpeg, f.may_big_jpeg])
        );
        assert_eq!(
            run(&mut conn, ListKind::Conversations, "filename:notes").await,
            vec![f.jane_direct]
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "-body:avocado body:any")
                .await
                .len(),
            11
        );
    }
}

mod people_words {
    use super::*;

    #[tokio::test]
    async fn from_to_and_with() {
        let (pool, _dir, f) = seeded().await;
        let mut conn = pool.acquire().await.unwrap();
        // Spec case 4.
        assert_eq!(
            run(
                &mut conn,
                ListKind::Messages,
                r#"from:me to:"Jane Doe" (avocado or "guacamole night")"#
            )
            .await,
            sorted(vec![f.jane_avocado_from_me, f.jane_guac_from_me])
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "from:jane").await,
            sorted(vec![
                f.jane_avocado_to_me,
                f.jane_2018,
                f.feb_big_jpeg,
                f.feb_small_jpeg,
                f.feb_pdf,
                f.may_big_jpeg
            ])
        );
        assert_eq!(run(&mut conn, ListKind::Messages, "from:me").await.len(), 4);
        assert_eq!(run(&mut conn, ListKind::Messages, "to:me").await.len(), 10);
        assert_eq!(
            run(&mut conn, ListKind::Messages, "from:gmail.com")
                .await
                .len(),
            6
        );
        assert_eq!(
            run(
                &mut conn,
                ListKind::Conversations,
                &format!("with:#{}", f.jane)
            )
            .await,
            sorted(vec![f.jane_direct, f.big_group])
        );
        assert_eq!(
            run(&mut conn, ListKind::Conversations, "with:sam").await,
            sorted(vec![f.sam_direct, f.archive_group, f.big_group])
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "with:bo body:old").await,
            vec![f.archive_msg]
        );
        // "Robin" is a participant the source only named: no handle of their
        // own, so only the participant-row leg of `with:` can find them.
        assert_eq!(
            run(&mut conn, ListKind::Conversations, "with:robin").await,
            vec![f.archive_group]
        );
    }

    #[tokio::test]
    async fn in_one_conversation() {
        let (pool, _dir, f) = seeded().await;
        let mut conn = pool.acquire().await.unwrap();
        assert_eq!(
            run(
                &mut conn,
                ListKind::Messages,
                &format!("in:#{}", f.jane_direct)
            )
            .await
            .len(),
            8
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "in:club").await,
            vec![f.big_group_msg]
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "in:+15550002").await,
            vec![f.bo_2023]
        );
    }

    #[tokio::test]
    async fn contact_groups_on_every_list() {
        let (pool, _dir, f) = seeded().await;
        let mut conn = pool.acquire().await.unwrap();
        assert_eq!(
            run(&mut conn, ListKind::Contacts, "group:Family").await,
            vec![f.ana]
        );
        assert_eq!(
            run(&mut conn, ListKind::Contacts, "group:family").await,
            vec![f.ana]
        );
        assert_eq!(
            run(
                &mut conn,
                ListKind::Contacts,
                &format!("group:#{}", f.family)
            )
            .await,
            vec![f.ana]
        );
        assert_eq!(
            run(&mut conn, ListKind::Contacts, "group:none").await,
            sorted(vec![f.bo, f.cy, f.jane, f.sam, f.nameless])
        );
        assert_eq!(
            run(&mut conn, ListKind::Contacts, "group:unknown").await,
            vec![f.nameless]
        );
        assert_eq!(
            run(&mut conn, ListKind::Conversations, "group:Family").await,
            sorted(vec![f.ana_direct, f.archive_group, f.big_group])
        );
        assert_eq!(
            run(&mut conn, ListKind::Conversations, "-group:Family").await,
            sorted(vec![f.bo_direct, f.jane_direct, f.sam_direct])
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "group:Family body:hello").await,
            vec![f.ana_2018]
        );
    }

    #[tokio::test]
    async fn message_tags_on_every_list() {
        let (pool, _dir, f) = seeded().await;
        let mut conn = pool.acquire().await.unwrap();
        assert_eq!(
            run(&mut conn, ListKind::Conversations, "tag:Archive").await,
            vec![f.archive_group]
        );
        assert_eq!(
            run(&mut conn, ListKind::Conversations, "tag:none")
                .await
                .len(),
            5
        );
        assert_eq!(
            run(&mut conn, ListKind::Contacts, "tag:Archive").await,
            sorted(vec![f.ana, f.bo, f.sam])
        );
        assert_eq!(
            run(
                &mut conn,
                ListKind::Messages,
                &format!("tag:#{}", f.archive)
            )
            .await,
            vec![f.archive_msg]
        );
    }

    #[tokio::test]
    async fn tag_none_is_the_true_complement_on_every_list() {
        let (pool, _dir, f) = seeded().await;
        let mut conn = pool.acquire().await.unwrap();
        // Ana, Bo, and Sam are all in Old Times, which carries Archive, so
        // none of them is untagged — even though each of them also has a
        // conversation that carries no tag.
        assert_eq!(
            run(&mut conn, ListKind::Contacts, "tag:none").await,
            sorted(vec![f.cy, f.jane, f.nameless])
        );
        // On Messages the tag belongs to the message's own conversation.
        let untagged = run(&mut conn, ListKind::Messages, "tag:none").await;
        assert!(!untagged.contains(&f.archive_msg));
        assert!(untagged.contains(&f.jane_avocado_from_me));
    }

    #[tokio::test]
    async fn import_runs() {
        let (pool, _dir, f) = seeded().await;
        let mut conn = pool.acquire().await.unwrap();
        let run_id: i64 = sqlx::query_scalar(
            "INSERT INTO vault_imports (account_id, source, mode, status, started_at)
             VALUES ($1, 'imessage', 'push', 'completed', '2024-01-01T00:00:00Z') RETURNING id",
        )
        .bind(ACCOUNT)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
        sqlx::query("UPDATE messages SET import_id = $1 WHERE id = $2")
            .bind(run_id)
            .bind(f.bo_2023)
            .execute(&mut *conn)
            .await
            .unwrap();
        // The duplicate-only message is part of this run too: a search about
        // one Import Run looks at every message it touched, duplicates
        // included, since a re-import is often nothing but duplicates.
        sqlx::query("UPDATE messages SET import_id = $1 WHERE id = $2")
            .bind(run_id)
            .bind(f.dup_only_msg)
            .execute(&mut *conn)
            .await
            .unwrap();
        assert_eq!(
            run(&mut conn, ListKind::Messages, "import:last").await,
            sorted(vec![f.bo_2023, f.dup_only_msg])
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, &format!("import:#{run_id}")).await,
            sorted(vec![f.bo_2023, f.dup_only_msg])
        );
        assert_eq!(
            run(&mut conn, ListKind::Conversations, "import:last").await,
            sorted(vec![f.bo_direct, f.dup_only_conv])
        );
        // Without `import:`, the duplicate-only conversation stays hidden:
        // only a search about the run itself looks at it.
        assert!(
            !run(&mut conn, ListKind::Conversations, "")
                .await
                .contains(&f.dup_only_conv)
        );
    }
}

mod kind_words {
    use super::*;

    #[tokio::test]
    async fn kind_service_and_source() {
        let (pool, _dir, f) = seeded().await;
        let mut conn = pool.acquire().await.unwrap();
        assert_eq!(
            run(&mut conn, ListKind::Conversations, "kind:direct").await,
            sorted(vec![f.ana_direct, f.bo_direct, f.jane_direct, f.sam_direct])
        );
        assert_eq!(
            run(&mut conn, ListKind::Contacts, "kind:group").await,
            sorted(vec![f.ana, f.bo, f.jane, f.sam])
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "service:sms").await,
            vec![f.bo_2023]
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "service:sms,whatsapp").await,
            sorted(vec![f.bo_2023, f.archive_msg])
        );
        assert_eq!(
            run(&mut conn, ListKind::Contacts, "service:sms").await,
            vec![f.bo]
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "source:whatsapp").await,
            vec![f.archive_msg]
        );
        assert_eq!(
            run(&mut conn, ListKind::Conversations, "source:imessage")
                .await
                .len(),
            5
        );
    }

    #[tokio::test]
    async fn attachments_by_kind_and_size() {
        let (pool, _dir, f) = seeded().await;
        let mut conn = pool.acquire().await.unwrap();
        assert_eq!(
            run(&mut conn, ListKind::Messages, "attachment:image").await,
            sorted(vec![f.feb_big_jpeg, f.feb_small_jpeg, f.may_big_jpeg])
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "attachment:pdf").await,
            vec![f.feb_pdf]
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "attachment:document").await,
            vec![f.feb_pdf]
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "attachment:video").await,
            Vec::<i64>::new()
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "attachment:any")
                .await
                .len(),
            4
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "attachment:none")
                .await
                .len(),
            10
        );
        assert_eq!(
            run(&mut conn, ListKind::Conversations, "attachment:image").await,
            vec![f.jane_direct]
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "size:>500k").await,
            sorted(vec![f.feb_big_jpeg, f.feb_pdf, f.may_big_jpeg])
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "size:<500k").await,
            vec![f.feb_small_jpeg]
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "size:100k..2M")
                .await
                .len(),
            4
        );
    }

    #[tokio::test]
    async fn trash_is_a_word() {
        let (pool, _dir, f) = seeded().await;
        let mut conn = pool.acquire().await.unwrap();
        assert_eq!(
            run(&mut conn, ListKind::Conversations, "trashed:yes").await,
            vec![f.trashed_conv]
        );
        assert_eq!(
            run(&mut conn, ListKind::Conversations, "trashed:no")
                .await
                .len(),
            6
        );
        assert_eq!(
            run(&mut conn, ListKind::Conversations, "trashed:any")
                .await
                .len(),
            7
        );
        assert_eq!(
            run(&mut conn, ListKind::Conversations, "trashed:yes gone").await,
            vec![f.trashed_conv]
        );
        sqlx::query("INSERT INTO trashed_contacts (account_id, contact_id) VALUES ($1, $2)")
            .bind(ACCOUNT)
            .bind(f.cy)
            .execute(&mut *conn)
            .await
            .unwrap();
        assert_eq!(
            run(&mut conn, ListKind::Contacts, "trashed:yes").await,
            vec![f.cy]
        );
        assert!(!run(&mut conn, ListKind::Contacts, "").await.contains(&f.cy));
    }

    /// Pins the unchanged default: a Messages query that never mentions
    /// `trashed:` still leaves the trashed conversation's message out, even
    /// one that matches the query on its own text. This must pass before
    /// and after the Messages arm gets its `trashed:` gate — if it ever
    /// goes red, the gate broke the default every Export and download
    /// relies on.
    #[tokio::test]
    async fn messages_still_exclude_trash_by_default() {
        let (pool, _dir, f) = seeded().await;
        let mut conn = pool.acquire().await.unwrap();
        assert!(
            !run(&mut conn, ListKind::Messages, "")
                .await
                .contains(&f.trashed_msg)
        );
        assert!(run(&mut conn, ListKind::Messages, "gone").await.is_empty());
    }

    /// `trashed:yes` on Messages answers for the trashed conversation's
    /// message only, the same as it does on Contacts and Conversations.
    #[tokio::test]
    async fn messages_trashed_yes_finds_the_trashed_conversations_message() {
        let (pool, _dir, f) = seeded().await;
        let mut conn = pool.acquire().await.unwrap();
        assert_eq!(
            run(&mut conn, ListKind::Messages, "trashed:yes").await,
            vec![f.trashed_msg]
        );
    }

    /// `trashed:any` on Messages lifts the default: both the trashed
    /// conversation's message and an ordinary one come back.
    #[tokio::test]
    async fn messages_trashed_any_returns_both() {
        let (pool, _dir, f) = seeded().await;
        let mut conn = pool.acquire().await.unwrap();
        let any = run(&mut conn, ListKind::Messages, "trashed:any").await;
        assert!(any.contains(&f.trashed_msg));
        assert!(any.contains(&f.ana_2018));
    }
}

mod measure_words {
    use super::*;

    #[tokio::test]
    async fn dates_on_every_list() {
        let (pool, _dir, f) = seeded().await;
        let mut conn = pool.acquire().await.unwrap();
        // Spec case 2.
        assert_eq!(
            run(
                &mut conn,
                ListKind::Messages,
                "date:2024-01..2024-03 attachment:image size:>500k"
            )
            .await,
            vec![f.feb_big_jpeg]
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "date:2018").await,
            sorted(vec![f.ana_2018, f.jane_2018])
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "date:>=2024-05").await,
            vec![f.may_big_jpeg]
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "date:<2019").await.len(),
            2
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "date:2024-02-12").await,
            vec![f.jane_avocado_to_me]
        );
        assert_eq!(
            run(&mut conn, ListKind::Contacts, "date:2023").await,
            vec![f.bo]
        );
        assert_eq!(
            run(&mut conn, ListKind::Conversations, "date:2019").await,
            vec![f.archive_group]
        );
        // A relative span resolves against the request's today, 2026-09-02.
        assert_eq!(
            run(&mut conn, ListKind::Messages, "date:1y").await,
            Vec::<i64>::new()
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "date:<1y").await.len(),
            14
        );
    }

    #[tokio::test]
    async fn first_and_last_message() {
        let (pool, _dir, f) = seeded().await;
        let mut conn = pool.acquire().await.unwrap();
        // Spec case 5.
        assert_eq!(
            run(
                &mut conn,
                ListKind::Contacts,
                "first-message:<2020 last-message:>=2024-01-01 handle:@gmail.com"
            )
            .await,
            vec![f.jane]
        );
        assert_eq!(
            run(&mut conn, ListKind::Contacts, "first-message:<2019").await,
            sorted(vec![f.ana, f.jane])
        );
        assert_eq!(
            run(&mut conn, ListKind::Contacts, "last-message:<2024-03").await,
            Vec::<i64>::new()
        );
        assert_eq!(
            run(&mut conn, ListKind::Conversations, "last-message:<2022").await,
            sorted(vec![f.ana_direct, f.archive_group])
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "first-message:2018 body:hi").await,
            vec![f.ana_2021]
        );
    }

    /// `messages:` on Contacts and the number in the contact drawer must
    /// agree once something is trashed: both leave trashed conversations
    /// out (#328). Messages in the trashed group change nobody's count.
    #[tokio::test]
    async fn a_contacts_message_count_leaves_trashed_conversations_out() {
        let (pool, _dir, f) = seeded().await;
        let mut conn = pool.acquire().await.unwrap();
        let before_any = run(&mut conn, ListKind::Contacts, "messages:>0").await;
        let before_many = run(&mut conn, ListKind::Contacts, "messages:>=3").await;
        for i in 0..5 {
            let ts = format!("2024-06-0{}T10:00:00Z", i + 1);
            message(
                &mut conn,
                ACCOUNT,
                msg(f.trashed_conv, &ts, false, Some(f.ana_handle), "gone"),
            )
            .await;
        }
        assert_eq!(
            run(&mut conn, ListKind::Contacts, "messages:>0").await,
            before_any
        );
        assert_eq!(
            run(&mut conn, ListKind::Contacts, "messages:>=3").await,
            before_many
        );
        // Asking for the trash lifts the default and the messages count.
        assert!(
            run(&mut conn, ListKind::Contacts, "trashed:any messages:>=3")
                .await
                .contains(&f.ana),
            "with trashed:any the trashed group's messages count for Ana"
        );
    }

    #[tokio::test]
    async fn counts() {
        let (pool, _dir, f) = seeded().await;
        let mut conn = pool.acquire().await.unwrap();
        // Spec case 1: Ana is in Family, Cy has no messages.
        assert_eq!(
            run(&mut conn, ListKind::Contacts, "group:none messages:>0").await,
            sorted(vec![f.bo, f.jane, f.sam])
        );
        assert_eq!(
            run(&mut conn, ListKind::Contacts, "messages:0").await,
            sorted(vec![f.cy, f.nameless])
        );
        assert_eq!(
            run(&mut conn, ListKind::Contacts, "conversations:0").await,
            sorted(vec![f.cy, f.nameless])
        );
        assert_eq!(
            run(&mut conn, ListKind::Contacts, "conversations:>=3").await,
            sorted(vec![f.ana, f.bo, f.sam])
        );
        assert_eq!(
            run(&mut conn, ListKind::Contacts, "groups:>0").await,
            vec![f.ana]
        );
        assert_eq!(
            run(&mut conn, ListKind::Conversations, "messages:>=2").await,
            sorted(vec![f.ana_direct, f.jane_direct])
        );
        // Spec case 3.
        assert_eq!(
            run(
                &mut conn,
                ListKind::Conversations,
                "participants:>2 -tag:Archive"
            )
            .await,
            vec![f.big_group]
        );
        // The archive group now carries a fourth, name-only participant
        // (Robin), so it clears the bar too, alongside the book club.
        assert_eq!(
            run(&mut conn, ListKind::Messages, "participants:>3").await,
            sorted(vec![f.archive_msg, f.big_group_msg])
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "attachments:>0")
                .await
                .len(),
            4
        );
        assert_eq!(
            run(&mut conn, ListKind::Messages, "attachments:0 date:2024-02")
                .await
                .len(),
            4
        );
    }
}

mod coverage {
    use super::*;
    use crate::search::fields::{FIELDS, ValueType};

    /// One representative value per shape, plus every keyword a word lists.
    ///
    /// `import:` is documented (`parse.rs`) as the one Name word with no
    /// plain-name fallback — it takes only `#id` or `last` — so it is the
    /// one word this function special-cases rather than offering the plain
    /// text and quoted-name samples every other Name/Person word accepts.
    fn sample_values(word: &str, vt: ValueType, keywords: &[&str]) -> Vec<String> {
        let mut out: Vec<String> = keywords.iter().map(|k| k.to_string()).collect();
        match vt {
            ValueType::Text => out.extend(["x".into(), "pre*".into(), "\"two words\"".into()]),
            ValueType::Name | ValueType::Person if word == "import" => out.push("#7".into()),
            ValueType::Name | ValueType::Person => {
                out.extend(["x".into(), "#7".into(), "\"Two Words\"".into()]);
            }
            ValueType::Date => out.extend([
                "2019".into(),
                ">=2024-05".into(),
                "<7d".into(),
                "2019..2021".into(),
            ]),
            ValueType::Count => out.extend(["0".into(), ">1".into(), "1..3".into()]),
            ValueType::Size => out.extend(["1M".into(), "<500k".into(), "100k..2M".into()]),
            ValueType::Choice | ValueType::Flag => {}
        }
        out
    }

    #[tokio::test]
    async fn every_word_compiles_and_runs_on_every_list_it_claims() {
        let (pool, _dir, _f) = seeded().await;
        let mut conn = pool.acquire().await.unwrap();
        for spec in FIELDS {
            for list in spec.lists {
                for value in sample_values(spec.word, spec.value_type, spec.values) {
                    for q in [
                        format!("{}:{value}", spec.word),
                        format!("-{}:{value}", spec.word),
                        format!("{}:{value} or x", spec.word),
                    ] {
                        run(&mut conn, *list, &q).await;
                    }
                }
            }
        }
    }

    #[test]
    fn every_word_compiles_for_postgres_too() {
        for spec in FIELDS {
            for list in spec.lists {
                for value in sample_values(spec.word, spec.value_type, spec.values) {
                    let q = format!("{}:{value}", spec.word);
                    let f = compile(CompileRequest {
                        list: *list,
                        query: &q,
                        account_id: ACCOUNT,
                        engine: DbEngine::Postgres,
                        today: today(),
                        zone: chrono_tz::UTC,
                    })
                    .unwrap_or_else(|e| panic!("{q} on {list:?}: {}", e.message));
                    assert_eq!(
                        f.where_sql().matches('?').count(),
                        f.params().len(),
                        "{q} on {list:?}"
                    );
                    assert!(
                        !f.where_sql().contains("COLLATE NOCASE"),
                        "{q}: SQLite collation leaked into Postgres SQL"
                    );
                }
            }
        }
    }

    /// The registry is the one place a word exists, so a slip here would
    /// hand the parser, the web's suggestions, and the docs page three
    /// different languages.
    #[test]
    fn the_registry_is_well_formed() {
        assert_eq!(FIELDS.len(), 27, "the language has twenty-seven words");
        for (i, spec) in FIELDS.iter().enumerate() {
            assert!(
                FIELDS[..i].iter().all(|f| f.word != spec.word),
                "{} appears twice",
                spec.word
            );
            assert!(
                spec.example.starts_with(&format!("{}:", spec.word)),
                "{}'s example does not start with the word itself: {}",
                spec.word,
                spec.example
            );
            assert!(!spec.lists.is_empty(), "{} is on no list", spec.word);
            for value in spec.values {
                assert_eq!(
                    *value,
                    value.to_lowercase(),
                    "{}'s value {value} is not lower case",
                    spec.word
                );
            }
        }
    }

    #[test]
    fn every_word_is_described_on_every_list_it_claims() {
        for spec in FIELDS {
            for list in spec.lists {
                let docs = crate::search::describe(*list);
                let doc = docs
                    .iter()
                    .find(|d| d.word == spec.word)
                    .unwrap_or_else(|| panic!("{} missing from describe({list:?})", spec.word));
                assert_eq!(doc.value_type, spec.value_type, "{} on {list:?}", spec.word);
                assert!(
                    !doc.help.is_empty() && !doc.example.is_empty(),
                    "{} on {list:?}",
                    spec.word
                );
            }
        }
    }
}

mod docs {
    use crate::search::ListKind;
    use crate::search::fields::{FIELDS, for_list, lookup};

    const SEARCH_PAGE: &str =
        include_str!("../../../../../docs/src/content/docs/vault/user/how-to/search.mdx");
    const API_PAGE: &str =
        include_str!("../../../../../docs/src/content/docs/vault/developer/reference/api.md");
    const BROWSE_PAGE: &str =
        include_str!("../../../../../docs/src/content/docs/vault/user/browse-your-messages.md");

    /// Every backticked `word:` token on `line`, in order. `search.mdx`
    /// lists one per table row; `api.md`'s prose bullets often name several
    /// before the dash (`` `date:`, `first-message:`, `last-message:` ``),
    /// so both pages read through this rather than assuming one-per-line.
    fn words_on_line(line: &str) -> impl Iterator<Item = String> + '_ {
        line.split('`').filter_map(|token| {
            let word = token.strip_suffix(':')?;
            (!word.is_empty() && word.chars().all(|c| c.is_ascii_lowercase() || c == '-'))
                .then(|| word.to_string())
        })
    }

    /// The words `search.mdx`'s table lists, one row at a time.
    fn search_page_words() -> Vec<String> {
        SEARCH_PAGE
            .lines()
            .filter(|l| l.starts_with("| `"))
            .flat_map(words_on_line)
            .collect()
    }

    /// The words `api.md`'s bullets list, a bullet possibly naming several.
    ///
    /// Only the bullets under the "Search operators" heading count. The page has
    /// backticked `word:` bullets elsewhere — the two `Content-Type:` lines
    /// under "Import body" — and reading the whole file would report them as
    /// search words the language does not have, which is a confusing way for
    /// this test to fail.
    fn api_page_words() -> Vec<String> {
        API_PAGE
            .lines()
            .skip_while(|l| *l != SEARCH_SECTION)
            .skip(1)
            .take_while(|l| !l.starts_with("## "))
            .filter(|l| l.trim_start().starts_with("- `"))
            .flat_map(words_on_line)
            .collect()
    }

    /// The heading `api_page_words` scans between. A rename in `api.md`
    /// empties that scan, so this test asserts the heading is still there.
    const SEARCH_SECTION: &str = "## Search operators (`q`)";

    #[test]
    fn the_api_reference_still_has_a_search_operators_section() {
        assert!(
            API_PAGE.lines().any(|l| l == SEARCH_SECTION),
            "api.md no longer has a {SEARCH_SECTION:?} heading, so \
             the_api_reference_lists_every_messages_word_and_nothing_else \
             would scan nothing"
        );
    }

    /// The word in front of the colon in every backticked `word:value` token
    /// in `page`. `words_on_line` only sees a token that *ends* in a colon,
    /// which is how a reference table names a word; a page writing prose
    /// names one by spelling out a whole term instead.
    ///
    /// Tokens holding a `/` or a space are skipped, so a URL, a file path, or
    /// a header (`http://127.0.0.1:8080`, `crates/…/lib.rs:7`) does not read
    /// as a search word.
    fn prose_words(page: &str) -> Vec<String> {
        page.split('`')
            .skip(1)
            .step_by(2)
            .filter(|token| !token.contains('/') && !token.contains(' '))
            .filter_map(|token| {
                let word = token.split_once(':')?.0;
                (!word.is_empty() && word.chars().all(|c| c.is_ascii_lowercase() || c == '-'))
                    .then(|| word.to_string())
            })
            .collect()
    }

    /// The tour page names its search words in prose rather than in a table,
    /// so `the_page_lists_every_word_and_nothing_else` never read it. It told
    /// people to search `is:group` for years — an operator the language has
    /// never had. This is the check that would have caught it.
    #[test]
    fn the_browse_page_names_only_words_the_language_has() {
        let words = prose_words(BROWSE_PAGE);
        assert!(
            !words.is_empty(),
            "browse-your-messages.md names no search word at all, so this test \
             is no longer reading what it thinks it is"
        );
        for word in &words {
            assert!(
                lookup(word).is_some(),
                "browse-your-messages.md tells people to search {word}:, which \
                 the language does not have"
            );
        }
    }

    /// The letters inside `<ListTiles on="..." />` on one table row of
    /// `search.mdx`: which lists the row says the word applies to.
    fn tiles_on_line(line: &str) -> Option<Vec<char>> {
        let marker = "<ListTiles on=\"";
        let start = line.find(marker)? + marker.len();
        let end = start + line[start..].find('"')?;
        Some(
            line[start..end]
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect(),
        )
    }

    /// The letter `search.mdx` uses for a list.
    fn tile_letter(list: ListKind) -> char {
        match list {
            ListKind::Contacts => 'C',
            ListKind::Conversations => 'V',
            ListKind::Messages => 'M',
        }
    }

    /// The table's per-row tiles must say the same thing as the registry's
    /// `lists`. This drifted on the first change that touched it: `trashed:`
    /// was registered for Messages too and the row kept saying `C V` (#328).
    #[test]
    fn the_page_marks_each_word_for_exactly_the_lists_the_registry_does() {
        for line in SEARCH_PAGE.lines().filter(|l| l.starts_with("| `")) {
            let Some(word) = words_on_line(line).next() else {
                continue;
            };
            let Some(spec) = lookup(&word) else {
                continue; // reported by the_page_lists_every_word_and_nothing_else
            };
            let mut documented = tiles_on_line(line)
                .unwrap_or_else(|| panic!("search.mdx row for {word}: has no <ListTiles on=…/>"));
            documented.sort_unstable();
            let mut registered: Vec<char> = spec.lists.iter().map(|l| tile_letter(*l)).collect();
            registered.sort_unstable();
            assert_eq!(
                documented, registered,
                "search.mdx marks {word}: for {documented:?} but the registry says {registered:?}"
            );
        }
    }

    #[test]
    fn the_page_lists_every_word_and_nothing_else() {
        let documented = search_page_words();
        for spec in FIELDS {
            assert!(
                documented.contains(&spec.word.to_string()),
                "search.mdx is missing {}:",
                spec.word
            );
        }
        for word in &documented {
            assert!(
                FIELDS.iter().any(|f| f.word == word),
                "search.mdx lists {word}:, which the language does not have"
            );
        }
    }

    /// `api.md` describes what an Export Run's `query` scope accepts, which
    /// compiles as the Messages list (`messages_api::message_filter`). It
    /// should name every word the registry marks for Messages, and no word
    /// the registry does not have at all — the same shape of check as
    /// `the_page_lists_every_word_and_nothing_else`, scoped to one list.
    #[test]
    fn the_api_reference_lists_every_messages_word_and_nothing_else() {
        let documented = api_page_words();
        let messages_words: Vec<&'static str> =
            for_list(ListKind::Messages).map(|f| f.word).collect();
        for word in &messages_words {
            assert!(
                documented.iter().any(|d| d == word),
                "api.md is missing {word}:, which fields.rs marks for the Messages list"
            );
        }
        for word in &documented {
            assert!(
                messages_words.contains(&word.as_str()),
                "api.md lists {word}:, but fields.rs does not mark it for the Messages list \
                 ({})",
                match lookup(word) {
                    Some(spec) => format!("it registers {word}: for {}", tiles_str(spec.lists)),
                    None => format!("the language does not have {word}: at all"),
                }
            );
        }
    }

    /// The tile letters `<ListTiles on="…" />` uses, as `search.mdx`'s own
    /// intro states them: "C" Contacts, "V" Conversations, "M" Messages.
    fn tile(letter: &str, line: &str) -> ListKind {
        match letter {
            "C" => ListKind::Contacts,
            "V" => ListKind::Conversations,
            "M" => ListKind::Messages,
            other => panic!("search.mdx ListTiles has an unknown tile {other:?} in: {line}"),
        }
    }

    /// The `ListKind`s named in one row's `<ListTiles on="…" />`.
    fn row_tiles(line: &str) -> Vec<ListKind> {
        let after_on = line
            .split("on=\"")
            .nth(1)
            .unwrap_or_else(|| panic!("search.mdx row has no ListTiles on=\"…\": {line}"));
        let letters = after_on
            .split('"')
            .next()
            .unwrap_or_else(|| panic!("search.mdx row's ListTiles on=\"…\" never closes: {line}"));
        letters.split_whitespace().map(|l| tile(l, line)).collect()
    }

    /// `lists`, rendered as the same letters `<ListTiles on="…" />` uses, in
    /// Contacts/Conversations/Messages order, for an assertion message.
    fn tiles_str(lists: &[ListKind]) -> String {
        [
            (ListKind::Contacts, "C"),
            (ListKind::Conversations, "V"),
            (ListKind::Messages, "M"),
        ]
        .into_iter()
        .filter(|(kind, _)| lists.contains(kind))
        .map(|(_, letter)| letter)
        .collect::<Vec<_>>()
        .join(" ")
    }

    fn is_same_lists(a: &[ListKind], b: &[ListKind]) -> bool {
        let has = |lists: &[ListKind], kind: ListKind| lists.contains(&kind);
        [
            ListKind::Contacts,
            ListKind::Conversations,
            ListKind::Messages,
        ]
        .into_iter()
        .all(|kind| has(a, kind) == has(b, kind))
    }

    /// Issue #328: the word-only check above says nothing about *which*
    /// lists a row claims a word applies to. `trashed:` sat at `on="C V"`
    /// after a pull request registered it for Messages too, with CI green
    /// throughout, because nothing read the tile letters. This reads them
    /// and compares against `fields.rs`.
    #[test]
    fn each_rows_list_tiles_match_the_registry() {
        for line in SEARCH_PAGE.lines().filter(|l| l.starts_with("| `")) {
            let word = words_on_line(line)
                .next()
                .unwrap_or_else(|| panic!("search.mdx row names no word: {line}"));
            let spec = lookup(&word).unwrap_or_else(|| {
                panic!("search.mdx lists {word}:, which the language does not have")
            });
            let page_lists = row_tiles(line);
            if !is_same_lists(&page_lists, spec.lists) {
                panic!(
                    "search.mdx says {word}: applies to {}, but fields.rs registers it for {}",
                    tiles_str(&page_lists),
                    tiles_str(spec.lists),
                );
            }
        }
    }
}

/// Reads `tests/fixtures/search/web-queries.txt`, generated by
/// `web/src/lib/searchQuery.test.ts`: one line per query the web's search
/// builders (`web/src/lib/searchQuery.ts`) can produce, as `list<TAB>query`.
/// This is the other half of that generation — the web writes the fixture,
/// this reads it back, and the two sides can only agree because each query
/// actually parses on the list its own first column names. Nothing here
/// checks the query against seeded data; that is what `every_word_compiles_*`
/// above already does per word. This checks the builders' *composition* of
/// several words together, the shape a hand-picked per-word sample can't
/// cover.
mod web_fixture {
    use super::*;

    const FIXTURE: &str = include_str!("../../../../../tests/fixtures/search/web-queries.txt");

    fn list_named(name: &str) -> Option<ListKind> {
        match name {
            "contacts" => Some(ListKind::Contacts),
            "conversations" => Some(ListKind::Conversations),
            "messages" => Some(ListKind::Messages),
            _ => None,
        }
    }

    #[test]
    fn every_query_the_web_can_build_parses_on_its_list() {
        for (i, line) in FIXTURE.lines().enumerate() {
            let lineno = i + 1;
            if line.is_empty() {
                continue;
            }
            let (list_name, query) = line.split_once('\t').unwrap_or_else(|| {
                panic!(
                    "tests/fixtures/search/web-queries.txt:{lineno}: no tab between the \
                     list name and the query: {line:?}"
                )
            });
            let list = list_named(list_name).unwrap_or_else(|| {
                panic!(
                    "tests/fixtures/search/web-queries.txt:{lineno}: {list_name:?} is not \
                     a list name (want contacts, conversations, or messages)"
                )
            });
            compile(CompileRequest {
                list,
                query,
                account_id: ACCOUNT,
                engine: DbEngine::Sqlite,
                today: today(),
                zone: chrono_tz::UTC,
            })
            .unwrap_or_else(|e| {
                panic!(
                    "tests/fixtures/search/web-queries.txt:{lineno}: {list_name} query \
                     {query:?} does not parse: {}",
                    e.message
                )
            });
        }
    }
}

mod refusals {
    use super::*;
    use crate::search::error::QueryErrorKind;

    #[test]
    fn a_refusal_never_queries_and_names_the_word() {
        let e = err(ListKind::Contacts, "from:me");
        assert_eq!(e.kind, QueryErrorKind::WrongList);
        assert_eq!(e.span, 0..7);
        assert_eq!(e.field, Some("from"));
        // Not a search word on any list, and not within two edits of one, so
        // no "Did you mean" is attached.
        let e = err(ListKind::Messages, "wombat:Family");
        assert_eq!(e.kind, QueryErrorKind::UnknownWord);
        assert_eq!(e.did_you_mean, None);
        let e = err(ListKind::Conversations, "paticipants:>2");
        assert_eq!(e.did_you_mean, Some("participants"));
        assert_eq!(
            err(ListKind::Messages, "tag:").kind,
            QueryErrorKind::EmptyValue
        );
        assert_eq!(
            err(ListKind::Messages, "(a or b").kind,
            QueryErrorKind::Unbalanced
        );
        assert_eq!(
            err(ListKind::Messages, "date:2019-13").kind,
            QueryErrorKind::BadValue
        );
        assert_eq!(
            err(ListKind::Messages, &"a ".repeat(40)).kind,
            QueryErrorKind::TooComplex
        );
        assert_eq!(
            err(ListKind::Messages, &"x".repeat(3000)).kind,
            QueryErrorKind::TooLong
        );
    }

    /// An unknown word's message is the plain "word: is not a search word."
    /// sentence, full stop; any "Did you mean" suffix comes only from a word
    /// that is actually in today's word list, never from a spelling the
    /// language used to have. These nine are invented, made-up spellings
    /// with no history in this language at all.
    #[test]
    fn an_unknown_word_answers_plainly_and_any_suggestion_comes_from_the_current_words() {
        for made_up in [
            "postmark:2020",
            "afterglow:2020",
            "carries:attachment",
            "resembles:direct",
            "biggerthan:1M",
            "amongst:Family",
            "caption:x",
            "wording:hi",
            "mediakind:image",
        ] {
            let e = err(ListKind::Messages, made_up);
            assert_eq!(e.kind, QueryErrorKind::UnknownWord, "{made_up}");
            let word = made_up.split(':').next().unwrap();
            assert_eq!(
                e.message.trim_end_matches(|c: char| c != '.'),
                format!("{word}: is not a search word."),
                "{made_up}"
            );
        }
    }
}
