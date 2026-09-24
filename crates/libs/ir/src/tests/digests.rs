//! Known answers for the two hashes the rest of the product keys on.
//!
//! `file_sha256` is behind every digest check and asset key in push, staging
//! and transcode. `stable_guid` is every message's identity. A change to
//! either silently re-keys everything already in a vault, so each is pinned
//! to a value computed outside Rust (Python's `hashlib`).

use crate::{file_sha256, stable_guid};

/// A file in the temp directory, removed when dropped.
struct TempFile(std::path::PathBuf);

impl TempFile {
    fn with(name: &str, bytes: &[u8]) -> Self {
        let path = std::env::temp_dir().join(format!("message-ir-{}-{name}", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        Self(path)
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn file_sha256_of_an_empty_file() {
    let file = TempFile::with("empty", b"");
    assert_eq!(
        file_sha256(&file.0).unwrap(),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn file_sha256_of_a_file_that_spans_several_reads() {
    // 200,000 bytes is three full 64 KiB reads and a partial fourth.
    let bytes: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let file = TempFile::with("large", &bytes);
    assert_eq!(
        file_sha256(&file.0).unwrap(),
        "e24bc62381f1224fbbb74688663f8f9743b9680b193edd666835e97b06e730eb"
    );
}

#[test]
fn file_sha256_names_a_missing_file() {
    let path = std::env::temp_dir().join("message-ir-no-such-file");
    let err = file_sha256(&path).unwrap_err();
    assert!(err.to_string().contains("message-ir-no-such-file"), "{err}");
}

#[test]
fn stable_guid_known_answers() {
    assert_eq!(
        stable_guid("+15555550101", "2021-01-01T00:00:00Z", false, "hello", &[]),
        "07e5d3fd8d1c7e45808baa87dc9bd38f777e5880acb7001a9bd17b7319d87a54"
    );
    assert_eq!(
        stable_guid(
            "+15555550101",
            "2021-01-01T00:00:00Z",
            true,
            "hello",
            &["a".repeat(64)]
        ),
        "7c4cbca9c6dc661fd3090f2089dca5358cc5d1065ab385380ca305a4e5c9c9b1"
    );
}

#[test]
fn stable_guid_tells_apart_messages_that_differ_only_in_attachment() {
    // Two photo-only messages sent in the same second.
    let guid = |digest: &str| {
        stable_guid(
            "+15555550101",
            "2021-01-01T00:00:00Z",
            true,
            "",
            &[digest.to_string()],
        )
    };
    assert_ne!(guid(&"a".repeat(64)), guid(&"b".repeat(64)));
    assert_eq!(guid(&"a".repeat(64)), guid(&"a".repeat(64)));
}
