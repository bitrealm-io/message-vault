//! Where an attachment's bytes are, and decrypting one out of an encrypted
//! backup when the app asks.

use std::path::{Path, PathBuf};

use crabapple::error::BackupError;
use imessage_database::tables::attachment::Attachment;
use imessage_reader_protocol::Event;

use crate::{backup::decrypt_file, error::RuntimeError, session::MailSession};

/// The path Messages resolves for this attachment on the selected platform,
/// or `None` when it has no file.
pub(crate) fn resolved_path(session: &MailSession, attachment: &Attachment) -> Option<PathBuf> {
    attachment
        .resolved_attachment_path(
            &session.options.platform,
            &session.options.db_path,
            session.options.attachment_root.as_deref(),
        )
        .map(PathBuf::from)
}

/// Answer one attachment request: for an encrypted backup, decrypt the entry
/// into the scratch folder and name the file; otherwise name the path itself
/// when it exists.
///
/// A missing entry is not an error. The app records the attachment as
/// missing and moves on, the same as it does for a plain file that is gone,
/// and the reason is on the log so a run's worth of gaps does not go
/// unexplained.
pub(crate) fn decrypt_for_app(session: &MailSession, source: &Path) -> Event {
    let Some(backup) = session
        .data_source
        .backup
        .as_ref()
        .filter(|b| b.is_encrypted())
    else {
        return Event::Attachment {
            path: source.is_file().then(|| source.to_path_buf()),
        };
    };
    match decrypt_file(backup, source, session.options.scratch_dir()) {
        Ok(temp) => Event::Attachment { path: Some(temp) },
        Err(RuntimeError::BackupError(BackupError::FileNotFoundInBackup(_))) => {
            session.options.emit_log(format!(
                "warning: attachment {} not found in encrypted backup; skipping bytes",
                source.display()
            ));
            Event::Attachment { path: None }
        }
        Err(e) => {
            session.options.emit_log(format!(
                "warning: attachment {} could not be decrypted: {e}",
                source.display()
            ));
            Event::Attachment { path: None }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::FixtureDb;

    /// On a Mac the row's `filename` is the path; a row without one has no
    /// file.
    #[test]
    fn a_mac_attachment_resolves_to_the_path_the_row_names() {
        let fixture = FixtureDb::write();
        let session = fixture.session();
        let messages = FixtureDb::messages(&session);
        let mut attachments =
            Attachment::from_message(session.data_source.db(), &messages[0]).unwrap();
        assert_eq!(attachments.len(), 1);

        assert_eq!(
            resolved_path(&session, &attachments[0]),
            Some(fixture.dir.path().join("photo.jpg"))
        );

        attachments[0].filename = None;
        assert_eq!(resolved_path(&session, &attachments[0]), None);
    }

    /// Without an encrypted backup there is nothing to decrypt: the answer
    /// names the file when it exists and nothing when it does not.
    #[test]
    fn an_unencrypted_source_answers_with_the_path_itself() {
        let fixture = FixtureDb::write();
        let session = fixture.session();
        let photo = fixture.dir.path().join("photo.jpg");

        assert!(matches!(
            decrypt_for_app(&session, &photo),
            Event::Attachment { path: Some(p) } if p == photo
        ));
        assert!(matches!(
            decrypt_for_app(&session, &fixture.dir.path().join("gone.jpg")),
            Event::Attachment { path: None }
        ));
    }
}
