use super::*;
use media::MediaMode;
use std::fs;

/// An attachment size limit for tests that do not look at it.
const ASSET_MAX_BYTES: u64 = 512 * 1024 * 1024;

fn test_options(owner_phones: Vec<String>) -> ExtractOptions {
    ExtractOptions {
        backup_password: String::new(),
        attachment_media: AttachmentMedia::default(),
        media_max_resolution: MaxResolution::default(),
        media_max_fps: "30".into(),
        media_min_size: "20M".into(),
        obfuscate: false,
        timezone: String::new(),
        owner_phones,
        owner_emails: Vec::new(),
        attachment_root: String::new(),
        apple_contacts: String::new(),
        whatsapp_key: String::new(),
        whatsapp_wa: String::new(),
        whatsapp_media: String::new(),
        whatsapp_db: String::new(),
        whatsapp_business: false,
    }
}

#[test]
fn convert_and_compress_stage_originals_and_defer_the_media_step() {
    // The desktop runs conversion as its own pass so a gate can sit in
    // front of it. Asking the exporter to convert would spend the time
    // before the user has approved anything. Checked against the
    // iMessage source; every source routes attachment_media through
    // `exporter_attachment_media` before `Form` sees it.
    for chosen in [AttachmentMedia::Convert, AttachmentMedia::Compress] {
        let mut options = test_options(vec!["+15550100".into()]);
        options.attachment_media = chosen;
        let config = build_exporter_config("imessage-ios", "/backup", "/out", &options).unwrap();
        assert_eq!(
            config.media.mode,
            MediaMode::Clone,
            "{chosen:?} must stage originals"
        );
    }
}

#[test]
fn copy_and_skip_reach_the_exporter_unchanged() {
    for chosen in [AttachmentMedia::Clone, AttachmentMedia::Disabled] {
        let mut options = test_options(vec!["+15550100".into()]);
        options.attachment_media = chosen;
        let config = build_exporter_config("imessage-ios", "/backup", "/out", &options).unwrap();
        assert_eq!(
            config.media.mode,
            chosen.media_mode(),
            "{chosen:?} must reach the exporter unchanged"
        );
    }
}

#[test]
fn non_imessage_sources_defer_the_media_step_too() {
    // `exporter_attachment_media` gates `Form.attachment_media` for every
    // source, so a non-iMessage source (whatsapp-android here) must also
    // reach the exporter with Clone when Convert or Compress was chosen.
    let dump = tempfile::tempdir().unwrap();
    for chosen in [AttachmentMedia::Convert, AttachmentMedia::Compress] {
        let mut options = test_options(vec!["+15555550100".into()]);
        options.attachment_media = chosen;
        let config = build_exporter_config(
            "whatsapp-android",
            dump.path().to_str().unwrap(),
            "/out",
            &options,
        )
        .unwrap();
        assert_eq!(
            config.media.mode,
            MediaMode::Clone,
            "{chosen:?} must stage originals"
        );
    }
}

#[test]
fn compress_validates_the_minimum_size_before_staging() {
    // `Form.attachment_media` reads Clone for a real Compress choice (so
    // the exporter stages originals instead of converting), which means
    // `Form`'s own compress validation never runs for it. Without
    // `media_settings_for`, a malformed `media_min_size` would sail through
    // and only surface hours later, at the review.
    let mut options = test_options(Vec::new());
    options.attachment_media = AttachmentMedia::Compress;
    options.media_min_size = "banana".into();
    let err = media_settings_for(&options, ASSET_MAX_BYTES).unwrap_err();
    assert!(
        err.contains("banana"),
        "expected the malformed min-size value to be named: {err}"
    );
}

#[test]
fn compress_with_an_empty_max_fps_is_refused_naming_the_field() {
    // #1153: the form's Max FPS is free text, and a cleared field used to
    // fail only after hours of Staging, at the summary.
    for fps in ["", "  ", "fast", "0", "-5", "NaN", "inf"] {
        let mut options = test_options(Vec::new());
        options.attachment_media = AttachmentMedia::Compress;
        options.media_max_fps = fps.into();
        let err = media_settings_for(&options, ASSET_MAX_BYTES).unwrap_err();
        assert!(err.contains("Max FPS"), "{fps:?}: {err}");
    }
}

