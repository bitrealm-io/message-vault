use super::*;
use media::{CompressOptions, MediaMode};
use message_ir::{ConversationDocument, IrAttachment};
use message_ir_format::read_conversation_jsonl;
use message_vault_io_core::LogSink;
use std::fs;
use std::sync::{Arc, Mutex};

fn att(name: &str) -> IrAttachment {
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

/// A one-message document with `count` attachments, keyed by `who` so
/// each unit lands on its own conversation file.
fn doc_with(who: &str, count: usize) -> ConversationDocument {
    let mut doc = message_ir::testutil::sample_document("hello");
    doc.conversation.chat_identifier = who.into();
    doc.conversation.participants[0].handle = Some(who.into());
    doc.messages[0].attachments = (0..count).map(|i| att(&format!("f{i}.jpg"))).collect();
    doc
}

fn unit_from(doc: ConversationDocument, sources: Vec<AttachmentSource>) -> ConversationUnit {
    let mut it = sources.into_iter();
    ConversationUnit::from_doc(doc, |_, _att| {
        let source = it.next().unwrap_or(AttachmentSource::Missing);
        let hint = match &source {
            AttachmentSource::Bytes(b) => Some(b.len() as u64),
            _ => None,
        };
        (source, hint)
    })
}

fn options(media: MediaMode, resume: bool) -> WriteQueueOptions {
    WriteQueueOptions {
        media,
        compress: CompressOptions::default(),
        resume,
        writer_count: 1,
    }
}

fn drain(
    dir: &Path,
    units: Vec<ConversationUnit>,
    options: &WriteQueueOptions,
) -> anyhow::Result<WriteQueueReport> {
    drain_write_queue_with_loader(
        dir,
        units,
        options,
        &mut load_attachment_source,
        None,
        None,
        None,
    )
}

#[test]
fn drains_units_and_writes_conversation_files_last() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("source.jpg");
    fs::write(&src, b"path-bytes").unwrap();
    let out = tmp.path().join("out");
    fs::create_dir_all(&out).unwrap();

    let units = vec![
        unit_from(
            doc_with("+15550000001", 1),
            vec![AttachmentSource::Bytes(b"inline-bytes".to_vec())],
        ),
        unit_from(
            doc_with("+15550000002", 1),
            vec![AttachmentSource::Path(src)],
        ),
    ];
    let report = drain(&out, units, &options(MediaMode::Clone, false)).unwrap();

    assert_eq!(report.conversations_written, 2);
    assert_eq!(report.conversations_skipped, 0);
    assert_eq!(report.attachments_saved, 2);

    let files: Vec<_> = fs::read_dir(&out)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".jsonl"))
        .collect();
    assert_eq!(files.len(), 2, "one conversation file per unit");

    for name in files {
        let doc = read_conversation_jsonl(&out.join(&name)).unwrap();
        let a = &doc.messages[0].attachments[0];
        assert!(a.path.as_deref().unwrap().starts_with("attachments/"));
        assert_eq!(a.digest_sha256.as_ref().unwrap().len(), 64);
        assert!(a.size_bytes.unwrap() > 0);
        assert!(a.bytes.is_none(), "bytes never reach the written file");
        assert!(
            out.join(a.path.as_ref().unwrap()).is_file(),
            "a conversation file on disk means its attachments are too"
        );
    }
}

#[test]
fn resume_skips_a_unit_whose_conversation_file_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    fs::create_dir_all(&out).unwrap();
    let build = || {
        vec![
            unit_from(
                doc_with("+15550000001", 1),
                vec![AttachmentSource::Bytes(b"a".to_vec())],
            ),
            unit_from(
                doc_with("+15550000002", 1),
                vec![AttachmentSource::Bytes(b"b".to_vec())],
            ),
        ]
    };
    drain(&out, build(), &options(MediaMode::Clone, false)).unwrap();

    let mut never = |_: &mut AttachmentSource| -> Result<Option<Vec<u8>>, String> {
        panic!("a skipped unit must not load anything")
    };
    let report = drain_write_queue_with_loader(
        &out,
        build(),
        &options(MediaMode::Clone, true),
        &mut never,
        None,
        None,
        None,
    )
    .unwrap();

    assert_eq!(report.conversations_skipped, 2);
    assert_eq!(report.conversations_written, 0);
}

