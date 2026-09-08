use message_ir::{
    ConversationDocument, ConversationHeader, IrConversationType, IrMessage, SCHEMA_VERSION,
};

use super::*;

/// `demo_seed.toml` shrunk to a dozen contacts. Every section keeps the shape
/// of the checked-in file, so every writer still runs: iMessage-only,
/// Android-only, overlap, WhatsApp, groups, unassigned handles, orphans, and
/// both empty threads. Conversations stay long enough (about a hundred
/// messages) to reach the photo, other-attachment, tapback, and reply strides.
const SMALL_SEED_TOML: &str = r#"
seed = 7
out = "replaced by the test"
reference_time = "2026-08-01T12:00:00Z"

[contacts]
count = 12
no_name = 0.1
first_last = 0.6
first_middle_last = 0.2
first_only = 0.2
us_phones = 0.8
inactive_fraction = 0.1
no_messages_fraction = 0.1
multi_phone_fraction = 0.2

[labels]
names = ["Family", "Work", "College", "Inactive"]
family = 0.3
work = 0.3
college = 0.3

[one_to_one]
typical_min = 40
typical_max = 60
min_per_year = 10
max_per_year = 120
low_tail = 0.1
high_tail = 0.1
span_mean_years = 2.0
span_mean_jitter = 0.5
span_max_years = 4.0
newest_days = 7
one_to_one_fraction = 0.9

[groups]
per_contact_mean = 2.0
per_contact_min = 0
per_contact_max = 4
participants_mean = 3.0
participants_min = 2
participants_max = 6
large_min_count = 1
large_participants_min = 4
large_participants_max = 6
typical_min = 40
typical_max = 80
min_per_year = 10
max_per_year = 200
low_tail = 0.1
high_tail = 0.1
span_mean_years = 1.5
span_max_years = 3.0
phone_only_fraction = 0.25

[messages]
emoji_probability = 0.05
jpg_base_stride = 5
other_base_stride = 7
tapback_stride = 6
reply_stride = 8
apple_fallback_transport_fraction = 0.2

[edge_cases]
unassigned_phones = 2
unassigned_emails = 1
orphaned_messages = 3
empty_individual = true
empty_group = true

[sources]
android_only_fraction = 0.25
overlap_count = 2
overlap_shared_fraction = 0.5
overlap_android_extra_min = 3
overlap_android_extra_max = 6
whatsapp_contact_fraction = 0.5
"#;

/// The counts seed 7 produces from [`SMALL_SEED_TOML`].
fn expected_small_stats() -> GenStats {
    GenStats {
        contacts: 12,
        conversation_files: 31,
        messages: 2663,
        attachment_refs: 126,
        groups: 5,
    }
}

/// Write [`SMALL_SEED_TOML`] into `dir` and return its path.
fn write_small_seed_toml(dir: &Path) -> PathBuf {
    let path = dir.join("demo_seed.toml");
    fs::write(&path, SMALL_SEED_TOML).expect("write the small seed file");
    path
}

/// Load the small seed file from `dir` with `out` pointed at `dir/demo`.
fn small_config(dir: &Path) -> SeedConfig {
    let mut cfg = SeedConfig::load(&write_small_seed_toml(dir)).expect("load the small seed file");
    cfg.out = dir
        .join("demo")
        .to_str()
        .expect("UTF-8 test path")
        .to_string();
    cfg
}

/// Read one demo conversation file: a header line, then one message per line.
fn read_document(path: &Path) -> ConversationDocument {
    let text = fs::read_to_string(path).expect("read conversation file");
    let mut lines = text.lines();
    let header: ConversationHeader = serde_json::from_str(lines.next().expect("header line"))
        .unwrap_or_else(|error| panic!("parse header of {}: {error}", path.display()));
    assert_eq!(header.schema_version, SCHEMA_VERSION, "{}", path.display());
    let messages: Vec<IrMessage> = lines
        .map(|line| {
            serde_json::from_str(line)
                .unwrap_or_else(|error| panic!("parse message in {}: {error}", path.display()))
        })
        .collect();
    header.into_document(messages, None)
}

/// Every conversation file under `out/staging/<source>`, with its source folder name.
fn read_bundle(out: &Path) -> Vec<(String, ConversationDocument)> {
    let mut documents = Vec::new();
    for source in [IMESSAGE_SOURCE, SBR_SOURCE, WHATSAPP_SOURCE] {
        let staging = out.join("staging").join(source);
        let mut paths: Vec<PathBuf> = fs::read_dir(&staging)
            .expect("list staging folder")
            .map(|entry| entry.expect("staging entry").path())
            .filter(|path| is_jsonl_file(path))
            .collect();
        paths.sort();
        for path in paths {
            documents.push((source.to_string(), read_document(&path)));
        }
    }
    documents
}

