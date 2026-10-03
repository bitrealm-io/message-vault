//! The identities read through the real `imessage-reader` process.
//!
//! The test writes the small `chat.db` from `chat-db-fixture`, builds the
//! program ([`build_imessage_reader`]), and asks it which addresses the
//! device sent from.

use std::fs;

use chat_db_fixture::{OWNER, OWNER_EMAIL, write_chat_db};
use ios_backup::reader_build::build_imessage_reader;

#[test]
fn identities_come_back_cleaned_from_the_helper_process() {
    build_imessage_reader();
    let dir = tempfile::tempdir().unwrap();
    let db_path = write_chat_db(dir.path());
    let scratch_root = tempfile::tempdir().unwrap();

    let mut identities =
        ios_backup::backup_identities(&db_path, false, None, scratch_root.path()).unwrap();
    let left: Vec<_> = fs::read_dir(scratch_root.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .filter(|name| name != ".lock")
        .collect();
    assert!(
        left.is_empty(),
        "the request's scratch folder stays: {left:?}"
    );
    identities.sort();
    assert_eq!(
        identities,
        vec![OWNER.to_string(), OWNER_EMAIL.to_string()],
        "the phone from `P:`, bare and `tel:`-prefixed caller ids, and the \
         email from `E:`, each once, and the NULL caller id on one outgoing \
         row adds nothing"
    );
}
