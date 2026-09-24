//! Read which addresses a backup's device sent from, before any parsing.
//!
//! The Import screen's identity check calls [`backup_identities`] right after
//! the user starts an iMessage import, before the import session is created.
//! The values come from the `imessage-reader` program, which opens the
//! source the same way the real run does, so every method (Mac `chat.db`,
//! iPhone backup folder, jailbreak `sms.db`) and both encryption states go
//! through one code path. The program strips Apple's `P:`, `E:` and `tel:`
//! prefixes before the values cross the pipe
//! (`imessage_reader_protocol::bare_address`), the same rule it applies to a
//! message's owner address, so an identity here and a sender handle in the
//! export are spelled alike. Deduplication happens here.

use std::{collections::HashSet, fs::File, path::Path};

use anyhow::bail;
use imessage_reader_protocol::{Event, Platform, Request, Source};

use crate::helper::Helper;

/// `Info.plist` → `Phone Number` from an iOS backup folder.
///
/// Returns `None` when the file is missing or cannot be parsed. `Info.plist`
/// is plaintext even in an encrypted backup.
pub fn ios_backup_phone_number(backup_root: &Path) -> Option<String> {
    let file = File::open(backup_root.join("Info.plist")).ok()?;
    let value = plist::Value::from_reader(file).ok()?;
    let dict = value.as_dictionary()?;
    match dict.get("Phone Number") {
        Some(plist::Value::String(number)) => Some(number.clone()),
        _ => None,
    }
}

/// Addresses the backup's device sent from: the union of
/// `chat.account_login`, `message.destination_caller_id`, and (for iOS
/// backups) `Info.plist` → `Phone Number`, deduplicated.
///
/// # Errors
///
/// Returns an error when the source cannot be opened: missing database,
/// missing or wrong backup password, not an iPhone backup, or no
/// `imessage-reader` program to open it with.
pub fn backup_identities(
    db_path: &Path,
    ios: bool,
    backup_password: Option<&str>,
) -> anyhow::Result<Vec<String>> {
    let request = Request::Identities(Source {
        db_path: db_path.to_path_buf(),
        platform: if ios { Platform::Ios } else { Platform::MacOs },
        backup_password: backup_password.map(str::to_string),
    });
    let helper = Helper::spawn(&request, None, None)?;
    let mut values = read_identities(helper)?;
    if ios {
        values.extend(ios_backup_phone_number(db_path));
    }
    Ok(dedupe(values))
}

/// The values a started program answers the identities request with.
/// [`Helper::next_event`] refuses a program on another protocol version, and
/// one that answers before its [`Event::Source`] line.
fn read_identities(mut helper: Helper) -> anyhow::Result<Vec<String>> {
    let raw = loop {
        match helper.next_event()? {
            Event::Source { .. } => {}
            Event::Identities { values } => break values,
            other => bail!("expected the identities answer, got {other:?}"),
        }
    };
    helper.finish()?;
    Ok(raw)
}

/// Keep the first spelling of each address and drop what has no digits or
/// letters to compare (the `Info.plist` number can be blank).
fn dedupe(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut identities = Vec::new();
    for value in values {
        let value = value.trim().to_string();
        let key = identity_key(&value);
        if key.is_empty() || !seen.insert(key) {
            continue;
        }
        identities.push(value);
    }
    identities
}

/// Deduplication key: emails lowercased, phones as US national digits
/// (matching `toUsNationalDigits` in the web app and the vault's
/// `sanitize_number`).
fn identity_key(value: &str) -> String {
    if value.contains('@') {
        return value.to_ascii_lowercase();
    }
    let mut digits: String = value.chars().filter(char::is_ascii_digit).collect();
    if digits.len() == 11 && digits.starts_with('1') {
        digits.remove(0);
    }
    digits
}

#[cfg(test)]
mod tests {
    use super::{dedupe, identity_key, ios_backup_phone_number};

    #[test]
    fn identity_key_normalizes_phones_and_emails() {
        assert_eq!(identity_key("+1 (555) 000-1111"), "5550001111");
        assert_eq!(identity_key("5550001111"), "5550001111");
        assert_eq!(identity_key("Owner@Example.com"), "owner@example.com");
    }

    #[test]
    fn dedupe_keeps_the_first_spelling_and_drops_blanks() {
        let values = vec![
            "+1 (555) 000-1111".to_string(),
            "Owner@Example.com".to_string(),
            "+15550001111".to_string(),
            " ".to_string(),
            "owner@example.com".to_string(),
        ];
        let mut identities = dedupe(values);
        identities.sort();
        assert_eq!(
            identities,
            vec![
                "+1 (555) 000-1111".to_string(),
                "Owner@Example.com".to_string()
            ]
        );
    }

    #[test]
    fn info_plist_phone_number_reads_string() {
        let dir = tempfile::tempdir().unwrap();
        let body = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Phone Number</key>
  <string>+1 (555) 000-1111</string>
</dict>
</plist>
"#;
        std::fs::write(dir.path().join("Info.plist"), body).unwrap();
        assert_eq!(
            ios_backup_phone_number(dir.path()),
            Some("+1 (555) 000-1111".to_string())
        );

        let missing = tempfile::tempdir().unwrap();
        assert_eq!(ios_backup_phone_number(missing.path()), None);
    }

    #[cfg(unix)]
    mod through_a_fake_helper {
        use imessage_reader_protocol::{PROTOCOL_VERSION, Platform, Request, Source};

        use super::super::read_identities;
        use crate::helper::tests::{fake_helper, source_line, spawn_fake};

        fn request() -> Request {
            Request::Identities(Source {
                db_path: "/nowhere/chat.db".into(),
                platform: Platform::MacOs,
                backup_password: None,
            })
        }

        const ANSWER: &str = r#"echo '{"event":"identities","values":["+15550001111"]}'"#;

        #[test]
        fn the_answer_follows_the_source_event() {
            let dir = tempfile::tempdir().unwrap();
            let body = format!("{}\n{ANSWER}", source_line(PROTOCOL_VERSION));
            let helper = spawn_fake(&fake_helper(dir.path(), &body), &request());

            assert_eq!(read_identities(helper).unwrap(), vec!["+15550001111"]);
        }

        #[test]
        fn a_helper_on_another_protocol_version_is_refused() {
            let dir = tempfile::tempdir().unwrap();
            let body = format!("{}\n{ANSWER}", source_line(PROTOCOL_VERSION + 1));
            let helper = spawn_fake(&fake_helper(dir.path(), &body), &request());

            let err = read_identities(helper).unwrap_err().to_string();
            assert_eq!(
                err,
                format!(
                    "imessage-reader speaks protocol version {}, this app speaks \
                     {PROTOCOL_VERSION}; the two were not built together",
                    PROTOCOL_VERSION + 1
                )
            );
        }

        #[test]
        fn a_helper_that_skips_the_source_event_is_refused() {
            let dir = tempfile::tempdir().unwrap();
            let helper = spawn_fake(&fake_helper(dir.path(), ANSWER), &request());

            let err = read_identities(helper).unwrap_err().to_string();
            assert!(
                err.starts_with(
                    "imessage-reader answered without saying which protocol version it speaks"
                ),
                "{err}"
            );
        }
    }
}