/// Every file under `root` as `(relative path, bytes)`, sorted by path.
fn tree_contents(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).expect("list directory") {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .expect("path under root")
                    .to_path_buf();
                files.push((relative, fs::read(&path).expect("read file")));
            }
        }
    }
    files.sort();
    files
}

#[test]
fn generate_writes_three_backups_the_config_files_and_a_readme() {
    let temp = tempfile::tempdir().expect("create test directory");
    let cfg = small_config(temp.path());
    let out = Path::new(&cfg.out);

    let stats = generate(&cfg).expect("generate the small bundle");

    assert_eq!(stats, expected_small_stats());
    for relative in [
        "staging/imessage/attachments",
        "staging/sms-backup-restore/attachments",
        "staging/whatsapp/attachments",
        "config",
    ] {
        assert!(out.join(relative).is_dir(), "{relative} is a directory");
    }
    for relative in [
        "config/config.toml",
        "config/seed.toml",
        "config/contacts.vcf",
        "README.md",
        "staging/imessage/attachments/sunset.jpg",
        "staging/sms-backup-restore/attachments/sunset.jpg",
        "staging/whatsapp/attachments/sunset.jpg",
    ] {
        assert!(out.join(relative).is_file(), "{relative} is a file");
    }
    let vcf = fs::read_to_string(out.join("config/contacts.vcf")).expect("read contacts.vcf");
    assert_eq!(vcf.matches("BEGIN:VCARD").count(), 12);
    let readme = fs::read_to_string(out.join("README.md")).expect("read README.md");
    assert!(readme.contains("## Contents (seed 7)"), "{readme}");
    assert!(readme.contains("| Contacts (VCF) | 12 |"), "{readme}");
    let leftovers: Vec<String> = fs::read_dir(temp.path())
        .expect("list test directory")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.starts_with(".demo-seed-"))
        .collect();
    assert_eq!(leftovers, Vec::<String>::new());
}

#[test]
fn every_conversation_file_is_a_current_schema_document_and_the_counts_match_the_stats() {
    let temp = tempfile::tempdir().expect("create test directory");
    let cfg = small_config(temp.path());

    let stats = generate(&cfg).expect("generate the small bundle");
    let documents = read_bundle(Path::new(&cfg.out));

    assert_eq!(documents.len(), stats.conversation_files);
    let messages: usize = documents.iter().map(|(_, doc)| doc.messages.len()).sum();
    assert_eq!(messages, stats.messages);
    let attachments: usize = documents
        .iter()
        .flat_map(|(_, doc)| &doc.messages)
        .map(|message| message.attachments.len())
        .sum();
    assert_eq!(attachments, stats.attachment_refs);
    let groups = documents
        .iter()
        .filter(|(source, doc)| {
            source == IMESSAGE_SOURCE
                && doc.conversation.conversation_type == IrConversationType::Group
        })
        .count();
    assert_eq!(
        groups,
        stats.groups + 1,
        "every roster group plus the empty group"
    );
    for (source, doc) in &documents {
        assert_eq!(
            &doc.export.source, source,
            "{}",
            doc.conversation.chat_identifier
        );
        assert_eq!(doc.export.tool, "demo-seed");
    }
    let per_source = |wanted: &str| {
        documents
            .iter()
            .filter(|(source, _)| source == wanted)
            .count()
    };
    assert_eq!(per_source(IMESSAGE_SOURCE), 20);
    assert_eq!(per_source(SBR_SOURCE), 5);
    assert_eq!(per_source(WHATSAPP_SOURCE), 6);
    let empty_threads = documents
        .iter()
        .filter(|(_, doc)| doc.messages.is_empty())
        .count();
    assert_eq!(empty_threads, 2, "one empty individual and one empty group");
    let replies = documents
        .iter()
        .flat_map(|(_, doc)| &doc.messages)
        .filter(|message| message.imessage.as_ref().is_some_and(|im| im.is_reply))
        .count();
    assert_eq!(replies, 167);
    let tapbacks = documents
        .iter()
        .flat_map(|(_, doc)| &doc.messages)
        .filter(|message| {
            message
                .imessage
                .as_ref()
                .is_some_and(|im| im.tapbacks.is_some())
        })
        .count();
    assert_eq!(tapbacks, 241);
}

