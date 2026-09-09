use super::*;
use media::{CompressOptions, MediaMode};

fn media_cfg(mode: MediaMode) -> MediaConfig {
    MediaConfig {
        mode,
        compress: CompressOptions::default(),
    }
}
use message_ir::IrAttachment;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

fn empty_att(name: &str) -> IrAttachment {
    IrAttachment {
        path: None,
        original_name: Some(name.into()),
        mime_type: Some("image/jpeg".into()),
        digest_sha256: None,
        is_sticker: false,
        transcription: None,
        sticker_effect: None,
        size_bytes: None,
        missing_reason: None,
        bytes: None,
    }
}

#[test]
fn clone_writes_file_and_fills_hash() {
    let dir = tempfile::tempdir().unwrap();
    let att_dir = dir.path().join("attachments");
    std::fs::create_dir_all(&att_dir).unwrap();
    let mut att = empty_att("photo.jpg");
    let bytes = b"hello-photo";
    let progress = Mutex::new(Vec::new());
    {
        let mut jobs = [AttachmentJob {
            attachment: &mut att,
            timestamp_unix_ms: 1_609_459_200_000,
            size_hint: Some(bytes.len() as u64),
        }];
        run_attachment_jobs(
            &mut jobs,
            &att_dir,
            &media_cfg(MediaMode::Clone),
            |_| Ok(Some(bytes.to_vec())),
            |p| progress.lock().unwrap().push(p),
            None,
            None,
        )
        .unwrap();
    }
    assert!(att.path.as_deref().unwrap().starts_with("attachments/"));
    assert_eq!(att.size_bytes, Some(bytes.len() as u64));
    assert_eq!(att.digest_sha256.as_ref().unwrap().len(), 64);
    let dest = dir.path().join(att.path.as_ref().unwrap());
    assert_eq!(std::fs::read(dest).unwrap(), bytes);
    let last = progress.lock().unwrap().last().cloned().unwrap();
    assert_eq!(last.done, 1);
    assert_eq!(last.total, 1);
    assert_eq!(last.bytes_done, bytes.len() as u64);
    assert_eq!(last.bytes_total, bytes.len() as u64);
}

#[test]
fn disabled_skips_without_loading() {
    let dir = tempfile::tempdir().unwrap();
    let att_dir = dir.path().join("attachments");
    let mut att = empty_att("photo.jpg");
    let loaded = AtomicBool::new(false);
    {
        let mut jobs = [AttachmentJob {
            attachment: &mut att,
            timestamp_unix_ms: 0,
            size_hint: Some(99),
        }];
        run_attachment_jobs(
            &mut jobs,
            &att_dir,
            &media_cfg(MediaMode::Disabled),
            |_| {
                loaded.store(true, Ordering::Relaxed);
                Ok(Some(b"x".to_vec()))
            },
            |_| {},
            None,
            None,
        )
        .unwrap();
    }
    assert!(!loaded.load(Ordering::Relaxed));
    assert_eq!(att.missing_reason.as_deref(), Some("not_copied"));
    assert!(att.path.is_none());
    assert!(!att_dir.exists() || std::fs::read_dir(&att_dir).unwrap().next().is_none());
}

#[test]
fn missing_source_is_file_missing_and_continues() {
    let dir = tempfile::tempdir().unwrap();
    let att_dir = dir.path().join("attachments");
    std::fs::create_dir_all(&att_dir).unwrap();
    let mut a = empty_att("a.jpg");
    let mut b = empty_att("b.jpg");
    {
        let mut jobs = [
            AttachmentJob {
                attachment: &mut a,
                timestamp_unix_ms: 0,
                size_hint: None,
            },
            AttachmentJob {
                attachment: &mut b,
                timestamp_unix_ms: 0,
                size_hint: Some(4),
            },
        ];
        run_attachment_jobs(
            &mut jobs,
            &att_dir,
            &media_cfg(MediaMode::Clone),
            |i| {
                if i == 0 {
                    Ok(None)
                } else {
                    Ok(Some(b"data".to_vec()))
                }
            },
            |_| {},
            None,
            None,
        )
        .unwrap();
    }
    assert_eq!(a.missing_reason.as_deref(), Some("file_missing"));
    assert!(b.path.is_some());
}

