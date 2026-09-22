use super::*;
use media::ffmpeg_available;
use std::sync::atomic::Ordering;

/// A staging folder holding one conversation and one attachment.
///
/// Writes `attachments/<name>` with `bytes`, and one `.jsonl` whose
/// single message has non-empty text and one attachment pointing at
/// `attachments/<name>`. Built with `message_ir::testutil::sample_document`
/// and written with `write_conversation_jsonl_to`, so the fixture and the
/// code under test agree on the on-disk shape.
///
/// Returns (staging dir, conversation file path, attachment path).
fn staged_one(name: &str, bytes: &[u8]) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let attachments_dir = dir.path().join("attachments");
    std::fs::create_dir_all(&attachments_dir).unwrap();
    let rel = format!("attachments/{name}");
    let original = dir.path().join(&rel);
    std::fs::write(&original, bytes).unwrap();

    let mut doc = message_ir::testutil::sample_document("hello from the fixture");
    doc.messages[0].attachments = vec![IrAttachment {
        path: Some(rel),
        original_name: Some(name.to_string()),
        mime_type: None,
        digest_sha256: None,
        is_sticker: false,
        transcription: None,
        sticker_effect: None,
        size_bytes: Some(bytes.len() as u64),
        missing_reason: None,
        bytes: None,
    }];
    doc.finalize_stats();

    let jsonl = dir.path().join(format!("{}.jsonl", doc.filename_stem()));
    write_conversation_jsonl_to(&jsonl, &doc).unwrap();
    (dir, jsonl, original)
}

fn options(mode: MediaMode, limit: u64) -> TranscodeOptions {
    TranscodeOptions {
        mode,
        compress: CompressOptions::default(),
        asset_max_bytes: limit,
    }
}

fn test_png_bytes() -> Vec<u8> {
    #[rustfmt::skip]
    const PNG_1X1_RGB: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
        0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0x00,
        0x00, 0x03, 0x01, 0x01, 0x00, 0xc9, 0xfe, 0x92, 0xef, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
        0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    PNG_1X1_RGB.to_vec()
}

fn hex_sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[test]
fn a_converted_attachment_is_patched_before_its_final_name_exists() {
    if !ffmpeg_available() {
        return;
    }
    let (dir, jsonl, original) = staged_one("photo.png", &test_png_bytes());
    let report = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        None,
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(report.converted, 1);
    let doc = read_conversation_jsonl(&jsonl).unwrap();
    let att = &doc.messages[0].attachments[0];
    assert_eq!(att.path.as_deref(), Some("attachments/photo-mv.jpg"));
    assert!(
        !original.exists(),
        "original deleted after the patch committed"
    );
    assert!(dir.path().join("attachments/photo-mv.jpg").exists());
    assert!(
        std::fs::read_dir(dir.path().join("attachments"))
            .unwrap()
            .flatten()
            .all(|e| !e.file_name().to_string_lossy().ends_with(".in_progress")),
        "no marker survives a completed file"
    );
}

#[test]
fn the_digest_and_size_are_recomputed_from_the_derivative() {
    if !ffmpeg_available() {
        return;
    }
    // Decision 29: ffmpeg output is not byte-identical across runs, so a
    // replayed digest would be a silent corruption — the vault dedupes
    // assets by sha256.
    let (dir, jsonl, _) = staged_one("photo.png", &test_png_bytes());
    transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        None,
        &mut |_| {},
    )
    .unwrap();

    let doc = read_conversation_jsonl(&jsonl).unwrap();
    let att = &doc.messages[0].attachments[0];
    let derivative = dir.path().join("attachments/photo-mv.jpg");
    let on_disk = std::fs::read(&derivative).unwrap();
    assert_eq!(
        att.digest_sha256.as_deref(),
        Some(hex_sha256(&on_disk).as_str())
    );
    assert_eq!(att.size_bytes, Some(on_disk.len() as u64));
    assert_eq!(att.mime_type.as_deref(), Some("image/jpeg"));
}

