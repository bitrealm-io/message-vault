//! The search language: one parser and one SQL compiler for the Contacts,
//! Conversations, and Messages lists. See
//! `docs/adr/0004-one-search-language-compiled-in-one-module.md`.

pub(crate) mod bridge;
pub(crate) mod emit;
pub mod error;
pub(crate) mod fields;
pub(crate) mod fts;
pub(crate) mod lex;
pub(crate) mod parse;
#[cfg(test)]
pub(crate) mod tests;
pub(crate) mod value;

pub use error::QueryError;
pub use fields::FieldDoc;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::db::engine::DbEngine;
use crate::db::sql::SqlParam;

/// Which list a query is compiled for. Each list accepts its own subset of
/// the words, and every filter is expressed against that list's base row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ListKind {
    /// One row per contact; base alias `ct`.
    Contacts,
    /// One row per conversation; base alias `c`.
    Conversations,
    /// One row per message; base alias `m`.
    Messages,
}

impl ListKind {
    /// The list's name as a person reads it in an error message.
    pub fn label(self) -> &'static str {
        match self {
            Self::Contacts => "Contacts",
            Self::Conversations => "Conversations",
            Self::Messages => "Messages",
        }
    }

    /// The one table alias a compiled fragment may mention. The tests run
    /// every fragment against a bare `FROM <table> <alias>` to prove it.
    #[cfg(test)]
    pub(crate) fn base_alias(self) -> &'static str {
        match self {
            Self::Contacts => "ct",
            Self::Conversations => "c",
            Self::Messages => "m",
        }
    }
}

/// The words one list accepts, for the web's suggestions and the docs page.
pub fn describe(list: ListKind) -> Vec<FieldDoc> {
    fields::describe(list)
}

/// Everything `compile` needs that is not in the query string.
#[derive(Debug, Clone, Copy)]
pub struct CompileRequest<'a> {
    /// Which list the query is for.
    pub list: ListKind,
    /// The query string as typed.
    pub query: &'a str,
    /// The signed-in account; every fragment is scoped to it.
    pub account_id: i64,
    /// Which engine's SQL to write.
    pub engine: DbEngine,
    /// Relative dates resolve against this day. Never read from the clock here.
    pub today: NaiveDate,
    /// The account's time zone: the anchor that turns a stored instant into a
    /// day or a year for `date:`, `first-message:` and `last-message:`.
    pub zone: chrono_tz::Tz,
}

/// Today's date in `zone`, for the relative date forms (`today`, `7d`, `1y`).
pub fn today_in(zone: chrono_tz::Tz) -> NaiveDate {
    chrono::Utc::now().with_timezone(&zone).date_naive()
}

/// One parenthesised boolean expression plus the values it binds.
#[derive(Debug, Clone, PartialEq)]
pub struct Filter {
    where_sql: String,
    params: Vec<SqlParam>,
}

impl Filter {
    /// One parenthesised expression with `?` placeholders. Never empty: an
    /// empty query compiles to the account scope and the defaults alone.
    pub fn where_sql(&self) -> &str {
        &self.where_sql
    }

    /// The values to bind, in the textual order of `where_sql`.
    pub fn params(&self) -> &[SqlParam] {
        &self.params
    }
}

/// Parse `query` and compile it for `list`. Pure: no database, no clock.
///
/// # Errors
///
/// A [`QueryError`] naming the word and the list, with a byte span into the input.
pub fn compile(req: CompileRequest<'_>) -> Result<Filter, QueryError> {
    let tokens = lex::tokenize(req.query)?;
    let expr = parse::parse(req.list, &tokens, req.today)?;
    emit::compile(
        req.list,
        expr.as_ref(),
        req.account_id,
        req.engine,
        req.zone,
    )
}
