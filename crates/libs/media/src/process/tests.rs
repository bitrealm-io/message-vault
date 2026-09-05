use super::*;
use crate::tools::ffmpeg_available;

/// Write a minimal valid 1x1 PNG, readable by ffmpeg, for conversion tests.
///
/// Plain RGB (PNG color type 2), not RGBA: this build's ffmpeg PNG decoder
/// chokes on a 1x1 RGBA image ("chunk too big" / decode error) but reads
/// this one cleanly.
fn write_test_png(path: &Path) {
    #[rustfmt::skip]
    const PNG_1X1_RGB: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
        0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0x00,
        0x00, 0x03, 0x01, 0x01, 0x00, 0xc9, 0xfe, 0x92, 0xef, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
        0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    fs::write(path, PNG_1X1_RGB).unwrap();
}

/// Write a coarsely-quantized JPEG through ffmpeg at `-q:v 20` that grows
/// when re-encoded at compress mode's finer `-q:v 5`.
///
/// Calibrated empirically against this repo's ffmpeg build: random noise
/// written to independent Y/Cb/Cr planes (`nullsrc`'s default `yuv420p`,
/// fed by `geq`) runs about 0.44 bytes/pixel at `-q:v 20`, and
/// re-encoding it at `-q:v 5` (much less quantization) comes out roughly
/// 50% *larger* — noise has no redundancy for the finer quantization
/// step to exploit, so asking for more detail just spends more bits
/// recording the same randomness. That is the opposite of the usual
/// "worse quality = smaller file" case a typical photo re-encode hits,
/// which is exactly why it exercises the keep-smaller guard. (An earlier
/// version of this helper tried the reverse — a `-q:v 2` source
/// re-encoded at `-q:v 5` — expecting noise's incompressibility to make
/// it a wash; it consistently shrank by ~25% instead, at every
/// resolution tried. Coarser quantization shrinks even incompressible
/// content, so don't retry that direction.)
fn write_jpeg_that_grows_on_finer_reencode(path: &Path, target_size: u64) {
    let pixels = (target_size as f64 / 0.44).max(4.0);
    let mut width = ((pixels * 4.0 / 3.0).sqrt() as u32).max(2);
    width -= width % 2;
    let mut height = width * 3 / 4;
    height -= height % 2;
    let args = vec![
        "-y".into(),
        "-f".into(),
        "lavfi".into(),
        "-i".into(),
        format!("nullsrc=size={width}x{height},geq=random(1)*255:random(1)*255:random(1)*255"),
        "-frames:v".into(),
        "1".into(),
        "-update".into(),
        "1".into(),
        "-q:v".into(),
        "20".into(),
        path_str(path),
    ];
    run_ffmpeg(&args).expect("generate incompressible jpeg fixture");
}

#[test]
fn compress_keeps_the_original_jpeg_when_the_re_encode_is_not_smaller() {
    let _tools = crate::tools::tools_test_lock();
    if !ffmpeg_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let attachments = dir.path().join("attachments");
    fs::create_dir_all(&attachments).unwrap();

    // A JPEG that is already tight for its pixel count: re-encoding at -q:v 5
    // produces a file no smaller than the source. Over 500 KB so the size gate
    // in process_one does not skip it outright.
    let jpeg = attachments.join("already-tight.jpg");
    write_jpeg_that_grows_on_finer_reencode(&jpeg, 900 * 1024);
    let before = fs::read(&jpeg).unwrap();
    assert!(
        fs::metadata(&jpeg).unwrap().len() > JPEG_COMPRESS_FLOOR,
        "fixture must clear the floor gate: otherwise run_one skips at the \
         floor and every assertion below holds whether or not the \
         keep-smaller guard exists"
    );

    let files = collect_media_files(&attachments).unwrap();
    let (report, remap) = process_attachment_files(
        dir.path(),
        &files,
        MediaMode::Compress,
        &CompressOptions::default(),
        None,
    )
    .unwrap();

    assert_eq!(fs::read(&jpeg).unwrap(), before, "original bytes replaced");
    assert!(
        !remap.contains_key("attachments/already-tight.jpg"),
        "a kept file must not be remapped: a remap entry tells the caller to \
         recompute a digest that did not change"
    );
    assert_eq!(report.processed, 0);
    assert_eq!(report.skipped, 1);
}

#[test]
fn transcode_file_writes_the_derivative_and_leaves_the_original_alone() {
    let _tools = crate::tools::tools_test_lock();
    if !ffmpeg_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("photo.png");
    write_test_png(&src);
    let before = fs::read(&src).unwrap();

    let name = derivative_name(&src, MediaMode::Convert).expect("png is converted");
    assert_eq!(name, "photo.jpg");
    let dest = dir.path().join(format!("{name}.in_progress"));

    let outcome =
        transcode_file(&src, &dest, MediaMode::Convert, &CompressOptions::default()).unwrap();

    assert_eq!(outcome, TranscodeOutcome::Produced);
    assert!(dest.exists(), "derivative written where the caller asked");
    assert!(
        !dir.path().join("photo.jpg").exists(),
        "the final name must not exist until the caller renames it: a file \
         under its final name means fully patched"
    );
    assert_eq!(
        fs::read(&src).unwrap(),
        before,
        "the original is the caller's to delete, after it commits"
    );
}

