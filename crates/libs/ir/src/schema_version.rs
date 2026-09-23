//! The one check that refuses a file from another schema version.
//!
//! Every reader of a [`ConversationDocument`](crate::ConversationDocument) or
//! its JSON Lines header — the format reader, the push client, the vault's
//! import — refuses a version other than [`SCHEMA_VERSION`] with the same
//! words, and refuses it before parsing the rest of the file: a version-3
//! file is not expected to match the version-4 field shapes, and the person
//! should read "schema version 3", not whichever field failed first.

use crate::SCHEMA_VERSION;
use serde::Deserialize;
use std::fmt;

/// A document or header whose `schema_version` is not [`SCHEMA_VERSION`].
/// Nothing is upgraded: the file is re-exported with current tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedSchemaVersion {
    /// The version the file carries.
    pub found: u32,
}

impl fmt::Display for UnsupportedSchemaVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "This file is schema version {}; the vault reads version {SCHEMA_VERSION}",
            self.found
        )
    }
}

impl std::error::Error for UnsupportedSchemaVersion {}

/// Refuse `found` unless it is [`SCHEMA_VERSION`].
///
/// # Errors
///
/// Returns [`UnsupportedSchemaVersion`] for any other version.
pub fn check_schema_version(found: u32) -> Result<(), UnsupportedSchemaVersion> {
    if found == SCHEMA_VERSION {
        Ok(())
    } else {
        Err(UnsupportedSchemaVersion { found })
    }
}

/// Refuse a raw JSON document or header line whose `schema_version` is not
/// [`SCHEMA_VERSION`], reading nothing but that field.
///
/// Text that is not a JSON object, or has no numeric `schema_version`, passes:
/// the full parse that follows reports what is wrong with it.
///
/// # Errors
///
/// Returns [`UnsupportedSchemaVersion`] when the field is present and is
/// another version.
pub fn check_schema_version_in_json(json: &str) -> Result<(), UnsupportedSchemaVersion> {
    #[derive(Deserialize)]
    struct Peek {
        schema_version: u32,
    }
    match serde_json::from_str::<Peek>(json) {
        Ok(peek) => check_schema_version(peek.schema_version),
        Err(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_version_found_and_the_version_read() {
        assert_eq!(
            check_schema_version(3).unwrap_err().to_string(),
            "This file is schema version 3; the vault reads version 4"
        );
        assert_eq!(check_schema_version(SCHEMA_VERSION), Ok(()));
    }

    #[test]
    fn peeks_at_the_version_before_anything_else() {
        assert_eq!(
            check_schema_version_in_json(r#"{"schema_version":3,"export":{}}"#),
            Err(UnsupportedSchemaVersion { found: 3 })
        );
        assert_eq!(
            check_schema_version_in_json(r#"{"schema_version":4,"export":{}}"#),
            Ok(())
        );
        assert_eq!(check_schema_version_in_json(r#"{"export":{}}"#), Ok(()));
        assert_eq!(check_schema_version_in_json("not json"), Ok(()));
    }
}
