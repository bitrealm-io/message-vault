use super::*;

const LIMIT: u64 = 50 * 1024 * 1024;

fn probe(codec: &str, width: u32, height: u32, fps: Option<f32>, bitrate: u64) -> MediaProbe {
    MediaProbe {
        codec: codec.into(),
        width,
        height,
        fps,
        bitrate,
    }
}

#[test]
fn a_small_file_is_fine_without_probing() {
    // Under the probe band: no ffprobe call, decided on size alone.
    assert_eq!(
        classify_probed(
            1024,
            None,
            "heic",
            MediaMode::Convert,
            &CompressOptions::default(),
            LIMIT
        ),
        SizeVerdict::FitsAsIs
    );
}

#[test]
fn heic_under_the_limit_may_grow_past_it() {
    // Decision 12's headline case: HEIC is about half an equivalent JPEG,
    // so converting grows it. 30 MB in, over 50 MB out.
    let p = probe("hevc", 4032, 3024, None, 0);
    assert_eq!(
        classify_probed(
            30 * 1024 * 1024,
            Some(&p),
            "heic",
            MediaMode::Convert,
            &CompressOptions::default(),
            LIMIT
        ),
        SizeVerdict::MayGrow
    );
}

#[test]
fn uppercase_extension_is_matched_case_insensitively() {
    // Same fixture and expectation as the test above, spelled the way a
    // real file name would be: `IMG_0001.HEIC`. `format_factor` matches
    // extensions as lowercase literals, so an ext this function does not
    // normalize first would miss the "heic" arm, fall through to the
    // video codec arm instead, and read as `FitsAsIs`.
    let p = probe("hevc", 4032, 3024, None, 0);
    assert_eq!(
        classify_probed(
            30 * 1024 * 1024,
            Some(&p),
            "HEIC",
            MediaMode::Convert,
            &CompressOptions::default(),
            LIMIT
        ),
        SizeVerdict::MayGrow
    );
}

#[test]
fn a_huge_video_compressed_down_is_likely_to_fit() {
    // 4K60 at 400 MB, compressed to 1080p30: pixel ratio 0.25, fps ratio
    // 0.5, format factor 0.7 (HEVC over the efficient-skip's resolution
    // cap, so it actually gets re-encoded) — 400 * 0.25 * 0.5 * 0.7 =
    // 35 MB, comfortably under the 40 MB margin. The bitrate is set high
    // so the efficient-skip gate (which the resolution already fails)
    // isn't what's carrying this test.
    let p = probe("hevc", 3840, 2160, Some(60.0), 20_000_000);
    assert_eq!(
        classify_probed(
            400 * 1024 * 1024,
            Some(&p),
            "mov",
            MediaMode::Compress,
            &CompressOptions::default(),
            LIMIT
        ),
        SizeVerdict::LikelyFits
    );
}

#[test]
fn a_video_that_stays_over_the_limit_says_so() {
    let p = probe("h264", 1920, 1080, Some(30.0), 0);
    assert_eq!(
        classify_probed(
            900 * 1024 * 1024,
            Some(&p),
            "mp4",
            MediaMode::Compress,
            &CompressOptions::default(),
            LIMIT
        ),
        SizeVerdict::ProbablyTooBig
    );
}

#[test]
fn an_estimate_just_under_the_limit_still_reads_as_too_big() {
    // The 80% margin: a near miss must not read as a promise.
    let p = probe("h264", 1920, 1080, Some(30.0), 0);
    let size = 60 * 1024 * 1024;
    let estimate = estimate_bytes(
        size,
        Some(&p),
        "mp4",
        MediaMode::Compress,
        &CompressOptions::default(),
    );
    assert!(estimate < LIMIT, "test needs an estimate under the limit");
    assert!(estimate > (LIMIT as f64 * PROBABLY_FITS_MARGIN) as u64);
    assert_eq!(
        classify_probed(
            size,
            Some(&p),
            "mp4",
            MediaMode::Compress,
            &CompressOptions::default(),
            LIMIT
        ),
        SizeVerdict::ProbablyTooBig
    );
}

#[test]
fn efficient_hevc_is_skipped_by_compress_so_it_stays_too_big() {
    // Decision the review caught: `compress_video` skips re-encoding
    // (and only remuxes) an HEVC source that is already within the
    // resolution cap and under the ~12 Mbps efficient-bitrate threshold
    // (`is_efficient` in process.rs). A forecast that does not know this
    // scales the file down as if it were being re-encoded — 55 MB * 0.7
    // = 38.5 MB, under the 40 MB margin — and promises `LikelyFits` for
    // a file the pass will not actually touch. It stays 55 MB, over the
    // 50 MB limit: `ProbablyTooBig`.
    let p = probe("hevc", 1920, 1080, Some(30.0), 9_000_000);
    assert_eq!(
        classify_probed(
            55 * 1024 * 1024,
            Some(&p),
            "mp4",
            MediaMode::Compress,
            &CompressOptions::default(),
            LIMIT
        ),
        SizeVerdict::ProbablyTooBig
    );
}