#[test]
fn the_same_seed_writes_the_same_bundle_twice() {
    let first = tempfile::tempdir().expect("create first test directory");
    let second = tempfile::tempdir().expect("create second test directory");

    let first_stats = generate(&small_config(first.path())).expect("generate the first bundle");
    let second_stats = generate(&small_config(second.path())).expect("generate the second bundle");

    assert_eq!(first_stats, second_stats);
    assert_eq!(
        tree_contents(&first.path().join("demo")),
        tree_contents(&second.path().join("demo"))
    );
}

#[test]
fn generate_replaces_an_earlier_bundle_and_removes_its_backup() {
    let temp = tempfile::tempdir().expect("create test directory");
    let cfg = small_config(temp.path());
    let out = Path::new(&cfg.out);
    let stale = out
        .join("staging")
        .join(IMESSAGE_SOURCE)
        .join("stale.jsonl");
    fs::create_dir_all(stale.parent().expect("stale parent")).expect("create stale staging");
    fs::write(&stale, b"{}\n").expect("write stale conversation");
    fs::write(out.join("README.md"), b"old readme").expect("write old readme");

    let stats = generate(&cfg).expect("generate over the earlier bundle");

    assert_eq!(stats, expected_small_stats());
    assert!(
        !stale.exists(),
        "the earlier staging folder is replaced whole"
    );
    let readme = fs::read_to_string(out.join("README.md")).expect("read README.md");
    assert!(
        readme.starts_with("# Message Vault demo dataset"),
        "{readme}"
    );
    assert!(!out.join(".previous-active").exists());
}

#[test]
fn generate_to_loads_the_seed_file_and_writes_the_bundle_at_out() {
    let temp = tempfile::tempdir().expect("create test directory");
    let seed_file = write_small_seed_toml(temp.path());
    let out = temp.path().join("bundle");

    let stats = generate_to(&seed_file, &out).expect("generate from the seed file");

    assert_eq!(stats, expected_small_stats());
    assert!(out.join("README.md").is_file());
    assert_eq!(read_bundle(&out).len(), stats.conversation_files);
}

#[test]
fn generate_to_fails_when_the_seed_file_is_missing() {
    let temp = tempfile::tempdir().expect("create test directory");
    let seed_file = temp.path().join("missing.toml");
    let out = temp.path().join("bundle");

    let error = generate_to(&seed_file, &out).expect_err("no seed file, no bundle");

    assert!(
        error.to_string().contains("read demo-seed config"),
        "{error:#}"
    );
    assert!(!out.exists());
}

#[test]
fn failed_generation_preserves_existing_bundle() {
    let temp = tempfile::tempdir().expect("create test directory");
    let active = temp.path().join("active");
    let prepared = temp.path().join("prepared");
    let existing_file = active
        .join("staging")
        .join(IMESSAGE_SOURCE)
        .join("existing.jsonl");
    let existing_parent = existing_file.parent().expect("existing file parent");
    fs::create_dir_all(existing_parent).expect("create active staging");
    let original = b"existing demo bytes\n";
    fs::write(&existing_file, original).expect("write existing file");

    let result = prepare_and_replace(&active, &prepared, |root| {
        fs::create_dir_all(root.join("staging").join(IMESSAGE_SOURCE))?;
        fs::write(
            root.join("staging")
                .join(IMESSAGE_SOURCE)
                .join("partial.jsonl"),
            b"partial replacement\n",
        )?;
        anyhow::bail!("preparation failed on purpose");
    });

    assert!(result.is_err());
    assert_eq!(
        fs::read(&existing_file).expect("read existing file"),
        original
    );
}

#[test]
fn move_path_copies_file_when_rename_crosses_devices() {
    let temp = tempfile::tempdir().expect("create test directory");
    let source = temp.path().join("README.md");
    let destination = temp.path().join("backup").join("README.md");
    fs::write(&source, b"new readme").expect("write source file");
    fs::create_dir_all(destination.parent().expect("backup parent"))
        .expect("create backup directory");

    move_path_with(&source, &destination, |_source, _destination| {
        Err(std::io::Error::new(
            std::io::ErrorKind::CrossesDevices,
            "Invalid cross-device link",
        ))
    })
    .expect("copy after cross-device rename");

    assert!(!source.exists(), "source file must be removed after copy");
    assert_eq!(
        fs::read(&destination).expect("read destination file"),
        b"new readme"
    );
}

