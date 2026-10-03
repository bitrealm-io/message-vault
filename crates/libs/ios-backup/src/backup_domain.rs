//! Have `imessage-reader` decrypt one domain of an encrypted iPhone backup.
//!
//! The WhatsApp importer calls this before it runs wtsexporter: wtsexporter
//! asks for a backup password on a terminal and takes it no other way, and
//! the only code in the repository that can decrypt an iPhone backup is the
//! GPL `crabapple`, which stays behind the helper's process boundary
//! (`docs/adr/0014-gpl-code-only-behind-a-process-boundary.md`).

use std::path::Path;

use anyhow::{Result, bail};
use imessage_reader_protocol::{BackupDomainRequest, Event, Request};
use message_crate_core::{LogSink, ProgressSink};

use crate::helper::Helper;

/// What [`decrypt_ios_backup_domain`] wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecryptedDomain {
    /// Files written. Zero means the backup does not hold the domain.
    pub files: u64,
    /// Files the backup lists that could not be decrypted; each has a line
    /// on the log.
    pub failures: u64,
}

/// Decrypt every file of `domain` in the encrypted iPhone backup at
/// `backup_root` into `<out_dir>/<domain>/`, keeping each file's path inside
/// the domain. Log lines and progress counts go to the given sinks.
///
/// # Errors
///
/// Returns an error when the folder is not an encrypted iPhone backup, the
/// password is wrong, or there is no `imessage-reader` program to run.
pub fn decrypt_ios_backup_domain(
    backup_root: &Path,
    backup_password: &str,
    domain: &str,
    out_dir: &Path,
    log: Option<LogSink>,
    progress: Option<ProgressSink>,
) -> Result<DecryptedDomain> {
    let request = Request::BackupDomain(BackupDomainRequest {
        backup_path: backup_root.to_path_buf(),
        backup_password: backup_password.to_string(),
        domain: domain.to_string(),
        out_dir: out_dir.to_path_buf(),
    });
    read_answer(Helper::spawn(&request, log, progress)?)
}

/// The counts a started program answers the request with.
fn read_answer(mut helper: Helper) -> Result<DecryptedDomain> {
    let answer = loop {
        match helper.next_event()? {
            Event::Source { .. } => {}
            Event::BackupDomainDone { files, failures } => {
                break DecryptedDomain { files, failures };
            }
            other => bail!("expected the decrypted-domain answer, got {other:?}"),
        }
    };
    helper.finish()?;
    Ok(answer)
}

#[cfg(all(test, unix))]
mod tests {
    use imessage_reader_protocol::{BackupDomainRequest, PROTOCOL_VERSION, Request};

    use super::{DecryptedDomain, read_answer};
    use crate::testutil::{fake_helper, source_line, spawn_fake};

    fn request() -> Request {
        Request::BackupDomain(BackupDomainRequest {
            backup_path: "/nowhere/backup".into(),
            backup_password: "secret".into(),
            domain: "AppDomainGroup-group.net.whatsapp.WhatsApp.shared".into(),
            out_dir: "/nowhere/out".into(),
        })
    }

    #[test]
    fn the_counts_follow_the_source_event() {
        let dir = tempfile::tempdir().unwrap();
        let body = format!(
            "{}\necho '{{\"event\":\"backup_domain_done\",\"files\":12,\"failures\":1}}'",
            source_line(PROTOCOL_VERSION)
        );
        let helper = spawn_fake(&fake_helper(dir.path(), &body), &request());
        assert_eq!(
            read_answer(helper).unwrap(),
            DecryptedDomain {
                files: 12,
                failures: 1
            }
        );
    }
}
