//! WhatsApp's files out of an encrypted iPhone backup.
//!
//! wtsexporter reads an unencrypted iPhone backup by itself. For an
//! encrypted one it asks for the password on a terminal and takes it no
//! other way, so the app decrypts WhatsApp's app-group domain into the
//! run's work dir first (through `imessage-reader`, the only program here
//! that can decrypt an iPhone backup) and wtsexporter reads that folder as
//! it reads a backup someone extracted by hand.

use anyhow::{Result, bail};
use imessage_ir_exporter::{DecryptedDomain, decrypt_ios_backup_domain, ios_backup_encrypted_flag};
use message_crate_core::{ExporterConfig, WhatsappConfig};
use std::path::{Path, PathBuf};

/// The app-group domain WhatsApp keeps its data under in an iPhone backup.
const DOMAIN: &str = "AppDomainGroup-group.net.whatsapp.WhatsApp.shared";
/// The same for the WhatsApp Business app.
const BUSINESS_DOMAIN: &str = "AppDomainGroup-group.net.whatsapp.WhatsAppSMB.shared";

/// Where the decrypted files are.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct DecryptedWhatsapp {
    /// The domain's folder: media, contacts and preferences sit under it.
    pub domain_dir: PathBuf,
    /// `ChatStorage.sqlite`, the message database.
    pub database: PathBuf,
}

/// Decrypt WhatsApp's files into `work` when `source.backup` is an
/// encrypted iPhone backup. `None` means the backup is not encrypted (or is
/// not a backup folder) and wtsexporter reads it as it is.
///
/// # Errors
///
/// Returns an error when the backup is encrypted and no password was given,
/// a password was given for a backup that is not encrypted, the password is
/// wrong, or the backup does not hold WhatsApp.
pub(crate) fn decrypt_if_encrypted(
    source: &WhatsappConfig,
    work: &Path,
    config: &ExporterConfig,
) -> Result<Option<DecryptedWhatsapp>> {
    decrypt_with(source, work, |backup, password, domain| {
        config.emit_log("The iPhone backup is encrypted; decrypting WhatsApp's files...");
        decrypt_ios_backup_domain(
            backup,
            password,
            domain,
            work,
            config.log.clone(),
            config.progress.clone(),
        )
    })
}

