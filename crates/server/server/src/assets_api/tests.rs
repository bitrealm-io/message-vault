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

/// The fingerprint names a file under the assets directory, so anything that
/// is not exactly 64 hex digits is refused: 64 characters that are not hex
/// could be a path, and a wrong length names no file the server wrote.
#[test]
fn normalize_sha256_takes_only_64_hex_digits() {
    let sha = "a".repeat(64);
    assert_eq!(normalize_sha256(&sha), Some(sha.clone()));
    assert_eq!(
        normalize_sha256(&format!(" {} ", "AB".repeat(32))),
        Some("ab".repeat(32))
    );

    let traversal = format!("../{}", "a".repeat(61));
    assert_eq!(traversal.len(), 64);
    assert_eq!(normalize_sha256(&traversal), None);
    assert_eq!(normalize_sha256(&"g".repeat(64)), None);
    assert_eq!(normalize_sha256(&"a".repeat(63)), None);
    assert_eq!(normalize_sha256(&"a".repeat(65)), None);
    assert_eq!(normalize_sha256(""), None);
}

#[test]
fn store_verified_replaces_corrupt_destination() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let source = root.join("source.bin");
    fs::write(&source, b"valid-asset").unwrap();
    let sha = hash_file(&source).unwrap();
    let destination = root.join(shard_rel_path(&sha, ""));
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
fn store_verified_processes_share_one_path() {
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
                    "assets_api::tests::filesystem_install_worker",
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
#[ignore = "helper launched by store_verified_processes_share_one_path"]
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
    let (fixture, user) = crate::test_support::fixture_with_account().await;

    // Arbitrary non-UTF-8 bytes, to prove the round trip preserves the
    // raw content rather than only text that happens to decode.
    let bytes: Vec<u8> = vec![0xff, 0x00, 0xde, 0xad, 0xbe, 0xef, b'\n', b'x'];
    let sha = sha256_hex(&bytes);
    let path = format!("/v1/assets/{sha}?source=sms-backup-restore");
    let server = crate::test_support::serve(&fixture.state).await;
    let put = |content_type: Option<&str>| {
        let mut request = reqwest::Client::new()
            .put(format!("{}{path}", server.base()))
            .bearer_auth(&user.token)
            .body(bytes.clone());
        if let Some(content_type) = content_type {
            request = request.header(reqwest::header::CONTENT_TYPE, content_type);
        }
        request.send()
    };

    // Raw bytes with no Content-Type say nothing about what they are.
    let response = put(None).await.unwrap();
    let status = response.status();
    let text = response.text().await.unwrap();
    crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::UnsupportedMediaType,
    );

    // The first PUT stores the asset, a creation that names it; the second
    // finds it already held and makes nothing.
    let response = put(Some("application/octet-stream")).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(
        response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok()),
        Some(path.as_str())
    );
    let response = put(Some("application/octet-stream")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let again: serde_json::Value = response.json().await.unwrap();
    assert_eq!(again["already_present"], true);

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

/// Looking at an attachment is not exporting it. A logged-in session reads
/// its own account's attachment with the export permission off; an API token
/// reads one only when it may export.
#[tokio::test]
async fn a_session_reads_an_attachment_without_export_and_a_token_needs_it() {
    let fixture = crate::test_support::test_fixture().await;
    let state = fixture.state.clone();
    let owner =
        crate::test_support::claim_as_owner(&state, "asset-read-keeper", "hunter2hunter2").await;
    let user =
        crate::test_support::register_via_api(&state, "asset-read-user", "hunter2hunter2").await;

    let bytes: Vec<u8> = b"a photo".to_vec();
    let sha = sha256_hex(&bytes);
    let path = format!("/v1/assets/{sha}?source=imessage");
    let server = crate::test_support::serve(&state).await;
    let response = reqwest::Client::new()
        .put(format!("{}{path}", server.base()))
        .bearer_auth(&user.token)
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .body(bytes.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let token = |can_import: bool, can_export: bool| {
        let state = state.clone();
        let user_token = user.token.clone();
        let tokens_path = format!("/v1/accounts/{}/api-tokens", user.account_id);
        async move {
            let (_location, created): (String, serde_json::Value) =
                crate::test_support::post_created_json(
                    &state,
                    &tokens_path,
                    &user_token,
                    serde_json::json!({
                        "label": "t",
                        "can_import": can_import,
                        "can_export": can_export
                    }),
                )
                .await;
            created["token"].as_str().unwrap().to_string()
        }
    };
    let import_only = token(true, false).await;
    let may_export = token(false, true).await;
    assert_eq!(
        crate::test_support::get_status(&state, &path, &import_only).await,
        StatusCode::FORBIDDEN,
        "a token that may not export must not fetch attachment bytes"
    );
    assert_eq!(
        crate::test_support::get_status(&state, &path, &may_export).await,
        StatusCode::OK
    );

    assert_eq!(
        crate::test_support::patch_status(
            &state,
            &format!("/v1/accounts/{}", user.account_id),
            &owner.token,
            serde_json::json!({ "can_export": false }),
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        crate::test_support::get_status(&state, &path, &user.token).await,
        StatusCode::OK,
        "an account with export off still sees its own attachments"
    );
    assert_eq!(
        crate::test_support::get_status(&state, "/v1/exports", &user.token).await,
        StatusCode::FORBIDDEN,
        "the export permission still decides Export Runs"
    );
}

#[tokio::test]
async fn an_asset_get_for_an_unknown_sha_is_a_json_404() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;

    let unknown = "0".repeat(64);
    let (status, text) = crate::test_support::get_raw(
        &fixture.state,
        &format!("/v1/assets/{unknown}?source=sms-backup-restore"),
        &user.token,
    )
    .await;
    crate::test_support::expect_problem(status, &text, crate::problem::ProblemType::NotFound);
}

/// A part body past the part size its upload started with is a 413, and the
/// sentence names that part size. The layer lets a part through uncapped, so
/// the handler's own check is what answers. `docs/architecture/http-api.md`:
/// the status carries the meaning.
#[tokio::test]
async fn an_upload_part_over_the_part_size_is_a_json_413() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let mut state = fixture.state.clone();
    // The part size is a field of the state, so a test can lower it without
    // rebuilding the config.
    state.asset_part_size = 16;

    let sha = "0".repeat(64);
    let (status, text) = crate::test_support::post_raw(
        &state,
        &format!("/v1/assets/{sha}/uploads?source=sms-backup-restore"),
        &user.token,
        "application/json",
        serde_json::json!({ "bytes": 40 }).to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{text}");
    let started: serde_json::Value = serde_json::from_str(&text).unwrap();
    let upload_id = started["upload_id"].as_str().unwrap();
    let (status, text) = crate::test_support::put_raw(
        &state,
        &format!("/v1/assets/{sha}/uploads/{upload_id}/parts/1?source=sms-backup-restore"),
        &user.token,
        "application/octet-stream",
        vec![b'x'; 4096],
    )
    .await;
    let problem = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::PayloadTooLarge,
    );
    assert_eq!(
        problem.detail.as_deref(),
        Some("a part of this upload is at most 16 bytes"),
        "the sentence must be the handler's own, proving the layer did not answer: {text}"
    );
}

/// The attachment size limit is read from the Server Settings on each upload:
/// the owner lowers it, and the next upload over it is refused by the server
/// that was already running, whether it is sent as one `PUT` or opened as a
/// multipart upload. The same file was accepted a moment before.
#[tokio::test]
async fn an_upload_over_the_limit_the_owner_just_set_is_refused() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let mut state = fixture.state.clone();
    state.asset_part_size = 16;
    let owner = crate::test_support::claim_as_owner(&state, "keeper", "hunter2hunter2").await;
    let bytes: Vec<u8> = (0u8..40).collect();
    let sha = sha256_hex(&bytes);
    let start = format!("/v1/assets/{sha}/uploads?source=imessage");
    let declared = serde_json::json!({ "bytes": bytes.len(), "mime": "image/png" });

    assert_eq!(
        crate::test_support::post_status(&state, &start, &user.token, declared.clone()).await,
        StatusCode::CREATED,
        "40 bytes is under the 512 MiB a new Message Crate allows"
    );

    let _: serde_json::Value = crate::test_support::patch_json(
        &state,
        "/v1/server/settings",
        &owner.token,
        serde_json::json!({ "asset_max_bytes": 32 }),
    )
    .await;

    let (status, text) = crate::test_support::post_raw(
        &state,
        &start,
        &user.token,
        "application/json",
        declared.to_string(),
    )
    .await;
    let problem = crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::AssetUploadInvalid,
    );
    assert!(
        problem
            .detail
            .as_deref()
            .is_some_and(|d| d.contains("32 byte")),
        "the refusal names the limit now in force: {text}"
    );

    let (status, text) = crate::test_support::put_raw(
        &state,
        &format!("/v1/assets/{sha}?source=imessage"),
        &user.token,
        "image/png",
        bytes,
    )
    .await;
    crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::PayloadTooLarge,
    );
}

