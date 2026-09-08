use super::*;
use std::io::Write;
use std::process::Command;
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};
use tempfile::tempdir;

fn files_named_with_sha(root: &Path, sha: &str) -> Vec<std::fs::DirEntry> {
    let shard = root.join(&sha[..2]);
    let mut installed = Vec::new();
    for entry in fs::read_dir(shard).unwrap() {
        let entry = entry.unwrap();
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if name.starts_with(sha) {
            installed.push(entry);
        }
    }
    installed
}

#[test]
fn store_verified_replaces_corrupt_destination() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let source = root.join("source.bin");
    fs::write(&source, b"valid-asset").unwrap();
    let sha = hash_file(&source).unwrap();
    let destination = root.join(shard_rel_path(&sha, ".bin"));
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    fs::write(&destination, b"corrupt").unwrap();

    let (stored, already_present) =
        store_verified(&source, &sha, root, None, false, false).unwrap();

    assert!(!already_present);
    assert_eq!(
        fs::read(root.join(stored.assets_path)).unwrap(),
        b"valid-asset"
    );
}

#[test]
fn store_verified_concurrent_installers_leave_valid_destination() {
    let dir = tempdir().unwrap();
    let root = Arc::new(dir.path().to_path_buf());
    let source_a = root.join("source-a.bin");
    let source_b = root.join("source-b.dat");
    fs::write(&source_a, b"shared-asset").unwrap();
    fs::write(&source_b, b"shared-asset").unwrap();
    let sha = hash_file(&source_a).unwrap();
    let barrier = Arc::new(Barrier::new(2));

    let desired_path = root.join(shard_rel_path(&sha, ""));
    let installers: Vec<_> = [source_a, source_b]
        .into_iter()
        .enumerate()
        .map(|(index, source)| {
            let root = Arc::clone(&root);
            let sha = sha.clone();
            let barrier = Arc::clone(&barrier);
            let desired_path = desired_path.clone();
            std::thread::spawn(move || {
                store_verified_inner(
                    &source,
                    &sha,
                    &root,
                    None,
                    false,
                    || {},
                    || {
                        barrier.wait();
                        if index == 1 {
                            let deadline = Instant::now() + Duration::from_secs(5);
                            while !desired_path.is_file() {
                                assert!(
                                    Instant::now() < deadline,
                                    "timed out waiting for winning installer"
                                );
                                std::thread::sleep(Duration::from_millis(1));
                            }
                        }
                    },
                )
            })
        })
        .collect();

    let mut results = Vec::new();
    for installer in installers {
        results.push(installer.join().unwrap().unwrap());
    }
    let newly_stored = results.iter().filter(|(_, present)| !present).count();
    assert_eq!(newly_stored, 1);
    assert_eq!(results[0].0.assets_path, results[1].0.assets_path);
    let installed = files_named_with_sha(root.as_path(), &sha);
    assert_eq!(installed.len(), 1);
    assert_eq!(
        fs::read(root.join(&results[0].0.assets_path)).unwrap(),
        b"shared-asset"
    );
}

#[test]
fn store_verified_processes_share_one_mixed_extension_path() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let source_a = root.join("process-a.bin");
    let source_b = root.join("process-b.dat");
    fs::write(&source_a, b"cross-process-asset").unwrap();
    fs::write(&source_b, b"cross-process-asset").unwrap();
    let sha = hash_file(&source_a).unwrap();
    let test_binary = std::env::current_exe().unwrap();

    let children: Vec<_> = [("a", source_a), ("b", source_b)]
        .into_iter()
        .map(|(worker, source)| {
            Command::new(&test_binary)
                .args([
                    "--ignored",
                    "--exact",
                    "assets::tests::filesystem_install_worker",
                    "--nocapture",
                ])
                .env("ASSET_TEST_ROOT", root)
                .env("ASSET_TEST_SOURCE", source)
                .env("ASSET_TEST_SHA", &sha)
                .env("ASSET_TEST_WORKER", worker)
                .spawn()
                .unwrap()
        })
        .collect();

    for mut child in children {
        assert!(child.wait().unwrap().success());
    }

    let result_a = fs::read_to_string(root.join("result-a")).unwrap();
    let result_b = fs::read_to_string(root.join("result-b")).unwrap();
    assert_eq!(result_a, result_b);
    assert!(Path::new(&result_a).extension().is_none());
    let installed = files_named_with_sha(root, &sha);
    assert_eq!(installed.len(), 1);
    assert_eq!(
        fs::read(root.join(result_a)).unwrap(),
        b"cross-process-asset"
    );
}