#[test]
fn derivative_name_is_none_for_a_file_the_mode_leaves_alone() {
    let dir = tempfile::tempdir().unwrap();
    let gif = dir.path().join("loop.gif");
    fs::write(&gif, b"not really a gif").unwrap();
    assert_eq!(derivative_name(&gif, MediaMode::Convert), None);

    let jpeg = dir.path().join("photo.jpg");
    fs::write(&jpeg, b"not really a jpeg").unwrap();
    assert_eq!(derivative_name(&jpeg, MediaMode::Convert), None);

    let doc = dir.path().join("notes.pdf");
    fs::write(&doc, b"%PDF").unwrap();
    assert_eq!(derivative_name(&doc, MediaMode::Convert), None);
}

#[test]
fn derivative_name_matches_what_the_media_step_actually_produces() {
    let _tools = crate::tools::tools_test_lock();
    if !ffmpeg_available() {
        return;
    }
    // The forecast and the patch both trust derivative_name. If it disagrees
    // with the pass, a conversation file points at a name nothing wrote.
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("photo.png");
    write_test_png(&src);
    let name = derivative_name(&src, MediaMode::Convert).unwrap();
    let dest = dir.path().join("out").join(&name);
    let outcome =
        transcode_file(&src, &dest, MediaMode::Convert, &CompressOptions::default()).unwrap();
    // dest is built from name, so the file-name equality below would hold
    // even if transcode_file wrote nothing. Pin down that it actually ran.
    assert_eq!(outcome, TranscodeOutcome::Produced);
    assert!(
        dest.exists(),
        "derivative_name promised a name nothing wrote"
    );
    assert_eq!(
        dest.file_name().and_then(|n| n.to_str()),
        Some(name.as_str())
    );
}

#[test]
fn transcode_file_clears_scratch_beside_the_source_only() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("photo.png");
    write_test_png(&src);
    let own_scratch = dir.path().join("photo.msgmedia.tmp.jpg");
    fs::write(&own_scratch, b"leftover").unwrap();
    let other_scratch = dir.path().join("other.msgmedia.tmp.jpg");
    fs::write(&other_scratch, b"in flight").unwrap();
    // Same stem as `src`, but the scratch extension a video producer
    // would write (e.g. an iOS Live Photo's IMG_0001.MOV, mid-encode,
    // sharing photo's stem). A stem-only match would wrongly sweep this;
    // photo.png is Kind::Image, so only its own "jpg" scratch is a
    // candidate.
    let same_stem_video_scratch = dir.path().join("photo.msgmedia.tmp.mp4");
    fs::write(&same_stem_video_scratch, b"another kind, in flight").unwrap();
    let marker = dir.path().join("photo.jpg.in_progress");
    fs::write(&marker, b"a previous attempt").unwrap();

    // Clone mode returns before any ffmpeg work, which is enough to show what
    // the entry point sweeps.
    let _ = transcode_file(
        &src,
        &dir.path().join("photo.jpg.in_progress"),
        MediaMode::Clone,
        &CompressOptions::default(),
    );

    assert!(!own_scratch.exists(), "this file's own leftovers go");
    assert!(
        other_scratch.exists(),
        "another file's in-flight scratch must survive: a folder-wide sweep \
         destroys work that is still running"
    );
    assert!(
        same_stem_video_scratch.exists(),
        "a same-stem sibling's scratch of a different kind must survive: a \
         stem-only match would delete an in-flight Live-Photo pair's video \
         scratch while converting the image half"
    );
    assert!(
        marker.exists(),
        "the .in_progress marker is the resume signal and must survive the \
         scratch sweep (decision 30)"
    );
}

#[test]
fn commit_produced_refuses_a_destination_equal_to_the_original() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("photo.jpg");
    fs::write(&original, b"jpeg-bytes").unwrap();
    let produced = dir.path().join("photo.msgmedia.tmp.jpg");
    fs::write(&produced, b"re-encoded-bytes").unwrap();

    // A caller that joined `derivative_name`'s output onto the source
    // directory without adding a distinct temp suffix (e.g. forgot
    // `.in_progress`) would ask to overwrite the original before any
    // commit has happened. That must be refused, not silently done.
    let err = commit_produced(Commit::To(&original), &original, &produced).unwrap_err();
    assert!(
        err.to_string().contains("original file"),
        "error should explain why: {err}"
    );
    assert!(original.exists(), "original must be untouched");
    assert_eq!(fs::read(&original).unwrap(), b"jpeg-bytes");
    assert!(
        produced.exists(),
        "the would-be derivative is left for the caller to clean up"
    );
}

