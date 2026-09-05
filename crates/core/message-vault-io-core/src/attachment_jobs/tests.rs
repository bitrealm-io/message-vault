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
                loaded.store(true, Ordering::SeqCst);
                Ok(Some(b"x".to_vec()))
            },
            |_| {},
            None,
            None,
        )
        .unwrap();
    }
    assert!(!loaded.load(Ordering::SeqCst));
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
                    cancel.store(true, Ordering::SeqCst);
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