#[test]
fn resume_rewrites_a_unit_whose_conversation_file_is_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    fs::create_dir_all(&out).unwrap();
    let build = || {
        vec![
            unit_from(
                doc_with("+15550000001", 1),
                vec![AttachmentSource::Bytes(b"a".to_vec())],
            ),
            unit_from(
                doc_with("+15550000002", 1),
                vec![AttachmentSource::Bytes(b"b".to_vec())],
            ),
        ]
    };
    drain(&out, build(), &options(MediaMode::Clone, false)).unwrap();

    let doomed = out.join(format!(
        "{}.jsonl",
        doc_with("+15550000002", 0).filename_stem()
    ));
    assert!(doomed.is_file());
    fs::remove_file(&doomed).unwrap();

    let report = drain(&out, build(), &options(MediaMode::Clone, true)).unwrap();
    assert_eq!(report.conversations_written, 1);
    assert_eq!(report.conversations_skipped, 1);
    assert!(doomed.is_file(), "the missing conversation file came back");
}

#[test]
fn resume_rewrites_a_conversation_file_that_is_empty_or_cut_off() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    fs::create_dir_all(&out).unwrap();
    let build = || {
        vec![
            unit_from(
                doc_with("+15550000001", 1),
                vec![AttachmentSource::Bytes(b"a".to_vec())],
            ),
            unit_from(
                doc_with("+15550000002", 1),
                vec![AttachmentSource::Bytes(b"b".to_vec())],
            ),
            unit_from(
                doc_with("+15550000003", 1),
                vec![AttachmentSource::Bytes(b"c".to_vec())],
            ),
        ]
    };
    drain(&out, build(), &options(MediaMode::Clone, false)).unwrap();

    let file_for = |who: &str| out.join(format!("{}.jsonl", doc_with(who, 0).filename_stem()));
    let intact = file_for("+15550000001");
    let cut_off = file_for("+15550000002");
    let empty = file_for("+15550000003");
    let intact_bytes = fs::read(&intact).unwrap();
    let full = fs::read(&cut_off).unwrap();
    // A power loss mid-write leaves the bytes that made it to disk: no
    // final newline, and possibly nothing at all.
    fs::write(&cut_off, &full[..full.len() / 2]).unwrap();
    fs::write(&empty, b"").unwrap();

    let report = drain(&out, build(), &options(MediaMode::Clone, true)).unwrap();
    assert_eq!(report.conversations_written, 2);
    assert_eq!(report.conversations_skipped, 1);

    assert_eq!(
        fs::read(&intact).unwrap(),
        intact_bytes,
        "the intact file is left alone"
    );
    assert_eq!(
        fs::read(&cut_off).unwrap(),
        full,
        "the cut-off file is whole again"
    );
    let doc = read_conversation_jsonl(&empty).unwrap();
    assert_eq!(doc.messages.len(), 1, "the empty file is written in full");
}

#[test]
fn disabled_mode_marks_not_copied_and_clears_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    fs::create_dir_all(&out).unwrap();
    let units = vec![unit_from(
        doc_with("+15550000001", 1),
        vec![AttachmentSource::Bytes(b"ignored".to_vec())],
    )];
    drain(&out, units, &options(MediaMode::Disabled, false)).unwrap();

    let stem = doc_with("+15550000001", 0).filename_stem();
    let doc = read_conversation_jsonl(&out.join(format!("{stem}.jsonl"))).unwrap();
    let a = &doc.messages[0].attachments[0];
    assert_eq!(a.missing_reason.as_deref(), Some("not_copied"));
    assert!(a.path.is_none());
    assert!(a.digest_sha256.is_none());
    let staged = out.join("attachments");
    let empty = !staged.is_dir() || fs::read_dir(&staged).unwrap().next().is_none();
    assert!(empty, "disabled mode writes no attachment files");
}

