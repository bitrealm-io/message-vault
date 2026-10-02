//! A small Apple Messages `chat.db` for tests.
//!
//! Two crates read it: `imessage-reader`, whose own tests open it in process
//! to reach the session, the emitter and the attachment code without building
//! the binary, and `imessage-ir-exporter`, whose process-seam test spawns the
//! built binary against it. The file is written with rusqlite alone, so the
//! exporter's test binary links no GPL code
//! (`docs/adr/0014-gpl-code-only-behind-a-process-boundary.md`).
//!
//! Nothing here comes from a real backup. Every row is made up.

use std::{
    fs,
    path::{Path, PathBuf},
};

use rusqlite::Connection;

/// The owner's number, as `message.destination_caller_id` carries it.
/// `chat.account_login` stores it with Apple's `P:` prefix.
pub const OWNER: &str = "+15550000001";

/// The owner's email address, the second identity the owner sends from, as
/// `message.destination_caller_id` carries it. `chat.account_login` stores
/// it with Apple's `E:` prefix (`E:owner@example.com`).
pub const OWNER_EMAIL: &str = "owner@example.com";

/// The first contact's number: the other side of the direct chat and a member
/// of the group chat.
pub const FRIEND_PHONE: &str = "+15550000002";

/// The second contact's address: a member of the group chat, and the other
/// side of the direct chat the owner runs from the email account.
pub const FRIEND_EMAIL: &str = "friend@example.com";

/// The first contact's email address. Apple links it to [`FRIEND_PHONE`]
/// by giving both handles one `person_centric_id`, as a recent macOS or iOS
/// does for two addresses on one contact card.
pub const FRIEND_PHONE_EMAIL: &str = "sam@example.com";

/// The group chat's identifier.
pub const GROUP_CHAT_IDENTIFIER: &str = "chat100";

/// The identifier of the group chat nobody named, which every member but
/// [`FRIEND_EMAIL`] has left.
pub const SHRUNK_GROUP_IDENTIFIER: &str = "chat200";

/// The name the group chat was given.
pub const GROUP_TITLE: &str = "Weekend plans";

/// The bytes of the one attachment on disk.
pub const PHOTO_BYTES: &[u8] = b"not really a jpeg";

/// Seconds since 2001-01-01 as the nanosecond stamp `chat.db` stores.
#[must_use]
pub fn apple_nanos(seconds_since_2001: i64) -> i64 {
    seconds_since_2001 * 1_000_000_000
}