#[test]
fn move_path_copies_directory_when_rename_crosses_devices() {
    let temp = tempfile::tempdir().expect("create test directory");
    let source = temp.path().join("config");
    let destination = temp.path().join("backup").join("config");
    fs::create_dir_all(&source).expect("create source directory");
    fs::write(source.join("marker"), b"hello").expect("write source file");
    fs::create_dir_all(destination.parent().expect("backup parent"))
        .expect("create backup directory");

    move_path_with(&source, &destination, |_source, _destination| {
        Err(std::io::Error::new(
            std::io::ErrorKind::CrossesDevices,
            "Invalid cross-device link",
        ))
    })
    .expect("copy after cross-device rename");

    assert!(
        !source.exists(),
        "source directory must be removed after copy"
    );
    assert_eq!(
        fs::read(destination.join("marker")).expect("read destination file"),
        b"hello"
    );
}

#[test]
fn replace_generated_paths_installs_when_every_rename_crosses_devices() {
    let temp = tempfile::tempdir().expect("create test directory");
    let active = temp.path().join("active");
    let prepared = temp.path().join("prepared");
    write_bundle_paths(&active, b"old");
    write_bundle_paths(&prepared, b"new");

    replace_generated_paths_with(&active, &prepared, |source, destination| {
        move_path_with(source, destination, |_source, _destination| {
            Err(std::io::Error::new(
                std::io::ErrorKind::CrossesDevices,
                "Invalid cross-device link",
            ))
        })
    })
    .expect("install after cross-device renames");

    assert_bundle_paths(&active, b"new");
}

#[test]
fn replacement_failure_at_each_generated_path_restores_all_old_paths() {
    for failing_install in 1..=3 {
        let temp = tempfile::tempdir().expect("create test directory");
        let active = temp.path().join("active");
        let prepared = temp.path().join("prepared");
        write_bundle_paths(&active, b"old");
        write_bundle_paths(&prepared, b"new");
        let mut installs = 0;

        let result = replace_generated_paths_with(&active, &prepared, |source, destination| {
            if source.starts_with(&prepared) && destination.starts_with(&active) {
                installs += 1;
                if installs == failing_install {
                    anyhow::bail!("install failed on purpose {failing_install}");
                }
            }
            fs::rename(source, destination).map_err(Into::into)
        });

        assert!(result.is_err(), "install {failing_install} must fail");
        assert_bundle_paths(&active, b"old");
    }
}

#[test]
fn restore_attempts_all_paths_after_one_restore_fails() {
    let temp = tempfile::tempdir().expect("create test directory");
    let active = temp.path().join("active");
    let prepared = temp.path().join("prepared");
    write_bundle_paths(&active, b"old");
    write_bundle_paths(&prepared, b"new");
    let mut installs = 0;
    let mut restored_staging = false;

    let result = replace_generated_paths_with(&active, &prepared, |source, destination| {
        if source.starts_with(&prepared) && destination.starts_with(&active) {
            installs += 1;
            if installs == 3 {
                anyhow::bail!("README install failed on purpose");
            }
        }
        if source.ends_with(".previous-active/config") {
            anyhow::bail!("config restore failed on purpose");
        }
        if source.ends_with(".previous-active/staging") {
            restored_staging = true;
        }
        fs::rename(source, destination).map_err(Into::into)
    });

    let error = result.expect_err("replacement must fail").to_string();
    assert!(
        restored_staging,
        "staging restoration must still be attempted"
    );
    assert!(error.contains("config restore failed on purpose"));
    assert!(prepared.join(".previous-active/config").exists());
}

/// Write `staging/marker`, `config/marker`, and `README.md` with the same bytes.
fn write_bundle_paths(root: &Path, marker: &[u8]) {
    fs::create_dir_all(root.join("staging")).expect("create staging directory");
    fs::create_dir_all(root.join("config")).expect("create config directory");
    fs::write(root.join("staging/marker"), marker).expect("write staging marker");
    fs::write(root.join("config/marker"), marker).expect("write config marker");
    fs::write(root.join("README.md"), marker).expect("write README marker");
}

/// Check that `staging/marker`, `config/marker`, and `README.md` still hold `marker`.
fn assert_bundle_paths(root: &Path, marker: &[u8]) {
    assert_eq!(
        fs::read(root.join("staging/marker")).expect("staging"),
        marker
    );
    assert_eq!(
        fs::read(root.join("config/marker")).expect("config"),
        marker
    );
    assert_eq!(fs::read(root.join("README.md")).expect("README"), marker);
}
