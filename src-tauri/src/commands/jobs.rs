//! Shared scaffolding for the background job commands (`extract`, `format`,
//! `pull`, `push`).
//!
//! Every job command clears a leftover cancel flag, shares a clone of the
//! flag with its worker thread, spawns the worker, and reports a failed job
//! as an `extract:error` event. These helpers hold that repeated part. What
//! differs per command — building the config, mapping progress events, and
//! shaping the finished summary — stays in the command.
//!
//! One job runs at a time in this process. Every job command clears the
//! shared cancel flag before it starts, which is what stops a leftover
//! cancel from the previous job leaking into the next one — a concurrent-job
//! design would need its own flag per job.

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
    st.cancel_flag.store(false, Ordering::SeqCst);
    Ok(st.cancel_flag.clone())
}

/// Spawn the worker thread and report a failed job as an `extract:error`
/// event carrying the full error chain.
pub(crate) fn spawn_job<F>(app: AppHandle, run: F)
where
    F: FnOnce() -> anyhow::Result<()> + Send + 'static,
{
    thread::spawn(move || {
        if let Err(err) = run() {
            events::emit(
                &app,
                events::ERROR,
                ExtractErrorEvent {
                    detail: format!("{err:#}"),
                    user_message: None,
                },
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_and_clone_returns_a_fresh_false_flag() {
        let state = Arc::new(Mutex::new(AppState::new()));
        let cancel = reset_and_clone_cancel(&state).unwrap();
        assert!(!cancel.load(Ordering::SeqCst));
    }

    #[test]
    fn reset_and_clone_clears_a_previous_cancel_and_shares_the_flag() {
        let state = Arc::new(Mutex::new(AppState::new()));
        state
            .lock()
            .unwrap()
            .cancel_flag
            .store(true, Ordering::SeqCst);
        let cancel = reset_and_clone_cancel(&state).unwrap();
        assert!(!cancel.load(Ordering::SeqCst));
        assert!(Arc::ptr_eq(&cancel, &state.lock().unwrap().cancel_flag));
    }
}