/// A limit below the part size in the config file is a working limit, not a
/// broken server: the part size the server hands out is never larger than the
/// limit, so a file exactly at the limit goes up as a multipart upload and is
/// served back, and a file one byte over is refused. The part size in this
/// state is the 64 MiB default; the limit is 40 bytes.
#[tokio::test]
async fn a_multipart_upload_works_under_a_limit_below_the_configured_part_size() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let state = fixture.state.clone();
    let owner = crate::test_support::claim_as_owner(&state, "keeper", "hunter2hunter2").await;
    let _: serde_json::Value = crate::test_support::patch_json(
        &state,
        "/v1/server/settings",
        &owner.token,
        serde_json::json!({ "asset_max_bytes": 40 }),
    )
    .await;

    let bytes: Vec<u8> = (0u8..40).collect();
    let sha = sha256_hex(&bytes);
    let server = crate::test_support::serve(&state).await;
    let url = |rest: &str| format!("{}/v1/assets/{sha}{rest}?source=imessage", server.base());
    let client = reqwest::Client::new();

    let response = client
        .post(url("/uploads"))
        .bearer_auth(&user.token)
        .json(&serde_json::json!({ "bytes": bytes.len(), "mime": "image/png" }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let started: serde_json::Value = response.json().await.unwrap();
    let upload_id = started["upload_id"].as_str().unwrap().to_string();
    let part_size = started["part_size"].as_u64().unwrap() as usize;
    assert_eq!(part_size, 40, "a part is never larger than the limit");

    for (index, chunk) in bytes.chunks(part_size).enumerate() {
        let response = client
            .put(url(&format!("/uploads/{upload_id}/parts/{}", index + 1)))
            .bearer_auth(&user.token)
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .body(chunk.to_vec())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "part {}", index + 1);
    }
    let response = client
        .post(url(&format!("/uploads/{upload_id}/complete")))
        .bearer_auth(&user.token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let response = client
        .get(url(""))
        .bearer_auth(&user.token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.bytes().await.unwrap().as_ref(), bytes.as_slice());

    // One byte over the limit is refused when the upload is opened.
    let over: Vec<u8> = (0u8..41).collect();
    let (status, text) = crate::test_support::post_raw(
        &state,
        &format!("/v1/assets/{}/uploads?source=imessage", sha256_hex(&over)),
        &user.token,
        "application/json",
        serde_json::json!({ "bytes": over.len() }).to_string(),
    )
    .await;
    crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::AssetUploadInvalid,
    );
}

