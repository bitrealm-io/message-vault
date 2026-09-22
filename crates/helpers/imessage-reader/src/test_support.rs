//! A session over the `chat-db-fixture` database, for tests inside this crate.
//!
//! The process seam (`imessage-ir-exporter/tests/helper_process.rs`) proves
//! the binary works; the tests here open the same database in process so the
//! session, the emitter and the attachment code can be asserted on directly.

use std::path::Path;

use imessage_database::tables::{messages::Message, table::Table};
use imessage_reader_protocol::{ExportRequest, Platform, Source};
use rusqlite::Connection;
use tempfile::TempDir;

use crate::{body::apply_body, options::ReaderOptions, session::MailSession};

/// A `chat.db` from the fixture crate, in a folder that lives as long as
/// the value.
pub(crate) struct FixtureDb {
    pub dir: TempDir,
    pub db_path: std::path::PathBuf,
}

impl FixtureDb {
    pub(crate) fn write() -> Self {
        let dir = tempfile::tempdir().expect("a temp dir");
        let db_path = chat_db_fixture::write_chat_db(dir.path());
        Self { dir, db_path }
    }

    /// The export request the app would send for this database.
    pub(crate) fn export_request(&self) -> ExportRequest {
        ExportRequest {
            source: mac_source(&self.db_path),
            attachment_root: None,
            contacts_path: None,
            use_caller_id: true,
            scratch_dir: Some(self.dir.path().to_path_buf()),
        }
    }

    pub(crate) fn options(&self) -> ReaderOptions {
        ReaderOptions::from_export(self.export_request())
    }

    pub(crate) fn session(&self) -> MailSession {
        MailSession::new(self.options()).expect("a session over the fixture")
    }

    /// A session that also has a contacts file naming the fixture's people
    /// (see [`fill_macos_address_book`]).
    pub(crate) fn session_with_contacts(&self) -> MailSession {
        let contacts_path = self.dir.path().join("AddressBook-v22.abcddb");
        fill_macos_address_book(&Connection::open(&contacts_path).expect("create the book"));
        let options = ReaderOptions::from_export(ExportRequest {
            contacts_path: Some(contacts_path),
            ..self.export_request()
        });
        MailSession::new(options).expect("a session over the fixture with contacts")
    }

    /// Every message row in the fixture, with its body applied, in rowid
    /// order.
    pub(crate) fn messages(session: &MailSession) -> Vec<Message> {
        let db = session.data_source.db();
        let mut statement =
            Message::stream_rows(db, &session.options.query_context).expect("the message query");
        let mut out: Vec<Message> = Message::rows(&mut statement, [])
            .expect("message rows")
            .map(|row| row.expect("a message row"))
            .collect();
        // The stream repeats a row once per attachment join; keep the first.
        out.sort_by_key(|message| message.rowid);
        out.dedup_by_key(|message| message.rowid);
        for message in &mut out {
            apply_body(message, db);
        }
        out
    }
}

/// A Mac source with no backup password.
pub(crate) fn mac_source(db_path: &Path) -> Source {
    Source {
        db_path: db_path.to_path_buf(),
        platform: Platform::MacOs,
        backup_password: None,
    }
}

/// The `ZABCD*` tables a macOS `AddressBook-v22.abcddb` holds, as far as the
/// reader queries them, naming the fixture's people: "Sam Example" at
/// [`chat_db_fixture::FRIEND_PHONE`] and `sam@example.com`, "Robin" at
/// [`chat_db_fixture::FRIEND_EMAIL`], and a nameless row at `+15559990000`.
/// Nothing here comes from a real address book.
pub(crate) fn fill_macos_address_book(conn: &Connection) {
    conn.execute_batch(
        "CREATE TABLE ZABCDRECORD (Z_PK INTEGER PRIMARY KEY, ZFIRSTNAME TEXT, ZLASTNAME TEXT);
         CREATE TABLE ZABCDPHONENUMBER (Z_PK INTEGER PRIMARY KEY, ZOWNER INTEGER, ZFULLNUMBER TEXT);
         CREATE TABLE ZABCDEMAILADDRESS (Z_PK INTEGER PRIMARY KEY, ZOWNER INTEGER, ZADDRESSNORMALIZED TEXT);
         INSERT INTO ZABCDRECORD VALUES (1, 'Sam', 'Example');
         INSERT INTO ZABCDRECORD VALUES (2, 'Robin', NULL);
         INSERT INTO ZABCDRECORD VALUES (3, NULL, NULL);
         INSERT INTO ZABCDPHONENUMBER VALUES (1, 1, '+1 (555) 000-0002');
         INSERT INTO ZABCDPHONENUMBER VALUES (2, 3, '+1 (555) 999-0000');
         INSERT INTO ZABCDEMAILADDRESS VALUES (1, 1, '<Sam@Example.com>');
         INSERT INTO ZABCDEMAILADDRESS VALUES (2, 2, 'friend@example.com');",
    )
    .expect("fill the macOS address book");
}

/// The full-text table an iOS backup's `AddressBook.sqlitedb` holds, with the
/// space-separated phone and email columns the reader splits, naming the same
/// people as [`fill_macos_address_book`].
pub(crate) fn fill_ios_address_book(conn: &Connection) {
    conn.execute_batch(
        "CREATE TABLE ABPersonFullTextSearch_content (c0First TEXT, c1Last TEXT, c16Phone TEXT, c17Email TEXT);
         INSERT INTO ABPersonFullTextSearch_content VALUES ('Sam', 'Example', '+15550000002 15550000002 5550000002', 'Sam@Example.com sam@work.example');
         INSERT INTO ABPersonFullTextSearch_content VALUES ('Robin', NULL, NULL, 'friend@example.com');
         INSERT INTO ABPersonFullTextSearch_content VALUES (NULL, NULL, '+15559990000', NULL);",
    )
    .expect("fill the iOS address book");
}
