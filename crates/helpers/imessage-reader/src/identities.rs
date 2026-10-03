//! The addresses a backup's device sent from.
//!
//! This side opens the database, reads two columns, and puts each value
//! through the protocol's one address rule ([`bare_address`]), so an
//! identity leaves the helper spelled the way the same address leaves it on
//! a message. The app deduplicates. The export reads the same columns into
//! [`OwnerAddresses`] to keep the owner out of every roster.

use std::{collections::HashSet, path::Path};

use imessage_reader_protocol::{IdentitiesRequest, bare_address};
use rusqlite::Connection;

use crate::{data_source::DataSource, error::RuntimeError, options::ReaderOptions};

/// What the identities request found.
pub(crate) struct Identities {
    /// Whether the source is an encrypted backup, for the `source` event.
    pub encrypted: bool,
    /// The union of `chat.account_login` and `message.destination_caller_id`,
    /// each through [`bare_address`].
    pub values: Vec<String>,
}

/// Open the source and read the addresses its device sent from. An
/// encrypted backup's databases are decrypted into the request's scratch
/// folder.
///
/// Each per-column query falls back to an empty list when the table or
/// column is missing, so an unusual schema degrades to fewer signals rather
/// than an error.
///
/// # Errors
///
/// Returns an error when the source cannot be opened: missing database,
/// missing or wrong backup password, not an iPhone backup.
pub(crate) fn identities(request: IdentitiesRequest) -> Result<Identities, RuntimeError> {
    let options = ReaderOptions::from_source(request.source, request.scratch_dir);
    let data_source = DataSource::from(&options)?;
    Ok(Identities {
        encrypted: data_source.is_encrypted(),
        values: sent_from(data_source.db()),
    })
}

/// The union of `chat.account_login` and `message.destination_caller_id`,
/// each through [`bare_address`].
fn sent_from(db: &Connection) -> Vec<String> {
    let mut raw = distinct_texts(db, "SELECT DISTINCT account_login FROM chat");
    raw.extend(distinct_texts(
        db,
        "SELECT DISTINCT destination_caller_id FROM message",
    ));
    raw.iter().filter_map(|value| bare_address(value)).collect()
}

/// The addresses a backup names as the owner's, which tell the owner's
/// handles apart from everyone else's: the addresses the device sent from
/// (`chat.account_login`, `message.destination_caller_id`), and the phone
/// number an iPhone backup's `Info.plist` gives for the device.
///
/// Apple spells one phone several ways (`+15555550106`, `+1 (555)
/// 000-0001`), so phone numbers compare by their digits and email
/// addresses without regard to case.
#[derive(Debug, Default)]
pub(crate) struct OwnerAddresses {
    keys: HashSet<String>,
}

impl OwnerAddresses {
    /// Read the owner's addresses from the Messages database and, for an
    /// iPhone backup, from the `Info.plist` in `backup_root`.
    pub(crate) fn read(db: &Connection, backup_root: Option<&Path>) -> Self {
        let mut addresses = sent_from(db);
        addresses.extend(backup_root.and_then(info_plist_phone));
        Self {
            keys: addresses.iter().filter_map(|a| owner_key(a)).collect(),
        }
    }

    /// Whether `address` is one of the owner's.
    pub(crate) fn contains(&self, address: &str) -> bool {
        owner_key(address).is_some_and(|key| self.keys.contains(&key))
    }
}

/// The form two spellings of one address share: an email address in lower
/// case, a phone number as its digits.
fn owner_key(address: &str) -> Option<String> {
    let bare = bare_address(address)?;
    if bare.contains('@') {
        return Some(bare.to_lowercase());
    }
    let digits: String = bare.chars().filter(char::is_ascii_digit).collect();
    Some(if digits.is_empty() { bare } else { digits })
}

/// The phone number an iPhone backup's `Info.plist` gives for the device.
/// Apple does not encrypt that file, even in an encrypted backup.
fn info_plist_phone(backup_root: &Path) -> Option<String> {
    let info = plist::Value::from_file(backup_root.join("Info.plist")).ok()?;
    info.as_dictionary()?
        .get("Phone Number")?
        .as_string()
        .and_then(bare_address)
}

