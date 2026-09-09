//! Content-addressed naming, and the two writes that must not overwrite.
//!
//! This module had no tests at all, which mutation testing showed plainly:
//! every function in it could be replaced with a constant — `digest_prefix`
//! with `""`, `attachment_dest_name` with `"xyzzy"`, `write_if_missing` and
//! `copy_if_missing` with `Ok(true)` or `Ok(false)` — and the whole workspace
//! suite stayed green.
//!
//! What breaks if it does: the destination name is what makes an attachment
//! content-addressed, so two exports of the same photo land on the same file
//! and neither is written twice. A constant name collapses every attachment in
//! an export onto one file; a `write_if_missing` that always writes overwrites
//! a file another conversation is already using.

use super::*;

/// A digest long enough to be shortened, and one that is not.
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

/// The name is `{local date}-{digest16}{ext}`, and each part has to be there:
/// the date so a folder listing reads chronologically, the digest so two
/// copies of one file share a name, the extension so the operating system
/// opens it.
#[test]
fn the_destination_name_carries_the_date_the_digest_and_the_extension() {
    // 2014-05-22 11:41:01 UTC. The date part is local time, so this asserts
    // the pieces rather than a fixed string, which would fail in another zone.
    let name = attachment_dest_name(1_400_773_261, DIGEST, ".jpg");

    assert!(name.ends_with("-4d1d2c17461355ae.jpg"), "got {name}");
    let date = name.split('-').next().expect("a date part");
    assert_eq!(date.len(), 15, "YYYYMMDD_HHMMSS, got {date}");
    assert!(date.contains('_'), "got {date}");
    assert!(
        date.chars().all(|c| c.is_ascii_digit() || c == '_'),
        "got {date}"
    );

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

/// A timestamp the calendar cannot represent falls back to the raw seconds
/// rather than producing an empty name, which would collapse those
/// attachments onto one file.
#[test]
fn an_unrepresentable_timestamp_falls_back_to_its_seconds() {
    let name = attachment_dest_name(i64::MIN, DIGEST, ".jpg");
    assert!(name.starts_with(&i64::MIN.to_string()), "got {name}");
    assert!(name.ends_with("-4d1d2c17461355ae.jpg"), "got {name}");
}

#[test]
fn writing_is_skipped_when_the_file_is_already_there() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("a.bin");

    assert!(write_if_missing(&path, b"first").expect("write"), "written");
    assert_eq!(std::fs::read(&path).expect("read"), b"first");

    assert!(
        !write_if_missing(&path, b"second").expect("write"),
        "the second write must be skipped, not performed"
    );
    assert_eq!(
        std::fs::read(&path).expect("read"),
        b"first",
        "the file another conversation is using must survive"
    );
}

#[test]
fn copying_is_skipped_when_the_destination_is_already_there() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("src.bin");
    let dest = dir.path().join("dest.bin");
    std::fs::write(&src, b"source bytes").expect("write source");

    assert!(copy_if_missing(&src, &dest).expect("copy"), "copied");
    assert_eq!(std::fs::read(&dest).expect("read"), b"source bytes");

    std::fs::write(&src, b"changed source").expect("rewrite source");
    assert!(
        !copy_if_missing(&src, &dest).expect("copy"),
        "the second copy must be skipped"
    );
    assert_eq!(
        std::fs::read(&dest).expect("read"),
        b"source bytes",
        "the destination must not be replaced"
    );
}

/// A missing source is an error naming both paths, not a silent `false` that
/// would leave an attachment recorded but absent.
#[test]
fn copying_a_missing_source_names_both_paths() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("gone.bin");
    let dest = dir.path().join("dest.bin");

    let err = copy_if_missing(&src, &dest).expect_err("a missing source is an error");
    let text = format!("{err:#}");
    assert!(text.contains("gone.bin"), "{text}");
    assert!(text.contains("dest.bin"), "{text}");
}
