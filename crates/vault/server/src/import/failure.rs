//! The reasons an import stops that the person who sent the file can act on.
//!
//! Everything else an import returns is an internal failure: the person
//! cannot fix it by changing the file, so the HTTP interface reports it as a
//! 500 and keeps the cause on stderr.

use message_ir::UnsupportedSchemaVersion;
use std::fmt;

/// A reason an import stopped that the sender can fix by changing the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportFailure {
    /// The conversation header's `schema_version` is not the one this vault
    /// reads. Nothing is upgraded: the sender re-exports with current tools.
    SchemaVersion {
        refusal: UnsupportedSchemaVersion,
        line: usize,
    },
    /// A line is not the message-ir JSON the vault expects: not JSON at all,
    /// a header or message with the wrong fields, or a message before any
    /// header.
    Parse { line: usize, detail: String },
}

impl fmt::Display for ImportFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SchemaVersion { refusal, line } => write!(f, "{refusal} (line {line})."),
            Self::Parse { line, detail } => {
                write!(f, "Could not read line {line} of the file: {detail}.")
            }
        }
    }
}

impl std::error::Error for ImportFailure {}

impl ImportFailure {
    /// The person-actionable failure inside `err`, if there is one.
    ///
    /// The import pipeline wraps errors in `anyhow` context on the way up;
    /// `downcast_ref` looks through every layer of context, so the parser can
    /// raise this type and the HTTP handler can find it without the layers in
    /// between knowing about it.
    pub fn in_error(err: &anyhow::Error) -> Option<&ImportFailure> {
        err.downcast_ref::<ImportFailure>()
    }
}

#[cfg(test)]
mod tests {
    use super::{ImportFailure, UnsupportedSchemaVersion};

    #[test]
    fn schema_version_names_both_versions_and_the_line() {
        let f = ImportFailure::SchemaVersion {
            refusal: UnsupportedSchemaVersion { found: 3 },
            line: 1,
        };
        assert_eq!(
            f.to_string(),
            "This file is schema version 3; the vault reads version 4 (line 1)."
        );
    }

    #[test]
    fn parse_names_the_line_and_the_detail() {
        let f = ImportFailure::Parse {
            line: 12,
            detail: "expected value at line 1 column 1".into(),
        };
        assert_eq!(
            f.to_string(),
            "Could not read line 12 of the file: expected value at line 1 column 1."
        );
    }

    #[test]
    fn in_error_finds_the_failure_under_anyhow_context() {
        let root: anyhow::Error = ImportFailure::Parse {
            line: 2,
            detail: "boom".into(),
        }
        .into();
        let wrapped = root
            .context("failed to parse message-ir JSONL in /tmp/x.jsonl")
            .context("import failed");
        let found = ImportFailure::in_error(&wrapped).expect("failure survives context");
        assert_eq!(
            *found,
            ImportFailure::Parse {
                line: 2,
                detail: "boom".into()
            }
        );
    }

    #[test]
    fn in_error_is_none_for_other_errors() {
        let err = anyhow::anyhow!("disk full");
        assert!(ImportFailure::in_error(&err).is_none());
    }
}