/// One column's distinct values; empty on any query error (older schemas).
fn distinct_texts(db: &Connection, sql: &str) -> Vec<String> {
    let Ok(mut stmt) = db.prepare(sql) else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([], |row| row.get::<_, Option<String>>(0)) else {
        return Vec::new();
    };
    rows.flatten().flatten().collect()
}

#[cfg(test)]
mod tests {
    use super::{OwnerAddresses, identities};
    use imessage_reader_protocol::{IdentitiesRequest, Platform, Source};
    use rusqlite::Connection;

    /// The identities request for the `chat.db` at `db_path`, with the
    /// folder beside it as the scratch folder.
    fn source(db_path: &std::path::Path) -> IdentitiesRequest {
        IdentitiesRequest {
            source: Source {
                db_path: db_path.to_path_buf(),
                platform: Platform::MacOs,
                backup_password: None,
            },
            scratch_dir: db_path.parent().unwrap().to_path_buf(),
        }
    }

    /// Both columns are read, and every value crosses the pipe bare: the
    /// `P:` and `E:` of `account_login` and the `tel:` some
    /// `destination_caller_id` rows carry are gone, a bare `E:` is dropped,
    /// and NULL adds nothing. The same phone spelled two ways is still two
    /// values here; the app deduplicates.
    #[test]
    fn identities_read_both_columns_and_strip_every_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("chat.db");
        let db = Connection::open(&db_path).unwrap();
        db.execute_batch(
            "CREATE TABLE chat (ROWID INTEGER PRIMARY KEY, account_login TEXT);
             CREATE TABLE message (ROWID INTEGER PRIMARY KEY, destination_caller_id TEXT);
             INSERT INTO chat (account_login) VALUES ('P:+15555550110'), ('E:');
             INSERT INTO message (destination_caller_id)
                 VALUES ('owner@example.com'), ('tel:+15555550110'), (NULL);",
        )
        .unwrap();
        drop(db);

        let found = identities(source(&db_path)).unwrap();
        assert!(!found.encrypted);
        let mut values = found.values;
        values.sort();
        assert_eq!(
            values,
            vec!["+15555550110", "+15555550110", "owner@example.com"]
        );
    }

    #[test]
    fn identities_survive_missing_columns() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("chat.db");
        let db = Connection::open(&db_path).unwrap();
        db.execute_batch("CREATE TABLE chat (ROWID INTEGER PRIMARY KEY);")
            .unwrap();
        drop(db);

        assert!(identities(source(&db_path)).unwrap().values.is_empty());
    }

    /// The owner is known by every spelling of each address: a phone with
    /// or without its punctuation and an email in any case. An iPhone
    /// backup's `Info.plist` adds the phone's own number.
    #[test]
    fn the_owner_is_every_address_the_backup_names() {
        let dir = tempfile::tempdir().unwrap();
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE chat (ROWID INTEGER PRIMARY KEY, account_login TEXT);
             CREATE TABLE message (ROWID INTEGER PRIMARY KEY, destination_caller_id TEXT);
             INSERT INTO chat (account_login) VALUES ('P:+15555550110'), ('E:Owner@Example.com');",
        )
        .unwrap();
        let mut info = plist::Dictionary::new();
        info.insert(
            "Phone Number".to_string(),
            plist::Value::String("+1 (555) 555-0113".to_string()),
        );
        plist::Value::Dictionary(info)
            .to_file_xml(dir.path().join("Info.plist"))
            .unwrap();

        let mac = OwnerAddresses::read(&db, None);
        assert!(mac.contains("+15555550110"));
        assert!(mac.contains("tel:+1 (555) 555-0110"));
        assert!(mac.contains("owner@example.com"));
        assert!(!mac.contains("+15555550113"), "no Info.plist on a Mac");
        assert!(!mac.contains("friend@example.com"));

        let iphone = OwnerAddresses::read(&db, Some(dir.path()));
        assert!(iphone.contains("+15555550113"));
        assert!(iphone.contains("+15555550110"));
    }
}
