use super::*;
use message_ir::IrAttachment;
use message_ir_format::{read_conversation_csv, read_conversation_json};
use message_vault_io_core::{FormatConfig, MediaConfig, ObfuscateConfig, SourceConfig};

fn write_fixture(dir: &Path, format: OutputFormat) {
    fs::create_dir_all(dir).unwrap();
    clean_previous_ir_output(dir).unwrap();
    let mut sink = FormatSink::open(dir, format, ExportTransforms::none()).unwrap();
    sink.write_document(message_ir::testutil::sample_document("hello reexport"))
        .unwrap();
    sink.finish().unwrap();
}

fn config(input: &Path, output: &Path, output_format: OutputFormat) -> ExporterConfig {
    ExporterConfig {
        inputs: vec![input.to_path_buf()],
        output: output.to_path_buf(),
        time_zone: None,
        obfuscate: ObfuscateConfig::default(),
        media: MediaConfig::default(),
        cancel: None,
        log: None,
        progress: None,
        output_format,
        resume: false,
        source: SourceConfig::Format(FormatConfig {}),
    }
}

/// The first file in `dir` with `extension`, skipping `.meta.json` sidecars.
fn find_file(dir: &Path, extension: &str) -> PathBuf {
    fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.extension().and_then(|found| found.to_str()) == Some(extension)
                && !path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with(".meta.json"))
        })
        .unwrap_or_else(|| panic!("no .{extension} file in {}", dir.display()))
}

fn attachment(name: &str, path: Option<&str>) -> IrAttachment {
    IrAttachment {
        path: path.map(str::to_string),
        original_name: Some(name.to_string()),
        mime_type: Some("text/plain".to_string()),
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
fn detect_json_and_convert_to_csv() {
    let source = tempfile::tempdir().unwrap();
    write_fixture(source.path(), OutputFormat::Json);
    assert_eq!(
        detect_ir_export(source.path()).unwrap().format,
        OutputFormat::Json
    );
    let destination = tempfile::tempdir().unwrap();
    let report = convert_export(
        source.path(),
        &config(source.path(), destination.path(), OutputFormat::Csv),
    )
    .unwrap();
    assert_eq!(report.conversations, 1);
    assert_eq!(report.detected_format, "json");
    let csv = fs::read_dir(destination.path())
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().and_then(|extension| extension.to_str()) == Some("csv"))
        .expect("csv");
    assert_eq!(
        read_conversation_csv(&csv).unwrap().messages[0].text,
        "hello reexport"
    );
}

#[test]
fn convert_csv_to_json() {
    let source = tempfile::tempdir().unwrap();
    write_fixture(source.path(), OutputFormat::Csv);
    let destination = tempfile::tempdir().unwrap();
    convert_export(
        source.path(),
        &config(source.path(), destination.path(), OutputFormat::Json),
    )
    .unwrap();
    let json = fs::read_dir(destination.path())
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.extension().and_then(|extension| extension.to_str()) == Some("json")
                && !path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with(".meta.json"))
        })
        .expect("json");
    assert_eq!(
        read_conversation_json(&json).unwrap().messages[0].text,
        "hello reexport"
    );
}

#[test]
fn convert_json_to_xml() {
    let source = tempfile::tempdir().unwrap();
    write_fixture(source.path(), OutputFormat::Json);
    let destination = tempfile::tempdir().unwrap();
    convert_export(
        source.path(),
        &config(source.path(), destination.path(), OutputFormat::Xml),
    )
    .unwrap();
    assert!(destination.path().join("smses.xml").is_file());
}

#[test]
fn convert_xml_with_ir_reader() {
    let source = tempfile::tempdir().unwrap();
    fs::write(
        source.path().join("smses.xml"),
        r#"<smses><sms protocol="0" address="+15555550101" date="1400773261000" type="1" body="hello xml" contact_name="Sam"/></smses>"#,
    )
    .unwrap();
    let destination = tempfile::tempdir().unwrap();
    let report = convert_export(
        source.path(),
        &config(source.path(), destination.path(), OutputFormat::Json),
    )
    .unwrap();
    assert_eq!(report.detected_format, "xml");
    assert_eq!(report.conversations, 1);
    let json = fs::read_dir(destination.path())
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().and_then(|extension| extension.to_str()) == Some("json"))
        .expect("json");
    assert_eq!(
        read_conversation_json(&json).unwrap().messages[0].text,
        "hello xml"
    );
}