/// A multipart upload keeps the part size it started with. The owner lowers
/// the attachment size limit to 10 bytes after a 40-byte upload has opened
/// with 16-byte parts, and every remaining 16-byte part is still stored and
/// the upload completes, because the limit holds from the next upload.
#[tokio::test]
async fn a_multipart_upload_keeps_its_part_size_when_the_limit_is_lowered() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let mut state = fixture.state.clone();
    state.asset_part_size = 16;
    let owner = crate::test_support::claim_as_owner(&state, "keeper", "hunter2hunter2").await;
    let bytes: Vec<u8> = (0u8..40).collect();
    let sha = sha256_hex(&bytes);
    let server = crate::test_support::serve(&state).await;
    let url = |rest: &str| format!("{}/v1/assets/{sha}{rest}?source=imessage", server.base());
    let client = reqwest::Client::new();

    let response = client
        .post(url("/uploads"))
        .bearer_auth(&user.token)
        .json(&serde_json::json!({ "bytes": bytes.len(), "mime": "image/png" }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let started: serde_json::Value = response.json().await.unwrap();
    let upload_id = started["upload_id"].as_str().unwrap().to_string();
    let part_size = started["part_size"].as_u64().unwrap() as usize;
    assert_eq!(part_size, 16);

    let _: serde_json::Value = crate::test_support::patch_json(
        &state,
        "/v1/server/settings",
        &owner.token,
        serde_json::json!({ "asset_max_bytes": 10 }),
    )
    .await;

    for (index, chunk) in bytes.chunks(part_size).enumerate() {
        let response = client
            .put(url(&format!("/uploads/{upload_id}/parts/{}", index + 1)))
            .bearer_auth(&user.token)
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .body(chunk.to_vec())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "part {}", index + 1);
    }
    let response = client
        .post(url(&format!("/uploads/{upload_id}/complete")))
        .bearer_auth(&user.token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let response = client
        .get(url(""))
        .bearer_auth(&user.token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.bytes().await.unwrap().as_ref(), bytes.as_slice());
}

/// What `serve` reads as it starts: with a stored limit below the part size
/// in the config file it gets working limits, not an error, so neither an
/// owner's setting nor an edit to `asset_part_size` can leave a server that
/// will not start.
#[tokio::test]
async fn the_limits_serve_starts_with_never_fail_on_a_limit_below_the_part_size() {
    let (fixture, _user) = crate::test_support::fixture_with_account().await;
    let state = fixture.state.clone();
    crate::test_support::store_asset_max_bytes(&state, 1024).await;

    let limits = state.upload_limits().await.unwrap();
    assert_eq!(limits.max_bytes, 1024);
    assert_eq!(limits.part_size, 1024);
}

/// The multipart upload over HTTP, the way `message-crate-push` sends a large file:
/// open the upload, send each part, complete it, and read the asset back.
/// Each step is tested alone in `asset_uploads`; this proves the routes join
/// up, that the part size the server hands out is the one it holds a part to,
/// and that `complete` installs bytes the server then serves.
#[tokio::test]
async fn a_multipart_upload_completes_end_to_end_over_http() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let mut state = fixture.state.clone();
    state.asset_part_size = 16;
    let bytes: Vec<u8> = (0u8..40).collect();
    let sha = sha256_hex(&bytes);
    let server = crate::test_support::serve(&state).await;
    let url = |rest: &str| format!("{}/v1/assets/{sha}{rest}?source=imessage", server.base());
    let client = reqwest::Client::new();

    let response = client
        .post(url("/uploads"))
        .bearer_auth(&user.token)
        .json(&serde_json::json!({ "bytes": bytes.len(), "mime": "image/png" }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let started: serde_json::Value = response.json().await.unwrap();
    let upload_id = started["upload_id"].as_str().unwrap().to_string();
    let part_size = started["part_size"].as_u64().unwrap() as usize;
    assert_eq!(part_size, 16);

    for (index, chunk) in bytes.chunks(part_size).enumerate() {
        let response = client
            .put(url(&format!("/uploads/{upload_id}/parts/{}", index + 1)))
            .bearer_auth(&user.token)
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .body(chunk.to_vec())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "part {}", index + 1);
    }

    // Completing the upload stores the asset: a creation, answered like the
    // single PUT that stores one.
    let response = client
        .post(url(&format!("/uploads/{upload_id}/complete")))
        .bearer_auth(&user.token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(
        response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok()),
        Some(format!("/v1/assets/{sha}?source=imessage").as_str())
    );
    let done: serde_json::Value = response.json().await.unwrap();
    assert_eq!(done["sha256"], sha.as_str());
    assert_eq!(done["already_present"], false);

    let response = client
        .get(url(""))
        .bearer_auth(&user.token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("image/png")
    );
    assert_eq!(response.bytes().await.unwrap().as_ref(), bytes.as_slice());
}

/// The MIME type recorded for a blob the store already holds. The export's
/// claim wins, then what the source file's name says. A blank claim is not a
/// claim. The stored file has no extension, so it never has a say.
#[test]
fn mime_for_a_stored_blob_is_the_claim_then_the_source_name() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let sha = sha256_hex(b"stored-blob");
    let dest = root.join(shard_rel_path(&sha, ""));
    fs::create_dir_all(dest.parent().unwrap()).unwrap();
    fs::write(&dest, b"stored-blob").unwrap();
    let source = root.join("source.png");
    fs::write(&source, b"stored-blob").unwrap();

    let stored = |export_mime: Option<&str>| {
        let (stored, present) =
            store_verified(&source, &sha, root, export_mime, false, false).unwrap();
        assert!(present, "the blob is already stored");
        stored.mime_type
    };

    assert_eq!(stored(Some("audio/amr")).as_deref(), Some("audio/amr"));
    assert_eq!(stored(None).as_deref(), Some("image/png"));
    assert_eq!(
        stored(Some("")).as_deref(),
        Some("image/png"),
        "an empty claim must fall through to the source file's name"
    );
}

