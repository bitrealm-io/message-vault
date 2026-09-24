use super::*;
use crate::kind_for_mime;

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
    let Some(_tools) = crate::testutil::real_ffmpeg_test_guard() else {
        return;
    };
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
    let Some(_tools) = crate::testutil::real_ffmpeg_test_guard() else {
        return;
    };
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
    let Some(_tools) = crate::testutil::real_ffmpeg_test_guard() else {
        return;
    };
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
fn is_efficient_accepts_only_hevc_within_the_resolution_and_bitrate_caps() {
    let opts = CompressOptions::default(); // 1080p: long edge 1920
    let cap = 12_000_000;
    assert!(is_efficient("hevc", 1920, 1080, cap, &opts));
    assert!(is_efficient("h265", 1080, 1920, 1_000_000, &opts));
    assert!(!is_efficient("hevc", 1921, 1080, 1_000_000, &opts));
    assert!(!is_efficient("hevc", 1080, 1921, 1_000_000, &opts));
    assert!(!is_efficient("hevc", 1920, 1080, cap + 1, &opts));
    assert!(!is_efficient("h264", 640, 480, 1_000_000, &opts));
    assert!(!is_efficient("", 0, 0, 0, &opts));
    let p720 = CompressOptions {
        max_resolution: crate::MaxResolution::P720,
        ..CompressOptions::default()
    };
    assert!(is_efficient("hevc", 1280, 720, 1_000_000, &p720));
    assert!(!is_efficient("hevc", 1281, 720, 1_000_000, &p720));
}