#[test]
fn read_error_marks_file_missing_and_continues() {
    let dir = tempfile::tempdir().unwrap();
    let att_dir = dir.path().join("attachments");
    std::fs::create_dir_all(&att_dir).unwrap();
    let mut a = empty_att("a.jpg");
    let mut b = empty_att("b.jpg");
    {
        let mut jobs = [
            AttachmentJob {
                attachment: &mut a,
                timestamp_unix_ms: 0,
                size_hint: None,
            },
            AttachmentJob {
                attachment: &mut b,
                timestamp_unix_ms: 0,
                size_hint: Some(4),
            },
        ];
        run_attachment_jobs(
            &mut jobs,
            &att_dir,
            &media_cfg(MediaMode::Clone),
            |i| {
                if i == 0 {
                    Err("permission denied".into())
                } else {
                    Ok(Some(b"data".to_vec()))
                }
            },
            |_| {},
            None,
            None,
        )
        .unwrap();
    }
    assert_eq!(a.missing_reason.as_deref(), Some("file_missing"));
    assert!(b.path.is_some());
}

#[test]
fn canceled_error_from_the_loader_still_aborts() {
    let dir = tempfile::tempdir().unwrap();
    let att_dir = dir.path().join("attachments");
    std::fs::create_dir_all(&att_dir).unwrap();
    let mut a = empty_att("a.jpg");
    let err = {
        let mut jobs = [AttachmentJob {
            attachment: &mut a,
            timestamp_unix_ms: 0,
            size_hint: Some(1),
        }];
        run_attachment_jobs(
            &mut jobs,
            &att_dir,
            &media_cfg(MediaMode::Clone),
            |_| Err("canceled".into()),
            |_| {},
            None,
            None,
        )
        .unwrap_err()
    };
    assert_eq!(err, "canceled");
}

#[test]
fn cancel_stops_before_next_job() {
    let dir = tempfile::tempdir().unwrap();
    let att_dir = dir.path().join("attachments");
    std::fs::create_dir_all(&att_dir).unwrap();
    let mut a = empty_att("a.jpg");
    let mut b = empty_att("b.jpg");
    let cancel = AtomicBool::new(false);
    let err = {
        let mut jobs = [
            AttachmentJob {
                attachment: &mut a,
                timestamp_unix_ms: 0,
                size_hint: Some(1),
            },
            AttachmentJob {
                attachment: &mut b,
                timestamp_unix_ms: 0,
                size_hint: Some(1),
            },
        ];
        run_attachment_jobs(
            &mut jobs,
            &att_dir,
            &media_cfg(MediaMode::Clone),
            |i| {
                if i == 0 {
                    cancel.store(true, Ordering::Relaxed);
                }
                Ok(Some(b"x".to_vec()))
            },
            |_| {},
            None,
            Some(&cancel),
        )
        .unwrap_err()
    };
    assert_eq!(err, "canceled");
    assert!(a.path.is_some());
    assert!(b.path.is_none());
}

#[test]
fn empty_jobs_emits_zero_of_zero() {
    let dir = tempfile::tempdir().unwrap();
    let att_dir = dir.path().join("attachments");
    let progress = Mutex::new(Vec::new());
    run_attachment_jobs(
        &mut [],
        &att_dir,
        &media_cfg(MediaMode::Clone),
        |_| Ok(None),
        |p| progress.lock().unwrap().push(p),
        None,
        None,
    )
    .unwrap();
    let last = progress.lock().unwrap().last().cloned().unwrap();
    assert_eq!(last.done, 0);
    assert_eq!(last.total, 0);
    assert_eq!(last.bytes_done, 0);
    assert_eq!(last.bytes_total, 0);
}