#[test]
fn an_interrupted_file_is_re_transcoded_not_adopted() {
    if !ffmpeg_available() {
        return;
    }
    // Decision 28: nothing distinguishes a complete .in_progress from a
    // truncated one without hashing it, so the marker's bytes are never used.
    let (dir, jsonl, _) = staged_one("photo.png", &test_png_bytes());
    let marker = dir.path().join("attachments/photo-mv.jpg.in_progress");
    std::fs::write(&marker, b"truncated garbage from a killed run").unwrap();

    transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        None,
        &mut |_| {},
    )
    .unwrap();

    let derivative = dir.path().join("attachments/photo-mv.jpg");
    assert_ne!(
        std::fs::read(&derivative).unwrap(),
        b"truncated garbage from a killed run".to_vec(),
        "the marker's bytes must never be adopted"
    );
    let doc = read_conversation_jsonl(&jsonl).unwrap();
    assert_eq!(
        doc.messages[0].attachments[0].path.as_deref(),
        Some("attachments/photo-mv.jpg")
    );
}

#[test]
fn an_already_converted_attachment_is_left_alone_on_a_second_run() {
    if !ffmpeg_available() {
        return;
    }
    let (dir, jsonl, _) = staged_one("photo.png", &test_png_bytes());
    transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        None,
        &mut |_| {},
    )
    .unwrap();
    let after_first = std::fs::read(dir.path().join("attachments/photo-mv.jpg")).unwrap();

    let second = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        None,
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(second.converted, 0, "resume must not redo finished work");
    assert_eq!(
        std::fs::read(dir.path().join("attachments/photo-mv.jpg")).unwrap(),
        after_first
    );
    let doc = read_conversation_jsonl(&jsonl).unwrap();
    assert_eq!(
        doc.messages[0].attachments[0].path.as_deref(),
        Some("attachments/photo-mv.jpg")
    );
}

#[test]
fn a_derivative_over_the_limit_becomes_too_large_and_keeps_the_message() {
    if !ffmpeg_available() {
        return;
    }
    // Decision 45: skipped, not reverted. Falling back to the original
    // would store the format the user asked to be rid of.
    let (dir, jsonl, original) = staged_one("photo.png", &test_png_bytes());
    let report = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, 1),
        None,
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(report.too_large, 1);
    assert_eq!(report.converted, 0);
    let doc = read_conversation_jsonl(&jsonl).unwrap();
    let msg = &doc.messages[0];
    assert!(!msg.text.is_empty(), "the message keeps its text");
    let att = &msg.attachments[0];
    assert_eq!(att.missing_reason.as_deref(), Some("too_large"));
    assert_eq!(att.path, None, "nothing to upload");
    assert!(!original.exists(), "the original is not kept as a fallback");
    assert!(!dir.path().join("attachments/photo-mv.jpg").exists());
}

#[test]
fn a_conversion_failure_becomes_a_per_item_reason_carrying_the_detail() {
    // Needs ffmpeg present and failing on this specific input: after the
    // ffmpeg preflight check, an *absent* ffmpeg now fails the whole
    // pass (see ffmpeg_unavailable_fails_the_whole_pass_up_front) rather
    // than reaching this per-item path.
    if !ffmpeg_available() {
        return;
    }
    let (dir, jsonl, original) = staged_one("broken.png", b"not a png at all");
    let report = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        None,
        &mut |_| {},
    );

    // ffmpeg failing on one file is an issue, never a failed pass.
    let report = report.unwrap();
    assert_eq!(report.failed, 1);
    let doc = read_conversation_jsonl(&jsonl).unwrap();
    let reason = doc.messages[0].attachments[0]
        .missing_reason
        .clone()
        .unwrap();
    assert!(
        reason.starts_with("convert_failed: "),
        "reason must stay inside the closed set: {reason}"
    );
    assert!(
        reason.len() > "convert_failed: ".len(),
        "the detail must survive"
    );
    assert!(
        original.exists(),
        "a file that failed to convert is still there"
    );
}

