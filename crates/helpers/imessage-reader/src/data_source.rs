//! Open macOS `chat.db` or an iOS backup (encrypted backups decrypt to a temp sms.db).

use std::{
    fs::remove_file,
    path::{Path, PathBuf},
};

use crabapple::Backup;
use imessage_database::{tables::table::get_connection, util::platform::Platform};
use rusqlite::Connection;

use crate::{
    backup::{decrypt_backup, get_decrypted_contacts_database, get_decrypted_message_database},
    contacts::{ContactsIndex, DEFAULT_PATH_IOS},
    error::RuntimeError,
    log::emit_log,
    options::ReaderOptions,
};

struct TempDatabase {
    path: PathBuf,
}

impl TempDatabase {
    /// Remember a temp database path so [`Drop`] can delete it.
    fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Path of the temp database file.
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        if let Err(why) = remove_file(&self.path) {
            emit_log(format!(
                "warning: failed to remove temporary messages database at {}: {why}",
                self.path.display(),
            ));
        }
    }
}

pub(crate) struct DataSource {
    messages_connection: Option<Connection>,
    pub contacts_index: ContactsIndex,
    pub backup: Option<Backup>,
    temp_messages_db: Option<TempDatabase>,
}

impl DataSource {
    /// Open the Messages database (and contacts index) for `options`.
    ///
    /// # Errors
    ///
    /// Returns an error when the backup cannot be decrypted or `chat.db` cannot
    /// be opened.
    pub fn from(options: &ReaderOptions) -> Result<Self, RuntimeError> {
        match options.platform {
            Platform::macOS => {
                let messages_path = options.get_db_path();
                let contacts_index =
                    Self::get_contacts_index(options.contacts_path.as_deref()).unwrap_or_default();

                Ok(Self {
                    messages_connection: Some(get_connection(&messages_path)?),
                    contacts_index,
                    backup: None,
                    temp_messages_db: None,
                })
            }
            Platform::iOS => match decrypt_backup(options)? {
                Some(backup) => {
                    let messages_db =
                        TempDatabase::new(get_decrypted_message_database(&backup, options)?);
                    let contacts_path = match get_decrypted_contacts_database(&backup, options) {
                        Ok(path) => Some(path),
                        Err(e) => {
                            emit_log(format!(
                                "Could not decrypt Contacts database from iOS backup: {e:#}; \
                                     continuing without contacts"
                            ));
                            None
                        }
                    };

                    emit_log(format!(
                        "Decrypted iOS backup: {} (version {})\n",
                        backup.lockdown().device_name,
                        backup.lockdown().product_version,
                    ));

                    let contacts_index =
                        Self::get_contacts_index(contacts_path.as_deref()).unwrap_or_default();

                    if let Some(ref cp) = contacts_path
                        && let Err(e) = remove_file(cp)
                    {
                        emit_log(format!(
                            "warning: failed to remove temporary contacts database at {}: {e}",
                            cp.display()
                        ));
                    }

                    let messages_connection = get_connection(messages_db.path())?;
                    Ok(Self {
                        messages_connection: Some(messages_connection),
                        contacts_index,
                        backup: Some(backup),
                        temp_messages_db: Some(messages_db),
                    })
                }
                None => {
                    let messages_path = options.get_db_path();
                    let contacts_index =
                        Self::get_contacts_index(Some(&options.db_path.join(DEFAULT_PATH_IOS)))
                            .unwrap_or_default();

                    Ok(Self {
                        messages_connection: Some(get_connection(&messages_path)?),
                        contacts_index,
                        backup: None,
                        temp_messages_db: None,
                    })
                }
            },
        }
    }

    /// Build a contacts index, or `None` (with a log line) when that fails.
    fn get_contacts_index(path: Option<&Path>) -> Option<ContactsIndex> {
        match ContactsIndex::build(path) {
            Ok(index) => Some(index),
            Err(e) => {
                emit_log(format!(
                    "Unable to build contacts index: {e}\nContinuing without contact names..."
                ));
                None
            }
        }
    }

    /// Whether attachment paths need this process to decrypt them.
    pub fn is_encrypted(&self) -> bool {
        self.backup.as_ref().is_some_and(|b| b.is_encrypted())
    }

    /// Open SQLite connection to the Messages database.
    pub fn db(&self) -> &Connection {
        match self.messages_connection.as_ref() {
            Some(db) => db,
            None => panic!("Database connection is closed!"),
        }
    }
}

impl Drop for DataSource {
    fn drop(&mut self) {
        if let Some(conn) = self.messages_connection.take() {
            conn.close().ok();
        }
        drop(self.temp_messages_db.take());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{FixtureDb, mac_source};
    use imessage_reader_protocol::ExportRequest;

    /// A Mac `chat.db` opens in place: no backup, nothing decrypted, nothing
    /// to clean up, and the connection answers queries.
    #[test]
    fn a_mac_database_opens_in_place() {
        let fixture = FixtureDb::write();
        let source = DataSource::from(&fixture.options()).unwrap();
        assert!(!source.is_encrypted());
        assert!(source.backup.is_none());
        assert!(source.temp_messages_db.is_none());
        let chats: i64 = source
            .db()
            .query_row("SELECT count(*) FROM chat", [], |row| row.get(0))
            .unwrap();
        assert_eq!(chats, 3);
    }

    /// A contacts file that cannot be read is logged and the run goes on
    /// without names, rather than failing the whole export.
    #[test]
    fn an_unreadable_contacts_file_leaves_the_index_empty() {
        let fixture = FixtureDb::write();
        let not_a_db = fixture.dir.path().join("contacts.db");
        std::fs::write(&not_a_db, b"not sqlite").unwrap();
        let options = ReaderOptions::from_export(ExportRequest {
            contacts_path: Some(not_a_db),
            ..fixture.export_request()
        });
        let source = DataSource::from(&options).unwrap();
        assert_eq!(source.contacts_index.lookup("+15550000002"), None);
        assert!(
            DataSource::get_contacts_index(Some(&fixture.dir.path().join("missing.db"))).is_none()
        );
    }

    /// A database that is not there is the error the app shows, not a panic.
    #[test]
    fn a_missing_database_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let options = ReaderOptions::from_source(mac_source(&dir.path().join("chat.db")));
        assert!(DataSource::from(&options).is_err());
    }

    /// A temp database is deleted when its handle drops, and a path that is
    /// already gone only logs.
    #[test]
    fn a_temp_database_is_removed_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sms.db");
        std::fs::write(&path, b"bytes").unwrap();
        let temp = TempDatabase::new(path.clone());
        assert_eq!(temp.path(), path);
        drop(temp);
        assert!(!path.exists());
        drop(TempDatabase::new(path));
    }
}