/// Write a one-second 320x240 test pattern with the given video codec.
fn write_test_video(path: &Path, codec: &[&str]) {
    let mut args: Vec<String> = [
        "-y",
        "-loglevel",
        "error",
        "-f",
        "lavfi",
        "-i",
        "testsrc=size=320x240:rate=10:duration=1",
        "-pix_fmt",
        "yuv420p",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect();
    args.extend(codec.iter().map(|s| (*s).to_string()));
    args.push(path_str(path));
    run_ffmpeg(&args).expect("generate test video");
}

/// One file through a media pass: its remap target, or `None` when skipped.
fn process_single(
    dir: &Path,
    file: &Path,
    mode: MediaMode,
    opts: &CompressOptions,
) -> Option<String> {
    let (report, mut remap) =
        process_attachment_files(dir, &[file.to_path_buf()], mode, opts, None).unwrap();
    assert_eq!(report.errors, Vec::<String>::new());
    let old_rel = rel_path(dir, file).unwrap();
    let new_rel = remap.remove(&old_rel);
    assert_eq!(report.processed, usize::from(new_rel.is_some()));
    assert_eq!(report.skipped, usize::from(new_rel.is_none()));
    new_rel
}

#[test]
fn convert_turns_a_mov_into_an_mp4_and_removes_the_original() {
    let Some(_tools) = crate::testutil::real_ffmpeg_test_guard() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let attachments = dir.path().join("attachments");
    fs::create_dir_all(&attachments).unwrap();
    let mov = attachments.join("clip.mov");
    write_test_video(&mov, &["-c:v", "libx264"]);

    let new_rel = process_single(
        dir.path(),
        &mov,
        MediaMode::Convert,
        &CompressOptions::default(),
    );

    assert_eq!(new_rel.as_deref(), Some("attachments/clip.mp4"));
    assert!(!mov.exists(), "the .mov was left behind");
    let probe = probe_video(&attachments.join("clip.mp4")).unwrap();
    assert_eq!(probe.codec, "h264", "a remux keeps the stream");
}

#[test]
fn compress_only_remuxes_a_video_under_the_minimum_size() {
    let Some(_tools) = crate::testutil::real_ffmpeg_test_guard() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let attachments = dir.path().join("attachments");
    fs::create_dir_all(&attachments).unwrap();
    let mov = attachments.join("small.mov");
    write_test_video(&mov, &["-c:v", "libx264"]);
    let mp4 = attachments.join("other.mp4");
    write_test_video(&mp4, &["-c:v", "libx264"]);
    let mp4_before = fs::read(&mp4).unwrap();
    let opts = CompressOptions {
        min_size_bytes: 100 * 1024 * 1024,
        ..CompressOptions::default()
    };

    let new_rel = process_single(dir.path(), &mov, MediaMode::Compress, &opts);
    assert_eq!(new_rel.as_deref(), Some("attachments/small.mp4"));
    assert!(!mov.exists());
    let probe = probe_video(&attachments.join("small.mp4")).unwrap();
    assert_eq!(
        probe.codec, "h264",
        "a small video is remuxed, not re-encoded"
    );

    // A small MP4 is already in the target container: nothing to do.
    assert_eq!(
        process_single(dir.path(), &mp4, MediaMode::Compress, &opts),
        None
    );
    assert_eq!(fs::read(&mp4).unwrap(), mp4_before);
}

#[test]
fn compress_re_encodes_a_large_video_and_skips_an_efficient_one() {
    let Some(_tools) = crate::testutil::real_ffmpeg_test_guard() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let attachments = dir.path().join("attachments");
    fs::create_dir_all(&attachments).unwrap();
    let opts = CompressOptions {
        min_size_bytes: 0,
        ..CompressOptions::default()
    };

    let avc = attachments.join("avc.mp4");
    write_test_video(&avc, &["-c:v", "libx264"]);
    // Exactly the minimum size is large enough.
    let at_minimum = CompressOptions {
        min_size_bytes: fs::metadata(&avc).unwrap().len(),
        max_fps: 5.0,
        ..CompressOptions::default()
    };
    let new_rel = process_single(dir.path(), &avc, MediaMode::Compress, &at_minimum);
    assert_eq!(new_rel.as_deref(), Some("attachments/avc.mp4"));
    let probe = crate::probe_media(&avc).unwrap();
    assert_eq!(probe.codec, "hevc", "a large H.264 video is re-encoded");
    assert_eq!(probe.fps, Some(5.0), "the frame rate is capped at max_fps");

    // Already HEVC, small and low bitrate: re-encoding would buy nothing.
    let hevc = attachments.join("hevc.mp4");
    write_test_video(&hevc, &["-c:v", "libx265", "-tag:v", "hvc1"]);
    let before = fs::read(&hevc).unwrap();
    assert_eq!(
        process_single(dir.path(), &hevc, MediaMode::Compress, &opts),
        None
    );
    assert_eq!(fs::read(&hevc).unwrap(), before);

    // With the skip turned off, the same file is re-encoded. A max fps of
    // zero means no cap was given, and the pass uses 30.
    let opts = CompressOptions {
        skip_efficient: false,
        max_fps: 0.0,
        ..opts
    };
    assert_eq!(
        process_single(dir.path(), &hevc, MediaMode::Compress, &opts).as_deref(),
        Some("attachments/hevc.mp4")
    );
    assert_ne!(fs::read(&hevc).unwrap(), before);
    assert_eq!(crate::probe_media(&hevc).unwrap().fps, Some(30.0));
}

#[test]
fn compress_re_encodes_a_large_mp3_when_the_result_is_smaller() {
    let Some(_tools) = crate::testutil::real_ffmpeg_test_guard() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let attachments = dir.path().join("attachments");
    fs::create_dir_all(&attachments).unwrap();
    let mp3 = attachments.join("song.mp3");
    run_ffmpeg(&[
        "-y".into(),
        "-loglevel".into(),
        "error".into(),
        "-f".into(),
        "lavfi".into(),
        "-i".into(),
        "sine=frequency=440:duration=10".into(),
        "-ac".into(),
        "2".into(),
        "-b:a".into(),
        "320k".into(),
        path_str(&mp3),
    ])
    .expect("generate mp3 fixture");
    let before = fs::metadata(&mp3).unwrap().len();
    assert!(before > MP3_COMPRESS_FLOOR, "fixture must clear the floor");

    let new_rel = process_single(
        dir.path(),
        &mp3,
        MediaMode::Compress,
        &CompressOptions::default(),
    );

    assert_eq!(new_rel.as_deref(), Some("attachments/song.mp3"));
    assert!(fs::metadata(&mp3).unwrap().len() < before);

    // Convert leaves an MP3 alone, whatever its size.
    let bytes = fs::read(&mp3).unwrap();
    assert_eq!(
        process_single(
            dir.path(),
            &mp3,
            MediaMode::Convert,
            &CompressOptions::default()
        ),
        None
    );
    assert_eq!(fs::read(&mp3).unwrap(), bytes);
}

#[test]
fn process_attachment_files_touches_only_the_listed_files() {
    let Some(_tools) = crate::testutil::real_ffmpeg_test_guard() else {
        return;
    };
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

#[test]
fn transcode_file_as_converts_an_extensionless_source_by_the_given_kind() {
    let Some(_tools) = crate::testutil::real_ffmpeg_test_guard() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    // The vault stores originals under their fingerprint alone.
    let src = dir.path().join("3b1f");
    write_test_png(&src);
    assert_eq!(classify(&src), None, "no extension, so no kind to read");
    let dest = dir.path().join("3b1f.jpg.in_progress");

    let outcome = transcode_file_as(
        &src,
        Kind::Image,
        &dest,
        MediaMode::Convert,
        &CompressOptions::default(),
    )
    .unwrap();

    assert_eq!(outcome, TranscodeOutcome::Produced);
    assert!(dest.exists());
    assert!(src.exists(), "the original is never touched");
    assert_eq!(
        transcode_file_as(
            &src,
            Kind::Image,
            &dest,
            MediaMode::Clone,
            &CompressOptions::default()
        )
        .unwrap(),
        TranscodeOutcome::Skipped,
        "clone mode never converts"
    );
}

#[test]
fn kind_for_mime_reads_the_top_level_type() {
    assert_eq!(kind_for_mime("image/heic"), Some(Kind::Image));
    assert_eq!(
        kind_for_mime("video/quicktime; codecs=hvc1"),
        Some(Kind::Video)
    );
    assert_eq!(kind_for_mime("AUDIO/amr"), Some(Kind::Audio));
    assert_eq!(kind_for_mime("application/pdf"), None);
    assert_eq!(kind_for_mime(""), None);
}

#[test]
fn kind_of_reads_the_extension_first_and_never_converts_a_gif() {
    assert_eq!(kind_of(Path::new("x.jpg"), None, &[]), Some(Kind::Image));
    assert_eq!(kind_of(Path::new("x.mp4"), None, &[]), Some(Kind::Video));
    assert_eq!(kind_of(Path::new("x.m4a"), None, &[]), Some(Kind::Audio));
    assert_eq!(kind_of(Path::new("x.gif"), None, &[]), None);
    assert_eq!(kind_of(Path::new("x.GIF"), Some("image/png"), &[]), None);
    assert_eq!(kind_of(Path::new("x.png"), Some("image/gif"), &[]), None);
    assert_eq!(
        kind_of(Path::new("x.bin"), Some("image/png"), &[]),
        Some(Kind::Image)
    );
    assert_eq!(kind_of(Path::new("x.bin"), Some("  "), &[]), None);
}

const SHA: &str = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";

#[test]
fn kind_of_falls_back_to_the_attachment_names_for_an_extensionless_blob() {
    let canonical = PathBuf::from(format!("ab/{SHA}"));
    for (name, expected) in [
        ("voice-note.amr", Some(Kind::Audio)),
        ("memo.wav", Some(Kind::Audio)),
        ("podcast.ogg", Some(Kind::Audio)),
        ("clip.3gp", Some(Kind::Video)),
        ("clip.webm", Some(Kind::Video)),
        ("movie.mkv", Some(Kind::Video)),
        ("scan.tiff", Some(Kind::Image)),
        ("notes.txt", None),
    ] {
        assert_eq!(
            kind_of(&canonical, None, &[Some(name), None]),
            expected,
            "unexpected kind for {name}"
        );
        // The original export path is the second-choice hint.
        assert_eq!(
            kind_of(
                &canonical,
                Some("  "),
                &[None, Some(&format!("media/{name}"))]
            ),
            expected,
            "unexpected kind for path hint media/{name}"
        );
    }
}

#[test]
fn kind_of_never_lets_a_name_hint_override_the_declared_media_type() {
    let canonical = PathBuf::from(format!("ab/{SHA}"));
    // A declared MIME is authoritative, including the deliberate GIF skip.
    assert_eq!(
        kind_of(&canonical, Some("image/gif"), &[Some("clip.mp4")]),
        None
    );
    assert_eq!(
        kind_of(&canonical, Some("application/pdf"), &[Some("clip.mp4")]),
        None
    );
    assert_eq!(
        kind_of(Path::new("ab/photo.gif"), None, &[Some("clip.mp4")]),
        None
    );
    // A GIF hint is passed over for the next hint, not treated as an image.
    assert_eq!(
        kind_of(&canonical, None, &[Some("still.gif"), Some("clip.mp4")]),
        Some(Kind::Video)
    );
}

/// Write a noise JPEG through ffmpeg at `-q:v 2`. Noise at that fine a
/// quantization shrinks when compress mode re-encodes it at `-q:v 5` (see
/// `write_jpeg_that_grows_on_finer_reencode` for the measurements).
fn write_jpeg_that_shrinks_on_compress(path: &Path) {
    let args = vec![
        "-y".into(),
        "-f".into(),
        "lavfi".into(),
        "-i".into(),
        "nullsrc=size=800x600,geq=random(1)*255:random(1)*255:random(1)*255".into(),
        "-frames:v".into(),
        "1".into(),
        "-update".into(),
        "1".into(),
        "-q:v".into(),
        "2".into(),
        path_str(path),
    ];
    run_ffmpeg(&args).expect("generate noise jpeg fixture");
}

#[test]
fn convert_never_overwrites_a_file_that_already_has_the_target_name() {
    let Some(_tools) = crate::testutil::real_ffmpeg_test_guard() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let attachments = dir.path().join("attachments");
    fs::create_dir_all(&attachments).unwrap();
    // a.png beside a.jpg, c.png beside c.jpg and c_1.jpg, and a plain b.png.
    for name in ["a.png", "b.png", "c.png"] {
        write_test_png(&attachments.join(name));
    }
    for name in ["a.jpg", "c.jpg", "c_1.jpg"] {
        fs::write(attachments.join(name), format!("the user's own {name}")).unwrap();
    }
    let files: Vec<PathBuf> = ["a.png", "b.png", "c.png"]
        .iter()
        .map(|name| attachments.join(name))
        .collect();

    let (report, remap) = process_attachment_files(
        dir.path(),
        &files,
        MediaMode::Convert,
        &CompressOptions::default(),
        None,
    )
    .unwrap();

    assert_eq!(report.errors, Vec::<String>::new());
    assert_eq!(report.processed, 3);
    assert_eq!(report.skipped, 0);
    let expected: HashMap<String, String> = [
        ("attachments/a.png", "attachments/a_1.jpg"),
        ("attachments/b.png", "attachments/b.jpg"),
        ("attachments/c.png", "attachments/c_2.jpg"),
    ]
    .into_iter()
    .map(|(from, to)| (from.to_string(), to.to_string()))
    .collect();
    assert_eq!(remap, expected);
    for name in ["a.jpg", "c.jpg", "c_1.jpg"] {
        assert_eq!(
            fs::read_to_string(attachments.join(name)).unwrap(),
            format!("the user's own {name}"),
            "{name} was overwritten"
        );
    }
    for name in ["a_1.jpg", "b.jpg", "c_2.jpg"] {
        let bytes = fs::read(attachments.join(name)).unwrap();
        assert!(bytes.starts_with(&[0xff, 0xd8]), "{name} is not a JPEG");
    }
    for name in ["a.png", "b.png", "c.png"] {
        assert!(!attachments.join(name).exists(), "{name} was left behind");
    }
}

#[test]
fn compress_shrinks_large_jpegs_converts_pngs_and_leaves_gifs_and_small_jpegs() {
    let Some(_tools) = crate::testutil::real_ffmpeg_test_guard() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let attachments = dir.path().join("attachments");
    fs::create_dir_all(&attachments).unwrap();
    let large = attachments.join("large.jpg");
    write_jpeg_that_shrinks_on_compress(&large);
    let large_before = fs::metadata(&large).unwrap().len();
    assert!(
        large_before > JPEG_COMPRESS_FLOOR,
        "fixture must clear the floor, or the size gate skips it"
    );
    let png = attachments.join("still.png");
    write_test_png(&png);
    // Neither of these reaches ffmpeg, so their bytes need not decode.
    let gif = attachments.join("loop.gif");
    fs::write(&gif, b"GIF89a animated").unwrap();
    let small = attachments.join("small.jpg");
    fs::write(&small, b"\xff\xd8\xff small jpeg").unwrap();
    let files = vec![large.clone(), png, gif.clone(), small.clone()];

    let (report, remap) = process_attachment_files(
        dir.path(),
        &files,
        MediaMode::Compress,
        &CompressOptions::default(),
        None,
    )
    .unwrap();

    assert_eq!(report.errors, Vec::<String>::new());
    assert_eq!((report.processed, report.skipped), (2, 2));
    assert!(
        fs::metadata(&large).unwrap().len() < large_before,
        "the large JPEG was not replaced by the smaller re-encode"
    );
    assert_eq!(
        remap.get("attachments/large.jpg").map(String::as_str),
        Some("attachments/large.jpg")
    );
    assert_eq!(
        remap.get("attachments/still.png").map(String::as_str),
        Some("attachments/still.jpg")
    );
    assert_eq!(fs::read(&gif).unwrap(), b"GIF89a animated");
    assert_eq!(fs::read(&small).unwrap(), b"\xff\xd8\xff small jpeg");
    assert_eq!(remap.len(), 2);
}

/// Staging forecasts output names from `derivative_name`, and recovers after
/// a crash by looking for them. It must answer as the pass itself decides.
#[test]
fn derivative_name_follows_the_media_step_for_every_mode_and_extension() {
    let dir = tempfile::tempdir().unwrap();
    let sized = |name: &str, len: u64| {
        let path = dir.path().join(name);
        fs::File::create(&path).unwrap().set_len(len).unwrap();
        path
    };
    let small = 10;
    let jpeg_floor = JPEG_COMPRESS_FLOOR;
    let mp3_floor = MP3_COMPRESS_FLOOR;
    let cases: &[(&str, u64, MediaMode, Option<&str>)] = &[
        ("x.png", small, MediaMode::Clone, None),
        ("x.png", small, MediaMode::Disabled, None),
        ("x.png", small, MediaMode::Convert, Some("x.jpg")),
        ("x.heic", small, MediaMode::Convert, Some("x.jpg")),
        ("x.gif", small, MediaMode::Convert, None),
        ("x.jpg", small, MediaMode::Convert, None),
        ("x.JPEG", small, MediaMode::Convert, None),
        ("x.m4a", small, MediaMode::Convert, Some("x.mp3")),
        ("x.mp3", small, MediaMode::Convert, None),
        ("x.mov", small, MediaMode::Convert, Some("x.mp4")),
        ("x.pdf", small, MediaMode::Convert, None),
        ("x.png", small, MediaMode::Compress, Some("x.jpg")),
        ("x.gif", jpeg_floor + 1, MediaMode::Compress, None),
        ("x.jpg", small, MediaMode::Compress, None),
        ("x.jpg", jpeg_floor, MediaMode::Compress, None),
        ("x.jpg", jpeg_floor + 1, MediaMode::Compress, Some("x.jpg")),
        ("x.jpeg", jpeg_floor + 1, MediaMode::Compress, Some("x.jpg")),
        ("x.m4a", small, MediaMode::Compress, Some("x.mp3")),
        ("x.mp3", mp3_floor, MediaMode::Compress, None),
        ("x.mp3", mp3_floor + 1, MediaMode::Compress, Some("x.mp3")),
        ("x.mp4", small, MediaMode::Compress, Some("x.mp4")),
    ];
    for (name, len, mode, expected) in cases {
        let path = sized(name, *len);
        assert_eq!(
            derivative_name(&path, *mode).as_deref(),
            *expected,
            "{name} ({len} bytes) in {mode:?}"
        );
    }
}

#[test]
fn derivative_name_for_missing_ignores_the_size_floors() {
    let dir = tempfile::tempdir().unwrap();
    let jpg = dir.path().join("gone.jpg");
    let mp3 = dir.path().join("gone.mp3");
    assert_eq!(
        derivative_name_for_missing(&jpg, MediaMode::Compress).as_deref(),
        Some("gone.jpg")
    );
    assert_eq!(
        derivative_name_for_missing(&mp3, MediaMode::Compress).as_deref(),
        Some("gone.mp3")
    );
    assert_eq!(derivative_name_for_missing(&jpg, MediaMode::Convert), None);
    // The stat-reading variant sees a missing file as size 0, under the floor.
    assert_eq!(derivative_name(&jpg, MediaMode::Compress), None);
}

#[test]
fn audio_becomes_mp3_and_a_small_mp3_is_left_alone() {
    let Some(_tools) = crate::testutil::real_ffmpeg_test_guard() else {
        return;
    };
    for mode in [MediaMode::Convert, MediaMode::Compress] {
        let dir = tempfile::tempdir().unwrap();
        let attachments = dir.path().join("attachments");
        fs::create_dir_all(&attachments).unwrap();
        let m4a = attachments.join("voice.m4a");
        run_ffmpeg(&[
            "-y".into(),
            "-f".into(),
            "lavfi".into(),
            "-i".into(),
            "sine=frequency=440:duration=1".into(),
            "-c:a".into(),
            "aac".into(),
            path_str(&m4a),
        ])
        .expect("generate m4a fixture");
        // Under the MP3 floor, and not decodable: ffmpeg must never see it.
        let mp3 = attachments.join("small.mp3");
        fs::write(&mp3, b"ID3 small mp3").unwrap();

        let (report, remap) = process_attachment_files(
            dir.path(),
            &[m4a.clone(), mp3.clone()],
            mode,
            &CompressOptions::default(),
            None,
        )
        .unwrap();

        assert_eq!(report.errors, Vec::<String>::new(), "{mode:?}");
        assert_eq!((report.processed, report.skipped), (1, 1), "{mode:?}");
        assert_eq!(
            remap.get("attachments/voice.m4a").map(String::as_str),
            Some("attachments/voice.mp3"),
            "{mode:?}"
        );
        assert!(!m4a.exists(), "{mode:?}");
        assert_eq!(fs::read(&mp3).unwrap(), b"ID3 small mp3", "{mode:?}");
    }
}

#[test]
fn collect_media_files_keeps_media_and_leaves_everything_else() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("attachments").join("a1");
    fs::create_dir_all(&nested).unwrap();
    fs::write(dir.path().join("photo.png"), b"png").unwrap();
    fs::write(dir.path().join("receipt.pdf"), b"pdf").unwrap();
    fs::write(nested.join("clip.mp4"), b"mp4").unwrap();
    fs::write(nested.join("clip.msgmedia.tmp.mp4"), b"partial").unwrap();

    let files = collect_media_files(dir.path()).unwrap();

    assert_eq!(
        files,
        vec![nested.join("clip.mp4"), dir.path().join("photo.png")],
        "only media, found in subfolders too, sorted; never a PDF or an \
         ffmpeg temp file"
    );
}
