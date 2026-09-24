//! The addresses a backup's device sent from.
//!
//! This side opens the database, reads two columns, and puts each value
//! through the protocol's one address rule ([`bare_address`]), so an
//! identity leaves the helper spelled the way the same address leaves it on
//! a message. The app deduplicates.

use imessage_reader_protocol::{Source, bare_address};
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

/// Open the source and read the addresses its device sent from.
///
/// Each per-column query falls back to an empty list when the table or
/// column is missing, so an unusual schema degrades to fewer signals rather
/// than an error.
///
/// # Errors
///
/// Returns an error when the source cannot be opened: missing database,
/// missing or wrong backup password, not an iPhone backup.
pub(crate) fn identities(source: Source) -> Result<Identities, RuntimeError> {
    let options = ReaderOptions::from_source(source);
    let data_source = DataSource::from(&options)?;
    let mut raw = distinct_texts(data_source.db(), "SELECT DISTINCT account_login FROM chat");
    raw.extend(distinct_texts(
        data_source.db(),
        "SELECT DISTINCT destination_caller_id FROM message",
    ));
    Ok(Identities {
        encrypted: data_source.is_encrypted(),
        values: raw.iter().filter_map(|value| bare_address(value)).collect(),
    })
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
    use super::identities;
    use imessage_reader_protocol::{Platform, Source};
    use rusqlite::Connection;

    fn source(db_path: &std::path::Path) -> Source {
        Source {
            db_path: db_path.to_path_buf(),
            platform: Platform::MacOs,
            backup_password: None,
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
             INSERT INTO chat (account_login) VALUES ('P:+15550001111'), ('E:');
             INSERT INTO message (destination_caller_id)
                 VALUES ('owner@example.com'), ('tel:+15550001111'), (NULL);",
        )
        .unwrap();
        drop(db);

        let found = identities(source(&db_path)).unwrap();
        assert!(!found.encrypted);
        let mut values = found.values;
        values.sort();
        assert_eq!(
            values,
            vec!["+15550001111", "+15550001111", "owner@example.com"]
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
}
