//! The raw addresses a backup's device sent from.
//!
//! The app cleans and deduplicates these; this side only knows how to open
//! the database and read two columns out of it.

use imessage_reader_protocol::Source;
use rusqlite::Connection;

use crate::{data_source::DataSource, error::RuntimeError, options::ReaderOptions};

/// What the identities request found.
pub(crate) struct RawIdentities {
    /// Whether the source is an encrypted backup, for the `source` event.
    pub encrypted: bool,
    /// The union of `chat.account_login` and `message.destination_caller_id`,
    /// as stored.
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
pub(crate) fn raw_identities(source: Source) -> Result<RawIdentities, RuntimeError> {
    let options = ReaderOptions::from_source(source);
    let data_source = DataSource::from(&options)?;
    let mut values = distinct_texts(data_source.db(), "SELECT DISTINCT account_login FROM chat");
    values.extend(distinct_texts(
        data_source.db(),
        "SELECT DISTINCT destination_caller_id FROM message",
    ));
    Ok(RawIdentities {
        encrypted: data_source.is_encrypted(),
        values,
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
    use super::raw_identities;
    use imessage_reader_protocol::{Platform, Source};
    use rusqlite::Connection;

    fn source(db_path: &std::path::Path) -> Source {
        Source {
            db_path: db_path.to_path_buf(),
            platform: Platform::MacOs,
            backup_password: None,
        }
    }

    #[test]
    fn raw_identities_reads_both_columns_uncleaned() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("chat.db");
        let db = Connection::open(&db_path).unwrap();
        db.execute_batch(
            "CREATE TABLE chat (ROWID INTEGER PRIMARY KEY, account_login TEXT);
             CREATE TABLE message (ROWID INTEGER PRIMARY KEY, destination_caller_id TEXT);
             INSERT INTO chat (account_login) VALUES ('P:+15550001111'), ('E:');
             INSERT INTO message (destination_caller_id) VALUES ('owner@example.com'), (NULL);",
        )
        .unwrap();
        drop(db);

        let found = raw_identities(source(&db_path)).unwrap();
        assert!(!found.encrypted);
        let mut values = found.values;
        values.sort();
        assert_eq!(values, vec!["E:", "P:+15550001111", "owner@example.com"]);
    }

    #[test]
    fn raw_identities_survives_missing_columns() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("chat.db");
        let db = Connection::open(&db_path).unwrap();
        db.execute_batch("CREATE TABLE chat (ROWID INTEGER PRIMARY KEY);")
            .unwrap();
        drop(db);

        assert!(raw_identities(source(&db_path)).unwrap().values.is_empty());
    }
}