#[test]
fn missing_source_becomes_file_missing_and_the_drain_continues() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    fs::create_dir_all(&out).unwrap();
    let units = vec![unit_from(
        doc_with("+15550000001", 2),
        vec![
            AttachmentSource::Missing,
            AttachmentSource::Bytes(b"present".to_vec()),
        ],
    )];
    let report = drain(&out, units, &options(MediaMode::Clone, false)).unwrap();
    assert_eq!(report.conversations_written, 1);

    let stem = doc_with("+15550000001", 0).filename_stem();
    let doc = read_conversation_jsonl(&out.join(format!("{stem}.jsonl"))).unwrap();
    let atts = &doc.messages[0].attachments;
    assert_eq!(atts[0].missing_reason.as_deref(), Some("file_missing"));
    assert!(atts[1].path.is_some(), "the readable one still landed");
}

#[test]
fn progress_lines_cover_all_units_with_global_counts() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    fs::create_dir_all(&out).unwrap();
    let lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink_lines = Arc::clone(&lines);
    let sink = LogSink::new(move |l: &str| sink_lines.lock().unwrap().push(l.to_string()));

    let units = vec![
        unit_from(
            doc_with("+15550000001", 1),
            vec![AttachmentSource::Bytes(b"a".to_vec())],
        ),
        unit_from(
            doc_with("+15550000002", 1),
            vec![AttachmentSource::Bytes(b"b".to_vec())],
        ),
    ];
    drain_write_queue_with_loader(
        &out,
        units,
        &options(MediaMode::Clone, false),
        &mut load_attachment_source,
        Some(&sink),
        None,
        None,
    )
    .unwrap();

    let lines = lines.lock().unwrap().clone();
    assert!(
        lines
            .iter()
            .any(|l| l == "Preparing 2 conversation file(s)..."),
        "banner missing from {lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.starts_with("  attachments 2/2 ")),
        "counts run across units, not per unit: {lines:?}"
    );
    assert!(
        lines.iter().any(|l| l == "Prepared 2 conversation file(s)"),
        "closing line missing from {lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l.contains("preparing 1/")),
        "per-conversation count lines would confuse the desktop scraper"
    );
}
#[test]
fn parallel_drain_writes_every_unit() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    fs::create_dir_all(&out).unwrap();
    let units: Vec<_> = (0..12)
        .map(|i| {
            unit_from(
                doc_with(&format!("+1555000{i:04}"), 1),
                vec![AttachmentSource::Bytes(format!("payload-{i}").into_bytes())],
            )
        })
        .collect();
    let mut options = options(MediaMode::Clone, false);
    options.writer_count = 4;

    let report = drain_write_queue(&out, units, &options, None, None, None).unwrap();

    assert_eq!(report.conversations_written, 12);
    assert_eq!(report.attachments_saved, 12);
    let written = fs::read_dir(&out)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".jsonl"))
        .count();
    assert_eq!(written, 12);
}

#[test]
fn parallel_drain_stops_on_the_first_error() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    fs::create_dir_all(&out).unwrap();
    // A directory sitting where a conversation file must go: the write
    // fails for that unit, and the drain reports it rather than
    // finishing quietly.
    let blocked = doc_with("+15550000003", 0).filename_stem();
    fs::create_dir_all(out.join(format!("{blocked}.jsonl"))).unwrap();

    let units: Vec<_> = (1..=4)
        .map(|i| {
            unit_from(
                doc_with(&format!("+1555000000{i}"), 1),
                vec![AttachmentSource::Bytes(b"x".to_vec())],
            )
        })
        .collect();
    let mut options = options(MediaMode::Clone, false);
    options.writer_count = 2;

    let err = drain_write_queue(&out, units, &options, None, None, None).unwrap_err();
    assert!(
        format!("{err:#}").contains(&blocked),
        "the error should name the conversation that failed: {err:#}"
    );
}