#[test]
fn mixed_formats_error() {
    let source = tempfile::tempdir().unwrap();
    write_fixture(source.path(), OutputFormat::Json);
    let mut sink =
        FormatSink::open(source.path(), OutputFormat::Csv, ExportTransforms::none()).unwrap();
    sink.write_document(message_ir::testutil::sample_document("hello reexport"))
        .unwrap();
    sink.finish().unwrap();
    let error = detect_ir_export(source.path()).unwrap_err().to_string();
    assert!(error.contains("mixed"), "{error}");
}

#[test]
fn same_path_errors() {
    let directory = tempfile::tempdir().unwrap();
    write_fixture(directory.path(), OutputFormat::Json);
    let error = convert_export(
        directory.path(),
        &config(directory.path(), directory.path(), OutputFormat::Csv),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("different"), "{error}");
}

#[test]
fn meta_json_does_not_count_as_json_export() {
    let source = tempfile::tempdir().unwrap();
    write_fixture(source.path(), OutputFormat::Csv);
    assert_eq!(
        detect_ir_export(source.path()).unwrap().format,
        OutputFormat::Csv
    );
}

#[test]
fn run_converts_an_export_and_reports_the_detected_format() {
    let source = tempfile::tempdir().unwrap();
    write_fixture(source.path(), OutputFormat::Jsonl);
    let destination = tempfile::tempdir().unwrap();

    let result = run(&config(
        source.path(),
        destination.path(),
        OutputFormat::Csv,
    ))
    .unwrap();

    assert_eq!(
        result.messages,
        vec![
            "Detected input format: jsonl".to_string(),
            "Conversations: 1".to_string(),
        ]
    );
    let csv = find_file(destination.path(), "csv");
    assert_eq!(
        read_conversation_csv(&csv).unwrap().messages[0].text,
        "hello reexport"
    );
}

#[test]
fn run_refuses_a_config_without_an_input() {
    let destination = tempfile::tempdir().unwrap();
    let mut config = config(Path::new("unused"), destination.path(), OutputFormat::Csv);
    config.inputs.clear();

    let error = run(&config).unwrap_err().to_string();

    assert_eq!(error, "input is required");
}

#[test]
fn run_refuses_an_empty_output_directory() {
    let source = tempfile::tempdir().unwrap();
    write_fixture(source.path(), OutputFormat::Json);
    let config = config(source.path(), Path::new(""), OutputFormat::Csv);

    let error = run(&config).unwrap_err().to_string();

    assert_eq!(error, "output directory is required");
}

#[test]
fn log_lines_name_the_detected_format_and_the_conversation_count() {
    let report = ReexportReport {
        detected_format: "mbox".to_string(),
        conversations: 3,
        sink: FormatSinkResult::default(),
    };

    assert_eq!(
        report.log_lines(),
        vec![
            "Detected input format: mbox".to_string(),
            "Conversations: 3".to_string(),
        ]
    );
}

#[test]
fn log_lines_append_the_sink_lines_after_the_count() {
    let report = ReexportReport {
        detected_format: "json".to_string(),
        conversations: 1,
        sink: FormatSinkResult {
            xml_path: Some(PathBuf::from("out/smses.xml")),
            obfuscated_docs: 2,
            ..FormatSinkResult::default()
        },
    };

    assert_eq!(
        report.log_lines(),
        vec![
            "Detected input format: json".to_string(),
            "Conversations: 1".to_string(),
            "Obfuscated 2 conversation(s)".to_string(),
            "Wrote out/smses.xml".to_string(),
        ]
    );
}

