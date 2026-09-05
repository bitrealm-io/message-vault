use super::*;
use media::MediaMode;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

fn test_options(owner_phones: Vec<String>) -> ExtractOptions {
    ExtractOptions {
        backup_password: String::new(),
        attachment_media: AttachmentMedia::default(),
        media_max_resolution: MaxResolution::default(),
        media_max_fps: "30".into(),
        media_min_size: "20M".into(),
        obfuscate: false,
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
    // iMessage source, which routes attachment_media through `Form` —
    // the only path that also exercises `exporter_attachment_media`.
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
        let mut options = test_options(Vec::new());
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
fn imessage_compress_still_validates_media_fields_up_front() {
    // `Form.attachment_media` reads Clone for a real Compress choice (so
    // the exporter stages originals instead of converting), which means
    // `Form`'s own compress validation no longer runs for it. Without the
    // explicit `parse_compress_options` call in `build_exporter_config`,
    // a malformed `media_min_size` would sail through here and only
    // surface hours later, at the approval gate.
    let mut options = test_options(Vec::new());
    options.attachment_media = AttachmentMedia::Compress;
    options.media_min_size = "banana".into();
    let err = build_exporter_config("imessage-ios", "/backup", "/out", &options).unwrap_err();
    assert!(
        err.contains("banana"),
        "expected the malformed min-size value to be named: {err}"
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
fn counts_exact_messages_written_to_jsonl_output() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("message-vault-extract-count-{unique}"));
    fs::create_dir_all(root.join("nested")).unwrap();
    fs::write(
        root.join("one.jsonl"),
        "{\"conversation\":{}}\n{\"guid\":\"one\"}\n{\"guid\":\"two\"}\n",
    )
    .unwrap();
    fs::write(
        root.join("nested/two.jsonl"),
        "{\"conversation\":{}}\n{\"guid\":\"three\"}\n",
    )
    .unwrap();
    fs::write(root.join("ignored.txt"), "not jsonl\n").unwrap();

    let counts = count_jsonl_output(&root).unwrap();

    assert_eq!(counts.files, 2);
    assert_eq!(counts.messages, 3);
    fs::remove_dir_all(root).unwrap();
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
    let mut options = test_options(Vec::new());
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
        }
        other => panic!("{other:?}"),
    }
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