#[test]
fn lookup_by_sha256_keeps_legacy_extension_paths_compatible() {
    let dir = tempdir().unwrap();
    let sha = sha256_hex(b"legacy-jpeg");
    let legacy = dir.path().join(shard_rel_path(&sha, ".jpg"));
    fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    fs::write(&legacy, b"legacy-jpeg").unwrap();

    let stored = lookup_by_sha256(dir.path(), &sha).unwrap();

    assert_eq!(stored.assets_path, shard_rel_path(&sha, ".jpg"));
    assert_eq!(stored.mime_type.as_deref(), Some("image/jpeg"));
}

#[test]
fn guess_mime_covers_phone_media_extensions() {
    for (ext, expected) in [
        ("amr", "audio/amr"),
        ("wav", "audio/wav"),
        ("ogg", "audio/ogg"),
        ("3gp", "video/3gpp"),
        ("3gpp", "video/3gpp"),
        ("webm", "video/webm"),
        ("mkv", "video/x-matroska"),
        ("avi", "video/x-msvideo"),
        ("mpg", "video/mpeg"),
        ("tiff", "image/tiff"),
        ("tif", "image/tiff"),
        ("bmp", "image/bmp"),
    ] {
        assert_eq!(
            guess_mime(Some(ext)).as_deref(),
            Some(expected),
            "unexpected MIME for .{ext}"
        );
    }
}

#[test]
fn store_verified_records_mime_for_extensionless_media_blobs() {
    let dir = tempdir().unwrap();
    for (name, expected) in [
        ("voice.amr", "audio/amr"),
        ("memo.wav", "audio/wav"),
        ("clip.3gp", "video/3gpp"),
        ("scan.tiff", "image/tiff"),
    ] {
        let source = dir.path().join(name);
        fs::write(&source, name.as_bytes()).unwrap();
        let sha = sha256_hex(name.as_bytes());

        let (stored, _) = store_verified(&source, &sha, dir.path(), None, false, false).unwrap();

        assert!(Path::new(&stored.assets_path).extension().is_none());
        assert_eq!(stored.mime_type.as_deref(), Some(expected));
        // The fingerprint-only path has no extension, so serving relies on
        // the MIME file written next to the stored attachment.
        assert_eq!(
            lookup_by_sha256(dir.path(), &sha)
                .unwrap()
                .mime_type
                .as_deref(),
            Some(expected)
        );
        assert_eq!(
            lookup_by_sha256_unverified(dir.path(), &sha)
                .unwrap()
                .mime_type
                .as_deref(),
            Some(expected)
        );
    }
}

#[test]
fn lookup_by_sha256_preserves_mime_for_extensionless_assets() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.jpg");
    fs::write(&source, b"new-jpeg").unwrap();
    let sha = sha256_hex(b"new-jpeg");

    let (stored, _) = store_verified(&source, &sha, dir.path(), None, false, false).unwrap();
    let looked_up = lookup_by_sha256(dir.path(), &sha).unwrap();

    assert!(Path::new(&stored.assets_path).extension().is_none());
    assert_eq!(looked_up.mime_type.as_deref(), Some("image/jpeg"));
}