#[test]
fn an_empty_max_fps_is_no_problem_when_nothing_is_compressed() {
    for chosen in [
        AttachmentMedia::Clone,
        AttachmentMedia::Convert,
        AttachmentMedia::Disabled,
    ] {
        let mut options = test_options(Vec::new());
        options.attachment_media = chosen;
        options.media_max_fps = String::new();
        let settings = media_settings_for(&options, ASSET_MAX_BYTES).unwrap();
        assert_eq!(settings.mode, chosen.media_mode());
        assert_eq!(settings.compress, media::CompressOptions::default());
    }
}

#[test]
fn the_media_settings_carry_the_mode_the_fields_and_the_limit() {
    let mut options = test_options(Vec::new());
    options.attachment_media = AttachmentMedia::Compress;
    options.media_max_resolution = MaxResolution::P720;
    options.media_max_fps = "24".into();
    options.media_min_size = "5M".into();

    let settings = media_settings_for(&options, 123_456_789).unwrap();

    assert_eq!(settings.mode, MediaMode::Compress);
    assert_eq!(settings.compress.max_resolution, MaxResolution::P720);
    assert_eq!(settings.compress.max_fps, 24.0);
    assert_eq!(settings.compress.min_size_bytes, 5 * 1024 * 1024);
    // The server's limit, passed in. The desktop app has no number of its own.
    assert_eq!(settings.asset_max_bytes, 123_456_789);
}

#[test]
fn staging_records_the_media_settings_in_the_folder() {
    // The Staging Review's summary and the Media stage read them from here,
    // so the whole run works to the values Staging was started with.
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("sms.xml");
    fs::write(
        &input,
        r#"<?xml version='1.0' encoding='UTF-8' standalone='yes' ?>
<smses count="1">
  <sms address="+15555550101" date="1400773400000" type="1" body="hi" />
</smses>
"#,
    )
    .unwrap();
    let output = tmp.path().join("out");
    let mut options = test_options(vec!["+15555550100".into()]);
    options.attachment_media = AttachmentMedia::Convert;
    let config = build_exporter_config(
        "sms-backup-restore",
        input.to_str().unwrap(),
        output.to_str().unwrap(),
        &options,
    )
    .unwrap();
    let settings = media_settings_for(&options, ASSET_MAX_BYTES).unwrap();

    run_staging(&config, &output, &settings).unwrap();

    assert_eq!(
        message_staging::read_media_settings(&output).unwrap(),
        settings
    );
}

#[test]
fn jailbreak_uses_macos_platform_and_attachment_root() {
    let mut options = test_options(Vec::new());
    options.attachment_root = "/mnt/iphone/Library/SMS".into();
    options.apple_contacts = "/mnt/iphone/AddressBook.sqlitedb".into();
    options.obfuscate = true;
    let config = build_exporter_config(
        "imessage-jailbreak",
        "/mnt/iphone/sms.db",
        "/tmp/out",
        &options,
    )
    .unwrap();
    match config.source {
        SourceConfig::Apple(apple) => {
            assert_eq!(apple.platform, Some(ApplePlatform::MacOs));
            assert_eq!(
                apple.attachment_root.as_deref(),
                Some("/mnt/iphone/Library/SMS")
            );
            assert_eq!(
                apple.apple_contacts.as_deref(),
                Some(std::path::Path::new("/mnt/iphone/AddressBook.sqlitedb"))
            );
            assert!(apple.backup_password.is_none());
        }
        other => panic!("expected Apple, got {other:?}"),
    }
    assert!(!config.obfuscate.enabled);
}

#[test]
fn ios_backup_does_not_forward_attachment_root() {
    let mut options = test_options(Vec::new());
    options.attachment_root = "/ignored".into();
    options.apple_contacts = "/ignored-contacts".into();
    options.backup_password = "pw".into();
    let config =
        build_exporter_config("imessage-ios", "/backups/iphone", "/tmp/out", &options).unwrap();
    match config.source {
        SourceConfig::Apple(apple) => {
            assert_eq!(apple.platform, Some(ApplePlatform::Ios));
            assert_eq!(apple.backup_password.as_deref(), Some("pw"));
            // extract.rs blanks both extras for imessage-ios.
            assert!(apple.attachment_root.is_none());
            assert!(apple.apple_contacts.is_none());
        }
        other => panic!("expected Apple, got {other:?}"),
    }
}

