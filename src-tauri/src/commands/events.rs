//! JSON shapes sent to the UI as Tauri events.
//!
//! The core library has a similar error type, but it cannot be sent through
//! Tauri because it is not serializable. These structs match the TypeScript
//! types in `web/src/lib/types.ts`.

use message_vault_io_core::ProgressEvent;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// One log line for the UI's log panel. Payload: `String`.
pub const LOG: &str = "extract:log";
/// Progress-bar numbers. Payload: [`ExtractProgressEvent`].
pub const PROGRESS: &str = "extract:progress";
/// One skipped or failed item for the Import Errors list. Payload: an issue row.
pub const ISSUE: &str = "extract:issue";
/// The job finished. Payload: the summary line or JSON the screen shows.
pub const FINISHED: &str = "extract:finished";
/// The job failed before it could finish. Payload: [`ExtractErrorEvent`].
pub const ERROR: &str = "extract:error";

/// Send one event to the UI. An emit fails only when no window is left to
/// receive it; the job carries on, and the miss goes to the process log so it
/// is not silent.
pub fn emit(app: &AppHandle, event: &str, payload: impl Serialize + Clone) {
    if let Err(error) = app.emit(event, payload) {
        eprintln!("warning: {event} event not delivered: {error}");
    }
}

/// Progress numbers the UI uses to update the progress bar.
#[derive(Debug, Clone, Serialize)]
pub struct ExtractProgressEvent {
    /// Current pipeline stage: `setup`, `parse`, `attachments`, `prepare`,
    /// `media`, or `upload`.
    pub step: String,
    /// Number of items finished so far.
    pub done: usize,
    /// Total items, or 0 when the total is unknown.
    pub total: usize,
    /// Bytes finished on the attachments step.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes_done: Option<u64>,
    /// Byte total on the attachments step (grows when a size was unknown).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes_total: Option<u64>,
    /// Extra step status the UI shows. On `setup` it is the step's label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

impl ExtractProgressEvent {
    /// A count-only event for `step`, with no bytes and no status.
    fn counts(step: &str, done: usize, total: usize) -> Self {
        Self {
            step: step.into(),
            done,
            total,
            bytes_done: None,
            bytes_total: None,
            status: None,
        }
    }
}

/// The exporters' typed progress event, in the shape the UI listens for.
/// The stage name becomes `step`; byte counts ride along on `attachments`
/// and the setup label rides along as `status`.
impl From<ProgressEvent> for ExtractProgressEvent {
    fn from(event: ProgressEvent) -> Self {
        match event {
            ProgressEvent::Setup { label, step, total } => Self {
                status: Some(label),
                ..Self::counts("setup", step, total)
            },
            ProgressEvent::Parse { done, total } => Self::counts("parse", done, total),
            ProgressEvent::Attachments {
                done,
                total,
                bytes_done,
                bytes_total,
            } => Self {
                bytes_done: Some(bytes_done),
                bytes_total: Some(bytes_total),
                ..Self::counts("attachments", done, total)
            },
            ProgressEvent::Prepare { done, total } => Self::counts("prepare", done, total),
            ProgressEvent::Media { done, total } => Self::counts("media", done, total),
        }
    }
}

/// Failure details for the `extract:error` event.
///
/// When `user_message` is missing, it is left out of the JSON so the
/// TypeScript type can treat it as optional.
#[derive(Debug, Clone, Serialize)]
pub struct ExtractErrorEvent {
    /// Full error chain, for logs and the advanced-details view.
    pub detail: String,
    /// Friendlier message for the UI, when one is known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_message: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_events_map_onto_the_ui_steps() {
        let setup = ExtractProgressEvent::from(ProgressEvent::Setup {
            label: "Deriving backup keys".into(),
            step: 1,
            total: 5,
        });
        assert_eq!(setup.step, "setup");
        assert_eq!((setup.done, setup.total), (1, 5));
        assert_eq!(setup.status.as_deref(), Some("Deriving backup keys"));
        assert_eq!(setup.bytes_done, None);

        let parse = ExtractProgressEvent::from(ProgressEvent::Parse {
            done: 500,
            total: 12_345,
        });
        assert_eq!(parse.step, "parse");
        assert_eq!((parse.done, parse.total), (500, 12_345));
        assert_eq!(parse.status, None);

        let attachments = ExtractProgressEvent::from(ProgressEvent::Attachments {
            done: 2,
            total: 3,
            bytes_done: 100,
            bytes_total: 500,
        });
        assert_eq!(attachments.step, "attachments");
        assert_eq!((attachments.done, attachments.total), (2, 3));
        assert_eq!(attachments.bytes_done, Some(100));
        assert_eq!(attachments.bytes_total, Some(500));

        let prepare = ExtractProgressEvent::from(ProgressEvent::Prepare { done: 2, total: 3 });
        assert_eq!(prepare.step, "prepare");
        assert_eq!((prepare.done, prepare.total), (2, 3));

        let media = ExtractProgressEvent::from(ProgressEvent::Media { done: 1, total: 4 });
        assert_eq!(media.step, "media");
        assert_eq!((media.done, media.total), (1, 4));
    }

    #[test]
    fn serialized_event_omits_absent_fields() {
        let json = serde_json::to_value(ExtractProgressEvent::from(ProgressEvent::Prepare {
            done: 0,
            total: 3,
        }))
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "step": "prepare", "done": 0, "total": 3 })
        );
    }
}