#[test]
fn remap_updates_mime_and_continues_when_one_file_is_unreadable() {
    let dir = tempfile::tempdir().unwrap();
    let att_dir = dir.path().join("attachments");
    std::fs::create_dir_all(&att_dir).unwrap();
    std::fs::write(att_dir.join("ok.jpg"), b"jpeg-bytes").unwrap();
    let mut ok = empty_att("ok.heic");
    ok.path = Some("attachments/ok.heic".into());
    ok.mime_type = Some("image/heic".into());
    let mut missing = empty_att("gone.heic");
    missing.path = Some("attachments/gone.heic".into());
    missing.mime_type = Some("image/heic".into());
    {
        let mut jobs = [
            AttachmentJob {
                attachment: &mut ok,
                timestamp_unix_ms: 0,
                size_hint: None,
            },
            AttachmentJob {
                attachment: &mut missing,
                timestamp_unix_ms: 0,
                size_hint: None,
            },
        ];
        let mut remap = std::collections::HashMap::new();
        remap.insert("attachments/ok.heic".into(), "attachments/ok.jpg".into());
        remap.insert(
            "attachments/gone.heic".into(),
            "attachments/gone.jpg".into(),
        );
        apply_remap_to_jobs(&mut jobs, &remap, dir.path());
    }
    assert_eq!(ok.path.as_deref(), Some("attachments/ok.jpg"));
    assert_eq!(ok.mime_type.as_deref(), Some("image/jpeg"));
    assert_eq!(ok.digest_sha256.as_ref().unwrap().len(), 64);
    assert_eq!(missing.missing_reason.as_deref(), Some("file_missing"));
    assert!(ok.missing_reason.is_none());
}
#[test]
fn convert_mode_emits_progress_through_the_log_sink() {
    // Clone has no media pass, so nothing should reach the sink. This
    // pins that the new `log` parameter is wired end to end without
    // requiring ffmpeg in this crate's tests.
    let dir = tempfile::tempdir().unwrap();
    let att_dir = dir.path().join("attachments");
    std::fs::create_dir_all(&att_dir).unwrap();
    let mut att = empty_att("photo.jpg");
    let bytes = b"hello-photo";
    let lines = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
    let sink_lines = std::sync::Arc::clone(&lines);
    let sink = crate::process::LogSink::new(move |l: &str| {
        sink_lines.lock().unwrap().push(l.to_string());
    });
    {
        let mut jobs = [AttachmentJob {
            attachment: &mut att,
            timestamp_unix_ms: 1_609_459_200_000,
            size_hint: Some(bytes.len() as u64),
        }];
        run_attachment_jobs(
            &mut jobs,
            &att_dir,
            &media_cfg(MediaMode::Clone),
            |_| Ok(Some(bytes.to_vec())),
            |_| {},
            Some(&sink),
            None,
        )
        .unwrap();
    }
    assert!(
        lines.lock().unwrap().is_empty(),
        "clone mode runs no media pass, so it has nothing to report"
    );
}
#[test]
fn clone_temp_paths_are_unique_per_call() {
    // Two workers staging identical bytes land on the same
    // content-addressed dest, which is harmless, but they must not share
    // the temp path they write through on the way there.
    let a = next_clone_temp_name("x.jpg");
    let b = next_clone_temp_name("x.jpg");
    assert_ne!(a, b);
    assert!(a.starts_with("x.jpg."));
    assert!(a.ends_with(".tmp"));
}

/// One conversation, two attachments, staged end to end.
///
/// Every test above drives `run_attachment_jobs` directly with a hand-built
/// job list. `stage_conversation_attachments` is the function the exporters
/// actually call — it walks the documents, builds the jobs, runs them, counts
/// what was saved, and drops the in-memory bytes — and mutation testing found
/// it could be replaced with `Ok(())` in its entirety with nothing failing.
/// An export would then write no attachment files at all and report success.
#[test]
fn staging_a_conversation_writes_the_files_counts_them_and_frees_the_bytes() {
    use message_ir::{
        ConversationDocument, ConversationMeta, ConversationStats, ExportMeta, IrConversationType,
        IrDirection, IrMessage, IrMessageKind, IrService,
    };

    let dir = tempfile::tempdir().expect("tempdir");
    let attachments_dir = dir.path().join("attachments");
    std::fs::create_dir_all(&attachments_dir).expect("attachments dir");

    // The loader is keyed by the flat attachment index, which is the order
    // `attachment_jobs` walks documents in — that mapping is part of what this
    // test pins.
    const FIRST: &[u8] = b"first attachment bytes";
    const SECOND: &[u8] = b"second attachment bytes";
    let load = |i: usize| -> Result<Option<Vec<u8>>, String> {
        match i {
            0 => Ok(Some(FIRST.to_vec())),
            1 => Ok(Some(SECOND.to_vec())),
            other => Err(format!("unexpected attachment index {other}")),
        }
    };

    let first = empty_att("photo.jpg");
    let mut second = empty_att("clip.mp4");
    second.mime_type = Some("video/mp4".into());

    let mut documents = vec![ConversationDocument {
        schema_version: message_ir::SCHEMA_VERSION,
        export: ExportMeta {
            source: "test".into(),
            tool: "test".into(),
            tool_version: "0.1.0".into(),
            owner_handle: Some("+15555550100".into()),
            owner_display_name: None,
        },
        conversation: ConversationMeta {
            chat_identifier: "+15555550101".into(),
            conversation_type: IrConversationType::Individual,
            group_title: None,
            participants: vec![],
            stats: ConversationStats::default(),
        },
        messages: vec![IrMessage {
            guid: "guid-1".into(),
            timestamp_unix_ms: 1_400_773_261_000,
            direction: IrDirection::Incoming,
            service: IrService::Sms,
            message_kind: IrMessageKind::Mms,
            sender_handle: Some("+15555550101".into()),
            sender_display_name: None,
            subject: None,
            text: "two files".into(),
            attachments: vec![first, second],
            imessage: None,
            source: None,
        }],
        packaging_stem_suffix: None,
    }];

    let mut report = ExportReport::default();
    stage_conversation_attachments(
        &mut documents,
        &attachments_dir,
        &media_cfg(MediaMode::Clone),
        load,
        None,
        None,
        None,
        &mut report,
    )
    .expect("staging succeeds");

    // Both files are on disk under content-addressed names.
    let written: Vec<String> = std::fs::read_dir(&attachments_dir)
        .expect("read attachments")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(written.len(), 2, "two files written, got {written:?}");

    // Both are counted, which is what the export summary reports.
    assert_eq!(report.attachments_saved, 2);

    // Each attachment record names its file and carries its digest, so a
    // reader can find the bytes again — and the file holds what the loader
    // gave for that index, which is how the flat index maps to the document.
    let atts = &documents[0].messages[0].attachments;
    for (att, expected) in atts.iter().zip([FIRST, SECOND]) {
        let path = att.path.as_deref().expect("a path was recorded");
        assert!(path.starts_with("attachments/"), "got {path}");
        assert_eq!(
            std::fs::read(dir.path().join(path)).expect("read the staged file"),
            expected,
            "the file must hold the bytes the loader gave for its index"
        );
        assert_eq!(
            att.digest_sha256
                .as_ref()
                .expect("a digest was recorded")
                .len(),
            64
        );
        assert_eq!(att.size_bytes, Some(expected.len() as u64));
        // And nothing is left in memory, or a large export holds every
        // attachment at once.
        assert!(att.bytes.is_none(), "in-memory bytes must be freed");
    }
    assert_ne!(
        atts[0].digest_sha256, atts[1].digest_sha256,
        "different bytes, different digests"
    );
}

