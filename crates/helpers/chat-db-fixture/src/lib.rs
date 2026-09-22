//! A small Apple Messages `chat.db` for tests.
//!
//! Two crates read it: `imessage-reader`, whose own tests open it in process
//! to reach the session, the emitter and the attachment code without building
//! the binary, and `imessage-ir-exporter`, whose process-seam test spawns the
//! built binary against it. The file is written with rusqlite alone, so the
//! exporter's test binary links no GPL code (`docs/agents/licences.md`).
//!
//! Nothing here comes from a real backup. Every row is made up.

use std::{
    fs,
    path::{Path, PathBuf},
};

use rusqlite::Connection;

/// The owner's number, as the `chat.account_login` and
/// `message.destination_caller_id` columns carry it.
pub const OWNER: &str = "+15550000001";

/// The first contact's number: the other side of the direct chat and a member
/// of the group chat.
pub const FRIEND_PHONE: &str = "+15550000002";

/// The second contact's address: a member of the group chat only.
pub const FRIEND_EMAIL: &str = "friend@example.com";

/// The group chat's identifier.
pub const GROUP_CHAT_IDENTIFIER: &str = "chat100";

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
/// The database holds two people, one direct chat (rowid 1) and one named
/// group chat (rowid 2), three messages, and one attachment whose file
/// (`photo.jpg`) is written beside the database:
///
/// - message 1: incoming from [`FRIEND_PHONE`] in the direct chat, no text,
///   carrying the photo, read by the owner one minute later (`date_read` set)
/// - message 2: outgoing "Nice" in the direct chat, never read (`date_read`
///   NULL)
/// - message 3: incoming "Saturday works" from [`FRIEND_EMAIL`] in the group
///   chat
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
        CREATE TABLE chat (ROWID INTEGER PRIMARY KEY, chat_identifier TEXT, service_name TEXT, display_name TEXT, account_login TEXT);
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

        INSERT INTO handle VALUES (1, '{friend_phone}', NULL, 'iMessage');
        INSERT INTO handle VALUES (2, '{friend_email}', NULL, 'iMessage');
        INSERT INTO chat VALUES (1, '{friend_phone}', 'iMessage', NULL, 'P:{owner}');
        INSERT INTO chat VALUES (2, '{group_chat}', 'iMessage', '{group_title}', 'P:{owner}');
        INSERT INTO chat_handle_join VALUES (1, 1), (2, 1), (2, 2);

        INSERT INTO message (ROWID, guid, text, service, handle_id, destination_caller_id, date, date_read, is_from_me, item_type, associated_message_type)
            VALUES (1, 'guid-1', NULL, 'iMessage', 1, '{owner}', {d1}, {d1_read}, 0, 0, 0);
        INSERT INTO message (ROWID, guid, text, service, handle_id, destination_caller_id, date, is_from_me, item_type, associated_message_type)
            VALUES (2, 'guid-2', 'Nice', 'iMessage', 0, '{owner}', {d2}, 1, 0, 0);
        INSERT INTO message (ROWID, guid, text, service, handle_id, destination_caller_id, date, is_from_me, item_type, associated_message_type)
            VALUES (3, 'guid-3', 'Saturday works', 'iMessage', 2, '{owner}', {d3}, 0, 0, 0);
        INSERT INTO chat_message_join VALUES (1, 1, {d1}), (1, 2, {d2}), (2, 3, {d3});

        INSERT INTO attachment VALUES (1, 'att-1', '{photo}', 'public.jpeg', 'image/jpeg', 'photo.jpg', {photo_len}, 0, 0, NULL);
        INSERT INTO message_attachment_join VALUES (1, 1);
        "#,
        friend_phone = FRIEND_PHONE,
        friend_email = FRIEND_EMAIL,
        owner = OWNER,
        group_chat = GROUP_CHAT_IDENTIFIER,
        group_title = GROUP_TITLE,
        d1 = apple_nanos(600_000_000),
        d1_read = apple_nanos(600_000_060),
        d2 = apple_nanos(600_000_060),
        d3 = apple_nanos(600_000_120),
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
        assert_eq!(count("SELECT count(*) FROM chat"), 2);
        assert_eq!(count("SELECT count(*) FROM handle"), 2);
        assert_eq!(count("SELECT count(*) FROM message"), 3);
        assert_eq!(count("SELECT count(*) FROM attachment"), 1);
        assert_eq!(fs::read(dir.path().join("photo.jpg")).unwrap(), PHOTO_BYTES);
    }
}
