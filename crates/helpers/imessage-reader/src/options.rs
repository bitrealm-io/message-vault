//! What one read of a Messages source needs.

use std::path::{Path, PathBuf};

use imessage_database::{
    tables::table::DEFAULT_PATH_IOS,
    util::{platform::Platform, query_context::QueryContext},
};
use imessage_reader_protocol::{Event, ExportRequest, Progress, Source};

use crate::log::{emit, emit_log};

/// Parsed options for one read.
#[derive(Debug)]
pub(crate) struct ReaderOptions {
    pub db_path: PathBuf,
    pub attachment_root: Option<String>,
    pub query_context: QueryContext,
    pub use_caller_id: bool,
    pub platform: Platform,
    pub cleartext_password: Option<String>,
    pub contacts_path: Option<PathBuf>,
    /// Where decrypted files go. The app owns this folder.
    pub scratch_dir: PathBuf,
}

impl ReaderOptions {
    /// Options for a full export.
    pub fn from_export(request: ExportRequest) -> Self {
        Self {
            attachment_root: request.attachment_root,
            contacts_path: request.contacts_path,
            use_caller_id: request.use_caller_id,
            ..Self::from_source(request.source, request.scratch_dir)
        }
    }

    /// Options for opening the source and nothing more. Decrypted files go
    /// to `scratch_dir` and nowhere else.
    pub fn from_source(source: Source, scratch_dir: PathBuf) -> Self {
        Self {
            db_path: source.db_path,
            attachment_root: None,
            query_context: QueryContext::default(),
            use_caller_id: true,
            platform: match source.platform {
                imessage_reader_protocol::Platform::MacOs => Platform::macOS,
                imessage_reader_protocol::Platform::Ios => Platform::iOS,
            },
            cleartext_password: source.backup_password,
            contacts_path: None,
            scratch_dir,
        }
    }

    /// Messages database path for the selected platform.
    pub fn get_db_path(&self) -> PathBuf {
        match self.platform {
            Platform::iOS => self.db_path.join(DEFAULT_PATH_IOS),
            Platform::macOS => self.db_path.clone(),
        }
    }

    /// The folder decrypted files are written to.
    pub fn scratch_dir(&self) -> &Path {
        &self.scratch_dir
    }

    /// Send one log line to the app.
    pub fn emit_log(&self, line: impl AsRef<str>) {
        emit_log(line);
    }

    /// Announce a numbered setup step (decrypting a backup, caching a table):
    /// a `[step/total] label...` log line for people and a
    /// [`Progress::Setup`] event for the progress bar.
    pub fn setup_step(&self, step: u64, total: u64, label: &str) {
        self.emit_log(format!("  [{step}/{total}] {label}..."));
        emit(&Event::Progress(Progress::Setup {
            label: label.to_string(),
            step,
            total,
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use imessage_reader_protocol::Source;
    use std::path::PathBuf;

    fn source(platform: imessage_reader_protocol::Platform) -> Source {
        Source {
            db_path: PathBuf::from("/backups/one"),
            platform,
            backup_password: Some("secret".to_string()),
        }
    }

    /// An export request carries everything the source alone does not: the
    /// attachment root, the contacts file, the caller-id choice and the
    /// scratch folder.
    #[test]
    fn an_export_request_fills_every_option() {
        let options = ReaderOptions::from_export(ExportRequest {
            source: source(imessage_reader_protocol::Platform::Ios),
            attachment_root: Some("/phone/files".to_string()),
            contacts_path: Some(PathBuf::from("/contacts.db")),
            use_caller_id: false,
            scratch_dir: PathBuf::from("/scratch"),
        });
        assert!(matches!(options.platform, Platform::iOS));
        assert_eq!(options.db_path, PathBuf::from("/backups/one"));
        assert_eq!(options.attachment_root.as_deref(), Some("/phone/files"));
        assert_eq!(options.contacts_path, Some(PathBuf::from("/contacts.db")));
        assert!(!options.use_caller_id);
        assert_eq!(options.cleartext_password.as_deref(), Some("secret"));
        assert_eq!(options.scratch_dir(), Path::new("/scratch"));
    }

    /// A source and a scratch folder are enough to open the database: the
    /// identities request sends nothing else. Decrypted files go to the
    /// scratch folder it names, never to the system's temporary folder.
    #[test]
    fn a_source_alone_opens_with_defaults() {
        let options = ReaderOptions::from_source(
            source(imessage_reader_protocol::Platform::MacOs),
            PathBuf::from("/app/scratch/one"),
        );
        assert!(matches!(options.platform, Platform::macOS));
        assert_eq!(options.attachment_root, None);
        assert_eq!(options.contacts_path, None);
        assert!(options.use_caller_id);
        assert_eq!(options.cleartext_password.as_deref(), Some("secret"));
        assert_eq!(options.scratch_dir(), Path::new("/app/scratch/one"));
    }

    /// A Mac `chat.db` is the path itself; an iOS backup folder holds the
    /// database under its hashed file name.
    #[test]
    fn the_database_path_depends_on_the_platform() {
        let mac = ReaderOptions::from_source(
            source(imessage_reader_protocol::Platform::MacOs),
            PathBuf::from("/scratch"),
        );
        assert_eq!(mac.get_db_path(), PathBuf::from("/backups/one"));

        let ios = ReaderOptions::from_source(
            source(imessage_reader_protocol::Platform::Ios),
            PathBuf::from("/scratch"),
        );
        assert_eq!(
            ios.get_db_path(),
            PathBuf::from("/backups/one").join(DEFAULT_PATH_IOS)
        );
    }

    /// A log line is one log event; a setup step is a numbered log line
    /// for people followed by a setup progress event for the bar.
    #[test]
    fn setup_steps_and_log_lines_are_emitted() {
        let options = ReaderOptions::from_source(
            source(imessage_reader_protocol::Platform::MacOs),
            PathBuf::from("/scratch"),
        );
        let events = crate::log::capture::events(|| {
            options.emit_log("hello");
            options.setup_step(1, 4, "Caching chats");
        });
        assert_eq!(
            events,
            [
                serde_json::json!({"event": "log", "line": "hello"}),
                serde_json::json!({"event": "log", "line": "  [1/4] Caching chats..."}),
                serde_json::json!({
                    "event": "progress",
                    "stage": "setup",
                    "label": "Caching chats",
                    "step": 1,
                    "total": 4,
                }),
            ]
        );
    }
}
