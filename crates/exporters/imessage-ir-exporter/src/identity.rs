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
use imessage_reader_protocol::{Event, IdentitiesRequest, Platform, Request, Source};

use crate::{helper::Helper, scratch::ScratchDir};

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
/// An encrypted backup's databases are decrypted into a folder of this
/// request's own under `scratch_root`, a folder the app owns. The folder is
/// deleted when the request ends, and what a killed request left under
/// `scratch_root` is deleted before this one starts ([`ScratchDir`]).
///
/// # Errors
///
/// Returns an error when the source cannot be opened: missing database,
/// missing or wrong backup password, not an iPhone backup, or no
/// `imessage-reader` program to open it with. Also when the scratch folder
/// cannot be made.
pub fn backup_identities(
    db_path: &Path,
    ios: bool,
    backup_password: Option<&str>,
    scratch_root: &Path,
) -> anyhow::Result<Vec<String>> {
    identities_with(db_path, ios, backup_password, scratch_root, |request| {
        Helper::spawn(request, None, None)
    })
}

/// [`backup_identities`], starting the program with `spawn`. Tests pass a
/// fake program.
fn identities_with(
    db_path: &Path,
    ios: bool,
    backup_password: Option<&str>,
    scratch_root: &Path,
    spawn: impl FnOnce(&Request) -> anyhow::Result<Helper>,
) -> anyhow::Result<Vec<String>> {
    let scratch = ScratchDir::create(scratch_root)?;
    let request = Request::Identities(IdentitiesRequest {
        source: Source {
            db_path: db_path.to_path_buf(),
            platform: if ios { Platform::Ios } else { Platform::MacOs },
            backup_password: backup_password.map(str::to_string),
        },
        scratch_dir: scratch.path().to_path_buf(),
    });
    // The program is stopped (by `finish`, or by its drop on an error)
    // before the scratch folder is deleted.
    let mut values = read_identities(spawn(&request)?)?;
    drop(scratch);
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
/// (matching `toUsNationalDigits` in the web app and the server's
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
        use std::{fs, path::Path};

        use imessage_reader_protocol::{
            IdentitiesRequest, PROTOCOL_VERSION, Platform, Request, Source,
        };

        use super::super::{identities_with, read_identities};
        use crate::helper::tests::{fake_helper, source_line, spawn_fake};

        fn request() -> Request {
            Request::Identities(IdentitiesRequest {
                source: Source {
                    db_path: "/nowhere/chat.db".into(),
                    platform: Platform::MacOs,
                    backup_password: None,
                },
                scratch_dir: "/nowhere/scratch".into(),
            })
        }

        const ANSWER: &str = r#"echo '{"event":"identities","values":["+15550001111"]}'"#;

        /// Shell lines that keep the request in `<dir>/request.json` and
        /// write a decrypted database into the scratch folder it names, as
        /// the real program does for an encrypted backup.
        fn decrypt_into_scratch(dir: &Path) -> String {
            format!(
                r#"printf '%s' "$request" > '{}/request.json'
scratch=$(printf '%s' "$request" | sed 's/.*"scratch_dir":"\([^"]*\)".*/\1/')
echo decrypted > "$scratch/crabapple-sms-x.db""#,
                dir.display()
            )
        }

        /// The names under `root`, sorted, without the root's lock file.
        fn left_in(root: &Path) -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(root)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .filter(|name| name != ".lock")
                .collect();
            names.sort();
            names
        }

        /// The identities request names a scratch folder of its own under
        /// the root the app gave, and once the answer is in, nothing the
        /// program decrypted is left there (#1135).
        #[test]
        fn the_request_carries_a_scratch_folder_that_is_deleted_after() {
            let dir = tempfile::tempdir().unwrap();
            let root = tempfile::tempdir().unwrap();
            let body = format!(
                "{}\n{}\n{ANSWER}",
                decrypt_into_scratch(dir.path()),
                source_line(PROTOCOL_VERSION)
            );
            let script = fake_helper(dir.path(), &body);

            let values = identities_with(
                Path::new("/nowhere/chat.db"),
                false,
                None,
                root.path(),
                |request| Ok(spawn_fake(&script, request)),
            )
            .unwrap();
            assert_eq!(values, vec!["+15550001111"]);

            let sent = fs::read_to_string(dir.path().join("request.json")).unwrap();
            let Ok(Request::Identities(sent)) = serde_json::from_str::<Request>(&sent) else {
                panic!("the program was sent an identities request: {sent}");
            };
            assert_eq!(sent.scratch_dir.parent(), Some(root.path()));
            assert!(
                left_in(root.path()).is_empty(),
                "{:?}",
                left_in(root.path())
            );
        }

        /// A program killed while it decrypts leaves its database in the
        /// scratch folder, and the app deletes the folder all the same.
        #[test]
        fn a_killed_program_leaves_no_decrypted_database() {
            let dir = tempfile::tempdir().unwrap();
            let root = tempfile::tempdir().unwrap();
            let body = format!("{}\nkill -9 $$", decrypt_into_scratch(dir.path()));
            let script = fake_helper(dir.path(), &body);

            identities_with(
                Path::new("/nowhere/chat.db"),
                false,
                None,
                root.path(),
                |request| Ok(spawn_fake(&script, request)),
            )
            .unwrap_err();
            assert!(
                left_in(root.path()).is_empty(),
                "{:?}",
                left_in(root.path())
            );
        }

        /// A decrypted database left by a request whose app was killed is
        /// deleted when the next request starts.
        #[test]
        fn a_leftover_database_is_deleted_at_the_next_request() {
            let dir = tempfile::tempdir().unwrap();
            let root = tempfile::tempdir().unwrap();
            let earlier = root.path().join("request-earlier");
            fs::create_dir(&earlier).unwrap();
            fs::write(earlier.join(".lock"), b"").unwrap();
            fs::write(earlier.join("crabapple-sms-x.db"), b"decrypted").unwrap();
            let body = format!("{}\n{ANSWER}", source_line(PROTOCOL_VERSION));
            let script = fake_helper(dir.path(), &body);

            identities_with(
                Path::new("/nowhere/chat.db"),
                false,
                None,
                root.path(),
                |request| Ok(spawn_fake(&script, request)),
            )
            .unwrap();
            assert!(
                left_in(root.path()).is_empty(),
                "{:?}",
                left_in(root.path())
            );
        }

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
