//! Shared scaffolding for exporter `convert_smoke` tests (behind `testutil`).

use crate::ExportReport;
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

/// Assert that a CSV export under `root` is complete: every file carries the
/// `contains` header columns and none of the `not_contains` ones, one of them
/// holds a data row whose named columns all hold the given values, and no
/// stray `.json` files remain.
///
/// `row` is the part that can fail on an empty export. Header columns are
/// written before the first message is, so a column assertion alone passes an
/// exporter that parsed nothing; pass the message body, direction and
/// timestamp the fixture is known to carry, and an exporter that wrote a
/// correct header and no messages — or the wrong body against the right
/// column — fails here.
///
/// Every file is checked rather than the alphabetically first, because one
/// export writes one unified header across its conversations and which
/// conversation sorts first is not part of the claim.
///
/// # Panics
///
/// Panics when there is no CSV, when a column is missing or unexpectedly
/// present, when a `.json` file was left behind, or when no row matches.
pub fn assert_csv_export(
    root: &Path,
    contains: &[&str],
    not_contains: &[&str],
    row: &[(&str, &str)],
) {
    assert!(
        !row.is_empty(),
        "assert_csv_export needs at least one column and value to check; \
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
    for path in &files {
        let contents = fs::read_to_string(path).expect("read the export");
        let header = contents.lines().next().expect("a header line");
        for col in contains {
            assert!(
                header.contains(col),
                "header of {} missing {col:?}",
                path.display()
            );
        }
        for col in not_contains {
            assert!(
                !header.contains(col),
                "header of {} unexpectedly has {col:?}",
                path.display()
            );
        }
    }
    assert_csv_row_in_any(&files, row);
}

/// Assert that one of `paths` holds a data row whose named columns all hold the
/// given values.
///
/// The row names its own `chat_identifier`, so which file it landed in is not
/// part of the claim. Searching every file keeps the assertion about the export
/// rather than about the alphabetical order of its conversations, which a new
/// fixture can change.
///
/// # Panics
///
/// Panics when no file holds a matching row, naming what was found instead.
pub fn assert_csv_row_in_any(paths: &[PathBuf], expected: &[(&str, &str)]) {
    let mut seen = Vec::new();
    for path in paths {
        let rows = csv_rows(path);
        if rows.iter().any(|row| row_matches(row, expected)) {
            return;
        }
        seen.push((path.display().to_string(), rows));
    }
    panic!(
        "no row in any of {} export file(s) has {expected:?}; found {seen:#?}",
        paths.len()
    );
}

/// True when every named column of `row` holds the wanted value.
fn row_matches(row: &BTreeMap<String, String>, expected: &[(&str, &str)]) -> bool {
    expected.iter().all(|(col, want)| {
        row.get(&col.to_ascii_lowercase())
            .is_some_and(|v| v == want)
    })
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
    let matched = rows.iter().any(|row| row_matches(row, expected));
    assert!(
        matched,
        "no row in {} has {:?}; rows were {:#?}",
        path.display(),
        expected,
        rows
    );
}

/// Sorted names of the `.jsonl` files directly under `dir`.
fn jsonl_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("read output")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".jsonl"))
        .collect();
    names.sort();
    names
}

/// Run a JSONL export into `out` twice, the second time with `resume` set, and
/// assert that the second run resumed the first instead of rewriting it.
///
/// `run` calls the exporter's own entry point with `OutputFormat::Jsonl`, the
/// output folder `out`, and the `resume` flag it is given. The first run must
/// write at least one conversation, one `.jsonl` file per conversation, and
/// skip nothing. The second must skip every conversation the first wrote,
/// still count them all, and leave the same files with the same bytes.
///
/// The file bytes alone prove nothing: the writer is deterministic, so a
/// resumed run that quietly rewrote every conversation would produce the same
/// bytes. `conversations_skipped` is the only observable difference between
/// resuming and starting over, so that is the assertion that matters.
///
/// Returns the first run's report, for assertions about the fixture's own
/// conversation count.
///
/// # Panics
///
/// Panics when either run fails or any of the above does not hold.
pub fn assert_jsonl_resumes(
    out: &Path,
    run: impl Fn(bool) -> anyhow::Result<ExportReport>,
) -> ExportReport {
    let report = run(false).expect("convert");
    assert!(
        report.conversations >= 1,
        "the fixture must produce at least one conversation"
    );
    assert_eq!(
        report.conversations_skipped, 0,
        "a first run into an empty folder skips nothing"
    );

    let first = jsonl_names(out);
    assert_eq!(
        first.len() as u64,
        report.conversations,
        "the queue wrote a file per conversation: {first:?}"
    );
    let bodies: Vec<String> = first
        .iter()
        .map(|n| fs::read_to_string(out.join(n)).expect("read jsonl"))
        .collect();

    let resumed = run(true).expect("resume convert");
    assert_eq!(
        resumed.conversations, report.conversations,
        "a resumed run still counts every conversation"
    );
    assert_eq!(
        resumed.conversations_skipped, report.conversations,
        "a resumed run must skip every conversation the first run wrote"
    );

    assert_eq!(jsonl_names(out), first, "same file set after a resume");
    for (name, before) in first.iter().zip(bodies) {
        assert_eq!(
            fs::read_to_string(out.join(name)).expect("reread"),
            before,
            "a resumed run must not rewrite {name}"
        );
    }
    report
}