#[test]
fn looks_like_smses_reads_only_the_first_line_and_ignores_case() {
    let dir = tempfile::tempdir().unwrap();
    let lower = dir.path().join("lower.xml");
    fs::write(&lower, "<smses count=\"1\"></smses>\n").unwrap();
    let upper = dir.path().join("upper.xml");
    fs::write(
        &upper,
        "<?xml version=\"1.0\"?><SMSES count=\"1\"></SMSES>\n",
    )
    .unwrap();
    let second_line = dir.path().join("second-line.xml");
    fs::write(
        &second_line,
        "<?xml version=\"1.0\"?>\n<smses count=\"1\"></smses>\n",
    )
    .unwrap();

    assert!(looks_like_smses(&lower));
    assert!(looks_like_smses(&upper));
    assert!(
        !looks_like_smses(&second_line),
        "only the first line is read"
    );
    assert!(!looks_like_smses(&dir.path().join("missing.xml")));
}

#[test]
fn a_backup_not_named_smses_xml_is_detected_by_its_first_line() {
    let source = tempfile::tempdir().unwrap();
    fs::write(
        source.path().join("sms-20240101.xml"),
        r#"<smses count="1"><sms protocol="0" address="+15555550101" date="1400773261000" type="1" body="hello xml" contact_name="Sam"/></smses>"#,
    )
    .unwrap();

    assert_eq!(
        detect_ir_export(source.path()).unwrap().format,
        OutputFormat::Xml
    );
}

#[test]
fn looks_like_ir_jsonl_accepts_a_header_line_without_messages() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), OutputFormat::Jsonl);
    let written = find_file(dir.path(), "jsonl");
    assert!(looks_like_ir_jsonl(&written).unwrap());

    let whole_document = dir.path().join("whole-document.jsonl");
    fs::write(
        &whole_document,
        format!(
            r#"{{"schema_version":{},"export":{{}},"conversation":{{}},"messages":[]}}"#,
            message_ir::SCHEMA_VERSION
        ),
    )
    .unwrap();
    assert!(
        !looks_like_ir_jsonl(&whole_document).unwrap(),
        "a whole document on one line is JSON, not JSON Lines"
    );

    let old_schema = dir.path().join("old-schema.jsonl");
    fs::write(
        &old_schema,
        r#"{"schema_version":3,"export":{},"conversation":{}}"#,
    )
    .unwrap();
    assert!(!looks_like_ir_jsonl(&old_schema).unwrap());

    let prose = dir.path().join("prose.jsonl");
    fs::write(&prose, "not json\n").unwrap();
    assert!(!looks_like_ir_jsonl(&prose).unwrap());

    let empty = dir.path().join("empty.jsonl");
    fs::write(&empty, "").unwrap();
    assert!(!looks_like_ir_jsonl(&empty).unwrap());

    assert!(looks_like_ir_jsonl(&dir.path().join("missing.jsonl")).is_err());
}

#[test]
fn dir_has_eml_finds_an_eml_file_whatever_its_case() {
    let dir = tempfile::tempdir().unwrap();
    let lower = dir.path().join("lower");
    fs::create_dir_all(&lower).unwrap();
    fs::write(lower.join("1.eml"), "").unwrap();
    let upper = dir.path().join("upper");
    fs::create_dir_all(&upper).unwrap();
    fs::write(upper.join("2.EML"), "").unwrap();
    let other = dir.path().join("other");
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join("notes.txt"), "").unwrap();
    let empty = dir.path().join("empty");
    fs::create_dir_all(&empty).unwrap();

    assert!(dir_has_eml(&lower).unwrap());
    assert!(dir_has_eml(&upper).unwrap());
    assert!(!dir_has_eml(&other).unwrap());
    assert!(!dir_has_eml(&empty).unwrap());
    assert!(dir_has_eml(&dir.path().join("missing")).is_err());
}

#[test]
fn an_eml_export_is_detected_from_its_folders() {
    let source = tempfile::tempdir().unwrap();
    write_fixture(source.path(), OutputFormat::Eml);

    assert_eq!(
        detect_ir_export(source.path()).unwrap().format,
        OutputFormat::Eml
    );
}