#[test]
fn a_convert_failed_attachment_keeps_its_path_and_is_retried_on_resume() {
    if !ffmpeg_available() {
        return;
    }
    // The original is still on disk after a transient ffmpeg failure;
    // clearing `path` would sever the only reference to bytes that still
    // exist and stop `pending_in` from ever retrying it.
    let (dir, jsonl, original) = staged_one("broken.png", b"not a png at all");
    let first = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        None,
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(first.failed, 1);
    let doc = read_conversation_jsonl(&jsonl).unwrap();
    let att = &doc.messages[0].attachments[0];
    assert_eq!(
        att.path.as_deref(),
        Some("attachments/broken.png"),
        "the path survives a transient failure"
    );
    assert!(original.exists());

    // Resume: pending_in must still see this as work, because the path
    // exists, derivative_name says Some, and the stem carries no -mv.
    let second = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        None,
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(
        second.failed, 1,
        "a resume must retry a convert_failed file, not skip it"
    );
}

#[test]
fn cancelling_stops_the_pass_without_corrupting_the_folder() {
    let (dir, jsonl, _) = staged_one("photo.png", &test_png_bytes());
    let cancel = CancelFlag::default();
    cancel.store(true, Ordering::Relaxed);

    let err = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        Some(&cancel),
        &mut |_| {},
    );

    let err = err.expect_err("a cancel requested before the call must surface as Err");
    assert_eq!(
        err.to_string(),
        "canceled",
        "spelled to match run_attachment_jobs; the web hook's isCancellation string-matches it"
    );
    let doc = read_conversation_jsonl(&jsonl).unwrap();
    assert_eq!(
        doc.messages[0].attachments[0].path.as_deref(),
        Some("attachments/photo.png"),
        "an untouched attachment still points at its original"
    );
}

#[test]
fn progress_counts_the_work_it_actually_has() {
    // Convert mode still probes for ffmpeg up front (parity with
    // process_attachments_dir) even though a PDF alone needs no
    // transcode, so this needs the tools present to reach that far.
    if !ffmpeg_available() {
        return;
    }
    let (dir, _, _) = staged_one("notes.pdf", b"%PDF-1.4");
    let mut seen = Vec::new();
    let report = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        None,
        &mut |p| seen.push((p.done, p.total)),
    )
    .unwrap();
    // A file the media step does not handle is not work.
    assert_eq!(report.converted, 0);
    assert!(seen.iter().all(|(_, total)| *total == 0));
}

#[test]
fn a_crash_between_the_patch_and_the_rename_heals_by_re_transcoding_the_original() {
    if !ffmpeg_available() {
        return;
    }
    // Hand-simulate the crash window between decision 28's steps 4-5
    // (patch committed, conversation file written) and step 6 (marker
    // renamed into its final name): the doc already points at the -mv
    // name, a marker sits under .in_progress, and the original is still
    // on disk under its old name because the delete never ran.
    let (dir, jsonl, original) = staged_one("photo.png", &test_png_bytes());
    let mut doc = read_conversation_jsonl(&jsonl).unwrap();
    {
        let att = &mut doc.messages[0].attachments[0];
        att.path = Some("attachments/photo-mv.jpg".into());
        att.digest_sha256 = Some("deadbeef".repeat(8));
        att.size_bytes = Some(1234);
        att.mime_type = Some("image/jpeg".into());
    }
    write_conversation_jsonl_to(&jsonl, &doc).unwrap();
    let marker = dir.path().join("attachments/photo-mv.jpg.in_progress");
    std::fs::write(&marker, b"leftover bytes from the crashed run").unwrap();
    assert!(
        original.exists(),
        "the original is still there before the pass runs"
    );

    let report = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        None,
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(report.converted, 1);
    let derivative = dir.path().join("attachments/photo-mv.jpg");
    assert!(
        derivative.exists(),
        "the derivative exists under the -mv name"
    );
    assert_ne!(
        std::fs::read(&derivative).unwrap(),
        b"leftover bytes from the crashed run".to_vec(),
        "the heal re-transcodes rather than adopting the marker's bytes"
    );
    assert!(!marker.exists(), "no marker survives a completed heal");
    assert!(
        !original.exists(),
        "the original is gone once the heal commits"
    );
    let doc = read_conversation_jsonl(&jsonl).unwrap();
    assert_eq!(
        doc.messages[0].attachments[0].path.as_deref(),
        Some("attachments/photo-mv.jpg"),
        "the doc points at the healed derivative"
    );
}