#[test]
fn classify_kinds() {
    assert!(matches!(classify(Path::new("a.HEIC")), Some(Kind::Image)));
    assert!(matches!(classify(Path::new("v.mov")), Some(Kind::Video)));
    assert!(matches!(classify(Path::new("x.caf")), Some(Kind::Audio)));
    assert!(classify(Path::new("doc.pdf")).is_none());
    assert!(classify(Path::new("a.msgmedia.tmp.jpg")).is_none());
}

#[test]
fn detects_msgmedia_temp_names() {
    assert!(is_msgmedia_temp(Path::new(
        "20150917_095137-I_1.msgmedia.tmp.jpg"
    )));
    assert!(!is_msgmedia_temp(Path::new("20150917_095137-I_1.jpg")));
}

#[test]
fn sweeps_leftover_msgmedia_temps() {
    let dir = tempfile::tempdir().unwrap();
    let att = dir.path().join("attachments");
    fs::create_dir_all(&att).unwrap();
    let junk = att.join("photo.msgmedia.tmp.jpg");
    fs::write(&junk, b"partial").unwrap();
    fs::write(att.join("keep.jpg"), b"ok").unwrap();

    remove_msgmedia_temps(&att).unwrap();
    assert!(!junk.exists());
    assert!(att.join("keep.jpg").exists());
}

#[test]
fn clone_is_noop() {
    let dir = tempfile::tempdir().unwrap();
    let (report, remap) = process_attachment_files(
        dir.path(),
        &[],
        MediaMode::Clone,
        &CompressOptions::default(),
        None,
    )
    .unwrap();
    assert_eq!(report.processed, 0);
    assert!(remap.is_empty());
}

#[test]
fn same_path_rewrite_reports_changed() {
    let dir = tempfile::tempdir().unwrap();
    let att = dir.path().join("attachments");
    fs::create_dir_all(&att).unwrap();
    let file = att.join("photo.jpg");
    fs::write(&file, b"jpeg-bytes").unwrap();
    let outcome = changed(dir.path(), "attachments/photo.jpg", &file).unwrap();
    match outcome {
        Outcome::Changed { old_rel, new_rel } => {
            assert_eq!(old_rel, "attachments/photo.jpg");
            assert_eq!(new_rel, "attachments/photo.jpg");
        }
        Outcome::Skipped => panic!("in-place rewrite must not look like Skipped"),
    }
}

#[test]
fn format_bytes_scales() {
    assert_eq!(format_bytes(500), "500 B");
    assert_eq!(format_bytes(12_500), "12.5 KB");
    assert_eq!(format_bytes(1_500_000), "1.5 MB");
    assert_eq!(format_bytes(2_500_000_000), "2.5 GB");
}

#[test]
fn attachments_dir_bytes_sums_non_temp_files() {
    let dir = tempfile::tempdir().unwrap();
    let att = dir.path().join("attachments");
    fs::create_dir_all(&att).unwrap();
    fs::write(att.join("a.jpg"), vec![0u8; 1000]).unwrap();
    fs::write(att.join("b.mp4"), vec![0u8; 2500]).unwrap();
    fs::write(att.join("orphan.msgmedia.tmp.jpg"), vec![0u8; 9999]).unwrap();
    assert_eq!(attachments_dir_bytes(&att).unwrap(), 3500);
}

#[test]
fn clone_with_log_emits_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut lines = Vec::new();
    let mut log = |line: &str| lines.push(line.to_string());
    let _ = process_attachment_files(
        dir.path(),
        &[],
        MediaMode::Clone,
        &CompressOptions::default(),
        Some(&mut log),
    )
    .unwrap();
    assert!(lines.is_empty());
}
#[test]
fn process_attachment_files_touches_only_the_listed_files() {
    let _tools = crate::tools::tools_test_lock();
    if !ffmpeg_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let attachments = dir.path().join("attachments");
    fs::create_dir_all(&attachments).unwrap();
    let listed = attachments.join("a.png");
    let unlisted = attachments.join("b.png");
    write_test_png(&listed);
    write_test_png(&unlisted);
    let unlisted_before = fs::read(&unlisted).unwrap();

    let (_report, remap) = process_attachment_files(
        dir.path(),
        std::slice::from_ref(&listed),
        MediaMode::Convert,
        &CompressOptions::default(),
        None,
    )
    .unwrap();

    assert!(
        remap.contains_key("attachments/a.png"),
        "the listed file must be converted"
    );
    assert!(
        !remap.contains_key("attachments/b.png"),
        "a file the caller did not list must be left alone: scoping the pass \
         to an explicit list is the whole point of taking one"
    );
    assert!(unlisted.is_file(), "unlisted file must survive the pass");
    assert_eq!(
        fs::read(&unlisted).unwrap(),
        unlisted_before,
        "unlisted file was rewritten"
    );
}