#[test]
fn copy_dir_recursive_copies_nested_files_and_folders() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    fs::create_dir_all(source.join("nested/deeper")).unwrap();
    fs::write(source.join("top.txt"), b"top").unwrap();
    fs::write(source.join("nested/middle.txt"), b"middle").unwrap();
    fs::write(source.join("nested/deeper/bottom.txt"), b"bottom").unwrap();
    let destination = dir.path().join("destination");
    fs::create_dir_all(&destination).unwrap();

    copy_dir_recursive(&source, &destination).unwrap();

    assert_eq!(fs::read(destination.join("top.txt")).unwrap(), b"top");
    assert_eq!(
        fs::read(destination.join("nested/middle.txt")).unwrap(),
        b"middle"
    );
    assert_eq!(
        fs::read(destination.join("nested/deeper/bottom.txt")).unwrap(),
        b"bottom"
    );
    assert_eq!(
        fs::read(source.join("top.txt")).unwrap(),
        b"top",
        "the source is left in place"
    );
}

#[test]
fn copy_dir_recursive_refuses_a_missing_source() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing");

    let error = copy_dir_recursive(&missing, dir.path())
        .unwrap_err()
        .to_string();

    assert_eq!(error, format!("read {}", missing.display()));
}

#[test]
fn apply_reexport_convert_restages_attachments_and_marks_missing_files() {
    if !media::ffmpeg_available() {
        // The media pass refuses to start without ffmpeg, whatever the
        // files are, so there is nothing to assert on a machine without it.
        return;
    }
    let output = tempfile::tempdir().unwrap();
    let attachments = output.path().join("attachments");
    fs::create_dir_all(&attachments).unwrap();
    fs::write(attachments.join("note.txt"), b"hello attachment").unwrap();
    let mut document = message_ir::testutil::sample_document("with attachments");
    document.messages[0].attachments = vec![
        attachment("note.txt", Some("attachments/note.txt")),
        attachment("gone.txt", Some("attachments/gone.txt")),
        attachment("never-staged.txt", None),
    ];
    let transforms = ExportTransforms {
        media: MediaMode::Convert,
        ..ExportTransforms::none()
    };

    apply_reexport_convert(
        std::slice::from_mut(&mut document),
        output.path(),
        &transforms,
    )
    .unwrap();

    let staged = &document.messages[0].attachments[0];
    assert_eq!(
        staged.digest_sha256.as_deref(),
        Some("7fa36b95d5c98859ed72b4787f3c28b29eaa103970786755c9711cbb19be631c")
    );
    assert_eq!(staged.size_bytes, Some(16));
    assert_eq!(staged.missing_reason, None);
    let path = staged.path.as_deref().unwrap();
    assert!(
        path.starts_with("attachments/") && path.ends_with("-7fa36b95d5c98859.txt"),
        "{path}"
    );
    assert_eq!(
        fs::read(output.path().join(path)).unwrap(),
        b"hello attachment"
    );
    assert_eq!(
        document.messages[0].attachments[1]
            .missing_reason
            .as_deref(),
        Some("file_missing")
    );
    assert_eq!(
        document.messages[0].attachments[2]
            .missing_reason
            .as_deref(),
        Some("file_missing")
    );
}

/// The sniffers are what stop the converter reading somebody else's file.
///
/// `detect_ir_export` matches on the extension and then asks a sniffer whether
/// the contents are really an IR export. Every one of those guards could be
/// replaced with `true` and nothing failed: the tests all pointed at real
/// exports, so the guards were never the thing that decided. A `.json` of
/// anything at all — a `package.json`, a browser bookmark dump — would have
/// been picked up as a conversation and read as empty.
#[test]
fn a_json_that_is_not_an_ir_export_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    for (name, body) in [
        // Valid JSON, wrong shape.
        ("package.json", r#"{"name":"thing","version":"1.0.0"}"#),
        // The right keys but the wrong schema version.
        (
            "old.json",
            r#"{"schema_version":3,"export":{},"conversation":{},"messages":[]}"#,
        ),
        // The right version but missing a required section.
        (
            "partial.json",
            r#"{"schema_version":4,"export":{},"messages":[]}"#,
        ),
        // Not JSON at all.
        ("broken.json", "{not json"),
    ] {
        std::fs::write(dir.path().join(name), body).unwrap();
    }

    let err = detect_ir_export(dir.path()).unwrap_err();
    assert!(
        err.to_string().contains("no Message Vault IR export found"),
        "unexpected error: {err}"
    );
}

