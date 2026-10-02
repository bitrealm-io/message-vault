//! Shared scaffolding for the background job commands (`extract`, `format`,
//! `pull`, `push`, `transcode_staging`).
//!
//! One job runs at a time in this process. Every job reports on the same
//! `extract:*` events, which do not say which job sent them, so a second job
//! would end the first one's wait in the web app with its own finished event.
//! A job command therefore starts its job with [`start_job`], which refuses
//! while another job runs and names that job. The job gets a cancel flag of
//! its own, and [`cancel_running_job`] sets the flag of the job that is
//! running, so a Cancel stops that job and never one started after it.
//!
//! [`spawn_job`] runs the job on a worker thread, ends it, and only then sends
//! its `extract:finished` or `extract:error` event: the web app starts the
//! next stage's job as soon as that event arrives, and the job must have
//! ended by then or the next one would be refused. What differs per command
//! (building the config, mapping progress events, and shaping the finished
//! summary) stays in the command.

use std::any::Any;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use message_crate_core::CancelFlag;
use tauri::AppHandle;

use super::events;
use super::events::ExtractErrorEvent;
use crate::state::{AppState, RunningJob};

/// The desktop job that is running. Dropping it ends the job, so a command
/// that returns an error after starting its job does not leave it running.
pub(crate) struct Job {
    state: Arc<Mutex<AppState>>,
    cancel: CancelFlag,
}

impl Job {
    /// This job's cancel flag, for the worker to read between steps.
    pub(crate) fn cancel_flag(&self) -> CancelFlag {
        self.cancel.clone()
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        // Only this job's own entry: never end a job started after it.
        if state
            .job
            .as_ref()
            .is_some_and(|running| Arc::ptr_eq(&running.cancel, &self.cancel))
        {
            state.job = None;
        }
    }
}

/// Start a job called `name`, with a cancel flag of its own.
///
/// # Errors
///
/// Returns an error naming the job that is running when another job runs, or
/// if another thread panicked while holding the shared state lock.
pub(crate) fn start_job(state: &Arc<Mutex<AppState>>, name: &'static str) -> Result<Job, String> {
    let mut st = state.lock().map_err(|e| e.to_string())?;
    if let Some(running) = &st.job {
        return Err(format!(
            "Another job is running: {}. Start this one once that one ends, or cancel it.",
            running.name
        ));
    }
    let cancel: CancelFlag = Arc::new(AtomicBool::new(false));
    st.job = Some(RunningJob {
        name,
        cancel: cancel.clone(),
    });
    Ok(Job {
        state: Arc::clone(state),
        cancel,
    })
}

/// Ask the job that is running to stop. Does nothing when no job runs.
///
/// # Errors
///
/// Returns an error if another thread panicked while holding the shared
/// state lock.
pub(crate) fn cancel_running_job(state: &Arc<Mutex<AppState>>) -> Result<(), String> {
    let st = state.lock().map_err(|e| e.to_string())?;
    if let Some(running) = &st.job {
        running.cancel.store(true, Ordering::Relaxed);
    }
    Ok(())
}

/// Run `job` on a worker thread, end it, and report its outcome: the summary
/// it returns as `extract:finished`, or its failure as `extract:error`.
pub(crate) fn spawn_job<F>(app: AppHandle, job: Job, run: F)
where
    F: FnOnce() -> Result<String, ExtractErrorEvent> + Send + 'static,
{
    thread::spawn(move || match run_job(job, run) {
        Ok(summary) => events::emit(&app, events::FINISHED, summary),
        Err(error) => events::emit(&app, events::ERROR, error),
    });
}

/// Run one job, end it, and return its finished summary or the
/// `extract:error` payload the UI needs.
///
/// A panic counts as a failure. Without this, a panicking job sends neither
/// `extract:finished` nor `extract:error`, and the UI waits forever.
fn run_job<F>(job: Job, run: F) -> Result<String, ExtractErrorEvent>
where
    F: FnOnce() -> Result<String, ExtractErrorEvent>,
{
    let outcome = panic::catch_unwind(AssertUnwindSafe(run));
    drop(job);
    outcome.unwrap_or_else(|payload| {
        Err(ExtractErrorEvent {
            detail: format!("the job panicked: {}", panic_message(payload.as_ref())),
            user_message: Some("The job stopped because of a bug in Message Crate.".into()),
        })
    })
}

