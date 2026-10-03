//! JSON Lines state journals: append-only logs rewritten by sorted compaction.
//!
//! A journal is one JSON object per line. Readers rebuild skip-sets from the
//! events; writers append rows and periodically rewrite the file compacted.
//! Events are opaque to this crate — callers bring their own serde type.
//!
//! All writes run under one process-wide lock, so a rewrite can never mix
//! bytes with a concurrent append.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// One lock for append and rewrite so two threads cannot mix bytes on a line
/// or rewrite the file while another thread is appending.
static JOURNAL_WRITE_LOCK: Mutex<()> = Mutex::new(());

/// Append one event as a single JSON Lines row and sync it to disk.
///
/// The event is serialized to a buffer first so a serialization failure cannot
/// tear a half-written row. When a crash cut the last row short, the new row
/// starts on a line of its own: joined to the partial row it would be one
/// corrupt line, skipped on every later load.
///
/// # Errors
///
/// Returns an error when the parent folder cannot be created, the file cannot
/// be opened, read or synced, the event cannot be serialized, or the write
/// fails.
pub fn append<E: Serialize>(label: &str, path: &Path, event: &E) -> Result<()> {
    let _guard = JOURNAL_WRITE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(path)
        .with_context(|| format!("open {label} for append {}", path.display()))?;
    let mut buf = Vec::new();
    if ends_in_a_torn_row(&mut file)
        .with_context(|| format!("read the end of {label} {}", path.display()))?
    {
        buf.push(b'\n');
    }
    serde_json::to_writer(&mut buf, event).context("serialize journal event")?;
    buf.push(b'\n');
    file.write_all(&buf)?;
    file.sync_data()
        .with_context(|| format!("sync {label} {}", path.display()))?;
    Ok(())
}

/// Whether the journal is not empty and its last byte is not a newline.
fn ends_in_a_torn_row(file: &mut File) -> std::io::Result<bool> {
    if file.metadata()?.len() == 0 {
        return Ok(false);
    }
    let mut last = [0_u8; 1];
    file.seek(SeekFrom::End(-1))?;
    file.read_exact(&mut last)?;
    Ok(last[0] != b'\n')
}

/// Parse every event from a journal file.
///
/// A missing file is treated as an empty journal. Each line that cannot be
/// parsed is reported to `on_corrupt(line_number, parse_error)` and skipped —
/// the caller decides whether to warn or stay silent.
///
/// # Errors
///
/// Returns an error when the file cannot be opened or a line cannot be read.
pub fn load_events<E: DeserializeOwned>(
    label: &str,
    path: &Path,
    on_corrupt: &mut dyn FnMut(usize, &serde_json::Error),
) -> Result<Vec<E>> {
    let mut events = Vec::new();
    if !path.is_file() {
        return Ok(events);
    }
    let file = File::open(path).with_context(|| format!("open {label} {}", path.display()))?;
    for (i, line) in BufReader::new(file).lines().enumerate() {
        let line = line.with_context(|| format!("read {label} line {}", i + 1))?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str(&line) {
            Ok(event) => events.push(event),
            Err(error) => on_corrupt(i + 1, &error),
        }
    }
    Ok(events)
}

/// Read the journal, transform the surviving events with `rebuild`, and
/// rewrite the file — all under one write-lock acquisition, so a concurrent
/// [`append`] either lands before the read or after the rewrite, never between
/// them.
///
/// Corrupt lines are skipped silently during the read, in the push journal
/// and the pull journal alike.
///
/// # Errors
///
/// Returns an error when the file cannot be read, the temporary file cannot
/// be written, or the rename fails.
pub fn compact_with<E, F>(label: &str, path: &Path, rebuild: F) -> Result<()>
where
    E: Serialize + DeserializeOwned,
    F: FnOnce(Vec<E>) -> Vec<E>,
{
    let _guard = JOURNAL_WRITE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // Corrupt lines are skipped silently.
    let events = load_events::<E>(label, path, &mut |_, _| {})?;
    let events = rebuild(events);
    write_unlocked(path, &events)
}