#[test]
fn a_heal_that_fails_to_transcode_repoints_at_the_original_before_recording_the_failure() {
    if !ffmpeg_available() {
        return;
    }
    // Same crash-window simulation as the other heal tests, but this
    // time the recovered original is garbage ffmpeg will fail on. The
    // Err arm must not simply "keep the path" the way a non-heal
    // failure does: `recorded_rel` here is the phantom -mv name from the
    // crashed run, which will never exist. It must repoint at the
    // recovered original first, then record the failure on it.
    let (dir, jsonl, original) = staged_one("broken.png", b"not a png at all");
    let mut doc = read_conversation_jsonl(&jsonl).unwrap();
    {
        let att = &mut doc.messages[0].attachments[0];
        att.path = Some("attachments/broken-mv.jpg".into());
        att.digest_sha256 = Some("deadbeef".repeat(8));
        att.size_bytes = Some(1234);
        att.mime_type = Some("image/jpeg".into());
    }
    write_conversation_jsonl_to(&jsonl, &doc).unwrap();
    assert!(
        original.exists(),
        "the original is still there before the pass runs"
    );

    let report = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        None,
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(report.failed, 1);
    let doc = read_conversation_jsonl(&jsonl).unwrap();
    let att = &doc.messages[0].attachments[0];
    assert_eq!(
        att.path.as_deref(),
        Some("attachments/broken.png"),
        "repointed at the recovered original, not left on the phantom -mv name"
    );
    let reason = att.missing_reason.clone().unwrap();
    assert!(
        reason.starts_with("convert_failed: "),
        "reason must stay inside the closed set: {reason}"
    );
    assert_eq!(
        att.digest_sha256.as_deref(),
        Some(hex_sha256(&std::fs::read(&original).unwrap()).as_str()),
        "digest recomputed from the recovered original, not the stale pre-crash value"
    );
    assert!(
        original.exists(),
        "the recovered original is untouched by a failed transcode"
    );
}

