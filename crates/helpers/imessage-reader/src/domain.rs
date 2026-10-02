//! Decrypt every file of one domain of an encrypted iPhone backup into a
//! folder the app owns.
//!
//! WhatsApp keeps its data in an app-group domain of the backup. The program
//! that reads WhatsApp (wtsexporter) asks for a backup password on a
//! terminal and takes it no other way, so the app has this helper write the
//! domain out in the clear first and points wtsexporter at the result. The
//! layout is the one wtsexporter extracts for itself:
//! `<out_dir>/<domain>/<path inside the domain>`.

use std::{
    fs::{File, create_dir_all},
    io::{BufWriter, Write, copy},
    path::{Component, Path, PathBuf},
};

use crabapple::Backup;
use imessage_reader_protocol::{BackupDomainRequest, Event, Platform, Progress, Source};
use rusqlite::Connection;

use crate::{
    backup::{decrypt_backup, restrict_permissions},
    error::{RuntimeError, UNENCRYPTED_BACKUP_CLEAR_PASSWORD},
    log::{emit, emit_log},
    options::ReaderOptions,
};

/// How many progress events a domain's files are reported in, at most.
const PROGRESS_EVENTS: u64 = 200;

/// `Manifest.db` flag for a regular file. Folders and symbolic links have
/// rows too, and no bytes.
const FLAG_FILE: i64 = 1;

/// What one run wrote.
pub(crate) struct Written {
    /// Files decrypted into the folder.
    pub files: u64,
    /// Files the manifest lists that could not be decrypted.
    pub failures: u64,
}

/// Open the encrypted backup `request` names.
///
/// # Errors
///
/// Returns an error when the folder is not an iPhone backup, the backup is
/// not encrypted, or the password is wrong.
pub(crate) fn open(request: &BackupDomainRequest) -> Result<Backup, RuntimeError> {
    let options = ReaderOptions::from_source(Source {
        db_path: request.backup_path.clone(),
        platform: Platform::Ios,
        backup_password: Some(request.backup_password.clone()),
    });
    decrypt_backup(&options)?
        .ok_or_else(|| RuntimeError::InvalidOptions(UNENCRYPTED_BACKUP_CLEAR_PASSWORD.to_string()))
}

/// Decrypt every file of `request.domain` into `request.out_dir`.
///
/// A file that cannot be decrypted is reported on the log and counted; the
/// rest are still written. A backup routinely lists a file whose bytes it
/// does not hold, and one missing photo must not cost the conversations.
///
/// # Errors
///
/// Returns an error when the manifest cannot be read or the domain is not a
/// plain folder name.
pub(crate) fn decrypt_domain(
    backup: &Backup,
    request: &BackupDomainRequest,
) -> Result<Written, RuntimeError> {
    let manifest = Connection::open(backup.manifest_db_path())?;
    let files = domain_files(&manifest, &request.domain)?;
    let total = files.len() as u64;
    let every = (total / PROGRESS_EVENTS).max(1);
    emit_log(format!("Decrypting {total} files from the backup..."));

    let mut written = Written {
        files: 0,
        failures: 0,
    };
    for (index, (file_id, relative_path)) in files.iter().enumerate() {
        let Some(target) = target_path(&request.out_dir, &request.domain, relative_path) else {
            return Err(RuntimeError::InvalidOptions(format!(
                "the backup names a file outside its own folder: {relative_path}"
            )));
        };
        match decrypt_to(backup, file_id, &target) {
            Ok(()) => written.files += 1,
            Err(why) => {
                written.failures += 1;
                emit_log(format!(
                    "warning: {relative_path} could not be decrypted: {why}"
                ));
            }
        }
        let done = index as u64 + 1;
        if done.is_multiple_of(every) || done == total {
            emit(&Event::Progress(Progress::Setup {
                label: "Decrypting files".to_string(),
                step: done,
                total,
            }));
        }
    }
    Ok(written)
}