/// A file in the shard named `<sha>.jpg` is not the asset: the store holds
/// one path per fingerprint, `<aa>/<sha>` with no extension, and looks
/// nowhere else.
#[test]
fn lookup_ignores_a_file_named_with_an_extension() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let sha = sha256_hex(b"named-with-extension");
    let named = root.join(shard_rel_path(&sha, ".jpg"));
    fs::create_dir_all(named.parent().unwrap()).unwrap();
    fs::write(&named, b"named-with-extension").unwrap();

    assert!(lookup_by_sha256_unverified(root, &sha).is_none());
    assert!(lookup_by_sha256(root, &sha).is_none());

    // Storing the bytes creates the extensionless path beside it.
    let source = root.join("source.jpg");
    fs::write(&source, b"named-with-extension").unwrap();
    let (stored, present) = store_verified(&source, &sha, root, None, false, false).unwrap();
    assert!(!present);
    assert_eq!(stored.assets_path, shard_rel_path(&sha, ""));
    assert!(root.join(shard_rel_path(&sha, "")).is_file());
}

/// The Content-Type on a PUT is the asset's own media type, so the server
/// records it and serves it back. `application/octet-stream` only says the
/// body is bytes, so nothing is recorded and the download falls back to it.
#[tokio::test]
async fn an_asset_put_keeps_its_media_type_but_not_octet_stream() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let server = crate::test_support::serve(&fixture.state).await;
    let client = reqwest::Client::new();
    let url = |sha: &str| format!("{}/v1/assets/{sha}?source=imessage", server.base());

    let jpeg = b"jpeg-bytes".to_vec();
    let jpeg_sha = sha256_hex(&jpeg);
    let blob = b"blob-bytes".to_vec();
    let blob_sha = sha256_hex(&blob);
    for (sha, bytes, content_type) in [
        (&jpeg_sha, jpeg.clone(), "image/jpeg; charset=binary"),
        (&blob_sha, blob.clone(), "application/octet-stream"),
    ] {
        let response = client
            .put(url(sha))
            .bearer_auth(&user.token)
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .body(bytes)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    let served_type = |sha: &str| {
        let request = client.get(url(sha)).bearer_auth(&user.token);
        async move {
            let response = request.send().await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        }
    };
    assert_eq!(served_type(&jpeg_sha).await.as_deref(), Some("image/jpeg"));
    assert_eq!(
        served_type(&blob_sha).await.as_deref(),
        Some("application/octet-stream")
    );

    // Stored files have no extension, so the served type is the sidecar's.
    let assets_dir = fixture
        .state
        .cfg
        .paths
        .assets_dir_for_account(user.account_id, "imessage");
    assert_eq!(
        read_mime_metadata(&assets_dir, &jpeg_sha).as_deref(),
        Some("image/jpeg")
    );
    assert!(
        !mime_metadata_path(&assets_dir, &blob_sha).exists(),
        "octet-stream must not be recorded as the asset's type"
    );
}

