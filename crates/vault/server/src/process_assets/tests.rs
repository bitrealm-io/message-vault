use std::time::{Duration, SystemTime};

use super::*;
use crate::db::engine;

const SHA: &str = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";

/// A row for a stored blob named `assets_path`, with nothing else known.
fn row(assets_path: &str) -> AssetRow {
    AssetRow {
        sha256: SHA.to_string(),
        assets_path: assets_path.to_string(),
        mime_type: None,
        derived_assets_path: None,
        original_name: None,
        source_path: None,
    }
}

/// The original is on disk and no preview exists: the state a fresh import leaves.
const FRESH: OnDisk = OnDisk {
    original_exists: true,
    derived_exists: false,
};

/// A pass over `assets_dir` for account `acc`, source `imessage`.
fn pass<'a>(
    opts: &'a ProcessAssetsOptions,
    work_dir: &'a Path,
    assets_dir: &Path,
    converted_dir: &Path,
) -> SourcePass<'a> {
    SourcePass {
        opts,
        work_dir,
        account_id: 7,
        source_id: "imessage",
        assets_dir: assets_dir.to_path_buf(),
        converted_dir: converted_dir.to_path_buf(),
    }
}

#[test]
fn derived_rel_path_layout() {
    assert_eq!(derived_rel_path(SHA, ".jpg"), format!("ab/{SHA}.jpg"));
    assert_eq!(derived_rel_path(SHA, ".jpeg"), format!("ab/{SHA}.jpg"));
}

#[test]
fn a_part_path_is_removed_and_never_converted() {
    let opts = ProcessAssetsOptions::default();
    let mut part = row("aa/upload.part");
    part.mime_type = Some("video/mp4".to_string());
    part.original_name = Some("clip.mp4".to_string());
    assert_eq!(plan(&part, &opts, FRESH).unwrap(), Plan::RemoveIncomplete);
    // Even when the file is already gone the plan is the same; the executor
    // deals with an absent file.
    let gone = OnDisk {
        original_exists: false,
        derived_exists: false,
    };
    assert_eq!(plan(&part, &opts, gone).unwrap(), Plan::RemoveIncomplete);
}

#[test]
fn a_blob_that_is_not_media_is_skipped() {
    let opts = ProcessAssetsOptions::default();
    assert_eq!(
        plan(&row("aa/notes.txt"), &opts, FRESH).unwrap(),
        Plan::Skip(SkipReason::NotMedia)
    );
    let mut pdf = row(&format!("ab/{SHA}"));
    pdf.mime_type = Some("application/pdf".to_string());
    pdf.original_name = Some("clip.mp4".to_string());
    assert_eq!(
        plan(&pdf, &opts, FRESH).unwrap(),
        Plan::Skip(SkipReason::NotMedia)
    );
}

#[test]
fn a_gif_is_skipped_because_an_animation_gets_no_still_preview() {
    let opts = ProcessAssetsOptions::default();
    assert_eq!(
        plan(&row("aa/photo.gif"), &opts, FRESH).unwrap(),
        Plan::Skip(SkipReason::NotMedia)
    );
    let mut declared = row(&format!("ab/{SHA}"));
    declared.mime_type = Some("image/gif".to_string());
    assert_eq!(
        plan(&declared, &opts, FRESH).unwrap(),
        Plan::Skip(SkipReason::NotMedia)
    );
}

#[test]
fn each_kind_is_derived_when_nothing_stands_in_the_way() {
    let opts = ProcessAssetsOptions::default();
    assert_eq!(
        plan(&row("aa/photo.jpg"), &opts, FRESH).unwrap(),
        Plan::Derive(Kind::Image)
    );
    assert_eq!(
        plan(&row("aa/clip.mp4"), &opts, FRESH).unwrap(),
        Plan::Derive(Kind::Video)
    );
    assert_eq!(
        plan(&row("aa/memo.m4a"), &opts, FRESH).unwrap(),
        Plan::Derive(Kind::Audio)
    );
}

#[test]
fn an_extensionless_blob_is_derived_by_its_declared_mime_or_its_attachment_name() {
    let opts = ProcessAssetsOptions::default();
    let mut by_mime = row(&format!("ab/{SHA}"));
    by_mime.mime_type = Some("image/heic".to_string());
    assert_eq!(
        plan(&by_mime, &opts, FRESH).unwrap(),
        Plan::Derive(Kind::Image)
    );
    let mut by_name = row(&format!("ab/{SHA}"));
    by_name.original_name = Some("voice-note.amr".to_string());
    assert_eq!(
        plan(&by_name, &opts, FRESH).unwrap(),
        Plan::Derive(Kind::Audio)
    );
}