/// The last attachment count a run reported, from both kinds of drain.
fn last_attachment_bytes(writer_count: usize) -> (u64, u64) {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    // Each source claims 100 bytes for a 5-byte file.
    let units: Vec<_> = (1..=3)
        .map(|i| {
            ConversationUnit::from_doc(doc_with(&format!("+1555000000{i}"), 1), |_, _| {
                (AttachmentSource::Bytes(b"xxxxx".to_vec()), Some(100))
            })
        })
        .collect();
    let mut options = options(MediaMode::Clone, false);
    options.writer_count = writer_count;
    let seen = Arc::new(Mutex::new(Vec::<ProgressEvent>::new()));
    let sink_seen = Arc::clone(&seen);
    let sink = ProgressSink::unpaced(move |event| sink_seen.lock().unwrap().push(event));

    if writer_count == 1 {
        drain_write_queue_with_loader(
            &out,
            units,
            &options,
            &mut load_attachment_source,
            None,
            Some(&sink),
            None,
        )
        .unwrap();
    } else {
        drain_write_queue(&out, units, &options, None, Some(&sink), None).unwrap();
    }

    let seen = seen.lock().unwrap();
    seen.iter()
        .filter_map(|event| match event {
            ProgressEvent::Attachments {
                done: 3,
                bytes_done,
                bytes_total,
                ..
            } => Some((*bytes_done, *bytes_total)),
            _ => None,
        })
        .max()
        .unwrap()
}

#[test]
fn the_byte_total_comes_down_to_the_files_when_hints_overstate_them() {
    assert_eq!(last_attachment_bytes(1), (15, 15), "sequential drain");
    assert_eq!(last_attachment_bytes(3), (15, 15), "parallel drain");
}

#[test]
fn typed_progress_covers_prepare_and_attachments_across_units() {
    // The desktop's progress bar reads these events and nothing else, so
    // the drain must say how many conversation files it will write
    // before the first one lands, count attachments to the full total,
    // and end with every unit prepared.
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    let units: Vec<_> = (1..=4)
        .map(|i| {
            unit_from(
                doc_with(&format!("+1555000000{i}"), 1),
                vec![AttachmentSource::Bytes(b"x".to_vec())],
            )
        })
        .collect();
    let mut options = options(MediaMode::Clone, false);
    options.writer_count = 2;

    let seen = Arc::new(Mutex::new(Vec::<ProgressEvent>::new()));
    let sink_seen = Arc::clone(&seen);
    let sink = ProgressSink::unpaced(move |event| sink_seen.lock().unwrap().push(event));

    drain_write_queue(&out, units, &options, None, Some(&sink), None).unwrap();

    let seen = seen.lock().unwrap().clone();
    assert_eq!(
        seen.first(),
        Some(&ProgressEvent::Prepare { done: 0, total: 4 }),
        "the unit count is announced before any file is written"
    );
    // Two writers report concurrently, so emission order is not count
    // order; the high-water marks are what must be right.
    let attachments_high = seen
        .iter()
        .filter_map(|event| match event {
            ProgressEvent::Attachments {
                done,
                total,
                bytes_done,
                bytes_total,
            } => Some((*done, *total, *bytes_done, *bytes_total)),
            _ => None,
        })
        .max()
        .unwrap();
    assert_eq!(attachments_high, (4, 4, 4, 4));
    let prepared_high = seen
        .iter()
        .filter_map(|event| match event {
            ProgressEvent::Prepare { done, total } => Some((*done, *total)),
            _ => None,
        })
        .max()
        .unwrap();
    assert_eq!(prepared_high, (4, 4));
    assert!(
        !seen
            .iter()
            .any(|event| matches!(event, ProgressEvent::Media { .. })),
        "clone mode runs no media pass"
    );
}

