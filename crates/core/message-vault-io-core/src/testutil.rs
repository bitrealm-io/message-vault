//! Shared scaffolding for exporter `convert_smoke` tests (behind `testutil`).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Sorted `.csv` paths under `root` (the smoke-test file collection block).
pub fn csv_files(root: &Path) -> Vec<PathBuf> {
    let mut files: Vec<_> = fs::read_dir(root)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("csv"))
        .collect();
    files.sort();
    files
}

/// Every data row of `path`, keyed by lower-cased column name.
///
/// This reads the export with the vault's own CSV reader, the one an import
/// would use, so a test asserts the value in a named column rather than a
/// substring of the file. The distinction matters: a substring search over the
/// whole file is satisfied by the header line, so `contains("direction")`
/// passes whether or not a single message was written.
///
/// # Panics
///
/// Panics when the file cannot be read or is not CSV — a test asserting on a
/// missing export has already failed.
pub fn csv_rows(path: &Path) -> Vec<BTreeMap<String, String>> {
    let (mut rdr, headers) =
        message_csv::open_csv_lowercase(path).expect("the export must be readable CSV");
    rdr.records()
        .map(|rec| {
            let rec = rec.expect("a readable CSV row");
            headers
                .iter()
                .enumerate()
                .map(|(i, h)| (h.clone(), rec.get(i).unwrap_or("").trim().to_string()))
                .collect()
        })
        .collect()
}

/// Assert that the first CSV under `root` has every `contains` header column,
/// none of the `not_contains` columns, and at least one data row whose named
/// columns all hold the given values; also assert no stray `.json` files
/// remain.
///
/// `row` is what makes this a test of the export rather than of the header:
/// pass the message body, direction and timestamp the fixture is known to
/// carry, so an exporter that wrote a correct header and no messages — or the
/// wrong body against the right column — fails here.
///
/// # Panics
///
/// Panics when there is no CSV, when a column is missing or unexpectedly
/// present, when a `.json` file was left behind, or when no row matches.
pub fn assert_csv_header(
    root: &Path,
    contains: &[&str],
    not_contains: &[&str],
    row: &[(&str, &str)],
) {
    assert!(
        !row.is_empty(),
        "assert_csv_header needs at least one column and value to check; \
         a header-only assertion cannot fail on an export with no messages"
    );
    let files = csv_files(root);
    assert!(!files.is_empty(), "expected at least one .csv");
    let json_count = fs::read_dir(root)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension().and_then(|x| x.to_str()) == Some("json")
                && !p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with(".meta.json"))
        })
        .count();
    assert_eq!(json_count, 0);
    let contents = fs::read_to_string(&files[0]).expect("read the export");
    let header = contents.lines().next().expect("a header line");
    for col in contains {
        assert!(header.contains(col), "header missing {col:?}");
    }
    for col in not_contains {
        assert!(!header.contains(col), "header unexpectedly has {col:?}");
    }
    assert_csv_row(&files[0], row);
}

/// Assert that `path` holds at least one data row whose named columns all hold
/// the given values.
///
/// # Panics
///
/// Panics when no row matches, naming the rows that were there.
pub fn assert_csv_row(path: &Path, expected: &[(&str, &str)]) {
    let rows = csv_rows(path);
    assert!(
        !rows.is_empty(),
        "{} has a header but no messages",
        path.display()
    );
    let matched = rows.iter().any(|row| {
        expected.iter().all(|(col, want)| {
            row.get(&col.to_ascii_lowercase())
                .is_some_and(|v| v == want)
        })
    });
    assert!(
        matched,
        "no row in {} has {:?}; rows were {:#?}",
        path.display(),
        expected,
        rows
    );
}
