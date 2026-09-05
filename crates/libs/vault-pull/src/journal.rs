//! Local log of which files were already downloaded by vault-pull.
//!
//! The file is `.vault-pull-state.jsonl`. JSON Lines means one JSON object per
//! line. A later download can skip attachments that are already on disk.

use std::collections::HashSet;
#[cfg(test)]
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Filename of the local download log, written in the output folder.
pub const PULL_JOURNAL_NAME: &str = ".vault-pull-state.jsonl";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
/// One row in `.vault-pull-state.jsonl`.
pub enum PullJournalEvent {
    /// One attachment is on disk, so a later run can skip downloading it.
    AssetOk {
        /// Vault base URL the attachment came from.
        url: String,
        /// Account username the run signed in as.
        username: String,
        /// Hex SHA-256 fingerprint of the attachment bytes; the skip key.
        sha256: String,
    },
    /// A whole download finished, with its counts.
    BackupComplete {
        /// Vault base URL the download came from.
        url: String,
        /// Account username the run signed in as.
        username: String,
        /// Conversations written.
        conversations: u64,
        /// Messages written.
        messages: u64,
        /// Attachments downloaded.
        assets: u64,
    },
}

#[derive(Debug, Default)]
/// Skip sets rebuilt from the journal for one vault URL and username.
pub struct PullJournalState {
    /// SHA-256 fingerprints (hex of the file bytes) of attachments already on disk.
    pub assets: HashSet<String>,
    /// True if the last run finished cleanly (a `backup_complete` event was written).
    pub backup_complete: bool,
}

/// Path of `.vault-pull-state.jsonl` inside the output folder.
pub fn journal_path(out_dir: &Path) -> PathBuf {
    out_dir.join(PULL_JOURNAL_NAME)
}

/// Read the journal and keep events that match this vault URL and username.
///
/// A missing file is treated as an empty journal. A line that cannot be parsed
/// is skipped so a newer event type does not break an older client.
///
/// # Errors
///
/// Returns an error when the file cannot be opened or a line cannot be read.
pub fn load(path: &Path, url: &str, username: &str) -> Result<PullJournalState> {
    let mut state = PullJournalState::default();
    let events: Vec<PullJournalEvent> =
        jsonl_journal::load_events("pull journal", path, &mut |_, _| {})?;
    for event in events {
        match event {
            PullJournalEvent::AssetOk {
                url: u,
                username: a,
                sha256,
                ..
            } if u == url && a == username => {
                state.assets.insert(sha256);
            }
            PullJournalEvent::BackupComplete {
                url: u,
                username: a,
                ..
            } if u == url && a == username => {
                state.backup_complete = true;
            }
            _ => {}
        }
    }
    Ok(state)
}

/// Append one event as a JSON Lines row and flush it to disk.
///
/// # Errors
///
/// Returns an error when the parent folder cannot be created, the file cannot
/// be opened, or the write fails.
pub fn append(path: &Path, event: &PullJournalEvent) -> Result<()> {
    jsonl_journal::append("pull journal", path, event)
}

/// Rewrite the journal from in-memory `state` for one vault URL and username.
///
/// # Errors
///
/// Returns an error when the temporary file cannot be written or the rename fails.
pub fn compact(path: &Path, url: &str, username: &str, state: &PullJournalState) -> Result<()> {
    jsonl_journal::compact_with::<PullJournalEvent, _>("pull journal", path, |_events| {
        let mut events: Vec<PullJournalEvent> = Vec::new();
        let mut assets: Vec<_> = state.assets.iter().collect();
        assets.sort_unstable();
        for sha in assets {
            events.push(PullJournalEvent::AssetOk {
                url: url.to_string(),
                username: username.to_string(),
                sha256: sha.clone(),
            });
        }
        if state.backup_complete {
            // Counts are unused on resume; a `backup_complete` row only means the last run finished.
            events.push(PullJournalEvent::BackupComplete {
                url: url.to_string(),
                username: username.to_string(),
                conversations: 0,
                messages: 0,
                assets: 0,
            });
        }
        events
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_asset_and_backup_complete_events() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PULL_JOURNAL_NAME);
        fs::write(
            &path,
            concat!(
                "{\"event\":\"asset_ok\",\"url\":\"http://vault\",\"username\":\"alice\",",
                "\"sha256\":\"aaabbbccc\",\"path\":\"attachments/aaabbbccc\",\"size_bytes\":12345}\n",
                "{\"event\":\"asset_ok\",\"url\":\"http://vault\",\"username\":\"alice\",",
                "\"sha256\":\"dddeeefff\",\"path\":\"attachments/dddeeefff\",\"size_bytes\":67890}\n",
                "{\"event\":\"backup_complete\",\"url\":\"http://vault\",\"username\":\"alice\",",
                "\"conversations\":2,\"messages\":100,\"assets\":2}\n",
            ),
        )
        .unwrap();

        let state = load(&path, "http://vault", "alice").unwrap();

        assert!(state.assets.contains("aaabbbccc"));
        assert!(state.assets.contains("dddeeefff"));
        assert!(state.backup_complete);
    }

    #[test]
    fn filters_by_url_and_username() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PULL_JOURNAL_NAME);
        fs::write(
            &path,
            concat!(
                "{\"event\":\"asset_ok\",\"url\":\"http://vault-a\",\"username\":\"alice\",",
                "\"sha256\":\"aaa\",\"path\":\"attachments/aaa\",\"size_bytes\":1}\n",
                "{\"event\":\"asset_ok\",\"url\":\"http://vault-b\",\"username\":\"bob\",",
                "\"sha256\":\"bbb\",\"path\":\"attachments/bbb\",\"size_bytes\":2}\n",
            ),
        )
        .unwrap();

        let state = load(&path, "http://vault-a", "alice").unwrap();
        assert!(state.assets.contains("aaa"));
        assert!(!state.assets.contains("bbb"));
    }

    #[test]
    fn compact_sorts_assets_and_rewrites() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PULL_JOURNAL_NAME);
        let mut state = PullJournalState::default();
        state.assets.insert("ccc".into());
        state.assets.insert("aaa".into());
        state.assets.insert("bbb".into());
        state.backup_complete = true;

        compact(&path, "http://vault", "alice", &state).unwrap();

        let reloaded = load(&path, "http://vault", "alice").unwrap();
        assert_eq!(reloaded.assets.len(), 3);
        assert!(reloaded.assets.contains("aaa"));
        assert!(reloaded.assets.contains("bbb"));
        assert!(reloaded.assets.contains("ccc"));
        assert!(reloaded.backup_complete);
    }
}
