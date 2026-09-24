//! The account holder's WhatsApp number, recorded on every message as the
//! address it was held at (`docs/adr/0015-the-owner-is-recorded-on-the-message.md`).
//!
//! An iPhone backup carries the number in WhatsApp's preferences plist, which
//! wtsexporter copies into its working directory along with the rest of the
//! app-group domain. An Android crypt backup carries nothing, so the number
//! comes from the import form.

use crate::jid::jid_to_e164;
use anyhow::{Context, Result};
use phone::OwnerHandleSet;
use std::path::{Path, PathBuf};

/// WhatsApp's preferences plist, relative to a folder that holds the
/// extracted app-group domains: wtsexporter's working directory, or a backup
/// someone extracted by hand. The Business app keeps the same layout under
/// its own domain.
const PREFERENCE_PLISTS: [&str; 2] = [
    "AppDomainGroup-group.net.whatsapp.WhatsApp.shared/Library/Preferences/group.net.whatsapp.WhatsApp.shared.plist",
    "AppDomainGroup-group.net.whatsapp.WhatsAppSMB.shared/Library/Preferences/group.net.whatsapp.WhatsAppSMB.shared.plist",
];

/// Keys that carry the holder's jid (`<number>@s.whatsapp.net`), in the order
/// they are tried. `OwnPhoneNumber` is not one of them: it is stored encoded.
const OWNER_JID_KEYS: [&str; 2] = ["OwnJabberID", "LastOwnJabberID"];

/// The holder's number from the first preferences plist under `roots` that
/// carries one, as E.164. `None` when no plist is there or none has an owner
/// key. A plist that cannot be read is reported on `log` and counts as
/// absent, so the form's number still applies.
pub(crate) fn owner_from_backup(roots: &[PathBuf], log: &mut Vec<String>) -> Option<String> {
    roots
        .iter()
        .flat_map(|root| PREFERENCE_PLISTS.iter().map(move |rel| root.join(rel)))
        .filter(|path| path.is_file())
        .find_map(|path| match owner_from_plist(&path) {
            Ok(owner) => owner,
            Err(err) => {
                log.push(format!("could not read {}: {err:#}", path.display()));
                None
            }
        })
}

/// The first owner key in `path` whose value is a user jid.
fn owner_from_plist(path: &Path) -> Result<Option<String>> {
    let value = plist::Value::from_file(path).context("parse plist")?;
    let Some(dict) = value.as_dictionary() else {
        return Ok(None);
    };
    Ok(OWNER_JID_KEYS.iter().find_map(|key| {
        dict.get(key)
            .and_then(plist::Value::as_string)
            .and_then(jid_to_e164)
    }))
}

/// The number typed on the form, under the handle key the vault uses for
/// its `handles` rows, so it matches the profile phone it was filled from.
///
/// # Errors
///
/// Returns an error when the value has no usable digits.
pub(crate) fn owner_from_form(phone: &str) -> Result<String> {
    OwnerHandleSet::from_phones(&[phone.to_string()])?
        .primary_owner_handle()
        .context("owner phone has no usable digits")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture_backup() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ios-backup")
    }

    /// The fixture is the layout wtsexporter leaves in its working directory:
    /// the app-group domain with `Library/Preferences/<group>.plist` inside.
    #[test]
    fn reads_the_owner_jid_from_the_backup_plist() {
        let mut log = Vec::new();
        let owner = owner_from_backup(&[fixture_backup()], &mut log);
        assert_eq!(owner.as_deref(), Some("+15555550100"));
        assert!(log.is_empty(), "{log:?}");
    }

    /// The working directory is searched first, then the backup folder; a
    /// folder without the domain contributes nothing.
    #[test]
    fn a_folder_without_the_plist_yields_no_owner() {
        let empty = tempfile::tempdir().unwrap();
        let mut log = Vec::new();
        assert_eq!(
            owner_from_backup(&[empty.path().to_path_buf()], &mut log),
            None
        );
        assert_eq!(
            owner_from_backup(&[empty.path().to_path_buf(), fixture_backup()], &mut log).as_deref(),
            Some("+15555550100")
        );
    }

    /// `OwnJabberID` wins; `LastOwnJabberID` is read only when the first key
    /// is missing. Written as XML, which the plist crate reads like binary.
    #[test]
    fn falls_back_to_the_last_own_jid() {
        let dir = tempfile::tempdir().unwrap();
        let prefs = dir.path().join(PREFERENCE_PLISTS[0]);
        fs::create_dir_all(prefs.parent().unwrap()).unwrap();
        let mut log = Vec::new();

        fs::write(
            &prefs,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>LastOwnJabberID</key><string>447700900123@s.whatsapp.net</string>
</dict></plist>"#,
        )
        .unwrap();
        assert_eq!(
            owner_from_backup(&[dir.path().to_path_buf()], &mut log).as_deref(),
            Some("+447700900123")
        );

        fs::write(
            &prefs,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>OwnJabberID</key><string>15555550122@s.whatsapp.net</string>
  <key>LastOwnJabberID</key><string>447700900123@s.whatsapp.net</string>
</dict></plist>"#,
        )
        .unwrap();
        assert_eq!(
            owner_from_backup(&[dir.path().to_path_buf()], &mut log).as_deref(),
            Some("+15555550122")
        );

        // Neither key: a Business-app backup or a moved key. The caller
        // falls back to the form.
        fs::write(
            &prefs,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>OwnPhoneNumber</key><string>AAAAAAAAAAAAAAAAAAAAAA==</string>
</dict></plist>"#,
        )
        .unwrap();
        assert_eq!(
            owner_from_backup(&[dir.path().to_path_buf()], &mut log),
            None
        );
        assert!(log.is_empty(), "{log:?}");
    }

    /// A plist that is not a plist is logged and counts as absent rather
    /// than stopping the import: the form's number covers it.
    #[test]
    fn an_unreadable_plist_is_logged_and_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let prefs = dir.path().join(PREFERENCE_PLISTS[0]);
        fs::create_dir_all(prefs.parent().unwrap()).unwrap();
        fs::write(&prefs, b"not a plist").unwrap();
        let mut log = Vec::new();
        assert_eq!(
            owner_from_backup(&[dir.path().to_path_buf()], &mut log),
            None
        );
        assert_eq!(log.len(), 1, "{log:?}");
        assert!(log[0].starts_with("could not read "), "{log:?}");
    }

    /// The form value is stored under the vault's handle key, so a number
    /// typed with spaces matches the profile phone it was filled from.
    #[test]
    fn the_form_number_is_normalized_like_a_vault_handle() {
        assert_eq!(owner_from_form("+1 555 555 0100").unwrap(), "+15555550100");
        assert_eq!(owner_from_form("(555) 555-0100").unwrap(), "+15555550100");
        assert!(owner_from_form("abc").is_err());
    }
}
