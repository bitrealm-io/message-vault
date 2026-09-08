//! What a query that cannot be compiled answers with.

use std::ops::Range;

use crate::server::ApiError;

/// Why a query was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryErrorKind {
    /// A `word:` the language does not have.
    UnknownWord,
    /// A word the language has, but not on the requested list.
    WrongList,
    /// A value the word does not understand.
    BadValue,
    /// A `word:` with nothing after the colon.
    EmptyValue,
    /// Parentheses or quotes that do not close, or an operator with no operand.
    Unbalanced,
    /// The query string is longer than the limit.
    TooLong,
    /// Too many terms or too much nesting.
    TooComplex,
}

/// A refused query: a user-facing sentence and where in the input it points.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct QueryError {
    /// Why the query was refused.
    pub kind: QueryErrorKind,
    /// The problem's `detail`. Names the word and the list; never names a
    /// spelling the language does not have.
    pub message: String,
    /// Byte range in the input the message is about.
    pub span: Range<usize>,
    /// The word involved, when there is one.
    pub field: Option<&'static str>,
    /// A word on this list within a small edit distance, when there is one.
    pub did_you_mean: Option<&'static str>,
}

impl QueryError {
    /// A query error of `kind` at `span`, with a message written for the person typing.
    pub(crate) fn new(
        kind: QueryErrorKind,
        span: Range<usize>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            message: message.into(),
            span,
            field: None,
            did_you_mean: None,
        }
    }
}

impl From<QueryError> for ApiError {
    fn from(e: QueryError) -> Self {
        ApiError::SearchQueryInvalid {
            detail: e.message,
            word: e.field,
            did_you_mean: e.did_you_mean,
        }
    }
}
