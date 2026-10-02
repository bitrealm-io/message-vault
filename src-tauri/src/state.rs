//! Shared state for the desktop process.
//!
//! The UI starts a long job and may later press Cancel. Those are separate
//! commands, so the job that is running, and its cancel flag, live here.

use message_crate_core::CancelFlag;

/// Data every command can reach through Tauri's managed state.
#[derive(Debug, Default)]
pub struct AppState {
    /// The desktop job that is running, if any. One runs at a time: a job
    /// command refuses to start while this is set (`commands::jobs`).
    pub job: Option<RunningJob>,
}

/// The one desktop job that is running.
#[derive(Debug)]
pub struct RunningJob {
    /// What the job is, in words for the error that refuses a second job.
    pub name: &'static str,
    /// This job's own cancel flag. The `cancel` command sets it, and the job
    /// reads it between steps and stops when it is true. A new flag is made
    /// for each job, so a Cancel never reaches a job started after it.
    pub cancel: CancelFlag,
}

impl AppState {
    /// Create state with no job running.
    pub fn new() -> Self {
        Self::default()
    }
}
