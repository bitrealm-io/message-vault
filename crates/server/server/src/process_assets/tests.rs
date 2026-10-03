use std::future::Future;

use super::*;
use crate::asset_store::tests::make_abandoned;
use crate::config::PathsConfig;
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

/// A pass over `assets_dir` for account 7.
fn pass<'a>(
    opts: &'a ProcessAssetsOptions,
    work_dir: &'a Path,
    assets_dir: &Path,
    converted_dir: &Path,
) -> AccountPass<'a> {
    AccountPass {
        opts,
        work_dir,
        account_id: 7,
        assets_dir: assets_dir.to_path_buf(),
        converted_dir: converted_dir.to_path_buf(),
    }
}

#[test]
fn derived_rel_path_layout() {
    assert_eq!(
        derived_rel_path(&crate::assets_api::Sha256::parse(SHA).unwrap(), ".jpg"),
        format!("ab/{SHA}.jpg")
    );
    assert_eq!(
        derived_rel_path(&crate::assets_api::Sha256::parse(SHA).unwrap(), ".jpeg"),
        format!("ab/{SHA}.jpg")
    );
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
fn a_label_names_the_account_and_the_stored_path() {
    let opts = ProcessAssetsOptions::default();
    let dir = tempfile::tempdir().unwrap();
    let pass = pass(&opts, dir.path(), dir.path(), dir.path());
    assert_eq!(pass.label(&row("aa/photo.jpg")), "7/aa/photo.jpg");
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
        sha256: crate::assets_api::sha256_hex(b"jpeg-bytes"),
        assets_path: derived_rel_path(&crate::assets_api::Sha256::of_bytes(b"jpeg-bytes"), ".jpg"),
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

/// The account every database test seeds.
pub(crate) const ACCOUNT: i64 = 7;

/// A 1x1 plain-RGB PNG, the smallest image this build's ffmpeg decodes
/// cleanly (an RGBA one of the same size makes its PNG decoder fail).
#[rustfmt::skip]
pub(crate) const PNG_1X1_RGB: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0x00,
    0x00, 0x03, 0x01, 0x01, 0x00, 0xc9, 0xfe, 0x92, 0xef, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
    0x44, 0xae, 0x42, 0x60, 0x82,
];

/// An opened fresh database with the schema applied and its data folder
/// under a temp dir, the shape [`run`] is handed by the command line.
async fn open_db() -> (OpenDb, tempfile::TempDir) {
    let (pool, dir) = engine::test_pool().await;
    schema::ensure_schema(&mut pool.acquire().await.unwrap())
        .await
        .unwrap();
    let cfg = Config {
        paths: PathsConfig {
            db: dir.path().join("messagecrate.db"),
            data_dir: dir.path().join("data"),
            assets_dir: "assets".into(),
            assets_converted_dir: "assets_converted".into(),
        },
        server: None,
    };
    (OpenDb { cfg, db: pool }, dir)
}

async fn seed_account(conn: &mut SqliteConnection, id: i64) {
    sqlx::query("INSERT INTO accounts (id, username) VALUES ($1, $2)")
        .bind(id)
        .bind(format!("user{id}"))
        .execute(&mut *conn)
        .await
        .unwrap();
}

/// One conversation with one message under `source` for [`ACCOUNT`],
/// returning the message id an attachment can hang off.
pub(crate) async fn seed_message(conn: &mut SqliteConnection, source: &str) -> i64 {
    let handle_id: i64 = sqlx::query_scalar(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES ($1, $2, $2, 'phone', 'phone') RETURNING id",
    )
    .bind(ACCOUNT)
    .bind(format!("+1555{source}"))
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let conversation_id: i64 = sqlx::query_scalar(
        "INSERT INTO conversations (account_id, chat_handle_id, conversation_type, source_file)
         VALUES ($1, $2, 'individual', 't') RETURNING id",
    )
    .bind(ACCOUNT)
    .bind(handle_id)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    sqlx::query_scalar(
        "INSERT INTO messages (conversation_id, account_id, source, timestamp, is_from_me, sort_order)
         VALUES ($1, $2, $3, '2020-01-01T00:00:00Z', 0, 0) RETURNING id",
    )
    .bind(conversation_id)
    .bind(ACCOUNT)
    .bind(source)
    .fetch_one(&mut *conn)
    .await
    .unwrap()
}

/// Store `bytes` as the original for an attachment of `message_id`, the way
/// an import leaves it: the blob at `<aa>/<sha><ext>` in the account's
/// assets folder and a row pointing at it. Returns the attachment id.
pub(crate) async fn attach_stored_blob(
    opened: &OpenDb,
    conn: &mut SqliteConnection,
    message_id: i64,
    sha: &str,
    ext: &str,
    bytes: &[u8],
) -> i64 {
    let rel = format!("{}/{sha}{ext}", &sha[..2]);
    let path = opened.cfg.paths.assets_dir_for_account(ACCOUNT).join(&rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, bytes).unwrap();
    sqlx::query_scalar(
        "INSERT INTO attachments (message_id, sha256, assets_path) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(message_id)
    .bind(sha)
    .bind(rel)
    .fetch_one(&mut *conn)
    .await
    .unwrap()
}

/// A database with one account and one PNG attachment on a message of
/// `source`.
async fn fixture_with_png(source: &str) -> (OpenDb, tempfile::TempDir, i64) {
    let (opened, dir) = open_db().await;
    let mut conn = opened.conn().await.unwrap();
    seed_account(&mut conn, ACCOUNT).await;
    let message_id = seed_message(&mut conn, source).await;
    let attachment_id =
        attach_stored_blob(&opened, &mut conn, message_id, SHA, ".png", PNG_1X1_RGB).await;
    (opened, dir, attachment_id)
}

/// The derived columns of one attachment row, `None` until a preview is recorded.
async fn derived_of(
    conn: &mut SqliteConnection,
    attachment_id: i64,
) -> Option<(String, String, String)> {
    let (sha, path, mime): (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT derived_sha256, derived_assets_path, derived_mime_type FROM attachments WHERE id = $1",
    )
    .bind(attachment_id)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    Some((sha?, path?, mime?))
}

/// Run `test` on its own runtime with the real ffmpeg held available, or
/// skip it the way every ffmpeg test in the workspace skips (and fail under
/// CI). The guard is taken outside the async block: holding it across an
/// await is what Clippy's `await_holding_lock` refuses.
fn with_real_ffmpeg(test: impl Future<Output = ()>) {
    let Some(_tools) = media::testutil::real_ffmpeg_test_guard() else {
        return;
    };
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(test);
}

fn stats(scanned: u64, derived: u64, skipped: u64, errors: u64) -> ProcessAssetsStats {
    ProcessAssetsStats {
        scanned,
        derived,
        skipped,
        errors,
    }
}

#[tokio::test]
async fn store_and_update_derived_db() {
    let (opened, dir) = open_db().await;
    let mut conn = opened.conn().await.unwrap();
    seed_account(&mut conn, ACCOUNT).await;
    let message_id = seed_message(&mut conn, "imessage").await;
    let attachment_id = attach_stored_blob(&opened, &mut conn, message_id, SHA, ".jpg", b"x").await;

    let converted = dir.path().join("converted");
    fs::create_dir_all(&converted).unwrap();
    let blob = store_derived_bytes(&converted, b"jpeg-bytes", ".jpg").unwrap();
    assert!(converted.join(&blob.assets_path).is_file());

    update_derived(&mut conn, ACCOUNT, SHA, &blob)
        .await
        .unwrap();

    assert_eq!(
        derived_of(&mut conn, attachment_id).await,
        Some((blob.sha256, blob.assets_path, "image/jpeg".to_string()))
    );
}

#[tokio::test]
async fn listed_attachments_carry_name_hints_for_extensionless_blobs() {
    let (opened, _dir) = open_db().await;
    let mut conn = opened.conn().await.unwrap();
    seed_account(&mut conn, ACCOUNT).await;
    let message_id = seed_message(&mut conn, "imessage").await;
    sqlx::query(
        "INSERT INTO attachments (message_id, sha256, assets_path, mime_type, original_name, path)
         VALUES ($1, $2, $3, NULL, 'voice-note.amr', 'attachments/voice-note.amr')",
    )
    .bind(message_id)
    .bind(SHA)
    .bind(format!("ab/{SHA}"))
    .execute(&mut *conn)
    .await
    .unwrap();

    let rows = list_attachments(&mut conn, ACCOUNT).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        plan(&rows[0], &ProcessAssetsOptions::default(), FRESH).unwrap(),
        Plan::Derive(Kind::Audio),
        "an extensionless blob with no declared MIME must classify from its attachment name"
    );
}

#[test]
fn a_run_writes_a_jpeg_preview_under_the_converted_folder_and_records_it() {
    with_real_ffmpeg(async {
        let (opened, _dir, attachment_id) = fixture_with_png("imessage").await;
        let opts = ProcessAssetsOptions::default();

        let first = run(&opened, &opts).await.unwrap();

        assert_eq!(first, stats(1, 1, 0, 0));
        let mut conn = opened.conn().await.unwrap();
        let (sha, rel, mime) = derived_of(&mut conn, attachment_id)
            .await
            .expect("the row points at its preview");
        let preview = opened
            .cfg
            .paths
            .assets_converted_dir_for_account(ACCOUNT)
            .join(&rel);
        let bytes = fs::read(&preview).expect("the preview is under assets_converted/");
        assert_eq!(&bytes[..2], [0xff, 0xd8], "a JPEG starts with SOI");
        assert_eq!(sha, crate::assets_api::sha256_hex(&bytes));
        assert_eq!(
            rel,
            derived_rel_path(&crate::assets_api::Sha256::parse(&sha).unwrap(), ".jpg")
        );
        assert_eq!(mime, "image/jpeg");

        // A second run leaves the preview alone; `force` makes it again.
        assert_eq!(run(&opened, &opts).await.unwrap(), stats(1, 0, 1, 0));
        let force = ProcessAssetsOptions {
            force: true,
            ..Default::default()
        };
        assert_eq!(run(&opened, &force).await.unwrap(), stats(1, 1, 0, 0));
    });
}

#[test]
fn a_dry_run_counts_the_preview_it_would_write_and_writes_nothing() {
    with_real_ffmpeg(async {
        let (opened, _dir, attachment_id) = fixture_with_png("imessage").await;
        let opts = ProcessAssetsOptions {
            dry_run: true,
            ..Default::default()
        };

        assert_eq!(run(&opened, &opts).await.unwrap(), stats(1, 1, 0, 0));

        let converted = opened.cfg.paths.assets_converted_dir_for_account(ACCOUNT);
        assert_eq!(fs::read_dir(&converted).unwrap().count(), 0);
        let mut conn = opened.conn().await.unwrap();
        assert_eq!(derived_of(&mut conn, attachment_id).await, None);
    });
}

/// One file named by messages of two sources is stored once in the
/// account's folder, converted once, and every row that names it, from
/// either source, points at the one preview.
#[test]
fn a_file_two_sources_share_is_converted_once_for_both() {
    with_real_ffmpeg(async {
        let (opened, _dir, imessage_attachment) = fixture_with_png("imessage").await;
        let mut conn = opened.conn().await.unwrap();
        let message_id = seed_message(&mut conn, "sms").await;
        let sms_attachment =
            attach_stored_blob(&opened, &mut conn, message_id, SHA, ".png", PNG_1X1_RGB).await;

        assert_eq!(
            run(&opened, &ProcessAssetsOptions::default())
                .await
                .unwrap(),
            stats(1, 1, 0, 0)
        );

        let preview = derived_of(&mut conn, imessage_attachment).await;
        assert!(preview.is_some());
        assert_eq!(derived_of(&mut conn, sms_attachment).await, preview);
    });
}

#[tokio::test]
async fn a_database_without_accounts_is_an_error() {
    let (opened, _dir) = open_db().await;

    let err = run(&opened, &ProcessAssetsOptions::default())
        .await
        .unwrap_err();

    assert!(
        err.to_string().starts_with("no accounts found"),
        "got: {err}"
    );
}

#[tokio::test]
async fn an_account_without_an_assets_folder_is_passed_over() {
    let (opened, _dir) = open_db().await;
    let mut conn = opened.conn().await.unwrap();
    seed_account(&mut conn, ACCOUNT).await;

    assert_eq!(
        run(&opened, &ProcessAssetsOptions::default())
            .await
            .unwrap(),
        stats(0, 0, 0, 0)
    );
}

#[tokio::test]
async fn a_blob_that_is_not_media_is_left_as_is_by_the_run() {
    let (opened, _dir) = open_db().await;
    let mut conn = opened.conn().await.unwrap();
    seed_account(&mut conn, ACCOUNT).await;
    let message_id = seed_message(&mut conn, "imessage").await;
    let attachment_id =
        attach_stored_blob(&opened, &mut conn, message_id, SHA, ".txt", b"notes").await;

    assert_eq!(
        run(&opened, &ProcessAssetsOptions::default())
            .await
            .unwrap(),
        stats(1, 0, 1, 0)
    );
    assert_eq!(derived_of(&mut conn, attachment_id).await, None);
}

#[tokio::test]
async fn a_missing_original_is_counted_as_a_failure_and_the_run_goes_on() {
    let (opened, _dir, attachment_id) = fixture_with_png("imessage").await;
    let mut conn = opened.conn().await.unwrap();
    let original = opened
        .cfg
        .paths
        .assets_dir_for_account(ACCOUNT)
        .join(format!("ab/{SHA}.png"));
    fs::remove_file(&original).unwrap();
    let message_id = seed_message(&mut conn, "sms").await;
    attach_stored_blob(
        &opened,
        &mut conn,
        message_id,
        &"b".repeat(64),
        ".txt",
        b"notes",
    )
    .await;

    assert_eq!(
        run(&opened, &ProcessAssetsOptions::default())
            .await
            .unwrap(),
        stats(2, 0, 1, 1)
    );
    assert_eq!(derived_of(&mut conn, attachment_id).await, None);
}

#[tokio::test]
async fn account_ids_come_from_the_table_or_else_from_the_data_folders() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    for folder in ["7", "12", "notes"] {
        fs::create_dir_all(data.join(folder)).unwrap();
    }
    fs::write(data.join("3"), b"a file, not an account").unwrap();

    // No accounts table and no data folder: nothing.
    let (pool, _db_dir) = engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    assert_eq!(
        list_account_ids(&mut conn, &dir.path().join("elsewhere"))
            .await
            .unwrap(),
        Vec::<i64>::new()
    );

    // No accounts table yet: the folders named by an id are the accounts.
    assert_eq!(list_account_ids(&mut conn, &data).await.unwrap(), [7, 12]);

    // A table with rows in it is the answer, and the folders are ignored.
    schema::ensure_schema(&mut conn).await.unwrap();
    seed_account(&mut conn, 5).await;
    assert_eq!(list_account_ids(&mut conn, &data).await.unwrap(), [5]);
}

