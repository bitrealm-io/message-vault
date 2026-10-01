//! Every column in `schema/sql/*.sql` carries a `--` comment on the line
//! above it. The developer reference at `docs/docs/developer/reference/database.md`
//! is written from those comments, so a column without one is a column the
//! documentation cannot describe.
//!
//! The test walks the directory rather than a fixed list, so a new SQL file is
//! covered the day it is added. It looks only inside `CREATE TABLE` and
//! `CREATE VIRTUAL TABLE` bodies; triggers, indexes, functions and `ALTER TABLE`
//! statements have no column list to comment.

use std::fs;
use std::path::{Path, PathBuf};

fn schema_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../schema/sql")
}

/// A column line inside a table body: four-space indent, an identifier, then a
/// SQLite storage class.
fn typed_column(line: &str) -> Option<&str> {
    let body = line.strip_prefix("    ")?;
    if body.starts_with(' ') {
        return None;
    }
    let (name, rest) = split_identifier(body)?;
    let rest = rest.trim_start();
    let ty = rest
        .split(|c: char| !c.is_ascii_alphabetic())
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    matches!(ty.as_str(), "INTEGER" | "TEXT" | "REAL" | "BLOB").then_some(name)
}

/// A column line inside an FTS5 virtual table: four-space indent, an
/// identifier, an optional trailing comma, nothing else.
fn fts_column(line: &str) -> Option<&str> {
    let body = line.strip_prefix("    ")?;
    if body.starts_with(' ') {
        return None;
    }
    let (name, rest) = split_identifier(body)?;
    let rest = rest.trim();
    (rest.is_empty() || rest == ",").then_some(name)
}

/// Split a leading SQL identifier (`[a-z_][a-z0-9_]*`) from the rest.
fn split_identifier(s: &str) -> Option<(&str, &str)> {
    let mut chars = s.char_indices();
    let (_, first) = chars.next()?;
    if !(first.is_ascii_alphabetic() || first == '_') {
        return None;
    }
    let end = s
        .char_indices()
        .find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '_'))
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    Some((&s[..end], &s[end..]))
}

fn starts_with_word(line: &str, words: &[&str]) -> bool {
    let upper = line.trim_start().to_ascii_uppercase();
    words.iter().any(|w| upper.starts_with(w))
}

/// Column names in `file` whose preceding non-empty line is not a comment,
/// each as `file:line:column`.
fn missing_comments(file: &Path) -> Vec<String> {
    let name = file.file_name().unwrap().to_string_lossy();
    let text = fs::read_to_string(file).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    let mut missing = Vec::new();
    let mut in_create = false;
    let mut in_fts = false;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if starts_with_word(trimmed, &["CREATE TABLE"]) {
            in_create = true;
            in_fts = false;
            continue;
        }
        if starts_with_word(trimmed, &["CREATE VIRTUAL TABLE"]) {
            in_create = true;
            in_fts = trimmed.to_ascii_uppercase().contains("USING FTS5");
            continue;
        }
        if in_create && trimmed == ");" {
            in_create = false;
            in_fts = false;
            continue;
        }
        if !in_create
            || starts_with_word(
                line,
                &[
                    "UNIQUE",
                    "PRIMARY KEY",
                    "FOREIGN KEY",
                    "CHECK",
                    "CONSTRAINT",
                ],
            )
        {
            continue;
        }
        if in_fts && starts_with_word(line, &["CONTENT", "TOKENIZE"]) {
            continue;
        }
        let column = if in_fts {
            fts_column(line)
        } else {
            typed_column(line)
        };
        let Some(column) = column else { continue };

        let commented = lines[..i]
            .iter()
            .rev()
            .map(|l| l.trim())
            .find(|l| !l.is_empty())
            .is_some_and(|l| l.starts_with("--"));
        if !commented {
            missing.push(format!("{name}:{}:{column}", i + 1));
        }
    }
    missing
}

#[test]
fn every_schema_column_has_a_comment() {
    let dir = schema_dir();
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "sql"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no .sql files under {}", dir.display());

    let missing: Vec<String> = files.iter().flat_map(|f| missing_comments(f)).collect();
    assert!(
        missing.is_empty(),
        "columns without a `--` comment on the line above:\n  {}",
        missing.join("\n  ")
    );
}

#[test]
fn the_check_sees_the_tables_it_should() {
    // A guard against the parser silently matching nothing: the real schema
    // has many columns, and every one of them is commented, so a version of
    // this parser that recognises no columns would also pass the test above.
    let sql = "CREATE TABLE IF NOT EXISTS t (\n    id INTEGER PRIMARY KEY,\n    name TEXT NOT NULL,\n    UNIQUE(name)\n);\nCREATE VIRTUAL TABLE IF NOT EXISTS f USING fts5(\n    body,\n    content='t'\n);\n";
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.sql");
    fs::write(&path, sql).unwrap();
    assert_eq!(
        missing_comments(&path),
        vec!["t.sql:2:id", "t.sql:3:name", "t.sql:7:body"]
    );
}
