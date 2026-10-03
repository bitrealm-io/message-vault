//! What can stop an import, by kind.
//!
//! [`ImportFailure`] is a reason the person who sent the file can act on.
//! [`ImportError`] is what the import pipeline returns: one of those, or an
//! internal failure (I/O or the database) the sender cannot fix by changing
//! the file. The HTTP interface maps each kind to a status once, in
//! `server.rs`, and no handler picks a status from an `anyhow` error.

use message_ir::{UnsafeAttachmentPath, UnsupportedSchemaVersion};
use std::fmt;

/// A reason an import stopped that the sender can fix by changing the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportFailure {
    /// A line is not JSON at all. Only this kind is a request that cannot be
    /// read.
    NotJson { line: usize, detail: String },
    /// The conversation header's `schema_version` is not the one this server
    /// reads. Nothing is upgraded: the sender re-exports with current tools.
    SchemaVersion {
        refusal: UnsupportedSchemaVersion,
        line: usize,
    },
    /// A line is JSON and breaks a rule of message-ir: a header or message
    /// with the wrong fields, a message before any header, or a file with no
    /// header.
    Invalid { line: usize, detail: String },
    /// The batch has no bytes.
    Empty,
    /// An attachment path could leave the folder it is read from.
    UnsafeAttachmentPath(UnsafeAttachmentPath),
    /// An attachment's bytes do not hash to the SHA-256 the batch states for
    /// it, or the stated SHA-256 is not one.
    AttachmentMismatch { path: String, detail: String },
    /// Messages whose `guid` is empty. The guid index is what makes a
    /// retried batch store nothing twice, and every exporter writes a guid,
    /// so a message without one is refused rather than stored outside it.
    /// `lines` holds the first [`MISSING_GUID_LINES_NAMED`] such lines, in
    /// order, and `total` counts them all.
    MissingGuid { lines: Vec<usize>, total: usize },
}

/// How many lines without a guid a refusal names; the rest are counted.
pub const MISSING_GUID_LINES_NAMED: usize = 10;

/// The server's `import` command reads files, so the line is a line of the file.
impl fmt::Display for ImportFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.sentence("the file"))
    }
}

impl std::error::Error for ImportFailure {}

impl From<UnsafeAttachmentPath> for ImportFailure {
    fn from(refusal: UnsafeAttachmentPath) -> Self {
        Self::UnsafeAttachmentPath(refusal)
    }
}

impl ImportFailure {
    /// The line the failure is on, counted from 1 with blank lines included,
    /// when it is about a line. For messages without a guid, the first of
    /// them.
    #[must_use]
    pub fn line(&self) -> Option<usize> {
        match self {
            Self::NotJson { line, .. }
            | Self::SchemaVersion { line, .. }
            | Self::Invalid { line, .. } => Some(*line),
            Self::MissingGuid { lines, .. } => lines.first().copied(),
            Self::Empty | Self::UnsafeAttachmentPath(_) | Self::AttachmentMismatch { .. } => None,
        }
    }

    /// The sentence for a failure in one batch of an Import Run.
    ///
    /// A batch is the body of one request, which a client such as Upload
    /// packs from parts of one or more staged files, so its line is a line of
    /// the batch and not of any file the sender has. The client turns it into
    /// a file and line of its own.
    #[must_use]
    pub fn batch_sentence(&self) -> String {
        self.sentence("the batch")
    }

    /// The sentence, naming the line as a line of `whole`.
    fn sentence(&self, whole: &str) -> String {
        match self {
            Self::NotJson { line, detail } => {
                format!("Could not read line {line} of {whole}: {detail}.")
            }
            Self::SchemaVersion { refusal, line } => {
                format!("{refusal} (line {line} of {whole}).")
            }
            Self::Invalid { line, detail } => format!("Line {line} of {whole}: {detail}."),
            Self::Empty => {
                "The batch is empty: send at least one conversation header and its messages."
                    .to_string()
            }
            Self::UnsafeAttachmentPath(refusal) => refusal.to_string(),
            Self::AttachmentMismatch { path, detail } => format!(
                "The attachment {path} does not match the SHA-256 the batch states: {detail}."
            ),
            Self::MissingGuid { lines, total } => {
                let named: Vec<String> = lines.iter().map(ToString::to_string).collect();
                let rest = total.saturating_sub(lines.len());
                let which = match (named.as_slice(), rest) {
                    ([one], 0) => format!("The message on line {one} of {whole} has"),
                    ([init @ .., last], 0) => format!(
                        "The messages on lines {} and {last} of {whole} have",
                        init.join(", ")
                    ),
                    (named, rest) => format!(
                        "The messages on lines {} and {rest} more of {whole} have",
                        named.join(", ")
                    ),
                };
                format!("{which} no guid; every message needs one.")
            }
        }
    }