#[tokio::test]
async fn opening_an_account_without_an_assets_folder_gives_nothing_to_process() {
    let (opened, _dir) = open_db().await;
    let opts = ProcessAssetsOptions::default();
    let work = tempfile::tempdir().unwrap();

    let pass = AccountPass::open(&opened.cfg, &opts, work.path(), ACCOUNT).unwrap();

    assert!(pass.is_none());
    assert!(
        !opened
            .cfg
            .paths
            .assets_converted_dir_for_account(ACCOUNT)
            .exists(),
        "no converted folder is made for an account with nothing in it"
    );
}

#[tokio::test]
async fn opening_an_account_makes_its_converted_folder_and_cleans_its_incoming_temps() {
    let (opened, _dir) = open_db().await;
    let opts = ProcessAssetsOptions::default();
    let work = tempfile::tempdir().unwrap();
    let assets = opened.cfg.paths.assets_dir_for_account(ACCOUNT);
    let part = assets.join(".incoming").join(format!("{SHA}-1.part"));
    fs::create_dir_all(part.parent().unwrap()).unwrap();
    fs::write(&part, b"half").unwrap();
    make_abandoned(&part);
    let live_part = assets.join(".incoming").join(format!("{SHA}-2.part"));
    fs::write(&live_part, b"an upload in progress").unwrap();

    let pass = AccountPass::open(&opened.cfg, &opts, work.path(), ACCOUNT)
        .unwrap()
        .expect("an account with an assets folder is processed");

    let converted = opened.cfg.paths.assets_converted_dir_for_account(ACCOUNT);
    assert_eq!(pass.assets_dir, assets);
    assert_eq!(pass.converted_dir, converted);
    assert_eq!(pass.account_id, ACCOUNT);
    assert!(converted.is_dir());
    assert!(
        !part.exists(),
        "an abandoned upload temp is removed on open"
    );
    assert!(live_part.exists(), "a live upload's temp is kept on open");
}