#[test]
fn macos_forwards_optional_attachment_root() {
    let mut options = test_options(Vec::new());
    options.attachment_root = "/Users/sam/Library/Messages".into();
    let config = build_exporter_config(
        "imessage-macos",
        "/Users/sam/Library/Messages/chat.db",
        "/tmp/out",
        &options,
    )
    .unwrap();
    match config.source {
        SourceConfig::Apple(apple) => {
            assert_eq!(apple.platform, Some(ApplePlatform::MacOs));
            assert_eq!(
                apple.attachment_root.as_deref(),
                Some("/Users/sam/Library/Messages")
            );
        }
        other => panic!("expected Apple, got {other:?}"),
    }
}

#[test]
fn sms_backup_restore_requires_owner_phones() {
    let err = build_exporter_config(
        "sms-backup-restore",
        "/tmp/backup",
        "/tmp/out",
        &test_options(Vec::new()),
    )
    .unwrap_err();
    assert!(
        err.contains("phone number"),
        "expected phone requirement error, got {err}"
    );
}

#[test]
fn sms_backup_restore_passes_owner_phones() {
    let backup = tempfile::tempdir().unwrap();
    let config = build_exporter_config(
        "sms-backup-restore",
        backup.path().to_str().unwrap(),
        "/tmp/out",
        &test_options(vec!["+15551111".into(), "+15552222".into()]),
    )
    .unwrap();
    match config.source {
        SourceConfig::SmsBackupRestore(s) => {
            assert_eq!(s.owner_phones, vec!["+15551111", "+15552222"]);
        }
        other => panic!("expected SmsBackupRestore, got {other:?}"),
    }
}

#[test]
fn every_source_requires_an_existing_input_path() {
    let err = build_exporter_config(
        "sms-backup-restore",
        "/does/not/exist-sms-backup",
        "/tmp/out",
        &test_options(vec!["+15551111".into()]),
    )
    .unwrap_err();
    assert!(
        err.contains("does not exist"),
        "expected input-exists error, got {err}"
    );
}

#[test]
fn sms_backup_plus_requires_owner_emails() {
    // SMS Backup+ archives are Gmail-backed, so the Form needs at least
    // one owner email to tell sent from received; an empty list is a
    // validation error the desktop surfaces, not something it papers over.
    let backup = tempfile::tempdir().unwrap();
    let err = build_exporter_config(
        "sms-backup-plus",
        backup.path().to_str().unwrap(),
        "/tmp/out",
        &test_options(vec!["+15551111".into()]),
    )
    .unwrap_err();
    assert!(
        err.contains("email"),
        "expected email requirement error, got {err}"
    );
}

#[test]
fn sms_backup_plus_passes_owner_phones_and_emails() {
    let backup = tempfile::tempdir().unwrap();
    let mut options = test_options(vec!["+15551111".into()]);
    options.owner_emails = vec!["me@example.com".into(), "Me@Work.example".into()];
    let config = build_exporter_config(
        "sms-backup-plus",
        backup.path().to_str().unwrap(),
        "/tmp/out",
        &options,
    )
    .unwrap();
    match config.source {
        SourceConfig::SmsBackupPlus(s) => {
            assert_eq!(s.owner_phones, vec!["+15551111"]);
            assert_eq!(s.owner_emails, vec!["me@example.com", "Me@Work.example"]);
        }
        other => panic!("expected SmsBackupPlus, got {other:?}"),
    }
}

