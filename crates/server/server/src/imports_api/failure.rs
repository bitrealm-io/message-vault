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
    /// The conversation header's `schema_version` is not the one this server
    /// reads. Nothing is upgraded: the sender re-exports with current tools.
    SchemaVersion {
        refusal: UnsupportedSchemaVersion,
        line: usize,
    },
    /// A line is not the message-ir JSON the server expects: not JSON at all,
    /// a header or message with the wrong fields, or a message before any
    /// header.
    Parse { line: usize, detail: String },
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

impl ImportFailure {
    /// The line the failure is on, counted from 1 with blank lines included.
    /// For messages without a guid, the first of them.
    #[must_use]
    pub fn line(&self) -> usize {
        match self {
            Self::SchemaVersion { line, .. } | Self::Parse { line, .. } => *line,
            Self::MissingGuid { lines, .. } => lines.first().copied().unwrap_or_default(),
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
            Self::SchemaVersion { refusal, line } => {
                format!("{refusal} (line {line} of {whole}).")
            }
            Self::Parse { line, detail } => {
                format!("Could not read line {line} of {whole}: {detail}.")
            }
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
            "This file is schema version 3; Message Crate reads version 4 (line 1 of the file)."
        );
    }

    #[test]
    fn a_batch_names_the_line_as_a_line_of_the_batch() {
        let f = ImportFailure::Parse {
            line: 3,
            detail: "boom".into(),
        };
        assert_eq!(f.line(), 3);
        assert_eq!(
            f.batch_sentence(),
            "Could not read line 3 of the batch: boom."
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
    fn missing_guid_names_one_line() {
        let f = ImportFailure::MissingGuid {
            lines: vec![3],
            total: 1,
        };
        assert_eq!(f.line(), 3);
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
        assert_eq!(f.line(), 2);
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