/// Starting a chunked upload for a blob the server already holds creates
/// nothing: the answer is 200 with where the bytes are, and no session.
#[tokio::test]
async fn starting_an_upload_for_a_stored_blob_answers_200_already_present() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let server = crate::test_support::serve(&fixture.state).await;
    let client = reqwest::Client::new();
    let bytes = b"already-stored".to_vec();
    let sha = sha256_hex(&bytes);
    let url = |rest: &str| format!("{}/v1/assets/{sha}{rest}?source=imessage", server.base());

    let response = client
        .put(url(""))
        .bearer_auth(&user.token)
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .body(bytes.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let created: serde_json::Value = response.json().await.unwrap();

    let response = client
        .post(url("/uploads"))
        .bearer_auth(&user.token)
        .json(&serde_json::json!({ "bytes": bytes.len(), "mime": "image/png" }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().get(reqwest::header::LOCATION).is_none());
    let started: serde_json::Value = response.json().await.unwrap();
    assert_eq!(started["already_present"], true);
    assert_eq!(started["upload_id"], serde_json::Value::Null);
    assert_eq!(started["part_size"], serde_json::Value::Null);
    assert_eq!(started["sha256"], sha.as_str());
    assert_eq!(started["assets_path"], created["assets_path"]);

    let incoming = fixture
        .state
        .cfg
        .paths
        .assets_dir_for_account(user.account_id, "imessage")
        .join(".incoming")
        .join(&sha);
    assert!(!incoming.exists(), "no upload session may be opened");
}

/// Aborting a chunked upload answers 204 and removes the session's folder,
/// manifest and parts included, so an abandoned upload holds no disk.
#[tokio::test]
async fn deleting_an_upload_answers_204_and_removes_its_files() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let mut state = fixture.state.clone();
    state.asset_part_size = 16;
    let server = crate::test_support::serve(&state).await;
    let client = reqwest::Client::new();
    let bytes: Vec<u8> = (0u8..40).collect();
    let sha = sha256_hex(&bytes);
    let url = |rest: &str| format!("{}/v1/assets/{sha}{rest}?source=imessage", server.base());

    let response = client
        .post(url("/uploads"))
        .bearer_auth(&user.token)
        .json(&serde_json::json!({ "bytes": bytes.len(), "mime": "image/png" }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let started: serde_json::Value = response.json().await.unwrap();
    let upload_id = started["upload_id"].as_str().unwrap().to_string();

    let response = client
        .put(url(&format!("/uploads/{upload_id}/parts/1")))
        .bearer_auth(&user.token)
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .body(bytes[..16].to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let assets_dir = state
        .cfg
        .paths
        .assets_dir_for_account(user.account_id, "imessage");
    let session = asset_uploads::session_dir(&assets_dir, &sha, &upload_id);
    assert!(session.join("manifest.json").is_file());
    assert!(session.join("part-0001").is_file());

    let response = client
        .delete(url(&format!("/uploads/{upload_id}")))
        .bearer_auth(&user.token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(!session.exists(), "the session folder must be gone");

    // The session is gone, so a part for it has nowhere to go.
    let response = client
        .put(url(&format!("/uploads/{upload_id}/parts/2")))
        .bearer_auth(&user.token)
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .body(bytes[16..32].to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// A zero-byte attachment is a file like any other. A PUT of no bytes
/// addressed by the empty file's fingerprint stores it, and a GET returns it
/// with a `Content-Length` of 0.
#[tokio::test]
async fn an_asset_put_of_the_empty_file_is_stored() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let server = crate::test_support::serve(&fixture.state).await;
    let sha = sha256_hex(b"");
    let url = format!("{}/v1/assets/{sha}?source=imessage", server.base());
    let client = reqwest::Client::new();
    let response = client
        .put(&url)
        .bearer_auth(&user.token)
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .body(Vec::new())
        .send()
        .await
        .unwrap();
    let status = response.status();
    let text = response.text().await.unwrap();
    assert_eq!(status, StatusCode::CREATED, "empty file refused: {text}");

    let response = client
        .get(&url)
        .bearer_auth(&user.token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok()),
        Some("0")
    );
    assert!(response.bytes().await.unwrap().is_empty());
}

/// A multipart upload of the empty file starts with `bytes: 0`, sends no
/// parts, and completes by storing the empty file.
#[tokio::test]
async fn a_multipart_upload_of_the_empty_file_completes_with_no_parts() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let server = crate::test_support::serve(&fixture.state).await;
    let sha = sha256_hex(b"");
    let url = |rest: &str| format!("{}/v1/assets/{sha}{rest}?source=imessage", server.base());
    let client = reqwest::Client::new();

    let response = client
        .post(url("/uploads"))
        .bearer_auth(&user.token)
        .json(&serde_json::json!({ "bytes": 0 }))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let text = response.text().await.unwrap();
    assert_eq!(status, StatusCode::CREATED, "start refused: {text}");
    let started: serde_json::Value = serde_json::from_str(&text).unwrap();
    let upload_id = started["upload_id"].as_str().unwrap().to_string();

    let response = client
        .post(url(&format!("/uploads/{upload_id}/complete")))
        .bearer_auth(&user.token)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let text = response.text().await.unwrap();
    assert_eq!(status, StatusCode::CREATED, "complete refused: {text}");
    let done: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(done["sha256"], sha.as_str());

    let assets_dir = fixture
        .state
        .cfg
        .paths
        .assets_dir_for_account(user.account_id, "imessage");
    assert!(lookup_by_sha256_unverified(&assets_dir, &sha).is_some());
}

/// A PUT with no bytes was read in full; it just does not hash to the
/// fingerprint it is addressed by, so it answers 422 like any other body that
/// does not, and stores nothing.
#[tokio::test]
async fn an_asset_put_with_an_empty_body_answers_422() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let server = crate::test_support::serve(&fixture.state).await;
    let sha = sha256_hex(b"never-sent");
    let response = reqwest::Client::new()
        .put(format!("{}/v1/assets/{sha}?source=imessage", server.base()))
        .bearer_auth(&user.token)
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .body(Vec::new())
        .send()
        .await
        .unwrap();
    let status = response.status();
    let text = response.text().await.unwrap();
    crate::test_support::expect_problem(
        status,
        &text,
        crate::problem::ProblemType::AssetUploadInvalid,
    );

    let assets_dir = fixture
        .state
        .cfg
        .paths
        .assets_dir_for_account(user.account_id, "imessage");
    assert!(lookup_by_sha256_unverified(&assets_dir, &sha).is_none());
}

/// When a single PUT stores the bytes while a chunked upload of the same
/// bytes is still open, `complete` finds the asset already held: it answers
/// 200 with no `Location`, like the PUT does, and drops the session.
#[tokio::test]
async fn completing_an_upload_for_a_blob_a_put_stored_first_answers_200() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let mut state = fixture.state.clone();
    state.asset_part_size = 16;
    let server = crate::test_support::serve(&state).await;
    let client = reqwest::Client::new();
    let bytes: Vec<u8> = (0u8..40).collect();
    let sha = sha256_hex(&bytes);
    let url = |rest: &str| format!("{}/v1/assets/{sha}{rest}?source=imessage", server.base());

    let response = client
        .post(url("/uploads"))
        .bearer_auth(&user.token)
        .json(&serde_json::json!({ "bytes": bytes.len(), "mime": "image/png" }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let started: serde_json::Value = response.json().await.unwrap();
    let upload_id = started["upload_id"].as_str().unwrap().to_string();
    for (index, chunk) in bytes.chunks(16).enumerate() {
        let response = client
            .put(url(&format!("/uploads/{upload_id}/parts/{}", index + 1)))
            .bearer_auth(&user.token)
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .body(chunk.to_vec())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    let response = client
        .put(url(""))
        .bearer_auth(&user.token)
        .header(reqwest::header::CONTENT_TYPE, "image/png")
        .body(bytes.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let response = client
        .post(url(&format!("/uploads/{upload_id}/complete")))
        .bearer_auth(&user.token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().get(reqwest::header::LOCATION).is_none());
    let done: serde_json::Value = response.json().await.unwrap();
    assert_eq!(done["already_present"], true);
    assert_eq!(done["sha256"], sha.as_str());

    let assets_dir = state
        .cfg
        .paths
        .assets_dir_for_account(user.account_id, "imessage");
    assert!(
        !asset_uploads::session_dir(&assets_dir, &sha, &upload_id).exists(),
        "the stale session must be dropped"
    );
}

/// An account's conversation with two stored photos: the first has a preview
/// that `process-assets` would have written, the second has none.
struct PreviewFixture {
    conversation_id: i64,
    /// Fingerprint of the original that has a preview.
    with_preview: String,
    /// Fingerprint of the original that has none.
    without_preview: String,
}

const ORIGINAL_BYTES: &[u8] = b"a photo as the phone took it";
const UNCONVERTED_BYTES: &[u8] = b"a photo with no preview";
const PREVIEW_BYTES: &[u8] = b"the same photo as a jpeg";

async fn seed_attachment_with_preview(state: &AppState, account_id: i64) -> PreviewFixture {
    let conversation_id = crate::test_support::seed_conversation(
        state,
        &crate::test_support::SeedConversation {
            account_id,
            handle: "+15555550142",
            conversation_type: "individual",
            group_title: None,
            source_file: "seed.jsonl",
            messages: &[crate::test_support::SeedMessage {
                source: "imessage",
                timestamp: "2020-01-01T00:00:00Z",
                is_from_me: true,
                body: "two photos",
            }],
        },
    )
    .await;
    let mut conn = state.db.acquire().await.unwrap();
    let message_id: i64 = sqlx::query_scalar("SELECT id FROM messages WHERE conversation_id = $1")
        .bind(conversation_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap();

    let paths = &state.cfg.paths;
    let with_preview = sha256_hex(ORIGINAL_BYTES);
    let without_preview = sha256_hex(UNCONVERTED_BYTES);
    let assets_dir = paths.assets_dir_for_account(account_id, "imessage");
    for (sha, bytes) in [
        (&with_preview, ORIGINAL_BYTES),
        (&without_preview, UNCONVERTED_BYTES),
    ] {
        let stored = assets_dir.join(shard_rel_path(sha, ""));
        fs::create_dir_all(stored.parent().unwrap()).unwrap();
        fs::write(stored, bytes).unwrap();
    }
    let preview_sha = sha256_hex(PREVIEW_BYTES);
    let preview_path = shard_rel_path(&preview_sha, ".jpg");
    let preview = paths
        .assets_converted_dir_for_account(account_id, "imessage")
        .join(&preview_path);
    fs::create_dir_all(preview.parent().unwrap()).unwrap();
    fs::write(preview, PREVIEW_BYTES).unwrap();

    for (sha, derived) in [(&with_preview, true), (&without_preview, false)] {
        sqlx::query(
            "INSERT INTO attachments (
                message_id, original_name, mime_type, sha256, assets_path,
                derived_sha256, derived_assets_path, derived_mime_type
             ) VALUES ($1, 'photo.heic', 'image/heic', $2, $3, $4, $5, $6)",
        )
        .bind(message_id)
        .bind(sha.as_str())
        .bind(shard_rel_path(sha, ""))
        .bind(derived.then_some(preview_sha.as_str()))
        .bind(derived.then_some(preview_path.as_str()))
        .bind(derived.then_some("image/jpeg"))
        .execute(&mut *conn)
        .await
        .unwrap();
    }
    PreviewFixture {
        conversation_id,
        with_preview,
        without_preview,
    }
}

/// GET `path` as a browser asks for an image, which admits no JSON, and
/// return the status, the `Content-Type` and the body.
async fn get_bytes(state: &AppState, path: &str, token: &str) -> (StatusCode, String, Vec<u8>) {
    let server = crate::test_support::serve(state).await;
    let response = reqwest::Client::new()
        .get(format!("{}{path}", server.base()))
        .bearer_auth(token)
        .header(reqwest::header::ACCEPT, "image/*")
        .send()
        .await
        .unwrap();
    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    (
        status,
        content_type,
        response.bytes().await.unwrap().to_vec(),
    )
}

/// The web app shows a preview only when the attachment says it has one, so
/// the conversation's messages must report it, and only for the attachment
/// `process-assets` converted.
#[tokio::test]
async fn an_attachment_with_a_preview_reports_its_media_type() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let seeded = seed_attachment_with_preview(&fixture.state, user.account_id).await;

    let page: serde_json::Value = crate::test_support::get_json(
        &fixture.state,
        &format!("/v1/conversations/{}/messages", seeded.conversation_id),
        &user.token,
    )
    .await;
    let attachments = page["items"][0]["attachments"].as_array().unwrap();
    assert_eq!(attachments.len(), 2);
    assert_eq!(attachments[0]["sha256"], seeded.with_preview);
    assert_eq!(attachments[0]["mime_type"], "image/heic");
    assert_eq!(attachments[0]["preview_mime_type"], "image/jpeg");
    assert_eq!(attachments[1]["sha256"], seeded.without_preview);
    assert!(
        attachments[1].get("preview_mime_type").is_none(),
        "an attachment with no preview must not claim one: {}",
        attachments[1]
    );
}

/// The preview route answers the bytes `process-assets` wrote, in their own
/// media type, and the original route goes on answering the original.
#[tokio::test]
async fn the_preview_route_serves_the_preview_and_the_asset_route_the_original() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let state = &fixture.state;
    let seeded = seed_attachment_with_preview(state, user.account_id).await;
    let sha = &seeded.with_preview;

    let (status, content_type, body) = get_bytes(
        state,
        &format!("/v1/assets/{sha}/preview?source=imessage"),
        &user.token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type, "image/jpeg");
    assert_eq!(body, PREVIEW_BYTES);

    let (status, _content_type, body) = get_bytes(
        state,
        &format!("/v1/assets/{sha}?source=imessage"),
        &user.token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, ORIGINAL_BYTES, "the asset route serves the original");

    // The preview belongs to the source it was made in.
    let (status, text) = crate::test_support::get_raw(
        state,
        &format!("/v1/assets/{sha}/preview?source=whatsapp"),
        &user.token,
    )
    .await;
    crate::test_support::expect_problem(status, &text, crate::problem::ProblemType::NotFound);

    // An asset with no preview has nothing at the preview route; the web app
    // shows the original for it.
    let (status, text) = crate::test_support::get_raw(
        state,
        &format!(
            "/v1/assets/{}/preview?source=imessage",
            seeded.without_preview
        ),
        &user.token,
    )
    .await;
    crate::test_support::expect_problem(status, &text, crate::problem::ProblemType::NotFound);
}

/// A preview is the attachment's content as much as the original is, so it is
/// read under the same rule: the account that holds it and nobody else, any
/// session of that account, an API token only with the export scope, and
/// never the owner (`docs/adr/0008`).
#[tokio::test]
async fn a_preview_is_read_under_the_same_rule_as_the_original() {
    let fixture = crate::test_support::test_fixture().await;
    let state = fixture.state.clone();
    let owner =
        crate::test_support::claim_as_owner(&state, "preview-keeper", "hunter2hunter2").await;
    let user =
        crate::test_support::register_via_api(&state, "preview-user", "hunter2hunter2").await;
    let other =
        crate::test_support::register_via_api(&state, "preview-other", "hunter2hunter2").await;
    let seeded = seed_attachment_with_preview(&state, user.account_id).await;
    let path = format!("/v1/assets/{}/preview?source=imessage", seeded.with_preview);

    let (status, text) = crate::test_support::get_raw(&state, &path, &other.token).await;
    crate::test_support::expect_problem(status, &text, crate::problem::ProblemType::NotFound);
    assert_eq!(
        crate::test_support::get_status(&state, &path, &owner.token).await,
        StatusCode::FORBIDDEN,
        "the owner never reads an attachment's bytes, a preview included"
    );

    let tokens_path = format!("/v1/accounts/{}/api-tokens", user.account_id);
    let mut tokens = Vec::new();
    for (can_import, can_export) in [(true, false), (false, true)] {
        let (_location, created): (String, serde_json::Value) =
            crate::test_support::post_created_json(
                &state,
                &tokens_path,
                &user.token,
                serde_json::json!({
                    "label": "t",
                    "can_import": can_import,
                    "can_export": can_export
                }),
            )
            .await;
        tokens.push(created["token"].as_str().unwrap().to_string());
    }
    assert_eq!(
        crate::test_support::get_status(&state, &path, &tokens[0]).await,
        StatusCode::FORBIDDEN,
        "a token that may not export must not fetch a preview"
    );
    assert_eq!(
        crate::test_support::get_status(&state, &path, &tokens[1]).await,
        StatusCode::OK
    );

    // Export off, and the conversation in the Trash: the account still opens
    // the conversation and still reads the original, so it reads the preview.
    assert_eq!(
        crate::test_support::patch_status(
            &state,
            &format!("/v1/accounts/{}", user.account_id),
            &owner.token,
            serde_json::json!({ "can_export": false }),
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        crate::test_support::post_status(
            &state,
            &format!("/v1/conversations/{}/trash", seeded.conversation_id),
            &user.token,
            serde_json::json!({}),
        )
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        crate::test_support::get_status(&state, &path, &user.token).await,
        StatusCode::OK
    );
}

/// C1-1: a server that cannot write its assets folder has a storage fault,
/// not a body that broke a rule. It answers 500, not 422.
#[tokio::test]
async fn c1_1_a_put_the_server_cannot_store_is_not_a_422() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let bytes: Vec<u8> = b"an attachment".to_vec();
    let sha = sha256_hex(&bytes);
    let assets_dir = fixture
        .state
        .cfg
        .paths
        .assets_dir_for_account(user.account_id, "imessage");
    std::fs::create_dir_all(&assets_dir).unwrap();
    // A file where the shard folder must go: create_dir_all in install_blob fails.
    std::fs::write(assets_dir.join(&sha[..2]), b"not a folder").unwrap();
    let (status, text) = crate::test_support::put_raw(
        &fixture.state,
        &format!("/v1/assets/{sha}?source=imessage"),
        &user.token,
        "application/octet-stream",
        bytes,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "a storage failure answered {status}: {text}"
    );
}

/// An upload id that names no upload names nothing, so a part or a
/// completion sent to it answers 404, not 422.
#[tokio::test]
async fn a_part_or_completion_for_an_unknown_upload_is_not_found() {
    let (fixture, user) = crate::test_support::fixture_with_account().await;
    let sha = sha256_hex(b"never started");

    let (status, text) = crate::test_support::put_raw(
        &fixture.state,
        &format!("/v1/assets/{sha}/uploads/abcdef01/parts/1?source=imessage"),
        &user.token,
        "application/octet-stream",
        b"part".to_vec(),
    )
    .await;
    crate::test_support::expect_problem(status, &text, crate::problem::ProblemType::NotFound);

    let (status, text) = crate::test_support::post_raw(
        &fixture.state,
        &format!("/v1/assets/{sha}/uploads/abcdef01/complete?source=imessage"),
        &user.token,
        "application/json",
        "{}",
    )
    .await;
    crate::test_support::expect_problem(status, &text, crate::problem::ProblemType::NotFound);
}
