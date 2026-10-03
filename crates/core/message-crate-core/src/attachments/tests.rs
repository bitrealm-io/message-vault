//! Content-addressed naming.
//!
//! This module had no tests at all, which mutation testing showed plainly:
//! every function in it could be replaced with a constant — `digest_prefix`
//! with `""`, `attachment_dest_name` with `"xyzzy"` — and the whole
//! workspace suite stayed green.
//!
//! What breaks if it does: the destination name is what makes an attachment
//! content-addressed, so two exports of the same photo land on the same
//! file. A constant name collapses every attachment in an export onto one
//! file.

use super::*;

/// A full SHA-256 digest, long enough to be shortened.
const DIGEST: &str = "4d1d2c17461355ae828fa0f1510e009e179d6b8e54a71d7179000934bd15a1ec";

#[test]
fn the_prefix_is_the_first_sixteen_hex_digits() {
    assert_eq!(digest_prefix(DIGEST), "4d1d2c17461355ae");
    assert_eq!(digest_prefix(DIGEST).len(), 16);
}

/// A short or empty digest must not panic on the slice. Attachments whose
/// bytes could not be read carry no digest, and the naming path still runs.
#[test]
fn a_short_digest_is_taken_whole_rather_than_panicking() {
    assert_eq!(digest_prefix("abc"), "abc");
    assert_eq!(digest_prefix(""), "");
    assert_eq!(digest_prefix("0123456789abcdef"), "0123456789abcdef");
    assert_eq!(digest_prefix("0123456789abcdef0"), "0123456789abcdef");
}

/// The name is `{UTC date}-{digest16}{ext}`, and each part has to be there:
/// the date so a folder listing reads chronologically, the digest so two
/// copies of one file share a name, the extension so the operating system
/// opens it.
#[test]
fn the_destination_name_carries_the_date_the_digest_and_the_extension() {
    // 2014-05-22 15:41:01 UTC, rendered in UTC whatever zone the machine is in.
    let name = attachment_dest_name(1_400_773_261, DIGEST, ".jpg");
    assert_eq!(name, "20140522_154101-4d1d2c17461355ae.jpg");

    // Two attachments with the same bytes at the same moment share a name;
    // different bytes do not.
    let same = attachment_dest_name(1_400_773_261, DIGEST, ".jpg");
    assert_eq!(name, same, "content addressing means one name per digest");
    let other = attachment_dest_name(1_400_773_261, "ffffffffffffffffff", ".jpg");
    assert_ne!(name, other);

    // No extension is a name without one, not a trailing dot.
    let bare = attachment_dest_name(1_400_773_261, DIGEST, "");
    assert!(bare.ends_with("-4d1d2c17461355ae"), "got {bare}");
}

/// One second before midnight UTC on 2024-03-15. Any zone east of Greenwich
/// is already on the 16th and any zone west of it is hours earlier, so a
/// name rendered in the host's zone would differ on almost every machine.
const JUST_BEFORE_MIDNIGHT_UTC: i64 = 1_710_547_199;

/// Prints the name for the fixed instant above. It is a test only so the
/// zone check below can run it in a fresh process under a chosen `TZ`; on
/// its own it does nothing. A fresh process is needed because chrono caches
/// the zone it read from `TZ` for a second, so a value set from inside a
/// running test would not be seen by a host-zone rendering, and the check
/// would pass against the very bug it guards.
#[test]
fn print_the_name_for_the_zone_check() {
    if std::env::var_os("MC_ZONE_CHECK").is_none() {
        return;
    }
    println!(
        "NAME={}",
        attachment_dest_name(JUST_BEFORE_MIDNIGHT_UTC, DIGEST, ".jpg")
    );
}

/// The name is an identifier: the same attachment gets the same name on every
/// machine that exports it. Running the naming under two zones on opposite
/// sides of the date line, for an instant a second before midnight UTC, must
/// give one name, and that name is the UTC rendering.
#[test]
fn the_name_does_not_depend_on_the_machines_zone() {
    let exe = std::env::current_exe().expect("the test binary's path");
    let name_under = |tz: &str| {
        let out = std::process::Command::new(&exe)
            .args(["print_the_name_for_the_zone_check", "--nocapture"])
            .env("TZ", tz)
            .env("MC_ZONE_CHECK", "1")
            .output()
            .expect("run the test binary");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        stdout
            .lines()
            .find_map(|line| line.strip_prefix("NAME="))
            .unwrap_or_else(|| panic!("no NAME= line in:\n{stdout}"))
            .to_string()
    };

    let honolulu = name_under("Pacific/Honolulu");
    let auckland = name_under("Pacific/Auckland");

    assert_eq!(honolulu, auckland, "the name changed with the zone");
    assert_eq!(honolulu, "20240315_235959-4d1d2c17461355ae.jpg");
}

/// A timestamp the calendar cannot represent falls back to the raw seconds
/// rather than producing an empty name, which would collapse those
/// attachments onto one file.
#[test]
fn an_unrepresentable_timestamp_falls_back_to_its_seconds() {
    let name = attachment_dest_name(i64::MIN, DIGEST, ".jpg");
    assert!(name.starts_with(&i64::MIN.to_string()), "got {name}");
    assert!(name.ends_with("-4d1d2c17461355ae.jpg"), "got {name}");
}