/// A preview left cut short by an interrupted run is written again when the
/// same bytes are stored, so `--force` repairs it.
#[test]
fn a_truncated_derived_file_is_rewritten() {
    let dir = tempfile::tempdir().unwrap();
    let buf = vec![7u8; 4096];
    let rel = derived_rel_path(&crate::assets_api::Sha256::of_bytes(&buf), ".jpg");
    let dest = dir.path().join(&rel);
    std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
    std::fs::write(&dest, &buf[..100]).unwrap();
    store_derived_bytes(dir.path(), &buf, ".jpg").unwrap();
    let stored = std::fs::read(&dest).unwrap();
    assert_eq!(stored.len(), buf.len(), "the cut-short file is replaced");
    assert!(stored == buf, "the stored bytes are the preview's bytes");
}

/// Storing a preview leaves no temporary file beside it.
#[test]
fn storing_a_derived_file_leaves_only_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let buf = vec![9u8; 512];
    let blob = store_derived_bytes(dir.path(), &buf, ".jpg").unwrap();
    let dest = dir.path().join(&blob.assets_path);
    let names: Vec<_> = std::fs::read_dir(dest.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, vec![dest.file_name().unwrap().to_owned()]);
    assert_eq!(std::fs::read(&dest).unwrap(), buf);
}
