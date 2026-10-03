//! The full-text index's answer to a free-text term on Messages: the
//! contentless FTS5 table, which indexes body, subject, attachment names,
//! and transcriptions.

use super::bridge::Sql;
use super::parse::TextTerm;

/// Quote for FTS5 so operators and punctuation are literal text. The
/// tokenizer then reads the quoted text as a phrase: `a&b` is `a` then `b`.
fn fts5_literal(term: &str) -> String {
    format!("\"{}\"", term.replace('"', "\"\""))
}

/// A `SELECT` of the ids of every message whose indexed text matches
/// `term`: the index asked once for the whole search. The caller puts it
/// inside `m.id IN (...)`. Never a correlated `EXISTS` per message row,
/// which SQLite cannot drive from the FTS index and so ran the match once
/// per candidate message (#413).
pub(crate) fn matching_ids(out: &mut Sql, term: &TextTerm) {
    out.push("SELECT rowid FROM messages_fts WHERE messages_fts MATCH ");
    out.bind_text(match_expr(term));
}

/// One term as an FTS5 query: a quoted phrase, with `*` after it for a prefix.
fn match_expr(term: &TextTerm) -> String {
    match term {
        TextTerm::Term { text, prefix: true } => format!("{}*", fts5_literal(text)),
        TextTerm::Term {
            text,
            prefix: false,
        }
        | TextTerm::Phrase(text) => fts5_literal(text),
    }
}

/// The FTS5 query a relevance order ranks by: any of `terms`, so `bm25()`
/// scores a message on every free-text word it has. `None` when there is no
/// term, and so nothing to rank by.
pub(crate) fn rank_query(terms: &[&TextTerm]) -> Option<String> {
    if terms.is_empty() {
        return None;
    }
    Some(
        terms
            .iter()
            .map(|t| match_expr(t))
            .collect::<Vec<_>>()
            .join(" OR "),
    )
}