/// The regular files `Manifest.db` lists under `domain`, as file id and
/// path inside the domain, in path order.
fn domain_files(
    manifest: &Connection,
    domain: &str,
) -> Result<Vec<(String, String)>, RuntimeError> {
    let mut statement = manifest.prepare(
        "SELECT fileID, relativePath FROM Files \
         WHERE domain = ?1 AND flags = ?2 ORDER BY relativePath",
    )?;
    let rows = statement.query_map((domain, FLAG_FILE), |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Where a file of `domain` is written, or `None` when the domain or the
/// path would leave `out_dir`: the manifest comes from the backup, and a
/// path with `..` or a root in it is not one to follow.
fn target_path(out_dir: &Path, domain: &str, relative_path: &str) -> Option<PathBuf> {
    let plain = |path: &Path| {
        path.components()
            .all(|part| matches!(part, Component::Normal(_)))
    };
    let domain = Path::new(domain);
    let relative = Path::new(relative_path);
    if domain.components().count() != 1 || !plain(domain) || !plain(relative) {
        return None;
    }
    if relative.as_os_str().is_empty() {
        return None;
    }
    Some(out_dir.join(domain).join(relative))
}

/// Decrypt the backup entry `file_id` into a new file at `target`, readable
/// by its owner only.
fn decrypt_to(backup: &Backup, file_id: &str, target: &Path) -> Result<(), RuntimeError> {
    let entry = backup.get_file(file_id)?;
    if let Some(parent) = target.parent() {
        create_dir_all(parent)?;
    }
    let file = File::create(target)?;
    restrict_permissions(&file)?;
    // An empty file has no ciphertext to decrypt.
    if entry.metadata.size == 0 {
        return Ok(());
    }
    let mut plain = backup.decrypt_entry_stream(&entry)?;
    let mut writer = BufWriter::new(file);
    copy(&mut plain, &mut writer)?;
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{decrypt_domain, domain_files, open, target_path};
    use crate::error::IOS_BACKUP_PASSWORD_INCORRECT;
    use imessage_reader_protocol::BackupDomainRequest;
    use rusqlite::Connection;
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    const DOMAIN: &str = "AppDomainGroup-group.net.whatsapp.WhatsApp.shared";

    /// Only the asked domain's regular files are listed: another domain's
    /// file, and a folder row of the same domain, are left out.
    #[test]
    fn only_the_regular_files_of_the_domain_are_listed() {
        let manifest = Connection::open_in_memory().unwrap();
        manifest
            .execute_batch(&format!(
                "CREATE TABLE Files (fileID TEXT, domain TEXT, relativePath TEXT, flags INTEGER);
                 INSERT INTO Files VALUES
                     ('aa', '{DOMAIN}', 'Message/Media/photo.jpg', 1),
                     ('bb', '{DOMAIN}', 'ChatStorage.sqlite', 1),
                     ('cc', '{DOMAIN}', 'Message', 2),
                     ('dd', 'HomeDomain', 'Library/SMS/sms.db', 1);"
            ))
            .unwrap();
        assert_eq!(
            domain_files(&manifest, DOMAIN).unwrap(),
            [
                ("bb".to_string(), "ChatStorage.sqlite".to_string()),
                ("aa".to_string(), "Message/Media/photo.jpg".to_string()),
            ]
        );
    }

    /// A file lands under the domain's own folder, and a manifest path that
    /// would climb out of it, or name a root, is refused.
    #[test]
    fn a_path_that_leaves_the_folder_is_refused() {
        let out = Path::new("/scratch");
        assert_eq!(
            target_path(out, DOMAIN, "Message/Media/photo.jpg"),
            Some(PathBuf::from(format!(
                "/scratch/{DOMAIN}/Message/Media/photo.jpg"
            )))
        );
        for bad in ["../outside", "Message/../../outside", "/etc/passwd", ""] {
            assert_eq!(target_path(out, DOMAIN, bad), None, "{bad:?}");
        }
        for bad_domain in ["..", "a/b", "/abs", ""] {
            assert_eq!(target_path(out, bad_domain, "file"), None, "{bad_domain:?}");
        }
    }

    fn request(backup: &Path, password: &str, out: &Path) -> BackupDomainRequest {
        BackupDomainRequest {
            backup_path: backup.to_path_buf(),
            backup_password: password.to_string(),
            domain: DOMAIN.to_string(),
            out_dir: out.to_path_buf(),
        }
    }

    /// The whole path against a backup encrypted the way an iPhone's is:
    /// the password opens it, the domain's files come out as their plain
    /// bytes under the domain's folder, another domain's file stays in the
    /// backup, and a file the manifest lists without bytes is counted and
    /// skipped rather than ending the run.
    #[test]
    fn an_encrypted_backup_gives_up_the_domain_and_nothing_else() {
        let backup = tempfile::tempdir().unwrap();
        let photo = vec![7u8; 100_000];
        encrypted::write_backup(
            backup.path(),
            "secret",
            &[
                (
                    DOMAIN,
                    "ChatStorage.sqlite",
                    Some(b"the database".as_slice()),
                ),
                (DOMAIN, "Message/Media/photo.jpg", Some(photo.as_slice())),
                (DOMAIN, "Message/Media/empty.txt", Some(b"".as_slice())),
                (DOMAIN, "Message/Media/gone.jpg", None),
                (
                    "HomeDomain",
                    "Library/SMS/sms.db",
                    Some(b"messages".as_slice()),
                ),
            ],
        );
        let out = tempfile::tempdir().unwrap();
        let request = request(backup.path(), "secret", out.path());

        let opened = open(&request).unwrap();
        let written = decrypt_domain(&opened, &request).unwrap();

        assert_eq!((written.files, written.failures), (3, 1));
        let domain = out.path().join(DOMAIN);
        assert_eq!(
            fs::read(domain.join("ChatStorage.sqlite")).unwrap(),
            b"the database"
        );
        assert_eq!(
            fs::read(domain.join("Message/Media/photo.jpg")).unwrap(),
            photo
        );
        assert_eq!(
            fs::read(domain.join("Message/Media/empty.txt")).unwrap(),
            b""
        );
        assert!(!out.path().join("HomeDomain").exists());
    }

    #[test]
    fn a_wrong_password_is_refused_in_the_apps_words() {
        let backup = tempfile::tempdir().unwrap();
        encrypted::write_backup(backup.path(), "secret", &[]);
        let out = tempfile::tempdir().unwrap();
        let Err(err) = open(&request(backup.path(), "wrong", out.path())) else {
            panic!("a wrong password opened the backup");
        };
        assert_eq!(err.to_string(), IOS_BACKUP_PASSWORD_INCORRECT);
    }

    /// Writes an encrypted iPhone backup, small and made up, in the layout
    /// `crabapple` reads: a key bag whose one class key is wrapped with the
    /// password-derived key, a `Manifest.db` encrypted under that class, and
    /// each file encrypted under its own wrapped key.
    mod encrypted {
        use aes_kw::{KeyInit, KwAes256};
        use crabapple::backup::crypto::{aes_encrypt_cbc_with_padding, derive_key_from_password};
        use plist::{Dictionary, Uid, Value};
        use rusqlite::Connection;
        use std::{fs, path::Path};

        const CLASS: u32 = 3;
        const CLASS_KEY: [u8; 32] = [0x11; 32];
        const MANIFEST_KEY: [u8; 32] = [0x22; 32];
        const FILE_KEY: [u8; 32] = [0x33; 32];

        /// One file: domain, path inside it, and its bytes (`None` lists the
        /// file in the manifest and leaves its bytes out of the backup).
        pub(super) type File<'a> = (&'a str, &'a str, Option<&'a [u8]>);

        pub(super) fn write_backup(root: &Path, password: &str, files: &[File<'_>]) {
            let (dpsl, salt) = (b"dpsl-salt".as_slice(), b"salt".as_slice());
            let master = derive_key_from_password(password.as_bytes(), dpsl, 2, salt, 2).unwrap();

            let mut bag = Vec::new();
            tlv(&mut bag, b"TYPE", &1u32.to_be_bytes());
            tlv(&mut bag, b"UUID", &[0xAA; 16]);
            tlv(&mut bag, b"WRAP", &0u32.to_be_bytes());
            tlv(&mut bag, b"DPSL", dpsl);
            tlv(&mut bag, b"DPIC", &2u32.to_be_bytes());
            tlv(&mut bag, b"SALT", salt);
            tlv(&mut bag, b"ITER", &2u32.to_be_bytes());
            tlv(&mut bag, b"UUID", &[0xBB; 16]);
            tlv(&mut bag, b"CLAS", &CLASS.to_be_bytes());
            tlv(&mut bag, b"WRAP", &2u32.to_be_bytes());
            tlv(&mut bag, b"WPKY", &wrap(master.as_ref(), &CLASS_KEY));

            let mut lockdown = Dictionary::new();
            for key in [
                "BuildVersion",
                "DeviceName",
                "ProductType",
                "ProductVersion",
                "SerialNumber",
                "UniqueDeviceID",
            ] {
                lockdown.insert(key.into(), Value::String("test".into()));
            }
            let mut manifest = Dictionary::new();
            manifest.insert("IsEncrypted".into(), Value::Boolean(true));
            manifest.insert("BackupKeyBag".into(), Value::Data(bag));
            manifest.insert(
                "ManifestKey".into(),
                Value::Data(class_wrapped(&MANIFEST_KEY)),
            );
            manifest.insert("Lockdown".into(), Value::Dictionary(lockdown));
            manifest.insert("Applications".into(), Value::Dictionary(Dictionary::new()));
            Value::Dictionary(manifest)
                .to_file_binary(root.join("Manifest.plist"))
                .unwrap();

            let plain_db = root.join("Manifest.plain.db");
            let db = Connection::open(&plain_db).unwrap();
            db.execute_batch(
                "CREATE TABLE Files (fileID TEXT PRIMARY KEY, domain TEXT, relativePath TEXT,
                                     flags INTEGER, file BLOB);",
            )
            .unwrap();
            for (index, (domain, path, bytes)) in files.iter().enumerate() {
                let file_id = format!("{index:040x}");
                let size = bytes.map_or(1, <[u8]>::len) as u64;
                db.execute(
                    "INSERT INTO Files VALUES (?1, ?2, ?3, 1, ?4)",
                    (&file_id, domain, path, metadata(size)),
                )
                .unwrap();
                if let Some(bytes) = bytes {
                    let folder = root.join(&file_id[..2]);
                    fs::create_dir_all(&folder).unwrap();
                    fs::write(folder.join(&file_id), encrypt(bytes, &FILE_KEY)).unwrap();
                }
            }
            drop(db);
            let plain = fs::read(&plain_db).unwrap();
            fs::remove_file(&plain_db).unwrap();
            fs::write(root.join("Manifest.db"), encrypt(&plain, &MANIFEST_KEY)).unwrap();
        }

        /// A file's `Manifest.db` metadata: the keyed archive iOS writes,
        /// cut down to the fields `crabapple` reads.
        fn metadata(size: u64) -> Vec<u8> {
            let mut file = Dictionary::new();
            for key in [
                "LastModified",
                "Flags",
                "GroupID",
                "LastStatusChange",
                "Birth",
                "Mode",
                "InodeNumber",
            ] {
                file.insert(key.into(), Value::Integer(1.into()));
            }
            file.insert("Size".into(), Value::Integer(size.into()));
            file.insert("ProtectionClass".into(), Value::Integer(CLASS.into()));
            file.insert("EncryptionKey".into(), Value::Uid(Uid::new(2)));
            let mut key = Dictionary::new();
            key.insert("NS.data".into(), Value::Data(class_wrapped(&FILE_KEY)));
            let mut top = Dictionary::new();
            top.insert("root".into(), Value::Uid(Uid::new(1)));
            let mut archive = Dictionary::new();
            archive.insert("$top".into(), Value::Dictionary(top));
            archive.insert(
                "$objects".into(),
                Value::Array(vec![
                    Value::String("$null".into()),
                    Value::Dictionary(file),
                    Value::Dictionary(key),
                ]),
            );
            let mut bytes = Vec::new();
            Value::Dictionary(archive)
                .to_writer_binary(&mut bytes)
                .unwrap();
            bytes
        }

        /// A key as the backup stores it: the protection class, then the key
        /// wrapped with that class's key.
        fn class_wrapped(key: &[u8; 32]) -> Vec<u8> {
            let mut out = CLASS.to_le_bytes().to_vec();
            out.extend(wrap(&CLASS_KEY, key));
            out
        }

        fn wrap(kek: &[u8], key: &[u8; 32]) -> Vec<u8> {
            let mut out = [0u8; 40];
            KwAes256::new_from_slice(kek)
                .unwrap()
                .wrap_key(key, &mut out)
                .unwrap();
            out.to_vec()
        }

        fn encrypt(bytes: &[u8], key: &[u8; 32]) -> Vec<u8> {
            aes_encrypt_cbc_with_padding(bytes, &key.to_vec().into()).unwrap()
        }

        fn tlv(out: &mut Vec<u8>, tag: &[u8; 4], value: &[u8]) {
            out.extend(tag);
            out.extend((value.len() as u32).to_be_bytes());
            out.extend(value);
        }
    }
}