/// The JSON Lines sniffer reads only the first line, so a file whose first
/// line is not a conversation must be refused whatever follows it.
#[test]
fn a_jsonl_whose_first_line_is_not_a_conversation_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("log.jsonl"),
        "{\"level\":\"info\",\"msg\":\"started\"}\n         {\"schema_version\":4,\"export\":{},\"conversation\":{},\"messages\":[]}\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("empty.ndjson"), "").unwrap();

    let err = detect_ir_export(dir.path()).unwrap_err();
    assert!(
        err.to_string().contains("no Message Vault IR export found"),
        "unexpected error: {err}"
    );
}

/// The CSV sniffer needs every column the IR writer emits. A spreadsheet that
/// happens to have a `text` column is not a conversation.
#[test]
fn a_csv_without_every_ir_column_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("budget.csv"),
        "date,text,amount\n2020-01-01,rent,1200\n",
    )
    .unwrap();
    // Every column but one: the check is `all`, not `any`.
    let mut headers: Vec<&str> = CSV_HEADERS.to_vec();
    headers.pop();
    std::fs::write(
        dir.path().join("almost.csv"),
        format!("{}\n", headers.join(",")),
    )
    .unwrap();

    let err = detect_ir_export(dir.path()).unwrap_err();
    assert!(
        err.to_string().contains("no Message Vault IR export found"),
        "unexpected error: {err}"
    );

    // And with every column it is accepted, so the refusal above was about the
    // missing one rather than about the file being unreadable.
    std::fs::remove_file(dir.path().join("almost.csv")).unwrap();
    std::fs::write(
        dir.path().join("real.csv"),
        format!("{}\n", CSV_HEADERS.join(",")),
    )
    .unwrap();
    assert_eq!(
        detect_ir_export(dir.path()).unwrap().format,
        OutputFormat::Csv
    );
}

/// An `.xml` is an SMS Backup & Restore export only when it is named
/// `smses.xml` or its first line says `<smses`. Any other XML in the folder —
/// an Android manifest, a settings dump — must be left alone.
#[test]
fn an_xml_that_is_not_an_smses_export_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("settings.xml"),
        "<?xml version=\"1.0\"?>\n<map><boolean name=\"x\" value=\"true\" /></map>\n",
    )
    .unwrap();

    let err = detect_ir_export(dir.path()).unwrap_err();
    assert!(
        err.to_string().contains("no Message Vault IR export found"),
        "unexpected error: {err}"
    );

    // Named `smses.xml`, it is taken whatever the first line says.
    std::fs::write(dir.path().join("smses.xml"), "<?xml version=\"1.0\"?>\n").unwrap();
    assert_eq!(
        detect_ir_export(dir.path()).unwrap().format,
        OutputFormat::Xml
    );
}

/// Sidecars are skipped by name, and each clause of that list was droppable
/// without failing a test. A `.tmp` counted as an export would make a
/// half-written file the thing the converter reads.
#[test]
fn every_kind_of_sidecar_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let ir_json = r#"{"schema_version":4,"export":{},"conversation":{},"messages":[]}"#;
    for name in [
        "conversation.meta.json",
        "conversation.json.tmp",
        ".hidden.json",
        "smses.xml.tmp",
        "smses.xml.sbrbody",
    ] {
        std::fs::write(dir.path().join(name), ir_json).unwrap();
    }
    std::fs::create_dir_all(dir.path().join("attachments")).unwrap();
    std::fs::write(dir.path().join("attachments/a.eml"), "From: x\n").unwrap();

    let err = detect_ir_export(dir.path()).unwrap_err();
    assert!(
        err.to_string().contains("no Message Vault IR export found"),
        "sidecars must not count as an export: {err}"
    );
}