#[test]
fn sequential_drain_reports_prepare_in_order_and_counts_resumed_units() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    fs::create_dir_all(&out).unwrap();
    let build = || {
        vec![
            unit_from(
                doc_with("+15550000001", 1),
                vec![AttachmentSource::Bytes(b"a".to_vec())],
            ),
            unit_from(
                doc_with("+15550000002", 1),
                vec![AttachmentSource::Bytes(b"b".to_vec())],
            ),
        ]
    };
    drain(&out, build(), &options(MediaMode::Clone, false)).unwrap();

    // A resumed run finds both files and skips them; progress still
    // describes the whole import, so it walks 0 -> 1 -> 2 of 2.
    let seen = Arc::new(Mutex::new(Vec::<ProgressEvent>::new()));
    let sink_seen = Arc::clone(&seen);
    let sink = ProgressSink::unpaced(move |event| sink_seen.lock().unwrap().push(event));
    drain_write_queue_with_loader(
        &out,
        build(),
        &options(MediaMode::Clone, true),
        &mut load_attachment_source,
        None,
        Some(&sink),
        None,
    )
    .unwrap();

    let prepared: Vec<(usize, usize)> = seen
        .lock()
        .unwrap()
        .iter()
        .filter_map(|event| match event {
            ProgressEvent::Prepare { done, total } => Some((*done, *total)),
            _ => None,
        })
        .collect();
    assert_eq!(prepared, [(0, 2), (1, 2), (2, 2)]);
}

#[test]
fn an_unreadable_attachment_is_logged_before_it_becomes_a_chip() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    fs::create_dir_all(&out).unwrap();
    let missing = tmp.path().join("gone.jpg");

    let lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink_lines = Arc::clone(&lines);
    let sink = LogSink::new(move |l: &str| sink_lines.lock().unwrap().push(l.to_string()));

    let units = vec![unit_from(
        doc_with("+15550000001", 1),
        vec![AttachmentSource::Path(missing)],
    )];
    let report = drain_write_queue(
        &out,
        units,
        &options(MediaMode::Clone, false),
        Some(&sink),
        None,
        None,
    )
    .unwrap();

    assert_eq!(report.conversations_written, 1, "the drain carries on");
    let lines = lines.lock().unwrap().clone();
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("warning: attachment ") && l.contains("could not be read")),
        "an unreadable attachment says why before it turns into a chip: {lines:?}"
    );

    let stem = doc_with("+15550000001", 0).filename_stem();
    let doc = read_conversation_jsonl(&out.join(format!("{stem}.jsonl"))).unwrap();
    assert_eq!(
        doc.messages[0].attachments[0].missing_reason.as_deref(),
        Some("file_missing")
    );
}

#[test]
fn headroom_shortfall_speaks_when_space_is_short() {
    assert!(headroom_shortfall(10 * 1024 * 1024 * 1024, 1024).is_some());
    assert_eq!(headroom_shortfall(1024, 10 * 1024 * 1024 * 1024), None);
    let msg = headroom_shortfall(2 * 1024 * 1024 * 1024, 1024).unwrap();
    assert!(msg.contains("free"), "{msg}");
    assert!(msg.contains("GB"), "{msg}");
}

/// A group conversation with its own chat identifier, titled or not, whose
/// only message says `text`.
fn group(chat_id: &str, title: Option<&str>, text: &str) -> ConversationDocument {
    let mut doc = message_ir::testutil::sample_document(text);
    doc.conversation.chat_identifier = chat_id.into();
    doc.conversation.conversation_type = message_ir::IrConversationType::Group;
    doc.conversation.group_title = title.map(str::to_string);
    doc
}

/// Each staged file's chat identifier and first message text, sorted.
fn staged_texts(out: &Path) -> Vec<(String, String)> {
    let mut texts: Vec<_> = fs::read_dir(out)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".jsonl"))
        .map(|e| {
            let doc = read_conversation_jsonl(&e.path()).unwrap();
            (
                doc.conversation.chat_identifier,
                doc.messages[0].text.clone(),
            )
        })
        .collect();
    texts.sort();
    texts
}