#[test]
fn a_heal_that_the_media_step_skips_repoints_at_the_original_deterministically() {
    if !ffmpeg_available() {
        return;
    }
    // A small mp4 under compress's min_size_bytes returns
    // TranscodeOutcome::Skipped without looking at the video's content
    // at all — compress_video's `ext == "mp4"` branch short-circuits
    // before any ffmpeg probe or encode — so this Skip is deterministic
    // regardless of the installed ffmpeg's version or behaviour, unlike
    // (say) an already-efficient-codec skip, which depends on what that
    // ffmpeg actually reports.
    let (dir, jsonl, original) = staged_one("clip.mp4", b"not really a video, but small");
    let mut doc = read_conversation_jsonl(&jsonl).unwrap();
    {
        let att = &mut doc.messages[0].attachments[0];
        att.path = Some("attachments/clip-mv.mp4".into());
        att.digest_sha256 = Some("deadbeef".repeat(8));
        att.size_bytes = Some(1234);
        att.mime_type = Some("video/mp4".into());
    }
    write_conversation_jsonl_to(&jsonl, &doc).unwrap();
    assert!(
        original.exists(),
        "the original is still there before the pass runs"
    );
    // Default min_size_bytes is 20 MB; our fixture is a few dozen bytes.
    assert!(CompressOptions::default().min_size_bytes > 1000);

    let report = transcode_staged(
        dir.path(),
        &options(MediaMode::Compress, u64::MAX),
        None,
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(report.skipped, 1);
    let doc = read_conversation_jsonl(&jsonl).unwrap();
    let att = &doc.messages[0].attachments[0];
    assert_eq!(
        att.path.as_deref(),
        Some("attachments/clip.mp4"),
        "repointed at the recovered original, not left on the phantom -mv name"
    );
    assert!(att.missing_reason.is_none());
    assert_eq!(
        att.digest_sha256.as_deref(),
        Some(hex_sha256(&std::fs::read(&original).unwrap()).as_str())
    );
    assert!(original.exists(), "a skipped file's original is left alone");
}

#[test]
fn a_crash_that_lost_both_the_marker_and_the_original_is_unrecoverable() {
    // The whole pass still needs ffmpeg present up front (the preflight
    // check runs before any per-attachment classification), even though
    // no transcode is ever attempted for this particular attachment.
    if !ffmpeg_available() {
        return;
    }
    let (dir, jsonl, original) = staged_one("photo.png", &test_png_bytes());
    let mut doc = read_conversation_jsonl(&jsonl).unwrap();
    {
        let att = &mut doc.messages[0].attachments[0];
        att.path = Some("attachments/photo-mv.jpg".into());
        // Seed non-None digest/size so the clearing assertions below can
        // actually fail if `set_missing` stops clearing them.
        att.digest_sha256 = Some("cafebabe".repeat(8));
        att.size_bytes = Some(999);
    }
    write_conversation_jsonl_to(&jsonl, &doc).unwrap();
    // Nothing recoverable is left: no marker, and the original itself is gone too.
    std::fs::remove_file(&original).unwrap();

    let report = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        None,
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(report.missing, 1);
    assert_eq!(report.converted, 0);
    let doc = read_conversation_jsonl(&jsonl).unwrap();
    let att = &doc.messages[0].attachments[0];
    assert_eq!(att.missing_reason.as_deref(), Some("file_missing"));
    assert_eq!(att.path, None);
    assert_eq!(att.digest_sha256, None);
}

#[test]
fn two_attachments_in_one_document_sharing_a_path_are_patched_together() {
    if !ffmpeg_available() {
        return;
    }
    let (dir, jsonl, original) = staged_one("photo.png", &test_png_bytes());
    // A second message in the same document, carrying an attachment
    // recorded at the exact same content-addressed path — a legitimate
    // state, not a fixture error.
    let mut doc = read_conversation_jsonl(&jsonl).unwrap();
    let mut second_msg = doc.messages[0].clone();
    second_msg.guid = "second-message-guid".into();
    second_msg.timestamp_unix_ms += 1000;
    doc.messages.push(second_msg);
    doc.finalize_stats();
    write_conversation_jsonl_to(&jsonl, &doc).unwrap();

    let report = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        None,
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(
        report.converted, 1,
        "one physical file, one transcode, however many attachments reference it"
    );
    assert!(!original.exists());
    let doc = read_conversation_jsonl(&jsonl).unwrap();
    assert_eq!(doc.messages.len(), 2);
    for msg in &doc.messages {
        let att = &msg.attachments[0];
        assert_eq!(
            att.path.as_deref(),
            Some("attachments/photo-mv.jpg"),
            "every attachment sharing the path gets patched"
        );
        assert!(att.digest_sha256.is_some());
    }
}

#[test]
fn two_documents_sharing_one_original_both_end_pointing_at_the_committed_derivative() {
    if !ffmpeg_available() {
        return;
    }
    let (dir, jsonl_a, original) = staged_one("shared.png", &test_png_bytes());

    // A second, independent conversation staged in the same folder whose
    // attachment happens to record the identical path — two different
    // chats that received the same bytes.
    let mut doc_b = message_ir::testutil::sample_document("second conversation, same photo");
    doc_b.conversation.chat_identifier = "+15555550199".into();
    doc_b.messages[0].guid = "doc-b-guid".into();
    doc_b.messages[0].attachments = vec![IrAttachment {
        path: Some("attachments/shared.png".into()),
        original_name: Some("shared.png".into()),
        mime_type: None,
        digest_sha256: None,
        is_sticker: false,
        transcription: None,
        sticker_effect: None,
        size_bytes: Some(test_png_bytes().len() as u64),
        missing_reason: None,
        bytes: None,
    }];
    doc_b.finalize_stats();
    let jsonl_b = dir.path().join(format!("{}.jsonl", doc_b.filename_stem()));
    write_conversation_jsonl_to(&jsonl_b, &doc_b).unwrap();

    let report = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        None,
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(report.converted, 1, "one physical file is transcoded once");
    assert_eq!(
        report.repointed, 1,
        "the second document is repointed, not re-transcoded"
    );
    assert!(!original.exists());

    let final_doc_a = read_conversation_jsonl(&jsonl_a).unwrap();
    let final_doc_b = read_conversation_jsonl(&jsonl_b).unwrap();
    let att_a = &final_doc_a.messages[0].attachments[0];
    let att_b = &final_doc_b.messages[0].attachments[0];
    assert_eq!(att_a.path.as_deref(), Some("attachments/shared-mv.jpg"));
    assert_eq!(att_b.path.as_deref(), Some("attachments/shared-mv.jpg"));
    assert!(att_a.digest_sha256.is_some());
    assert_eq!(
        att_a.digest_sha256, att_b.digest_sha256,
        "both documents recompute the same digest from the same on-disk derivative"
    );
}

#[cfg(unix)]
#[test]
fn a_write_failure_leaves_the_final_name_uncommitted_and_the_original_untouched() {
    if !ffmpeg_available() {
        return;
    }
    // The headline "patched before the final name exists" test only
    // checks terminal state, which would pass even if the patch and the
    // rename were swapped. This makes the ordering falsifiable: force
    // the conversation-file write to fail (a read-only staging dir, so
    // `write_conversation_jsonl_to`'s `.tmp` sibling can't be created)
    // after the transcode has already produced a derivative, and assert
    // the final name was never created and the original is untouched.
    use std::os::unix::fs::PermissionsExt;
    let (dir, _jsonl, original) = staged_one("photo.png", &test_png_bytes());
    let mut perms = std::fs::metadata(dir.path()).unwrap().permissions();
    perms.set_mode(0o555);
    std::fs::set_permissions(dir.path(), perms).unwrap();

    let result = transcode_staged(
        dir.path(),
        &options(MediaMode::Convert, u64::MAX),
        None,
        &mut |_| {},
    );

    // Restore before any assertion can panic, so the TempDir can still
    // clean itself up.
    let mut restore = std::fs::metadata(dir.path()).unwrap().permissions();
    restore.set_mode(0o755);
    std::fs::set_permissions(dir.path(), restore).unwrap();

    assert!(
        result.is_err(),
        "the conversation-file write failure must surface, not be swallowed"
    );
    assert!(
        !dir.path().join("attachments/photo-mv.jpg").exists(),
        "the final name must never exist without a committed patch"
    );
    assert!(
        original.exists(),
        "the original is untouched when the patch never committed"
    );
}

/// Write a JPEG through ffmpeg at `-q:v 2` (low compression), sized well
/// over the media crate's compress-mode same-format floor (500 KB) and
/// reliably smaller when re-encoded at compress mode's finer `-q:v 5` —
/// the ordinary "worse quality shrinks" direction, calibrated locally
/// against this repo's ffmpeg build (1024x768 random noise: ~856 KB at
/// `-q:v 2`, ~646 KB re-encoded at `-q:v 5`). See `media::process`'s
/// `write_jpeg_that_grows_on_finer_reencode` for the opposite,
/// incompressible-noise calibration this deliberately avoids.
fn jpeg_over_compress_floor_bytes() -> Vec<u8> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.jpg");
    let output = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "nullsrc=size=1024x768,geq=random(1)*255:random(1)*255:random(1)*255",
            "-frames:v",
            "1",
            "-update",
            "1",
            "-q:v",
            "2",
        ])
        .arg(&path)
        .output()
        .expect("run ffmpeg for jpeg fixture");
    assert!(
        output.status.success(),
        "ffmpeg jpeg fixture generation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::read(&path).unwrap()
}

