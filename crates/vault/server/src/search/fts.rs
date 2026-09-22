//! The full-text index's answer to a free-text term on Messages. SQLite
//! uses the contentless FTS5 table; Postgres uses the `search_tsv` column.
//! Both index body, subject, attachment names, and transcriptions.

use crate::db::engine::DbEngine;

use super::bridge::Sql;
use super::parse::TextTerm;

/// Quote for FTS5 so operators and punctuation are literal text.
fn fts5_literal(term: &str) -> String {
    format!("\"{}\"", term.replace('"', "\"\""))
}

/// `'term':*` for `to_tsquery`, or `None` when the term holds a quote or
/// backslash, which tsquery literals cannot carry.
fn pg_prefix(term: &str) -> Option<String> {
    if term.is_empty() || term.contains(['\\', '\'']) {
        return None;
    }
    Some(format!("'{term}':*"))
}

/// A `SELECT` of the ids of every message whose indexed text matches
/// `term`: the index asked once for the whole search. The caller puts it
/// inside `m.id IN (...)`. Never a correlated `EXISTS` per message row,
/// which SQLite cannot drive from the FTS index and so ran the match once
/// per candidate message (#413).
pub(crate) fn matching_ids(out: &mut Sql, engine: DbEngine, term: &TextTerm) {
    match engine {
        DbEngine::Sqlite => {
            let q = match term {
                TextTerm::Term { text, prefix: true } => format!("{}*", fts5_literal(text)),
                TextTerm::Term {
                    text,
                    prefix: false,
                }
                | TextTerm::Phrase(text) => fts5_literal(text),
            };
            out.push("SELECT rowid FROM messages_fts WHERE messages_fts MATCH ");
            out.bind_text(q);
        }
        DbEngine::Postgres => {
            let (func, arg) = match term {
                TextTerm::Term { text, prefix: true } => match pg_prefix(text) {
                    Some(q) => ("to_tsquery", q),
                    None => ("plainto_tsquery", text.clone()),
                },
                TextTerm::Term {
                    text,
                    prefix: false,
                } => ("plainto_tsquery", text.clone()),
                TextTerm::Phrase(text) => ("phraseto_tsquery", text.clone()),
            };
            out.push(&format!(
                "SELECT fm.id FROM messages fm WHERE fm.search_tsv @@ {func}('simple', "
            ));
            out.bind_text(arg);
            out.push(")");
        }
    }
}