#[test]
fn whatsapp_android_forwards_key_and_optional_paths() {
    let mut options = test_options(vec!["+15555550100".into()]);
    options.whatsapp_key = "deadbeef".into();
    options.whatsapp_wa = "/tmp/wa.db".into();
    options.whatsapp_media = "/tmp/WhatsApp".into();
    options.whatsapp_db = "/tmp/msgstore.db".into();
    options.whatsapp_business = true;
    let dump = tempfile::tempdir().unwrap();
    let config = build_exporter_config(
        "whatsapp-android",
        dump.path().to_str().unwrap(),
        "/tmp/out",
        &options,
    )
    .unwrap();
    assert_eq!(config.inputs, vec![dump.path().to_path_buf()]);
    match config.source {
        SourceConfig::Whatsapp(wa) => {
            assert_eq!(wa.platform, Some(WhatsappPlatform::Android));
            assert_eq!(wa.key.as_deref(), Some("deadbeef"));
            assert_eq!(wa.wa.as_deref(), Some(std::path::Path::new("/tmp/wa.db")));
            assert_eq!(
                wa.media.as_deref(),
                Some(std::path::Path::new("/tmp/WhatsApp"))
            );
            assert_eq!(
                wa.db.as_deref(),
                Some(std::path::Path::new("/tmp/msgstore.db"))
            );
            assert!(wa.backup.is_none());
            assert!(!wa.business);
            assert_eq!(wa.owner_phone.as_deref(), Some("+15555550100"));
        }
        other => panic!("{other:?}"),
    }
}

/// An Android crypt backup carries no owner number, so an empty field is
/// refused before the run starts rather than importing with no owner.
#[test]
fn whatsapp_android_refuses_an_empty_owner_phone() {
    let dump = tempfile::tempdir().unwrap();
    let err = build_exporter_config(
        "whatsapp-android",
        dump.path().to_str().unwrap(),
        "/tmp/out",
        &test_options(Vec::new()),
    )
    .unwrap_err();
    assert!(
        err.contains("Owner's WhatsApp number is required."),
        "{err}"
    );
}

/// An encrypted iPhone backup's password reaches the WhatsApp exporter, which
/// decrypts WhatsApp's files with it. Android has no such password.
#[test]
fn whatsapp_ios_forwards_the_backup_password() {
    let backup = tempfile::tempdir().unwrap();
    let path = backup.path().to_str().unwrap();
    let mut options = test_options(vec!["+15555550100".into()]);
    options.backup_password = "pw".into();
    for (source, expected) in [("whatsapp-ios", Some("pw")), ("whatsapp-android", None)] {
        let config = build_exporter_config(source, path, "/tmp/out", &options).unwrap();
        let SourceConfig::Whatsapp(wa) = config.source else {
            panic!("{:?}", config.source);
        };
        assert_eq!(wa.backup_password.as_deref(), expected, "{source}");
    }
}

/// iPhone reads the number from the backup, so the field may be empty; when
/// filled it reaches the exporter as the fallback.
#[test]
fn whatsapp_ios_forwards_the_owner_phone_as_a_fallback() {
    let backup = tempfile::tempdir().unwrap();
    let path = backup.path().to_str().unwrap();
    let empty =
        build_exporter_config("whatsapp-ios", path, "/tmp/out", &test_options(Vec::new())).unwrap();
    let SourceConfig::Whatsapp(wa) = empty.source else {
        panic!("{:?}", empty.source);
    };
    assert!(wa.owner_phone.is_none());

    let filled = build_exporter_config(
        "whatsapp-ios",
        path,
        "/tmp/out",
        &test_options(vec!["+15555550100".into()]),
    )
    .unwrap();
    let SourceConfig::Whatsapp(wa) = filled.source else {
        panic!("{:?}", filled.source);
    };
    assert_eq!(wa.owner_phone.as_deref(), Some("+15555550100"));
}