#[test]
fn two_documents_sharing_one_compressed_original_both_end_pointing_at_the_committed_derivative() {
    if !ffmpeg_available() {
        return;
    }
    // The compress-mode variant of the convert-mode test above: this is
    // the exact bug the final review caught. `final_derivative_name`
    // used to stat the (already-deleted) shared original for the
    // compress-mode JPEG floor, read size 0, read that as "under the
    // floor", and answered `None` — so document B's repoint never
    // queued and it was left pointing at a file that no longer existed,
    // with no `missing_reason`.
    let bytes = jpeg_over_compress_floor_bytes();
    assert!(
        bytes.len() as u64 > 500 * 1024,
        "fixture must clear the compress-mode same-format floor"
    );
    let (dir, jsonl_a, original) = staged_one("shared.jpg", &bytes);

    let mut doc_b = message_ir::testutil::sample_document("second conversation, same photo");
    doc_b.conversation.chat_identifier = "+15555550199".into();
    doc_b.messages[0].guid = "doc-b-guid".into();
    doc_b.messages[0].attachments = vec![IrAttachment {
        path: Some("attachments/shared.jpg".into()),
        original_name: Some("shared.jpg".into()),
        mime_type: None,
        digest_sha256: None,
        is_sticker: false,
        transcription: None,
        sticker_effect: None,
        size_bytes: Some(bytes.len() as u64),
        missing_reason: None,
        bytes: None,
    }];
    doc_b.finalize_stats();
    let jsonl_b = dir.path().join(format!("{}.jsonl", doc_b.filename_stem()));
    write_conversation_jsonl_to(&jsonl_b, &doc_b).unwrap();

    let report = transcode_staged(
        dir.path(),
        &options(MediaMode::Compress, u64::MAX),
        None,
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(report.converted, 1, "one physical file is compressed once");
    assert_eq!(
        report.repointed, 1,
        "the second document is repointed, not left dangling or re-compressed"
    );
    assert!(!original.exists());

    let final_doc_a = read_conversation_jsonl(&jsonl_a).unwrap();
    let final_doc_b = read_conversation_jsonl(&jsonl_b).unwrap();
    let att_a = &final_doc_a.messages[0].attachments[0];
    let att_b = &final_doc_b.messages[0].attachments[0];
    assert_eq!(att_a.path.as_deref(), Some("attachments/shared-mv.jpg"));
    assert_eq!(att_b.path.as_deref(), Some("attachments/shared-mv.jpg"));
    assert!(att_a.missing_reason.is_none());
    assert!(att_b.missing_reason.is_none());
    assert!(att_a.digest_sha256.is_some());
    assert_eq!(
        att_a.digest_sha256, att_b.digest_sha256,
        "both documents recompute the same digest from the same on-disk derivative"
    );
}

#[test]
fn a_missing_original_with_no_committed_derivative_becomes_file_missing() {
    if !ffmpeg_available() {
        return;
    }
    // Covers the other half of the same bug: a recorded path that is
    // gone for good (nothing shares it, and no committed derivative
    // exists either — the shared-original-deleted-by-too_large case, or
    // any other reason the file vanished). Before the fix this fell
    // through the repoint branch's `if let Some(name) = …` silently,
    // leaving the attachment dangling with no `missing_reason` at all.
    let (dir, jsonl, original) = staged_one(
        "ghost.jpg",
        b"content is irrelevant; deleted before the pass looks",
    );
    std::fs::remove_file(&original).unwrap();

    let report = transcode_staged(
        dir.path(),
        &options(MediaMode::Compress, u64::MAX),
        None,
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(report.missing, 1);
    assert_eq!(report.repointed, 0);
    assert_eq!(report.converted, 0);
    let doc = read_conversation_jsonl(&jsonl).unwrap();
    let att = &doc.messages[0].attachments[0];
    assert_eq!(att.missing_reason.as_deref(), Some("file_missing"));
    assert_eq!(att.path, None);
    assert_eq!(att.digest_sha256, None);
}

#[test]
fn the_committed_suffix_guard_excludes_an_already_final_video_from_pending() {
    // No ffmpeg needed: pending_in decides this from names alone.
    // media::derivative_name always answers Some("…mp4") for a video in
    // either mode (it cannot see CompressOptions), so without the -mv
    // exclusion a committed video derivative would look pending forever
    // and get re-degraded on every resume.
    let (dir, jsonl, _original) = staged_one("clip-mv.mp4", b"");
    let doc = read_conversation_jsonl(&jsonl).unwrap();

    let work = pending_in(dir.path(), &doc, MediaMode::Convert).unwrap();

    assert!(
        work.is_empty(),
        "a committed -mv name must never re-enter the pending list"
    );
}