#[test]
#[ignore = "helper launched by store_verified_processes_share_one_mixed_extension_path"]
fn filesystem_install_worker() {
    let root = PathBuf::from(std::env::var_os("ASSET_TEST_ROOT").unwrap());
    let source = PathBuf::from(std::env::var_os("ASSET_TEST_SOURCE").unwrap());
    let sha = std::env::var("ASSET_TEST_SHA").unwrap();
    let worker = std::env::var("ASSET_TEST_WORKER").unwrap();

    let (stored, _) = store_verified_inner(
        &source,
        &sha,
        &root,
        None,
        false,
        || {},
        || {
            fs::write(root.join(format!("ready-{worker}")), b"ready").unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            while !(root.join("ready-a").is_file() && root.join("ready-b").is_file()) {
                assert!(
                    Instant::now() < deadline,
                    "timed out waiting for peer installer"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        },
    )
    .unwrap();
    fs::write(root.join(format!("result-{worker}")), stored.assets_path).unwrap();
}

#[test]
fn store_verified_skips_temp_copy_on_valid_dedup() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let source = root.join("dedup.bin");
    fs::write(&source, b"dedup-asset").unwrap();
    let sha = sha256_hex(b"dedup-asset");

    let (first, present) = store_verified(&source, &sha, root, None, false, false).unwrap();
    assert!(!present);

    let copied = std::cell::Cell::new(false);
    let (second, present) =
        store_verified_inner(&source, &sha, root, None, false, || copied.set(true), || {}).unwrap();

    assert!(present);
    assert_eq!(second.assets_path, first.assets_path);
    assert!(
        !copied.get(),
        "storing over a valid destination must not copy the source into a temporary blob"
    );
}

#[test]
fn unverified_lookup_reads_no_content_while_verified_lookup_rejects_corruption() {
    let dir = tempdir().unwrap();
    let sha = sha256_hex(b"expected-bytes");
    let stored_path = dir.path().join(shard_rel_path(&sha, ""));
    fs::create_dir_all(stored_path.parent().unwrap()).unwrap();
    fs::write(&stored_path, b"corrupt-bytes").unwrap();

    let unverified = lookup_by_sha256_unverified(dir.path(), &sha)
        .expect("path lookup must not depend on file contents");
    assert_eq!(unverified.assets_path, shard_rel_path(&sha, ""));
    assert!(
        lookup_by_sha256(dir.path(), &sha).is_none(),
        "a file whose bytes do not match its fingerprint must not be reported as present"
    );
}

#[test]
fn store_verified_hashes_source_before_deduplication() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let mut src = tempfile::NamedTempFile::new().unwrap();
    src.write_all(b"hello-asset").unwrap();
    src.flush().unwrap();

    let sha = hash_file(src.path()).unwrap();
    let (first, present) =
        store_verified(src.path(), &sha, root, Some("text/plain"), false, false).unwrap();
    assert!(!present);
    assert_eq!(first.sha256, sha);
    assert!(src.path().is_file(), "non-consuming store must keep source");

    // A duplicate claim with different bytes must fail even when the valid
    // destination already exists.
    let mut other = tempfile::NamedTempFile::new().unwrap();
    other.write_all(b"different-bytes").unwrap();
    other.flush().unwrap();
    let err =
        store_verified(other.path(), &sha, root, Some("text/plain"), false, false).unwrap_err();
    assert!(err.to_string().contains("sha256 mismatch"));
    assert_eq!(
        fs::read(root.join(first.assets_path)).unwrap(),
        b"hello-asset"
    );
    assert!(lookup_by_sha256(root, &sha).is_some());
}

#[test]
fn store_verified_persists_the_bytes_that_were_hashed() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let source = root.join("mutable.bin");
    fs::write(&source, b"verified-bytes").unwrap();
    let sha = sha256_hex(b"verified-bytes");

    let (stored, present) = store_verified_inner(
        &source,
        &sha,
        root,
        None,
        false,
        || fs::write(&source, b"mutated-after-copy").unwrap(),
        || {},
    )
    .unwrap();

    assert!(!present);
    assert_eq!(
        fs::read(root.join(stored.assets_path)).unwrap(),
        b"verified-bytes"
    );
    assert_eq!(fs::read(source).unwrap(), b"mutated-after-copy");
}

#[test]
fn store_verified_renames_same_filesystem_temp() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let incoming = root.join(".incoming");
    fs::create_dir_all(&incoming).unwrap();
    let tmp = incoming.join("upload.part");
    fs::write(&tmp, b"rename-me").unwrap();
    let sha = hash_file(&tmp).unwrap();

    let (stored, present) = store_verified(
        &tmp,
        &sha,
        root,
        Some("application/octet-stream"),
        true,
        false,
    )
    .unwrap();
    assert!(!present);
    assert!(!tmp.exists(), "rename should consume the temp file");
    assert!(root.join(&stored.assets_path).is_file());
    assert_eq!(
        fs::read(root.join(&stored.assets_path)).unwrap(),
        b"rename-me"
    );
}