/// Write `events` to a temp file and rename it over the journal (lock held),
/// synced, so a power loss leaves the old journal or the whole new one.
fn write_unlocked<E: Serialize>(path: &Path, events: &[E]) -> Result<()> {
    message_ir::write_atomic(path, |out| {
        for event in events {
            serde_json::to_writer(&mut *out, event).context("serialize journal event")?;
            out.write_all(b"\n")?;
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct TestEvent {
        url: String,
        user: String,
        key: String,
    }

    #[test]
    fn missing_file_loads_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.jsonl");
        let events: Vec<TestEvent> = load_events("journal", &path, &mut |_, _| {}).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn corrupt_lines_are_reported_and_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.jsonl");
        fs::write(
            &path,
            concat!(
                "{\"url\":\"http://server\",\"user\":\"alice\",\"key\":\"a\"}\n",
                "{not json}\n",
                "{\"url\":\"http://server\",\"user\":\"alice\",\"key\":\"b\"}\n",
            ),
        )
        .unwrap();
        let mut reported = Vec::new();
        let events: Vec<TestEvent> = load_events("journal", &path, &mut |line, error| {
            reported.push((line, error.to_string()));
        })
        .unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].key, "b");
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].0, 2);
    }

    #[test]
    fn append_writes_complete_lines_under_contention() {
        let dir = tempfile::tempdir().unwrap();
        let path = std::sync::Arc::new(dir.path().join("state.jsonl"));
        let mut handles = Vec::new();
        for i in 0..8 {
            let path = std::sync::Arc::clone(&path);
            handles.push(std::thread::spawn(move || {
                for j in 0..50 {
                    append(
                        "journal",
                        &path,
                        &TestEvent {
                            url: "http://server".into(),
                            user: "alice".into(),
                            key: format!("g-{i}-{j}"),
                        },
                    )
                    .unwrap();
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        let text = fs::read_to_string(&*path).unwrap();
        let mut lines = 0usize;
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            serde_json::from_str::<TestEvent>(line).expect("torn line");
            lines += 1;
        }
        assert_eq!(lines, 8 * 50);
    }

    #[test]
    fn compact_with_rebuilds_under_the_lock() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.jsonl");
        append(
            "journal",
            &path,
            &TestEvent {
                url: "http://a".into(),
                user: "alice".into(),
                key: "old".into(),
            },
        )
        .unwrap();
        compact_with::<TestEvent, _>("journal", &path, |mut events| {
            events.push(TestEvent {
                url: "http://a".into(),
                user: "alice".into(),
                key: "new".into(),
            });
            events
        })
        .unwrap();
        let loaded: Vec<TestEvent> = load_events("journal", &path, &mut |_, _| {}).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[1].key, "new");
        assert!(!path.with_extension("jsonl.tmp").exists());
    }

    #[test]
    fn appends_concurrent_with_compact_are_never_lost() {
        let dir = tempfile::tempdir().unwrap();
        let path = std::sync::Arc::new(dir.path().join("state.jsonl"));
        let mut handles = Vec::new();
        for i in 0..4 {
            let path = std::sync::Arc::clone(&path);
            handles.push(std::thread::spawn(move || {
                for j in 0..25 {
                    append(
                        "journal",
                        &path,
                        &TestEvent {
                            url: "http://server".into(),
                            user: "alice".into(),
                            key: format!("c-{i}-{j}"),
                        },
                    )
                    .unwrap();
                }
            }));
        }
        for _ in 0..3 {
            compact_with::<TestEvent, _>("journal", &path, |events| events).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        for h in handles {
            h.join().unwrap();
        }
        let loaded: Vec<TestEvent> = load_events("journal", &path, &mut |_, _| {}).unwrap();
        assert_eq!(loaded.len(), 4 * 25);
        let text = fs::read_to_string(&*path).unwrap();
        let mut lines = 0usize;
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            serde_json::from_str::<TestEvent>(line).expect("torn line");
            lines += 1;
        }
        assert_eq!(lines, 4 * 25);
    }

    #[test]
    fn an_append_after_a_torn_last_line_is_not_lost() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("j.jsonl");
        // A crash cut the previous row short: no closing brace, no newline.
        fs::write(&path, b"{\"url\":\"u\",\"us").unwrap();
        let event = TestEvent {
            url: "u".into(),
            user: "me".into(),
            key: "file-1".into(),
        };
        append("test", &path, &event).unwrap();
        let events: Vec<TestEvent> = load_events("test", &path, &mut |_, _| {}).unwrap();
        assert_eq!(
            events,
            vec![event],
            "the event appended after the torn row is lost"
        );
    }
}