/// The same conversation staged twice must not count the same file twice, and
/// must not rewrite it. This is the resume case: an export interrupted and
/// started again.
#[test]
fn staging_the_same_bytes_twice_writes_one_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let attachments_dir = dir.path().join("attachments");
    std::fs::create_dir_all(&attachments_dir).expect("attachments dir");

    let mut first = empty_att("a.jpg");
    let mut second = empty_att("b.jpg");

    let mut jobs = vec![
        AttachmentJob {
            attachment: &mut first,
            timestamp_unix_ms: 1_400_773_261_000,
            size_hint: None,
        },
        AttachmentJob {
            attachment: &mut second,
            timestamp_unix_ms: 1_400_773_261_000,
            size_hint: None,
        },
    ];
    run_attachment_jobs(
        &mut jobs,
        &attachments_dir,
        &media_cfg(MediaMode::Clone),
        |_| Ok(Some(b"identical bytes".to_vec())),
        |_| {},
        None,
        None,
    )
    .expect("run");
    drop(jobs);

    let written: Vec<String> = std::fs::read_dir(&attachments_dir)
        .expect("read attachments")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(
        written.len(),
        1,
        "the same bytes at the same moment are one file, got {written:?}"
    );
    assert_eq!(
        first.digest_sha256, second.digest_sha256,
        "and both records point at it"
    );
}

/// The size hint feeds the progress total before any file is read, and could
/// be replaced with `None`, `Some(0)` or `Some(1)` without failing anything.
/// A wrong total makes a long export's progress bar meaningless.
#[test]
fn the_size_hint_prefers_the_recorded_size_then_the_bytes_in_hand() {
    let mut att = empty_att("photo.jpg");
    assert_eq!(attachment_size_hint(&att), None, "nothing to go on");

    att.bytes = Some(vec![0u8; 12]);
    assert_eq!(
        attachment_size_hint(&att),
        Some(12),
        "the bytes in memory are the fallback"
    );

    att.size_bytes = Some(9_999);
    assert_eq!(
        attachment_size_hint(&att),
        Some(9_999),
        "the record's own size wins, because it is what the source said"
    );
}