/// Write a Mac `chat.db` into `dir` and return its path.
///
/// The database holds four handles, six chats, twelve messages, and one
/// attachment whose file (`photo.jpg`) is written beside the database. The
/// owner sends from two addresses: the phone ([`OWNER`]) and the email
/// ([`OWNER_EMAIL`]), each stored the way Apple stores it, `P:`-prefixed and
/// `E:`-prefixed in `chat.account_login` and bare in
/// `message.destination_caller_id`, except for one row that carries the
/// phone as `tel:` + [`OWNER`], as real iPhone databases do on some
/// outgoing rows.
///
/// - handle 1: [`FRIEND_PHONE`], `person_centric_id` `person-sam`
/// - handle 2: [`FRIEND_EMAIL`], no `person_centric_id`
/// - handle 3: [`FRIEND_PHONE_EMAIL`], `person_centric_id` `person-sam`: the
///   same person as handle 1
/// - handle 4: [`OWNER`], the owner's own number, which Apple lists among a
///   group's members
///
/// `chat.style` is Apple's: 45 for a one-to-one chat, 43 for a group.
///
/// - chat 1: direct with [`FRIEND_PHONE`], `account_login` `P:` + [`OWNER`]
/// - chat 2: the group [`GROUP_TITLE`], both friends and the owner's own
///   handle, the same account
/// - chat 3: direct with [`FRIEND_EMAIL`], `account_login` `E:` +
///   [`OWNER_EMAIL`]
/// - chat 4: the unnamed group [`SHRUNK_GROUP_IDENTIFIER`], with
///   [`FRIEND_EMAIL`] the one member left
/// - chat 5: direct with [`FRIEND_PHONE_EMAIL`], with no
///   `chat_handle_join` rows
/// - chat 6: the owner's chat with the owner's own number [`OWNER`], whose
///   one member is the owner's handle
///
/// - message 1: incoming from [`FRIEND_PHONE`] in chat 1, no text, carrying
///   the photo, read by the owner one minute later (`date_read` set)
/// - message 2: outgoing "Nice" in chat 1, never read (`date_read` NULL)
/// - message 3: incoming "Saturday works" from [`FRIEND_EMAIL`] in chat 2
/// - message 4: outgoing "From my Mac" in chat 3, `destination_caller_id`
///   [`OWNER_EMAIL`]
/// - message 5: outgoing "Still me" in chat 1 with `destination_caller_id`
///   NULL, as Apple writes on many outgoing rows; it is the owner's all the
///   same
/// - message 6: outgoing "From the car" in chat 1 with
///   `destination_caller_id` `tel:` + [`OWNER`], the prefixed spelling some
///   iPhone rows carry; it is the same owner address as message 2's
/// - message 7: incoming "New address" from [`FRIEND_PHONE_EMAIL`] in chat 5
/// - message 8: incoming "Just us now" from [`FRIEND_EMAIL`] in chat 4
/// - message 9: incoming "No sender" in chat 2 with `handle_id` 0
/// - message 10: outgoing "Note to self" in chat 6
/// - message 11: the same "Note to self" received in chat 6 from the
///   owner's handle, the second row Apple writes for a message to oneself
/// - message 12: incoming "Lost" from [`FRIEND_EMAIL`] in no chat
///
/// The photo message has no `text` and no `attributedBody`. A real row
/// carries the attachment as a placeholder range inside `attributedBody`;
/// without that blob the body parser builds no parts, and the reader falls
/// back to every attachment join row, which is what the tests rely on.
///
/// # Panics
///
/// Panics when the directory is not writable; a test has nothing better to do.
#[must_use]
pub fn write_chat_db(dir: &Path) -> PathBuf {
    let db_path = dir.join("chat.db");
    let photo = dir.join("photo.jpg");
    fs::write(&photo, PHOTO_BYTES).expect("write the photo beside the database");

    let db = Connection::open(&db_path).expect("create chat.db");
    db.execute_batch(&format!(
        r#"
        CREATE TABLE handle (ROWID INTEGER PRIMARY KEY, id TEXT, person_centric_id TEXT, service TEXT);
        CREATE TABLE chat (ROWID INTEGER PRIMARY KEY, chat_identifier TEXT, service_name TEXT, display_name TEXT, account_login TEXT, style INTEGER);
        CREATE TABLE chat_handle_join (chat_id INTEGER, handle_id INTEGER);
        CREATE TABLE chat_message_join (chat_id INTEGER, message_id INTEGER, message_date INTEGER);
        CREATE TABLE chat_recoverable_message_join (chat_id INTEGER, message_id INTEGER);
        CREATE TABLE attachment (ROWID INTEGER PRIMARY KEY, guid TEXT, filename TEXT, uti TEXT, mime_type TEXT, transfer_name TEXT, total_bytes INTEGER, is_sticker INTEGER, hide_attachment INTEGER, emoji_image_short_description TEXT);
        CREATE TABLE message_attachment_join (message_id INTEGER, attachment_id INTEGER);
        CREATE TABLE message (
            ROWID INTEGER PRIMARY KEY, guid TEXT, text TEXT, service TEXT, handle_id INTEGER,
            destination_caller_id TEXT, subject TEXT, date INTEGER, date_read INTEGER, date_delivered INTEGER,
            is_from_me INTEGER, is_read INTEGER, item_type INTEGER, other_handle INTEGER, share_status INTEGER,
            share_direction INTEGER, group_title TEXT, group_action_type INTEGER, associated_message_guid TEXT,
            associated_message_type INTEGER, balloon_bundle_id TEXT, expressive_send_style_id TEXT,
            thread_originator_guid TEXT, thread_originator_part TEXT, date_edited INTEGER,
            associated_message_emoji TEXT, attributedBody BLOB, payload_data BLOB, message_summary_info BLOB
        );

        INSERT INTO handle VALUES (1, '{friend_phone}', 'person-sam', 'iMessage');
        INSERT INTO handle VALUES (2, '{friend_email}', NULL, 'iMessage');
        INSERT INTO handle VALUES (3, '{friend_phone_email}', 'person-sam', 'iMessage');
        INSERT INTO handle VALUES (4, '{owner}', NULL, 'iMessage');
        INSERT INTO chat VALUES (1, '{friend_phone}', 'iMessage', NULL, 'P:{owner}', 45);
        INSERT INTO chat VALUES (2, '{group_chat}', 'iMessage', '{group_title}', 'P:{owner}', 43);
        INSERT INTO chat VALUES (3, '{friend_email}', 'iMessage', NULL, 'E:{owner_email}', 45);
        INSERT INTO chat VALUES (4, '{shrunk_group}', 'iMessage', NULL, 'P:{owner}', 43);
        INSERT INTO chat VALUES (5, '{friend_phone_email}', 'iMessage', NULL, 'P:{owner}', 45);
        INSERT INTO chat VALUES (6, '{owner}', 'iMessage', NULL, 'P:{owner}', 45);
        INSERT INTO chat_handle_join VALUES (1, 1), (2, 1), (2, 2), (2, 4), (3, 2), (4, 2), (6, 4);

        INSERT INTO message (ROWID, guid, text, service, handle_id, destination_caller_id, date, date_read, is_from_me, item_type, associated_message_type)
            VALUES (1, 'guid-1', NULL, 'iMessage', 1, '{owner}', {d1}, {d1_read}, 0, 0, 0);
        INSERT INTO message (ROWID, guid, text, service, handle_id, destination_caller_id, date, is_from_me, item_type, associated_message_type)
            VALUES (2, 'guid-2', 'Nice', 'iMessage', 0, '{owner}', {d2}, 1, 0, 0);
        INSERT INTO message (ROWID, guid, text, service, handle_id, destination_caller_id, date, is_from_me, item_type, associated_message_type)
            VALUES (3, 'guid-3', 'Saturday works', 'iMessage', 2, '{owner}', {d3}, 0, 0, 0);
        INSERT INTO message (ROWID, guid, text, service, handle_id, destination_caller_id, date, is_from_me, item_type, associated_message_type)
            VALUES (4, 'guid-4', 'From my Mac', 'iMessage', 0, '{owner_email}', {d4}, 1, 0, 0);
        INSERT INTO message (ROWID, guid, text, service, handle_id, destination_caller_id, date, is_from_me, item_type, associated_message_type)
            VALUES (5, 'guid-5', 'Still me', 'iMessage', 0, NULL, {d5}, 1, 0, 0);
        INSERT INTO message (ROWID, guid, text, service, handle_id, destination_caller_id, date, is_from_me, item_type, associated_message_type)
            VALUES (6, 'guid-6', 'From the car', 'iMessage', 0, 'tel:{owner}', {d6}, 1, 0, 0);
        INSERT INTO message (ROWID, guid, text, service, handle_id, destination_caller_id, date, is_from_me, item_type, associated_message_type)
            VALUES (7, 'guid-7', 'New address', 'iMessage', 3, '{owner}', {d7}, 0, 0, 0);
        INSERT INTO message (ROWID, guid, text, service, handle_id, destination_caller_id, date, is_from_me, item_type, associated_message_type)
            VALUES (8, 'guid-8', 'Just us now', 'iMessage', 2, '{owner}', {d8}, 0, 0, 0);
        INSERT INTO message (ROWID, guid, text, service, handle_id, destination_caller_id, date, is_from_me, item_type, associated_message_type)
            VALUES (9, 'guid-9', 'No sender', 'iMessage', 0, '{owner}', {d9}, 0, 0, 0);
        INSERT INTO message (ROWID, guid, text, service, handle_id, destination_caller_id, date, is_from_me, item_type, associated_message_type)
            VALUES (10, 'guid-10', 'Note to self', 'iMessage', 0, '{owner}', {d10}, 1, 0, 0);
        INSERT INTO message (ROWID, guid, text, service, handle_id, destination_caller_id, date, is_from_me, item_type, associated_message_type)
            VALUES (11, 'guid-11', 'Note to self', 'iMessage', 4, '{owner}', {d11}, 0, 0, 0);
        INSERT INTO message (ROWID, guid, text, service, handle_id, destination_caller_id, date, is_from_me, item_type, associated_message_type)
            VALUES (12, 'guid-12', 'Lost', 'iMessage', 2, '{owner}', {d12}, 0, 0, 0);
        INSERT INTO chat_message_join VALUES (1, 1, {d1}), (1, 2, {d2}), (2, 3, {d3}), (3, 4, {d4}), (1, 5, {d5}), (1, 6, {d6}),
            (5, 7, {d7}), (4, 8, {d8}), (2, 9, {d9}), (6, 10, {d10}), (6, 11, {d11});

        INSERT INTO attachment VALUES (1, 'att-1', '{photo}', 'public.jpeg', 'image/jpeg', 'photo.jpg', {photo_len}, 0, 0, NULL);
        INSERT INTO message_attachment_join VALUES (1, 1);
        "#,
        friend_phone = FRIEND_PHONE,
        friend_email = FRIEND_EMAIL,
        friend_phone_email = FRIEND_PHONE_EMAIL,
        shrunk_group = SHRUNK_GROUP_IDENTIFIER,
        owner = OWNER,
        owner_email = OWNER_EMAIL,
        group_chat = GROUP_CHAT_IDENTIFIER,
        group_title = GROUP_TITLE,
        d1 = apple_nanos(600_000_000),
        d1_read = apple_nanos(600_000_060),
        d2 = apple_nanos(600_000_060),
        d3 = apple_nanos(600_000_120),
        d4 = apple_nanos(600_000_180),
        d5 = apple_nanos(600_000_240),
        d6 = apple_nanos(600_000_300),
        d7 = apple_nanos(600_000_360),
        d8 = apple_nanos(600_000_420),
        d9 = apple_nanos(600_000_480),
        d10 = apple_nanos(600_000_540),
        d11 = apple_nanos(600_000_541),
        d12 = apple_nanos(600_000_600),
        photo = photo.display(),
        photo_len = PHOTO_BYTES.len(),
    ))
    .expect("fill chat.db");
    drop(db);
    db_path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_database_holds_what_the_docs_say() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = write_chat_db(dir.path());
        let db = Connection::open(&db_path).unwrap();
        let count = |sql: &str| db.query_row(sql, [], |row| row.get::<_, i64>(0)).unwrap();
        assert_eq!(count("SELECT count(*) FROM chat"), 6);
        assert_eq!(count("SELECT count(*) FROM handle"), 4);
        assert_eq!(count("SELECT count(*) FROM message"), 12);
        assert_eq!(
            count("SELECT count(*) FROM handle WHERE person_centric_id = 'person-sam'"),
            2,
            "one person's phone and email share a person_centric_id"
        );
        assert_eq!(
            count(
                "SELECT count(*) FROM chat_handle_join JOIN handle ON handle.ROWID = handle_id \
                 WHERE chat_id = 2 AND handle.id = '+15550000001'"
            ),
            1,
            "the group lists the owner's own handle among its members"
        );
        assert_eq!(
            count(
                "SELECT count(*) FROM chat WHERE ROWID NOT IN (SELECT chat_id FROM chat_handle_join)"
            ),
            1,
            "one chat has no handle rows"
        );
        assert_eq!(
            count(
                "SELECT count(*) FROM message WHERE ROWID NOT IN (SELECT message_id FROM chat_message_join)"
            ),
            1,
            "one message belongs to no chat"
        );
        assert_eq!(
            count(
                "SELECT count(*) FROM message WHERE is_from_me = 1 AND destination_caller_id IS NULL"
            ),
            1,
            "one outgoing row carries no caller id, as Apple writes"
        );
        assert_eq!(
            count("SELECT count(*) FROM message WHERE destination_caller_id LIKE 'tel:%'"),
            1,
            "one outgoing row carries the owner's phone with the tel: prefix"
        );
        assert_eq!(
            count("SELECT count(DISTINCT account_login) FROM chat"),
            2,
            "the owner's phone and email accounts"
        );
        assert_eq!(count("SELECT count(*) FROM attachment"), 1);
        assert_eq!(fs::read(dir.path().join("photo.jpg")).unwrap(), PHOTO_BYTES);
    }
}