#[test]
fn store_verified_rejects_symlink_source() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let real = dir.path().join("real.bin");
    fs::write(&real, b"payload").unwrap();
    let link = dir.path().join("link.bin");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let sha = hash_file(&real).unwrap();
        let err = store_verified(&link, &sha, root, None, false, false).unwrap_err();
        assert!(
            err.to_string().contains("symlink"),
            "unexpected error: {err}"
        );
    }
}

#[test]
fn gc_stale_incoming_removes_old_sessions() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let session = root.join(".incoming").join("ab").join("deadbeef");
    fs::create_dir_all(&session).unwrap();
    fs::write(session.join("manifest.json"), b"{}").unwrap();
    let removed = gc_stale_incoming(root, 0).unwrap();
    assert_eq!(removed, 1);
    assert!(!session.exists());
}

#[tokio::test]
async fn an_asset_put_then_get_returns_the_same_bytes() {
    let vault = crate::test_support::test_vault().await;
    let user = crate::test_support::register_via_api(&vault.state, "alice", "hunter2hunter2").await;

    // Arbitrary non-UTF-8 bytes, to prove the round trip preserves the
    // raw content rather than only text that happens to decode.
    let bytes: Vec<u8> = vec![0xff, 0x00, 0xde, 0xad, 0xbe, 0xef, b'\n', b'x'];
    let sha = sha256_hex(&bytes);
    let path = format!(
        "/v1/assets/{sha}?source=sms-backup-restore&account={}",
        user.username
    );

    let (status, text) = crate::test_support::put_raw(
        &vault.state,
        &path,
        &user.token,
        "application/octet-stream",
        bytes.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{text}");

    let server = crate::test_support::serve(&vault.state).await;
    let response = reqwest::Client::new()
        .get(format!("{}{path}", server.base()))
        .bearer_auth(&user.token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let got = response.bytes().await.unwrap();
    assert_eq!(got.as_ref(), bytes.as_slice(), "the bytes must round-trip");
}

#[tokio::test]
async fn an_asset_get_for_an_unknown_sha_is_a_json_404() {
    let vault = crate::test_support::test_vault().await;
    let user = crate::test_support::register_via_api(&vault.state, "alice", "hunter2hunter2").await;

    let unknown = "0".repeat(64);
    let (status, text) = crate::test_support::get_raw(
        &vault.state,
        &format!(
            "/v1/assets/{unknown}?source=sms-backup-restore&account={}",
            user.username
        ),
        &user.token,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{text}");
    let body: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|_| panic!("non-JSON body: {text}"));
    assert!(body["detail"].is_string(), "{body}");
}

/// A part body past `upload_limits.part_size` is a 413. This is the one
/// oversize check reachable over HTTP: the layer limit is `max_body_bytes`
/// (512 MiB by default) and the part limit is far smaller, so the handler's
/// own check is what answers. `docs/agents/http-api-rules.md`: the status carries the meaning.
#[tokio::test]
async fn an_upload_part_over_the_part_size_is_a_json_413() {
    let vault = crate::test_support::test_vault().await;
    let mut state = vault.state.clone();
    // `UploadLimits` is `Copy` and `part_size` is public, so a test can lower
    // it without rebuilding the config.
    state.upload_limits.part_size = 16;
    let user = crate::test_support::register_via_api(&state, "alice", "hunter2hunter2").await;

    let sha = "0".repeat(64);
    let (status, text) = crate::test_support::put_raw(
        &state,
        &format!(
            "/v1/assets/{sha}/uploads/upload-1/parts/1?source=sms-backup-restore&account={}",
            user.username
        ),
        &user.token,
        "application/octet-stream",
        vec![b'x'; 4096],
    )
    .await;
    assert_eq!(
        status,
        axum::http::StatusCode::PAYLOAD_TOO_LARGE,
        "a part over part_size must be 413, got: {text}"
    );
    let body: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|_| panic!("non-JSON body: {text}"));
    assert_eq!(
        body["detail"], "request body too large",
        "the sentence must be the handler's own, proving the layer did not answer: {body}"
    );
}