#[test]
fn a_file_the_media_step_cannot_touch_says_so() {
    assert_eq!(
        classify_probed(
            80 * 1024 * 1024,
            None,
            "pdf",
            MediaMode::Convert,
            &CompressOptions::default(),
            LIMIT
        ),
        SizeVerdict::CannotProcess
    );
}

#[test]
fn gif_is_never_processed_so_it_is_judged_on_its_own_size() {
    // process_one skips GIF in both modes. Its size will not change.
    assert_eq!(
        classify_probed(
            80 * 1024 * 1024,
            None,
            "gif",
            MediaMode::Convert,
            &CompressOptions::default(),
            LIMIT
        ),
        SizeVerdict::ProbablyTooBig
    );
    assert_eq!(
        classify_probed(
            1024,
            None,
            "gif",
            MediaMode::Convert,
            &CompressOptions::default(),
            LIMIT
        ),
        SizeVerdict::FitsAsIs
    );
}

#[test]
fn gif_in_compress_mode_is_also_judged_on_its_own_size() {
    // Same guarantee as the Convert-mode test above, but for Compress —
    // a regression that narrowed `untouched_by`'s GIF arm to Convert
    // only would still pass every other test here (GIF's fallback
    // `format_factor` also happens to be a no-op for an un-probed file),
    // so this uses a probed GIF (ffprobe does report a stream for an
    // animated GIF) specifically to make the wrong branch compute a
    // different number: 55 MB * format_factor 0.7 = 38.5 MB, under the
    // margin, reads `LikelyFits` instead of the correct `ProbablyTooBig`.
    let p = probe("gif", 800, 600, Some(15.0), 0);
    assert_eq!(
        classify_probed(
            55 * 1024 * 1024,
            Some(&p),
            "gif",
            MediaMode::Compress,
            &CompressOptions::default(),
            LIMIT
        ),
        SizeVerdict::ProbablyTooBig
    );
}

#[test]
fn estimate_bytes_agrees_with_classify_probed_that_convert_leaves_jpeg_alone() {
    // classify_probed's untouched_by guard already keeps a big Convert-mode
    // JPEG ProbablyTooBig rather than LikelyFits; estimate_bytes must say
    // the same size, not silently apply format_factor's 0.7 "already
    // JPEG" compress-mode shrink to a file Convert never touches.
    let size = 55 * 1024 * 1024;
    assert_eq!(
        estimate_bytes(
            size,
            None,
            "jpg",
            MediaMode::Convert,
            &CompressOptions::default()
        ),
        size
    );
}

#[test]
fn jpeg_in_convert_is_untouched_so_a_big_one_stays_too_big() {
    // Convert mode leaves an already-JPEG file alone (`run_one`'s early
    // return). Dropping that arm from `untouched_by` would instead run
    // it through `format_factor`'s 0.7 "already JPEG" shrink and read
    // `LikelyFits` for a file whose size never actually changes.
    assert_eq!(
        classify_probed(
            55 * 1024 * 1024,
            None,
            "jpg",
            MediaMode::Convert,
            &CompressOptions::default(),
            LIMIT
        ),
        SizeVerdict::ProbablyTooBig
    );
}

#[test]
fn mp3_in_convert_is_untouched_so_a_big_one_stays_too_big() {
    // Same shape as the JPEG case above, for MP3 (`format_factor`'s 0.6
    // "already MP3" shrink is a Compress-mode fact, not a Convert one).
    assert_eq!(
        classify_probed(
            60 * 1024 * 1024,
            None,
            "mp3",
            MediaMode::Convert,
            &CompressOptions::default(),
            LIMIT
        ),
        SizeVerdict::ProbablyTooBig
    );
}

#[test]
fn clone_and_disabled_leave_every_file_alone() {
    // `run_one`'s last arm skips every file in Clone/Disabled mode
    // regardless of kind or extension. A forecast that does not know
    // this still applies HEIC's 1.8 convert-growth factor and predicts
    // `MayGrow` for a file that will be copied byte-for-byte.
    let p = probe("hevc", 4032, 3024, None, 0);
    assert_eq!(
        classify_probed(
            30 * 1024 * 1024,
            Some(&p),
            "heic",
            MediaMode::Clone,
            &CompressOptions::default(),
            LIMIT
        ),
        SizeVerdict::FitsAsIs
    );
    assert_eq!(
        classify_probed(
            30 * 1024 * 1024,
            None,
            "heic",
            MediaMode::Disabled,
            &CompressOptions::default(),
            LIMIT
        ),
        SizeVerdict::FitsAsIs
    );
}

#[test]
fn the_estimate_is_not_capped_at_the_original_size() {
    // Decision 12 says so in as many words. A cap would erase MayGrow.
    let p = probe("hevc", 4032, 3024, None, 0);
    let size = 10 * 1024 * 1024;
    assert!(
        estimate_bytes(
            size,
            Some(&p),
            "heic",
            MediaMode::Convert,
            &CompressOptions::default()
        ) > size
    );
}

#[test]
fn a_file_in_the_band_is_worth_probing_and_a_small_one_is_not() {
    assert!(!needs_probe(1024, LIMIT));
    assert!(needs_probe(30 * 1024 * 1024, LIMIT));
    assert!(needs_probe(900 * 1024 * 1024, LIMIT));
}