/// The text a panic was raised with. `panic!` carries a `&str` for a plain
/// literal and a `String` for a formatted message.
fn panic_message(payload: &(dyn Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("no message")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_state() -> Arc<Mutex<AppState>> {
        Arc::new(Mutex::new(AppState::new()))
    }

    #[test]
    fn a_job_cannot_start_while_another_runs_and_the_refusal_names_it() {
        let state = new_state();
        let _export = start_job(&state, "a download from the server").unwrap();
        let error = start_job(&state, "an extract")
            .err()
            .expect("a second job is refused");
        assert!(error.contains("a download from the server"), "{error}");
    }

    #[test]
    fn a_job_can_start_once_the_one_before_it_has_ended() {
        let state = new_state();
        let first = start_job(&state, "an extract").unwrap();
        drop(first);
        assert!(start_job(&state, "an upload").is_ok());
    }

    #[test]
    fn cancel_stops_the_running_job() {
        let state = new_state();
        let job = start_job(&state, "an upload").unwrap();
        let flag = job.cancel_flag();
        cancel_running_job(&state).unwrap();
        assert!(flag.load(Ordering::Relaxed));
    }

    #[test]
    fn a_cancel_does_not_reach_a_job_started_after_it() {
        let state = new_state();
        let first = start_job(&state, "an extract").unwrap();
        let first_flag = first.cancel_flag();
        cancel_running_job(&state).unwrap();
        drop(first);
        let second = start_job(&state, "an upload").unwrap();
        assert!(first_flag.load(Ordering::Relaxed));
        assert!(!second.cancel_flag().load(Ordering::Relaxed));
    }

    #[test]
    fn a_cancel_with_no_job_running_does_nothing() {
        let state = new_state();
        cancel_running_job(&state).unwrap();
        let job = start_job(&state, "an extract").unwrap();
        assert!(!job.cancel_flag().load(Ordering::Relaxed));
    }

    /// The job's finished or error event goes out after `run_job` returns. The
    /// web app starts the next stage's job as soon as that event arrives, so
    /// the job must have ended by then, or the next job would be refused.
    #[test]
    fn a_job_has_ended_by_the_time_its_outcome_is_reported() {
        let state = new_state();
        let job = start_job(&state, "an extract").unwrap();
        let end = run_job(job, || Ok("done".into()));
        assert_eq!(end.unwrap(), "done");
        assert!(state.lock().unwrap().job.is_none());

        let job = start_job(&state, "an extract").unwrap();
        assert!(run_job(job, || panic!("bug")).is_err());
        assert!(state.lock().unwrap().job.is_none());
    }

    fn run(
        run: impl FnOnce() -> Result<String, ExtractErrorEvent>,
    ) -> Result<String, ExtractErrorEvent> {
        run_job(start_job(&new_state(), "a test job").unwrap(), run)
    }

    #[test]
    fn a_job_that_succeeds_reports_its_summary() {
        assert_eq!(
            run(|| Ok("Export complete.".into())).unwrap(),
            "Export complete."
        );
    }

    #[test]
    fn a_job_that_fails_reports_its_error_chain() {
        let error = run(|| {
            Err(anyhow::anyhow!("disk full")
                .context("write chat.jsonl")
                .into())
        })
        .expect_err("a failed job reports an error");
        assert_eq!(error.detail, "write chat.jsonl: disk full");
        assert_eq!(error.user_message, None);
    }

    #[test]
    fn a_job_that_panics_with_a_str_reports_the_panic_message() {
        let error = run(|| panic!("index out of bounds")).expect_err("a panic reports an error");
        assert!(
            error.detail.contains("index out of bounds"),
            "{}",
            error.detail
        );
        assert!(error.user_message.is_some());
    }

    #[test]
    fn a_job_that_panics_with_a_string_reports_the_panic_message() {
        let row = 7;
        let error = run(|| panic!("bad row {row}")).expect_err("a panic reports an error");
        assert!(error.detail.contains("bad row 7"), "{}", error.detail);
    }
}