/// [`decrypt_if_encrypted`] with the decryption itself passed in, so the
/// rules around it can be tested without an encrypted backup.
fn decrypt_with(
    source: &WhatsappConfig,
    work: &Path,
    decrypt: impl FnOnce(&Path, &str, &str) -> Result<DecryptedDomain>,
) -> Result<Option<DecryptedWhatsapp>> {
    let Some(backup) = source.backup.as_deref() else {
        return Ok(None);
    };
    let password = source.backup_password.as_deref();
    match (ios_backup_encrypted_flag(backup), password) {
        (Some(true), None) => bail!("The backup is encrypted — fill Encryption password."),
        (Some(false), Some(_)) => {
            bail!("This backup is not encrypted. Clear Encryption password.")
        }
        (Some(true), Some(password)) => {
            let domain = if source.business {
                BUSINESS_DOMAIN
            } else {
                DOMAIN
            };
            decrypt(backup, password, domain)?;
            let domain_dir = work.join(domain);
            let database = domain_dir.join("ChatStorage.sqlite");
            if !database.is_file() {
                bail!(
                    "{} is not in this iPhone backup. {}",
                    if source.business {
                        "WhatsApp Business"
                    } else {
                        "WhatsApp"
                    },
                    if source.business {
                        "If the phone has WhatsApp, clear WhatsApp Business."
                    } else {
                        "If the phone has WhatsApp Business, tick WhatsApp Business."
                    }
                );
            }
            Ok(Some(DecryptedWhatsapp {
                domain_dir,
                database,
            }))
        }
        // Not encrypted, or not a folder with a readable Manifest.plist:
        // wtsexporter reads it, or says what is wrong with it.
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::{BUSINESS_DOMAIN, DOMAIN, DecryptedWhatsapp, decrypt_with};
    use imessage_ir_exporter::DecryptedDomain;
    use message_crate_core::WhatsappConfig;
    use std::fs;
    use std::path::Path;
    use tempfile::{TempDir, tempdir};

    /// A backup folder whose `Manifest.plist` says `encrypted`.
    fn backup(encrypted: bool) -> TempDir {
        let dir = tempdir().unwrap();
        let mut dict = plist::Dictionary::new();
        dict.insert("IsEncrypted".into(), plist::Value::Boolean(encrypted));
        plist::Value::Dictionary(dict)
            .to_file_xml(dir.path().join("Manifest.plist"))
            .unwrap();
        dir
    }

    fn source(backup: &Path, password: Option<&str>) -> WhatsappConfig {
        WhatsappConfig {
            backup: Some(backup.to_path_buf()),
            backup_password: password.map(str::to_string),
            ..WhatsappConfig::default()
        }
    }

    fn never(_: &Path, _: &str, _: &str) -> anyhow::Result<DecryptedDomain> {
        panic!("nothing should be decrypted");
    }

    /// An unencrypted backup is left for wtsexporter to read as it is.
    #[test]
    fn an_unencrypted_backup_is_not_decrypted() {
        let backup = backup(false);
        let work = tempdir().unwrap();
        let found = decrypt_with(&source(backup.path(), None), work.path(), never).unwrap();
        assert_eq!(found, None);
    }

    /// The form requires the password; a caller that skipped the form is
    /// told the same thing rather than left to a wtsexporter prompt that
    /// nobody can answer.
    #[test]
    fn an_encrypted_backup_without_a_password_is_refused() {
        let backup = backup(true);
        let work = tempdir().unwrap();
        let err = decrypt_with(&source(backup.path(), None), work.path(), never).unwrap_err();
        assert_eq!(
            err.to_string(),
            "The backup is encrypted — fill Encryption password."
        );
    }

    #[test]
    fn a_password_for_an_unencrypted_backup_is_refused() {
        let backup = backup(false);
        let work = tempdir().unwrap();
        let err =
            decrypt_with(&source(backup.path(), Some("secret")), work.path(), never).unwrap_err();
        assert_eq!(
            err.to_string(),
            "This backup is not encrypted. Clear Encryption password."
        );
    }

    /// The password and the app's own domain go to the decryption, and the
    /// answer names the files it left in the work dir.
    #[test]
    fn an_encrypted_backup_is_decrypted_into_the_work_dir() {
        let backup = backup(true);
        let work = tempdir().unwrap();
        for (business, expected_domain) in [(false, DOMAIN), (true, BUSINESS_DOMAIN)] {
            let source = WhatsappConfig {
                business,
                ..source(backup.path(), Some("secret"))
            };
            let found = decrypt_with(&source, work.path(), |from, password, domain| {
                assert_eq!(from, backup.path());
                assert_eq!(password, "secret");
                assert_eq!(domain, expected_domain);
                let dir = work.path().join(domain);
                fs::create_dir_all(&dir).unwrap();
                fs::write(dir.join("ChatStorage.sqlite"), b"db").unwrap();
                Ok(DecryptedDomain {
                    files: 1,
                    failures: 0,
                })
            })
            .unwrap();
            let domain_dir = work.path().join(expected_domain);
            assert_eq!(
                found,
                Some(DecryptedWhatsapp {
                    database: domain_dir.join("ChatStorage.sqlite"),
                    domain_dir,
                })
            );
        }
    }

    /// A backup of a phone with only the other WhatsApp app decrypts to
    /// nothing; the error names the checkbox that picks the app.
    #[test]
    fn a_backup_without_whatsapp_names_the_business_checkbox() {
        let backup = backup(true);
        let work = tempdir().unwrap();
        let err = decrypt_with(
            &source(backup.path(), Some("secret")),
            work.path(),
            |_, _, _| {
                Ok(DecryptedDomain {
                    files: 0,
                    failures: 0,
                })
            },
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "WhatsApp is not in this iPhone backup. \
             If the phone has WhatsApp Business, tick WhatsApp Business."
        );
    }
}