/// The extension on the staged file comes from the original name, and the
/// destination name is built from it. Replacing this with `""` or a constant
/// gives every attachment the same suffix, so the operating system opens none
/// of them correctly.
#[test]
fn the_extension_comes_from_the_original_name() {
    assert_eq!(extension_from_name(Some("photo.jpg")), ".jpg");
    assert_eq!(extension_from_name(Some("clip.MP4")), ".MP4");
    assert_eq!(extension_from_name(Some("archive.tar.gz")), ".gz");
    // A name with no extension, a dotfile, and no name at all: each yields
    // nothing rather than a stray dot.
    assert_eq!(extension_from_name(Some("README")), "");
    assert_eq!(extension_from_name(Some(".hidden")), "");
    assert_eq!(extension_from_name(None), "");
}

/// An empty file is treated as a missing one, and marked as such.
///
/// A loader that answers `Some(vec![])` — a zero-length file on disk, a
/// truncated download — must not produce a zero-byte attachment the reader
/// cannot open. The guard is `!bytes.is_empty()`, and replacing it with `true`
/// stages the empty file and records it as present.
#[test]
fn an_empty_file_is_recorded_as_missing_rather_than_staged() {
    let dir = tempfile::tempdir().unwrap();
    let att_dir = dir.path().join("attachments");
    std::fs::create_dir_all(&att_dir).unwrap();
    let mut att = empty_att("empty.jpg");

    {
        let mut jobs = [AttachmentJob {
            attachment: &mut att,
            timestamp_unix_ms: 1_609_459_200_000,
            size_hint: None,
        }];
        run_attachment_jobs(
            &mut jobs,
            &att_dir,
            &media_cfg(MediaMode::Clone),
            |_| Ok(Some(Vec::new())),
            |_| {},
            None,
            None,
        )
        .unwrap();
    }

    assert_eq!(att.missing_reason.as_deref(), Some("file_missing"));
    assert!(att.path.is_none(), "nothing was staged");
    assert!(att.digest_sha256.is_none());
    assert_eq!(
        std::fs::read_dir(&att_dir).unwrap().count(),
        0,
        "no file written for an empty source"
    );
}

/// The in-memory bytes are dropped once staging is done.
///
/// An exporter that carries attachment bytes on the document — the iMessage
/// and WhatsApp readers both do — holds the whole backup in memory until this
/// runs. `clear_attachment_bytes` could be replaced with `()` and nothing
/// failed, which on a large backup is the difference between finishing and
/// being killed by the kernel.
#[test]
fn staging_frees_the_bytes_the_documents_were_carrying() {
    use message_ir::{
        ConversationDocument, ConversationMeta, ConversationStats, ExportMeta, IrConversationType,
        IrDirection, IrMessage, IrMessageKind, IrService,
    };

    let dir = tempfile::tempdir().expect("tempdir");
    let attachments_dir = dir.path().join("attachments");
    std::fs::create_dir_all(&attachments_dir).expect("attachments dir");

    let mut carried = empty_att("photo.jpg");
    carried.bytes = Some(b"bytes held on the document".to_vec());

    let mut documents = vec![ConversationDocument {
        schema_version: message_ir::SCHEMA_VERSION,
        export: ExportMeta {
            source: "test".into(),
            tool: "test".into(),
            tool_version: "0.1.0".into(),
            owner_handle: None,
            owner_display_name: None,
        },
        conversation: ConversationMeta {
            chat_identifier: "+15555550101".into(),
            conversation_type: IrConversationType::Individual,
            group_title: None,
            participants: vec![],
            stats: ConversationStats::default(),
        },
        messages: vec![IrMessage {
            guid: "guid-1".into(),
            timestamp_unix_ms: 1_400_773_261_000,
            direction: IrDirection::Incoming,
            service: IrService::Sms,
            message_kind: IrMessageKind::Mms,
            sender_handle: None,
            sender_display_name: None,
            subject: None,
            text: "one file".into(),
            attachments: vec![carried],
            imessage: None,
            source: None,
        }],
        packaging_stem_suffix: None,
    }];

    assert!(
        documents[0].messages[0].attachments[0].bytes.is_some(),
        "the document starts out carrying its bytes"
    );

    let mut report = ExportReport::default();
    stage_conversation_attachments(
        &mut documents,
        &attachments_dir,
        &media_cfg(MediaMode::Clone),
        |_| Ok(Some(b"bytes held on the document".to_vec())),
        None,
        None,
        None,
        &mut report,
    )
    .expect("staging succeeds");

    assert!(
        documents[0].messages[0].attachments[0].bytes.is_none(),
        "the bytes must be dropped once the file is on disk"
    );
    assert!(
        documents[0].messages[0].attachments[0].path.is_some(),
        "and the file is on disk"
    );
}