#[test]
fn skip_image_turns_off_images_and_nothing_else() {
    let opts = ProcessAssetsOptions {
        skip_image: true,
        ..Default::default()
    };
    assert_eq!(
        plan(&row("aa/photo.jpg"), &opts, FRESH).unwrap(),
        Plan::Skip(SkipReason::KindDisabled)
    );
    assert_eq!(
        plan(&row("aa/clip.mp4"), &opts, FRESH).unwrap(),
        Plan::Derive(Kind::Video)
    );
    assert_eq!(
        plan(&row("aa/memo.m4a"), &opts, FRESH).unwrap(),
        Plan::Derive(Kind::Audio)
    );
}

#[test]
fn skip_video_turns_off_videos_and_nothing_else() {
    let opts = ProcessAssetsOptions {
        skip_video: true,
        ..Default::default()
    };
    assert_eq!(
        plan(&row("aa/clip.mp4"), &opts, FRESH).unwrap(),
        Plan::Skip(SkipReason::KindDisabled)
    );
    assert_eq!(
        plan(&row("aa/photo.jpg"), &opts, FRESH).unwrap(),
        Plan::Derive(Kind::Image)
    );
    assert_eq!(
        plan(&row("aa/memo.m4a"), &opts, FRESH).unwrap(),
        Plan::Derive(Kind::Audio)
    );
}

#[test]
fn skip_audio_turns_off_audio_and_nothing_else() {
    let opts = ProcessAssetsOptions {
        skip_audio: true,
        ..Default::default()
    };
    assert_eq!(
        plan(&row("aa/memo.m4a"), &opts, FRESH).unwrap(),
        Plan::Skip(SkipReason::KindDisabled)
    );
    assert_eq!(
        plan(&row("aa/photo.jpg"), &opts, FRESH).unwrap(),
        Plan::Derive(Kind::Image)
    );
    assert_eq!(
        plan(&row("aa/clip.mp4"), &opts, FRESH).unwrap(),
        Plan::Derive(Kind::Video)
    );
}

#[test]
fn an_existing_preview_is_kept_unless_force_is_given() {
    let derived = OnDisk {
        original_exists: true,
        derived_exists: true,
    };
    let mut photo = row("aa/photo.jpg");
    photo.derived_assets_path = Some(format!("ab/{SHA}.jpg"));
    assert_eq!(
        plan(&photo, &ProcessAssetsOptions::default(), derived).unwrap(),
        Plan::Skip(SkipReason::AlreadyDerived)
    );
    let force = ProcessAssetsOptions {
        force: true,
        ..Default::default()
    };
    assert_eq!(
        plan(&photo, &force, derived).unwrap(),
        Plan::Derive(Kind::Image)
    );
}

#[test]
fn a_disabled_kind_is_reported_before_an_existing_preview() {
    let opts = ProcessAssetsOptions {
        skip_image: true,
        ..Default::default()
    };
    let derived = OnDisk {
        original_exists: true,
        derived_exists: true,
    };
    assert_eq!(
        plan(&row("aa/photo.jpg"), &opts, derived).unwrap(),
        Plan::Skip(SkipReason::KindDisabled)
    );
}

#[test]
fn a_missing_original_is_an_error_only_when_a_conversion_is_wanted() {
    let opts = ProcessAssetsOptions::default();
    let missing = OnDisk {
        original_exists: false,
        derived_exists: false,
    };
    let err = plan(&row("aa/photo.jpg"), &opts, missing).unwrap_err();
    assert_eq!(err.to_string(), "missing original");
    // A preview already on disk, or a kind nobody wants, needs no original.
    let missing_but_derived = OnDisk {
        original_exists: false,
        derived_exists: true,
    };
    assert_eq!(
        plan(&row("aa/photo.jpg"), &opts, missing_but_derived).unwrap(),
        Plan::Skip(SkipReason::AlreadyDerived)
    );
    assert_eq!(
        plan(&row("aa/notes.txt"), &opts, missing).unwrap(),
        Plan::Skip(SkipReason::NotMedia)
    );
}

#[test]
fn derived_file_exists_reads_the_converted_folder() {
    let dir = tempfile::tempdir().unwrap();
    let rel = "ab/deadbeef.jpg";
    let dest = dir.path().join(rel);
    fs::create_dir_all(dest.parent().unwrap()).unwrap();
    fs::write(&dest, b"x").unwrap();
    assert!(derived_file_exists(Some(rel), dir.path()));
    assert!(!derived_file_exists(Some("missing.jpg"), dir.path()));
    assert!(!derived_file_exists(Some(""), dir.path()));
    assert!(!derived_file_exists(None, dir.path()));
}

