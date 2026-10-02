//! Cancelling an Apple Messages run while `imessage-reader` is running.
//!
//! The test sits in a test binary of its own because it finds the reader
//! among this process's children: in `helper_process.rs` other tests start
//! readers at the same time, and one of theirs could not be told from this
//! one. The children are listed with `ps`, so the test runs on Unix only.

#![cfg(unix)]

mod common;

use std::{
    path::Path,
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

use chat_db_fixture::write_chat_db;
use common::{config, helper_binary};
use message_crate_core::{ExporterConfig, LogSink, ProgressSink};

/// How long a cancelled run may take to return. The fixture exports in well
/// under a second, so a run still going after this is stuck.
const RUN_TIMEOUT: Duration = Duration::from_secs(60);

/// The process ids of this process's children named `imessage-reader`,
/// including any that have exited and not been waited for, since `ps` lists
/// those too.
fn readers_started_by_this_process() -> Vec<u32> {
    let output = Command::new("ps")
        .args(["-A", "-o", "ppid=", "-o", "pid=", "-o", "comm="])
        .output()
        .expect("run ps");
    assert!(output.status.success(), "ps failed: {output:?}");
    let me = std::process::id();
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let ppid: u32 = fields.next()?.parse().ok()?;
            let pid: u32 = fields.next()?.parse().ok()?;
            let command = fields.collect::<Vec<_>>().join(" ");
            (ppid == me && command.contains("imessage-reader")).then_some(pid)
        })
        .collect()
}

/// The run is cancelled once the reader has sent its first log line or
/// progress count, so the reader is running when cancel is set. The run must
/// return `cancelled`, and the reader must be gone: killed and waited for by
/// the exporter, not left running with the Messages database open.
#[test]
fn a_cancelled_run_stops_and_kills_the_reader() {
    helper_binary();
    let dir = tempfile::tempdir().unwrap();
    let db_path = write_chat_db(dir.path());
    let output = dir.path().join("out");
    let cancel = Arc::new(AtomicBool::new(false));

    // The readers running when the first event arrived, recorded once.
    let seen: Arc<Mutex<Option<Vec<u32>>>> = Arc::new(Mutex::new(None));
    let cancel_on_first_event = {
        let cancel = Arc::clone(&cancel);
        let seen = Arc::clone(&seen);
        move || {
            let mut seen = seen.lock().unwrap();
            if seen.is_none() {
                *seen = Some(readers_started_by_this_process());
                cancel.store(true, Ordering::Relaxed);
            }
        }
    };
    let on_log = cancel_on_first_event.clone();
    let on_progress = cancel_on_first_event;
    let config = ExporterConfig {
        log: Some(LogSink::new(move |_| on_log())),
        progress: Some(ProgressSink::unpaced(move |_| on_progress())),
        ..config(&db_path, &output, Some(Arc::clone(&cancel)))
    };

    // The run goes on its own thread so a run that never returns fails the
    // test instead of hanging it.
    let (done, result) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(imessage_ir_exporter::run(&config).map(|_| ()));
    });
    let result = result
        .recv_timeout(RUN_TIMEOUT)
        .unwrap_or_else(|_| panic!("the cancelled run had not returned after {RUN_TIMEOUT:?}"));

    let running_at_cancel = seen
        .lock()
        .unwrap()
        .clone()
        .expect("the reader sent a log line or a progress count");
    assert_eq!(
        running_at_cancel.len(),
        1,
        "one reader was running when cancel was set: {running_at_cancel:?}"
    );
    let err = result.expect_err("the run was cancelled");
    assert_eq!(err.to_string(), "cancelled");
    assert!(
        readers_started_by_this_process().is_empty(),
        "the reader {running_at_cancel:?} is still running, or exited and was never waited for"
    );
    assert!(no_jsonl_under(&output));
}

/// Whether `dir` holds no `.jsonl` file: a cancelled run writes no export.
fn no_jsonl_under(dir: &Path) -> bool {
    std::fs::read_dir(dir).map_or(true, |entries| {
        entries
            .flatten()
            .all(|entry| entry.path().extension().is_none_or(|ext| ext != "jsonl"))
    })
}
