//! What an iPhone backup folder says about itself before anything is opened.

use std::{fs::File, path::Path};

/// Whether `backup_root/Manifest.plist` is marked encrypted.
///
/// Returns `None` when the file is missing or cannot be parsed. That is
/// intentional: Import then leaves the password optional and the converter
/// still fails after start if the backup turns out to be encrypted.
pub fn ios_backup_encrypted_flag(backup_root: &Path) -> Option<bool> {
    let path = backup_root.join("Manifest.plist");
    let file = File::open(path).ok()?;
    let value = plist::Value::from_reader(file).ok()?;
    let dict = value.as_dictionary()?;
    match dict.get("IsEncrypted") {
        Some(plist::Value::Boolean(flag)) => Some(*flag),
        Some(_) => None,
        None => Some(false),
    }
}

#[cfg(test)]
mod tests {
    use super::ios_backup_encrypted_flag;
    use std::fs;

    fn write_plist(dir: &std::path::Path, is_encrypted: &str) {
        let body = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>IsEncrypted</key>
  <{is_encrypted}/>
</dict>
</plist>
"#
        );
        fs::write(dir.join("Manifest.plist"), body).unwrap();
    }

    #[test]
    fn encrypted_flag_none_when_manifest_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(ios_backup_encrypted_flag(dir.path()), None);
    }

    #[test]
    fn encrypted_flag_none_when_manifest_is_garbage() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Manifest.plist"), b"not a plist").unwrap();
        assert_eq!(ios_backup_encrypted_flag(dir.path()), None);
    }

    #[test]
    fn encrypted_flag_reads_is_encrypted_boolean() {
        let encrypted = tempfile::tempdir().unwrap();
        write_plist(encrypted.path(), "true");
        assert_eq!(ios_backup_encrypted_flag(encrypted.path()), Some(true));

        let plain = tempfile::tempdir().unwrap();
        write_plist(plain.path(), "false");
        assert_eq!(ios_backup_encrypted_flag(plain.path()), Some(false));
    }
}
