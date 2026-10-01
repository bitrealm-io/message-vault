//! Shared state for the desktop process.
//!
//! The UI can start a long export and later press Cancel. Those are separate
//! commands, so they share a cancel flag here.

use message_crate_core::CancelFlag;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

/// Data every command can reach through Tauri's managed state.
#[derive(Debug)]
pub struct AppState {
    /// Shared switch the background job checks. The `cancel` command sets it
    /// to true. The exporter reads it between steps and stops when it is true.
    pub cancel_flag: CancelFlag,
}

impl AppState {
    /// Create state with cancel turned off.
    pub fn new() -> Self {
        Self {
            cancel_flag: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
