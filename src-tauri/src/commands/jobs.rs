//! Shared scaffolding for the background job commands (`extract`, `format`,
//! `pull`, `push`).
//!
//! Every job command clears a leftover cancel flag, shares a clone of the
//! flag with its worker thread, spawns the worker, and reports a failed or
//! panicked job as an `extract:error` event. These helpers hold that repeated part. What
//! differs per command — building the config, mapping progress events, and
//! shaping the finished summary — stays in the command.
//!
//! One job runs at a time in this process. Every job command clears the
//! shared cancel flag before it starts, which is what stops a leftover
//! cancel from the previous job leaking into the next one — a concurrent-job
//! design would need its own flag per job.

use std::any::Any;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread;

use message_vault_io_core::CancelFlag;
use tauri::AppHandle;

use super::events;
use super::events::ExtractErrorEvent;
use crate::state::AppState;

/// Clear a leftover cancel from a previous job and return a clone of the
/// shared flag for the worker thread.
///
/// One lock round-trip replaces the earlier two (reset, then clone), so a
/// `cancel` call cannot slip between them and start the new job cancelled.
///
/// # Errors
///
/// Returns an error if another thread panicked while holding the shared
/// state lock.
pub(crate) fn reset_and_clone_cancel(state: &Arc<Mutex<AppState>>) -> Result<CancelFlag, String> {
    let st = state.lock().map_err(|e| e.to_string())?;
    st.cancel_flag.store(false, Ordering::Relaxed);
    Ok(st.cancel_flag.clone())
}

/// Spawn the worker thread and report a failed job as an `extract:error`
/// event carrying the full error chain.
pub(crate) fn spawn_job<F>(app: AppHandle, run: F)
where
    F: FnOnce() -> anyhow::Result<()> + Send + 'static,
{
    thread::spawn(move || {
        if let Some(error) = run_job(run) {
            events::emit(&app, events::ERROR, error);
        }
    });
}

/// Run one job and turn its outcome into the `extract:error` payload the UI
/// needs, or `None` when the job succeeded. A successful job sends its own
/// `extract:finished` event, because only the job knows its summary.
///
/// A panic counts as a failure. Without this, a panicking job sends neither
/// `extract:finished` nor `extract:error`, and the UI waits forever. The
/// shared cancel flag needs no cleanup: the next job clears it before it
/// starts.
fn run_job<F>(run: F) -> Option<ExtractErrorEvent>
where
    F: FnOnce() -> anyhow::Result<()>,
{
    match panic::catch_unwind(AssertUnwindSafe(run)) {
        Ok(Ok(())) => None,
        Ok(Err(err)) => Some(ExtractErrorEvent {
            detail: format!("{err:#}"),
            user_message: None,
        }),
        Err(payload) => Some(ExtractErrorEvent {
            detail: format!("the job panicked: {}", panic_message(payload.as_ref())),
            user_message: Some("The job stopped because of a bug in Message Vault.".into()),
        }),
    }
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

    #[test]
    fn reset_and_clone_clears_a_previous_cancel_and_shares_the_flag() {
        let state = Arc::new(Mutex::new(AppState::new()));
        state
            .lock()
            .unwrap()
            .cancel_flag
            .store(true, Ordering::Relaxed);
        let cancel = reset_and_clone_cancel(&state).unwrap();
        assert!(!cancel.load(Ordering::Relaxed));
        assert!(Arc::ptr_eq(&cancel, &state.lock().unwrap().cancel_flag));
    }

    #[test]
    fn a_job_that_succeeds_reports_no_error() {
        assert!(run_job(|| Ok(())).is_none());
    }

    #[test]
    fn a_job_that_fails_reports_its_error_chain() {
        let error = run_job(|| Err(anyhow::anyhow!("disk full").context("write chat.jsonl")))
            .expect("a failed job reports an error");
        assert_eq!(error.detail, "write chat.jsonl: disk full");
        assert_eq!(error.user_message, None);
    }

    #[test]
    fn a_job_that_panics_with_a_str_reports_the_panic_message() {
        let error = run_job(|| panic!("index out of bounds")).expect("a panic reports an error");
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
        let error = run_job(|| panic!("bad row {row}")).expect("a panic reports an error");
        assert!(error.detail.contains("bad row 7"), "{}", error.detail);
    }
}