#[test]
fn conversations_that_share_a_file_name_are_each_written() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    fs::create_dir_all(&out).unwrap();
    let units = vec![
        // Two groups with one title, and two untitled groups with the same
        // people: each pair reduces to one file name.
        unit_from(group("chat1", Some("Family"), "first family"), vec![]),
        unit_from(group("chat2", Some("Family"), "second family"), vec![]),
        unit_from(group("chat3", None, "first untitled"), vec![]),
        unit_from(group("chat4", None, "second untitled"), vec![]),
        // Differs only in case, which is one file on macOS and Windows.
        unit_from(group("chat5", Some("family"), "lower-case family"), vec![]),
    ];

    let report = drain(&out, units, &options(MediaMode::Clone, false)).unwrap();

    assert_eq!(report.conversations_written, 5);
    assert_eq!(
        staged_texts(&out),
        vec![
            ("chat1".to_string(), "first family".to_string()),
            ("chat2".to_string(), "second family".to_string()),
            ("chat3".to_string(), "first untitled".to_string()),
            ("chat4".to_string(), "second untitled".to_string()),
            ("chat5".to_string(), "lower-case family".to_string()),
        ],
        "no conversation's file replaces another's"
    );
}

#[test]
fn a_resumed_run_finds_the_names_a_clash_was_given() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    fs::create_dir_all(&out).unwrap();
    let units = || {
        vec![
            unit_from(group("chat1", Some("Family"), "first"), vec![]),
            unit_from(group("chat2", Some("Family"), "second"), vec![]),
        ]
    };
    drain(&out, units(), &options(MediaMode::Clone, false)).unwrap();

    let report = drain(&out, units(), &options(MediaMode::Clone, true)).unwrap();

    assert_eq!(
        report.conversations_skipped, 2,
        "both files are found again"
    );
    assert_eq!(report.conversations_written, 0);
}

#[test]
fn the_same_chat_twice_is_refused_rather_than_overwritten() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    fs::create_dir_all(&out).unwrap();
    let units = vec![
        unit_from(group("chat1", Some("Family"), "first"), vec![]),
        unit_from(group("chat1", Some("Family"), "second"), vec![]),
    ];

    let err = drain(&out, units, &options(MediaMode::Clone, false)).unwrap_err();

    assert!(err.to_string().contains("would both be written"), "{err:#}");
    assert_eq!(staged_texts(&out), vec![], "nothing was written");
}
/// A minimal valid 1x1 RGB PNG that ffmpeg reads cleanly.
#[rustfmt::skip]
const PNG_1X1_RGB: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0x00,
    0x00, 0x03, 0x01, 0x01, 0x00, 0xc9, 0xfe, 0x92, 0xef, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
    0x44, 0xae, 0x42, 0x60, 0x82,
];

#[test]
fn clone_mode_runs_no_media_pass() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    fs::create_dir_all(&out).unwrap();
    let units = vec![unit_from(
        doc_with("+15550000001", 1),
        vec![AttachmentSource::Bytes(b"plain".to_vec())],
    )];
    let report = drain(&out, units, &options(MediaMode::Clone, false)).unwrap();
    assert_eq!(report.media, media::MediaReport::default());
}

#[test]
fn convert_runs_as_a_pass_after_the_drain_stages_originals() {
    let Some(_tools) = media::testutil::real_ffmpeg_test_guard() else {
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    fs::create_dir_all(&out).unwrap();

    let mut doc = doc_with("+15550000001", 1);
    doc.messages[0].attachments[0].original_name = Some("shot.png".into());
    let units = vec![unit_from(
        doc,
        vec![AttachmentSource::Bytes(PNG_1X1_RGB.to_vec())],
    )];

    let report = drain(&out, units, &options(MediaMode::Convert, false)).unwrap();

    assert_eq!(report.conversations_written, 1);
    assert_eq!(
        report.media.processed, 1,
        "the post-pass converted the staged original"
    );

    let stem = doc_with("+15550000001", 0).filename_stem();
    let written = read_conversation_jsonl(&out.join(format!("{stem}.jsonl"))).unwrap();
    let path = written.messages[0].attachments[0].path.as_deref().unwrap();
    assert!(
        path.ends_with(".jpg"),
        "convert repoints the attachment at its derivative: {path}"
    );
    assert!(
        out.join(path).is_file(),
        "the derivative the conversation names is on disk"
    );
}