#[test]
fn part_paths_are_recognised_in_any_case() {
    assert!(is_part_path("aa/aabbcc.part"));
    assert!(is_part_path("upload.PART"));
    assert!(!is_part_path("aa/aabbcc.mp4"));
    assert!(!is_part_path("aa/aabbcc"));
}

#[test]
fn a_label_names_the_account_the_source_and_the_stored_path() {
    let opts = ProcessAssetsOptions::default();
    let dir = tempfile::tempdir().unwrap();
    let pass = pass(&opts, dir.path(), dir.path(), dir.path());
    assert_eq!(pass.label(&row("aa/photo.jpg")), "7/imessage/aa/photo.jpg");
}

#[test]
fn removing_an_incomplete_upload_deletes_the_part_file() {
    let opts = ProcessAssetsOptions::default();
    let dir = tempfile::tempdir().unwrap();
    let assets = dir.path().join("assets");
    fs::create_dir_all(assets.join("aa")).unwrap();
    let part = assets.join("aa/upload.part");
    fs::write(&part, b"half").unwrap();
    let pass = pass(&opts, dir.path(), &assets, dir.path());

    let outcome = pass
        .remove_incomplete(&row("aa/upload.part"), &part)
        .unwrap();

    assert!(matches!(outcome, Outcome::Skipped));
    assert!(!part.exists());
    // A file that is already gone is not an error.
    let outcome = pass
        .remove_incomplete(&row("aa/upload.part"), &part)
        .unwrap();
    assert!(matches!(outcome, Outcome::Skipped));
}

#[test]
fn a_dry_run_leaves_the_part_file_in_place() {
    let opts = ProcessAssetsOptions {
        dry_run: true,
        ..Default::default()
    };
    let dir = tempfile::tempdir().unwrap();
    let assets = dir.path().join("assets");
    fs::create_dir_all(assets.join("aa")).unwrap();
    let part = assets.join("aa/upload.part");
    fs::write(&part, b"half").unwrap();
    let pass = pass(&opts, dir.path(), &assets, dir.path());

    let outcome = pass
        .remove_incomplete(&row("aa/upload.part"), &part)
        .unwrap();

    assert!(matches!(outcome, Outcome::Skipped));
    assert!(part.is_file());
}

#[test]
fn a_work_file_the_media_pass_did_not_write_means_the_original_stays() {
    let opts = ProcessAssetsOptions::default();
    let dir = tempfile::tempdir().unwrap();
    let pass = pass(&opts, dir.path(), dir.path(), dir.path());
    let stored = pass
        .store_work_file(None, "image", "jpg", ".jpg", &row("aa/photo.jpg"))
        .unwrap();
    assert_eq!(stored, Derived::Skipped);
}

#[test]
fn a_work_file_is_stored_content_addressed_and_then_removed() {
    let opts = ProcessAssetsOptions::default();
    let dir = tempfile::tempdir().unwrap();
    let converted = dir.path().join("converted");
    fs::create_dir_all(&converted).unwrap();
    let out = dir.path().join("out-abcdef012345.jpg");
    fs::write(&out, b"jpeg-bytes").unwrap();
    let pass = pass(&opts, dir.path(), dir.path(), &converted);

    let stored = pass
        .store_work_file(
            Some(out.clone()),
            "image",
            "jpg",
            ".jpg",
            &row("aa/photo.jpg"),
        )
        .unwrap();

    let expected = DerivedBlob {
        sha256: crate::assets::sha256_hex(b"jpeg-bytes"),
        assets_path: derived_rel_path(&crate::assets::sha256_hex(b"jpeg-bytes"), ".jpg"),
        mime_type: "image/jpeg".to_string(),
    };
    assert_eq!(stored, Derived::Stored(expected.clone()));
    assert_eq!(
        fs::read(converted.join(&expected.assets_path)).unwrap(),
        b"jpeg-bytes"
    );
    assert!(!out.exists(), "the work file is removed once stored");
}

#[test]
fn a_dry_run_stores_nothing_and_still_removes_the_work_file() {
    let opts = ProcessAssetsOptions {
        dry_run: true,
        ..Default::default()
    };
    let dir = tempfile::tempdir().unwrap();
    let converted = dir.path().join("converted");
    fs::create_dir_all(&converted).unwrap();
    let out = dir.path().join("out-abcdef012345.jpg");
    fs::write(&out, b"jpeg-bytes").unwrap();
    let pass = pass(&opts, dir.path(), dir.path(), &converted);

    let stored = pass
        .store_work_file(
            Some(out.clone()),
            "image",
            "jpg",
            ".jpg",
            &row("aa/photo.jpg"),
        )
        .unwrap();

    assert_eq!(stored, Derived::DryRun);
    assert!(!out.exists());
    assert_eq!(fs::read_dir(&converted).unwrap().count(), 0);
}