    /// The person-actionable failure inside `err`, if there is one.
    ///
    /// The import pipeline wraps errors in `anyhow` context on the way up;
    /// `downcast_ref` looks through every layer of context, so the parser can
    /// raise this type and the pipeline's entry point can find it without the
    /// layers in between knowing about it.
    pub fn in_error(err: &anyhow::Error) -> Option<&ImportFailure> {
        err.downcast_ref::<ImportFailure>()
    }
}

/// What the import pipeline returns when it stops.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    /// The sender can fix it by changing the file. `failure` is what the
    /// sender is told; `cause` keeps the whole chain, with the file the
    /// failure was in, for a command line or a log.
    #[error("{cause:#}")]
    Rejected {
        failure: ImportFailure,
        cause: anyhow::Error,
    },
    /// I/O, the database, or a bug: nothing the sender can change.
    #[error(transparent)]
    Internal(anyhow::Error),
}

/// The stages of the pipeline return `anyhow` and raise an [`ImportFailure`]
/// inside it; the pipeline's entry point sorts the two apart here, once.
impl From<anyhow::Error> for ImportError {
    fn from(cause: anyhow::Error) -> Self {
        match ImportFailure::in_error(&cause).cloned() {
            Some(failure) => Self::Rejected { failure, cause },
            None => Self::Internal(cause),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ImportError, ImportFailure, UnsupportedSchemaVersion};

    #[test]
    fn schema_version_names_both_versions_and_the_line() {
        let f = ImportFailure::SchemaVersion {
            refusal: UnsupportedSchemaVersion { found: 3 },
            line: 1,
        };
        assert_eq!(
            f.to_string(),
            "This file is schema version 3; Message Crate reads version 4 (line 1 of the file)."
        );
    }

    #[test]
    fn a_batch_failure_names_the_line_of_the_batch() {
        let f = ImportFailure::NotJson {
            line: 3,
            detail: "boom".into(),
        };
        assert_eq!(f.line(), Some(3));
        assert_eq!(
            f.batch_sentence(),
            "Could not read line 3 of the batch: boom."
        );
    }

    #[test]
    fn not_json_names_the_line_and_the_detail() {
        let f = ImportFailure::NotJson {
            line: 12,
            detail: "expected value at line 1 column 1".into(),
        };
        assert_eq!(
            f.to_string(),
            "Could not read line 12 of the file: expected value at line 1 column 1."
        );
    }

    #[test]
    fn missing_guid_names_one_line() {
        let f = ImportFailure::MissingGuid {
            lines: vec![3],
            total: 1,
        };
        assert_eq!(f.line(), Some(3));
        assert_eq!(
            f.to_string(),
            "The message on line 3 of the file has no guid; every message needs one."
        );
    }

    #[test]
    fn missing_guid_names_every_line_of_the_batch_up_to_the_limit() {
        let f = ImportFailure::MissingGuid {
            lines: vec![2, 5, 9],
            total: 3,
        };
        assert_eq!(f.line(), Some(2));
        assert_eq!(
            f.batch_sentence(),
            "The messages on lines 2, 5 and 9 of the batch have no guid; every message needs one."
        );
    }

    #[test]
    fn missing_guid_counts_the_lines_past_the_limit() {
        let f = ImportFailure::MissingGuid {
            lines: vec![2, 3],
            total: 42,
        };
        assert_eq!(
            f.to_string(),
            "The messages on lines 2, 3 and 40 more of the file have no guid; every message needs one."
        );
    }

    #[test]
    fn in_error_finds_the_failure_under_anyhow_context() {
        let root: anyhow::Error = ImportFailure::Invalid {
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
            ImportFailure::Invalid {
                line: 2,
                detail: "boom".into()
            }
        );
    }

    #[test]
    fn an_error_without_a_failure_is_internal() {
        let err = anyhow::anyhow!("disk full");
        assert!(ImportFailure::in_error(&err).is_none());
        assert!(matches!(ImportError::from(err), ImportError::Internal(_)));
    }
}