#[test]
fn whatsapp_ios_omits_leftover_android_media_and_db() {
    let mut options = test_options(Vec::new());
    options.whatsapp_media = "/tmp/WhatsApp".into();
    options.whatsapp_db = "/tmp/msgstore.db".into();
    options.whatsapp_wa = "/tmp/ContactsV2.sqlite".into();
    let backup = tempfile::tempdir().unwrap();
    let config = build_exporter_config(
        "whatsapp-ios",
        backup.path().to_str().unwrap(),
        "/tmp/out",
        &options,
    )
    .unwrap();
    match config.source {
        SourceConfig::Whatsapp(wa) => {
            assert!(wa.media.is_none());
            assert!(wa.db.is_none());
            assert_eq!(
                wa.wa.as_deref(),
                Some(std::path::Path::new("/tmp/ContactsV2.sqlite"))
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn whatsapp_ios_sets_backup_from_folder_and_business() {
    let mut options = test_options(Vec::new());
    options.whatsapp_business = true;
    let backup = tempfile::tempdir().unwrap();
    let config = build_exporter_config(
        "whatsapp-ios",
        backup.path().to_str().unwrap(),
        "/tmp/out",
        &options,
    )
    .unwrap();
    match config.source {
        SourceConfig::Whatsapp(wa) => {
            assert_eq!(wa.platform, Some(WhatsappPlatform::Ios));
            assert_eq!(wa.backup.as_deref(), Some(backup.path()));
            assert!(wa.business);
            assert!(wa.key.is_none());
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn imazing_reads_dates_in_the_zone_the_screen_sent() {
    // iMazing dates carry no zone. Dropping this field leaves
    // `ExporterConfig.timezone` as None and the exporter reads every date in
    // the machine's zone, so the same folder exports differently on
    // different machines (#689).
    let mut options = test_options(Vec::new());
    options.timezone = "America/New_York".into();
    let folder = tempfile::tempdir().unwrap();
    let config = build_exporter_config(
        "imazing",
        folder.path().to_str().unwrap(),
        "/tmp/out",
        &options,
    )
    .unwrap();
    assert!(matches!(config.source, SourceConfig::Imazing(_)));
    assert_eq!(config.timezone.as_deref(), Some("America/New_York"));
}

#[test]
fn imazing_with_no_zone_leaves_the_exporter_its_fallback() {
    let options = test_options(Vec::new());
    let folder = tempfile::tempdir().unwrap();
    let config = build_exporter_config(
        "imazing",
        folder.path().to_str().unwrap(),
        "/tmp/out",
        &options,
    )
    .unwrap();
    assert_eq!(config.timezone, None);
}

/// Run the SMS Backup & Restore exporter on one MMS that carries an
/// attachment named `log.jsonl` whose bytes are the base64 `data`, and return
/// the conversation and message counts of the `extract:finished` payload.
fn counts_for_a_jsonl_attachment(data: &str) -> (u64, u64) {
    let tmp = tempfile::tempdir().unwrap();
    let xml = format!(
        r#"<?xml version='1.0' encoding='UTF-8' standalone='yes' ?>
<smses count="1">
  <mms date="1400773400000" msg_box="1" address="+15555550101">
    <parts>
      <part seq="0" ct="text/plain" text="the log" />
      <part seq="1" ct="application/octet-stream" name="log.jsonl" cl="log.jsonl" data="{data}" />
    </parts>
    <addrs>
      <addr address="+15555550101" type="137" charset="106" />
      <addr address="+15555550100" type="151" charset="106" />
    </addrs>
  </mms>
</smses>
"#
    );
    let input = tmp.path().join("sms.xml");
    fs::write(&input, xml).unwrap();
    let output = tmp.path().join("out");
    let config = build_exporter_config(
        "sms-backup-restore",
        input.to_str().unwrap(),
        output.to_str().unwrap(),
        &test_options(vec!["+15555550100".into()]),
    )
    .unwrap();

    let result = run_exporter(&config).unwrap();
    let staged: Vec<_> = fs::read_dir(output.join("attachments"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert!(
        staged
            .iter()
            .any(|path| path.extension().is_some_and(|ext| ext == "jsonl")),
        "the attachment is staged with its .jsonl extension: {staged:?}"
    );

    let payload: serde_json::Value = serde_json::from_str(&finished_payload(&result)).unwrap();
    (
        payload["files_parsed"].as_u64().unwrap(),
        payload["messages_parsed"].as_u64().unwrap(),
    )
}

#[test]
fn a_staged_jsonl_attachment_is_not_counted_as_a_conversation() {
    // Three JSON lines: {"a":1}, {"a":2}, {"a":3}.
    let counts = counts_for_a_jsonl_attachment("eyJhIjoxfQp7ImEiOjJ9CnsiYSI6M30K");

    assert_eq!(counts, (1, 1));
}

#[test]
fn a_staged_jsonl_attachment_that_is_not_utf8_does_not_fail_the_count() {
    // The bytes 0xff 0xfe and a newline, which are not UTF-8.
    let counts = counts_for_a_jsonl_attachment("//4K");

    assert_eq!(counts, (1, 1));
}
