//! Cooperative cancel flags, a log-line sink, and a scoped worker pool shared
//! by exporter runs.

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Shared cancel flag for cooperative in-process jobs.
pub type CancelFlag = Arc<AtomicBool>;

/// Callback for mid-run progress / warning lines. The desktop app sets one and
/// streams the lines to its log panel; `None` sends them to stderr.
#[derive(Clone)]
pub struct LogSink(Arc<dyn Fn(&str) + Send + Sync>);

impl LogSink {
    /// Wrap a callback that receives one log line at a time.
    pub fn new<F>(f: F) -> Self
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        Self(Arc::new(f))
    }

    /// Send one log line to the callback.
    pub fn emit(&self, line: &str) {
        (self.0)(line);
    }
}

impl fmt::Debug for LogSink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LogSink")
    }
}

/// Send a log line to `sink` when set. Otherwise print to stderr.
pub fn emit_log(sink: Option<&LogSink>, line: impl AsRef<str>) {
    let line = line.as_ref();
    match sink {
        Some(sink) => sink.emit(line),
        None => eprintln!("{line}"),
    }
}

/// Whether cancel has been requested.
pub fn is_cancelled(cancel: Option<&CancelFlag>) -> bool {
    cancel.is_some_and(|flag| flag.load(Ordering::Relaxed))
}

/// Error returned by [`check_cancel`] when cancel was requested.
///
/// Displays as `cancelled`, so callers at a `String` edge can use
/// `.map_err(|e| e.to_string())` and `anyhow` callers can use `?`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("cancelled")]
pub struct Cancelled;

/// Return `Err(Cancelled)` if cancel was requested.
///
/// # Errors
///
/// Returns [`Cancelled`] when the flag is set.
pub fn check_cancel(cancel: Option<&CancelFlag>) -> Result<(), Cancelled> {
    if is_cancelled(cancel) {
        Err(Cancelled)
    } else {
        Ok(())
    }
}

/// Run `f` over `jobs` on up to `workers` scoped threads and return one result
/// per job, in job order.
///
/// Workers pull the next job index from a shared counter (work stealing).
/// Cancel is checked before each job; once cancel is requested, every job not
/// yet started records `Err("cancelled")` instead of running `f`.
pub fn parallel_for_each<J, T, F>(
    jobs: &[J],
    workers: usize,
    cancel: Option<&CancelFlag>,
    f: F,
) -> Vec<Result<T, String>>
where
    J: Sync,
    T: Send,
    F: Fn(&J) -> Result<T, String> + Sync,
{
    if jobs.is_empty() {
        return Vec::new();
    }
    let worker_count = workers.max(1).min(jobs.len());
    let next_job = AtomicUsize::new(0);
    let results = Mutex::new(
        std::iter::repeat_with(|| None)
            .take(jobs.len())
            .collect::<Vec<Option<Result<T, String>>>>(),
    );
    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            scope.spawn(|| {
                loop {
                    let index = next_job.fetch_add(1, Ordering::Relaxed);
                    if index >= jobs.len() {
                        break;
                    }
                    let result = match check_cancel(cancel) {
                        Ok(()) => f(&jobs[index]),
                        Err(cancelled) => Err(cancelled.to_string()),
                    };
                    results.lock().expect("parallel job mutex poisoned")[index] = Some(result);
                }
            });
        }
    });
    results
        .into_inner()
        .expect("parallel job mutex poisoned")
        .into_iter()
        .map(|result| result.expect("every job has a result"))
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_displays_as_cancelled() {
        let flag: CancelFlag = Arc::new(AtomicBool::new(true));
        let err = check_cancel(Some(&flag)).unwrap_err();
        assert_eq!(err.to_string(), "cancelled");
    }

    #[test]
    fn parallel_for_each_keeps_job_order() {
        let jobs: Vec<usize> = (0..20).collect();
        let results = parallel_for_each(&jobs, 4, None, |job| {
            if job % 7 == 3 {
                Err(format!("bad {job}"))
            } else {
                Ok(job * 2)
            }
        });
        assert_eq!(results.len(), jobs.len());
        for (job, result) in jobs.iter().zip(&results) {
            match result {
                Ok(doubled) => assert_eq!(*doubled, job * 2),
                Err(message) => assert_eq!(message, &format!("bad {job}")),
            }
        }
    }

    #[test]
    fn parallel_for_each_reports_cancel_without_running_jobs() {
        let flag: CancelFlag = Arc::new(AtomicBool::new(true));
        let jobs = [1u8, 2, 3];
        let results = parallel_for_each(&jobs, 2, Some(&flag), |_| Ok::<_, String>(()));
        assert!(
            results
                .iter()
                .all(|r| r.as_ref().err().map(String::as_str) == Some("cancelled"))
        );
    }

    #[test]
    fn emit_log_uses_sink_when_set() {
        let lines = Arc::new(Mutex::new(Vec::new()));
        let lines_clone = Arc::clone(&lines);
        let sink = LogSink::new(move |line| {
            lines_clone.lock().unwrap().push(line.to_string());
        });
        emit_log(Some(&sink), "hello");
        assert_eq!(lines.lock().unwrap().as_slice(), ["hello"]);
    }
}