#[test]
fn an_upload_session_is_stale_after_a_day_by_its_manifest_or_its_folder() {
    let dir = tempfile::tempdir().unwrap();
    let with_manifest = dir.path().join("with-manifest");
    fs::create_dir_all(&with_manifest).unwrap();
    fs::write(with_manifest.join("manifest.json"), b"{}").unwrap();
    let without_manifest = dir.path().join("without-manifest");
    fs::create_dir_all(&without_manifest).unwrap();
    let now = SystemTime::now();
    let limit = Duration::from_secs(STALE_UPLOAD_SESSION_SECS);

    assert!(!upload_session_is_stale(&with_manifest, now).unwrap());
    assert!(!upload_session_is_stale(&without_manifest, now).unwrap());
    assert!(upload_session_is_stale(&with_manifest, now + limit).unwrap());
    assert!(upload_session_is_stale(&without_manifest, now + limit).unwrap());
    assert!(!upload_session_is_stale(&with_manifest, now + limit / 2).unwrap());
}

#[tokio::test]
async fn store_and_update_derived_db() {
    let (pool, dir) = engine::test_pool().await;
    schema::ensure_vault_schema(&mut pool.acquire().await.unwrap())
        .await
        .unwrap();
    let mut conn = pool.acquire().await.unwrap();
    sqlx::query("INSERT INTO accounts (id, username) VALUES (7, 'demo')")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES (7, '+1', '+1', 'phone', 'phone')",
    )
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO conversations (id, account_id, chat_handle_id, conversation_type, source_file)
         VALUES (1, 7, 1, 'individual', 't')",
    )
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO messages (id, conversation_id, account_id, source, timestamp, is_from_me, sort_order)
         VALUES (1, 1, 7, 'imessage', '2020-01-01T00:00:00Z', 0, 0)",
    )
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO attachments (id, message_id, sha256, assets_path, mime_type)
         VALUES (1, 1, 'aa11', 'aa/aa11.jpg', 'image/jpeg')",
    )
    .execute(&mut *conn)
    .await
    .unwrap();

    let converted = dir.path().join("converted");
    fs::create_dir_all(&converted).unwrap();
    let blob = store_derived_bytes(&converted, b"jpeg-bytes", ".jpg").unwrap();
    assert!(converted.join(&blob.assets_path).is_file());

    update_derived(&mut conn, 7, "imessage", "aa11", &blob)
        .await
        .unwrap();

    let (d_sha, d_path, d_mime): (String, String, String) = sqlx::query_as(
        "SELECT derived_sha256, derived_assets_path, derived_mime_type FROM attachments WHERE id = 1",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(d_sha, blob.sha256);
    assert_eq!(d_path, blob.assets_path);
    assert_eq!(d_mime, "image/jpeg");
}

#[tokio::test]
async fn listed_attachments_carry_name_hints_for_extensionless_blobs() {
    let (pool, _dir) = engine::test_pool().await;
    schema::ensure_vault_schema(&mut pool.acquire().await.unwrap())
        .await
        .unwrap();
    let mut conn = pool.acquire().await.unwrap();
    for statement in [
        "INSERT INTO accounts (id, username) VALUES (7, 'demo')".to_string(),
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
            VALUES (7, '+1', '+1', 'phone', 'phone')"
            .to_string(),
        "INSERT INTO conversations (id, account_id, chat_handle_id, conversation_type, source_file)
            VALUES (1, 7, 1, 'individual', 't')"
            .to_string(),
        "INSERT INTO messages (id, conversation_id, account_id, source, timestamp, is_from_me, sort_order)
            VALUES (1, 1, 7, 'imessage', '2020-01-01T00:00:00Z', 0, 0)"
            .to_string(),
        format!(
            "INSERT INTO attachments (id, message_id, sha256, assets_path, mime_type, original_name, path)
            VALUES (1, 1, '{SHA}', 'ab/{SHA}', NULL, 'voice-note.amr', 'attachments/voice-note.amr')"
        ),
    ] {
        sqlx::query(&statement).execute(&mut *conn).await.unwrap();
    }

    let rows = list_attachments(&mut conn, 7, "imessage").await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        plan(&rows[0], &ProcessAssetsOptions::default(), FRESH).unwrap(),
        Plan::Derive(Kind::Audio),
        "an extensionless blob with no declared MIME must classify from its attachment name"
    );
}
