//! Errors while opening a Messages source or reading rows out of it.
//!
//! `RuntimeError` and its `FileNameError` message are adapted from
//! `imessage-exporter` by Christopher Sardegna
//! (<https://github.com/ReagentX/imessage-exporter>, file
//! `imessage-exporter/src/app/error.rs`), GPL-3.0-or-later. Copyright for the
//! adapted parts remains with the original author.

use std::{
    error::Error,
    fmt::{Display, Formatter, Result},
    io::Error as IoError,
    path::PathBuf,
};

use crabapple::error::BackupError;
use imessage_database::error::{message::MessageError, plist::PlistParseError, table::TableError};

/// User-facing copy when an encrypted backup has no password.
pub const ENCRYPTED_BACKUP_PASSWORD_REQUIRED: &str =
    "The backup is encrypted — fill Encryption password.";
/// User-facing copy when a password is set on an unencrypted backup.
pub const UNENCRYPTED_BACKUP_CLEAR_PASSWORD: &str =
    "This backup is not encrypted. Clear Encryption password.";
/// User-facing copy when the supplied iOS backup password is wrong.
pub const IOS_BACKUP_PASSWORD_INCORRECT: &str = "The iOS backup password was incorrect.";
/// User-facing copy when the folder is not an iPhone backup (or Messages is missing).
pub const NOT_AN_IPHONE_BACKUP: &str =
    "This folder is not an iPhone backup, or Messages is missing from it.";

/// Runtime failures while opening sources or writing mail archives.
#[derive(Debug)]
pub(crate) enum RuntimeError {
    InvalidOptions(String),
    DiskError(IoError),
    DatabaseError(TableError),
    MessageError(MessageError),
    BackupError(BackupError),
    FileNameError { path: PathBuf, reason: &'static str },
}

impl Display for RuntimeError {
    fn fmt(&self, fmt: &mut Formatter<'_>) -> Result {
        match self {
            RuntimeError::InvalidOptions(why) => write!(fmt, "{why}"),
            RuntimeError::DiskError(why) => write!(fmt, "{why}"),
            RuntimeError::DatabaseError(why) => write!(fmt, "{why}"),
            RuntimeError::MessageError(why) => write!(fmt, "{why}"),
            RuntimeError::BackupError(why) => write!(fmt, "{why}"),
            RuntimeError::FileNameError { path, reason } => {
                write!(fmt, "Invalid file name at {}: {reason}", path.display())
            }
        }
    }
}

impl Error for RuntimeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            RuntimeError::DiskError(why) => Some(why),
            RuntimeError::DatabaseError(why) => Some(why),
            RuntimeError::MessageError(why) => Some(why),
            RuntimeError::BackupError(why) => Some(why),
            RuntimeError::InvalidOptions(_) | RuntimeError::FileNameError { .. } => None,
        }
    }
}

impl From<TableError> for RuntimeError {
    fn from(err: TableError) -> Self {
        RuntimeError::DatabaseError(err)
    }
}

impl From<MessageError> for RuntimeError {
    fn from(err: MessageError) -> Self {
        RuntimeError::MessageError(err)
    }
}

impl From<PlistParseError> for RuntimeError {
    fn from(err: PlistParseError) -> Self {
        RuntimeError::MessageError(MessageError::from(err))
    }
}

impl From<BackupError> for RuntimeError {
    fn from(err: BackupError) -> Self {
        RuntimeError::BackupError(err)
    }
}

impl From<IoError> for RuntimeError {
    fn from(err: IoError) -> Self {
        RuntimeError::DiskError(err)
    }
}

impl From<rusqlite::Error> for RuntimeError {
    fn from(err: rusqlite::Error) -> Self {
        RuntimeError::DatabaseError(TableError::from(err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_options_display_is_the_sentence() {
        let err = RuntimeError::InvalidOptions(ENCRYPTED_BACKUP_PASSWORD_REQUIRED.to_string());
        assert_eq!(err.to_string(), ENCRYPTED_BACKUP_PASSWORD_REQUIRED);
        assert!(!err.to_string().contains("Invalid options"));
    }
}
