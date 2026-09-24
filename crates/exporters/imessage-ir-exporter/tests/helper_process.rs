//! The exporter through the real `imessage-reader` process.
//!
//! These tests build the helper with cargo (a no-op once it is built), write
//! the small `chat.db` from `chat-db-fixture`, and run the exporter against
//! it. The exporter finds the program in `target/<profile>/` because this
//! test binary runs from `target/<profile>/deps/`. The build
//! is sent to the same target directory this test binary came from, so a run
//! under another one (cargo-llvm-cov uses `target/llvm-cov-target/`) still
//! puts the program where the exporter looks.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

use chat_db_fixture::{FRIEND_EMAIL, FRIEND_PHONE, OWNER, OWNER_EMAIL, PHOTO_BYTES, write_chat_db};
use message_ir::{ConversationDocument, IrDirection, IrMessage};
use message_ir_format::{
    read_conversation_csv, read_conversation_eml_dir, read_conversation_jsonl,
    read_conversation_mbox,
};
use message_vault_io_core::{
    AppleConfig, ApplePlatform, ExporterConfig, MediaConfig, OutputFormat, SourceConfig,
};

/// Build `imessage-reader` once per test binary and return its path.
fn helper_binary() -> &'static Path {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let mut command = Command::new(cargo);
        command
            .args([
                "build",
                "-p",
                "imessage-reader",
                "--message-format=json-render-diagnostics",
            ])
            .current_dir(env!("CARGO_MANIFEST_DIR"));
        if let Some(target_dir) = target_dir() {
            command.arg("--target-dir").arg(target_dir);
        }
        if !cfg!(debug_assertions) {
            command.arg("--release");
        }
        let output = command
            .output()
            .expect("run cargo build for imessage-reader");
        assert!(
            output.status.success(),
            "cargo build -p imessage-reader failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|message| message["reason"] == "compiler-artifact")
            .filter(|message| message["target"]["name"] == "imessage-reader")
            .find_map(|message| message["executable"].as_str().map(PathBuf::from))
            .expect("cargo reported the imessage-reader executable")
    })
}

/// The target directory this test binary was built into: it runs from
/// `<target>/<profile>/deps/`, so three levels up.
fn target_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().nth(3).map(Path::to_path_buf)
}

fn config(db_path: &Path, output: &Path, cancel: Option<Arc<AtomicBool>>) -> ExporterConfig {
    ExporterConfig {
        inputs: vec![db_path.to_path_buf()],
        output: output.to_path_buf(),
        timezone: None,
        obfuscate: Default::default(),
        media: MediaConfig::default(),
        cancel,
        log: None,
        progress: None,
        output_format: OutputFormat::Jsonl,
        resume: false,
        source: SourceConfig::Apple(AppleConfig {
            platform: Some(ApplePlatform::MacOs),
            ..AppleConfig::default()
        }),
    }
}

/// Every `.jsonl` file under `dir`, by name.
fn jsonl_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .collect();
    files.sort();
    files
}

#[test]
fn exports_a_mac_chat_db_through_the_helper_process() {
    helper_binary();
    let dir = tempfile::tempdir().unwrap();
    let db_path = write_chat_db(dir.path());
    let output = dir.path().join("out");

    let result = imessage_ir_exporter::run(&config(&db_path, &output, None)).unwrap();
    assert!(
        result
            .messages
            .iter()
            .any(|m| m.starts_with("Wrote jsonl export under")),
        "{:?}",
        result.messages
    );

    let files = jsonl_files(&output);
    assert_eq!(files.len(), 3, "one file per conversation: {files:?}");
    let all: String = files
        .iter()
        .map(|path| fs::read_to_string(path).unwrap())
        .collect();
    assert!(all.contains("\"guid-1\""), "{all}");
    assert!(all.contains("\"Nice\""), "{all}");
    assert!(all.contains("\"Saturday works\""), "{all}");
    assert!(
        all.contains("\"Weekend plans\""),
        "group title survives: {all}"
    );
    assert!(all.contains("friend@example.com"), "roster survives: {all}");
    assert!(all.contains("\"outgoing\""), "{all}");

    // The attachment was staged under attachments/ from the path the helper
    // resolved, and the document points at it.
    let staged: Vec<PathBuf> = walk(&output.join("attachments"));
    assert_eq!(staged.len(), 1, "{staged:?}");
    assert_eq!(fs::read(&staged[0]).unwrap(), PHOTO_BYTES);
    assert!(all.contains("\"attachments/"), "{all}");
}

/// Every file below `dir`, recursively.
fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(walk(&path));
            } else {
                out.push(path);
            }
        }
    }
    out
}

#[test]
fn identities_come_back_cleaned_from_the_helper_process() {
    helper_binary();
    let dir = tempfile::tempdir().unwrap();
    let db_path = write_chat_db(dir.path());

    let mut identities = imessage_ir_exporter::backup_identities(&db_path, false, None).unwrap();
    identities.sort();
    assert_eq!(
        identities,
        vec![OWNER.to_string(), OWNER_EMAIL.to_string()],
        "the phone from `P:`, bare and `tel:`-prefixed caller ids, and the \
         email from `E:`, each once, and the NULL caller id on one outgoing \
         row adds nothing"
    );
}

/// The exported document for the conversation Apple identifies as `chat_identifier`.
fn document_for(output: &Path, chat_identifier: &str) -> ConversationDocument {
    jsonl_files(output)
        .iter()
        .map(|path| read_conversation_jsonl(path).unwrap())
        .find(|doc| doc.conversation.chat_identifier == chat_identifier)
        .unwrap_or_else(|| panic!("no exported conversation for {chat_identifier}"))
}

/// The message with `guid`, from a document.
fn message<'a>(doc: &'a ConversationDocument, guid: &str) -> &'a IrMessage {
    doc.messages
        .iter()
        .find(|m| m.guid == guid)
        .unwrap_or_else(|| panic!("no message {guid} in {}", doc.conversation.chat_identifier))
}

/// The owner sends from a phone in one chat and from an email in another.
/// Every outgoing row is the owner's, whichever address it went out from,
/// and the row Apple left without a caller id takes its conversation's
/// owner address rather than nobody's.
#[test]
fn messages_from_either_owner_address_are_sent_by_the_owner() {
    helper_binary();
    let dir = tempfile::tempdir().unwrap();
    let db_path = write_chat_db(dir.path());
    let output = dir.path().join("out");

    imessage_ir_exporter::run(&config(&db_path, &output, None)).unwrap();

    let phone_chat = document_for(&output, FRIEND_PHONE);
    assert_eq!(phone_chat.export.owner_handle.as_deref(), Some(OWNER));
    let nice = message(&phone_chat, "guid-2");
    assert_eq!(nice.direction, IrDirection::Outgoing);
    assert_eq!(nice.sender_handle.as_deref(), Some(OWNER));
    assert_eq!(nice.owner_handle.as_deref(), Some(OWNER));
    let still_me = message(&phone_chat, "guid-5");
    assert_eq!(still_me.direction, IrDirection::Outgoing);
    assert_eq!(
        still_me.sender_handle.as_deref(),
        Some(OWNER),
        "a NULL caller id falls back to the conversation's owner"
    );
    let from_the_car = message(&phone_chat, "guid-6");
    assert_eq!(from_the_car.direction, IrDirection::Outgoing);
    assert_eq!(
        from_the_car.sender_handle.as_deref(),
        Some(OWNER),
        "a `tel:`-prefixed caller id is the same owner address, spelled as \
         the identities list spells it (#686)"
    );
    assert_eq!(from_the_car.owner_handle.as_deref(), Some(OWNER));
    let photo = message(&phone_chat, "guid-1");
    assert_eq!(photo.direction, IrDirection::Incoming);
    assert_eq!(photo.sender_handle.as_deref(), Some(FRIEND_PHONE));

    let email_chat = document_for(&output, FRIEND_EMAIL);
    assert_eq!(
        email_chat.export.owner_handle.as_deref(),
        Some(OWNER_EMAIL),
        "the email account's chat is owned by the email, with no `E:` prefix"
    );
    let from_mac = message(&email_chat, "guid-4");
    assert_eq!(from_mac.direction, IrDirection::Outgoing);
    assert_eq!(from_mac.sender_handle.as_deref(), Some(OWNER_EMAIL));
    assert_eq!(from_mac.owner_handle.as_deref(), Some(OWNER_EMAIL));
    let roster: Vec<_> = email_chat
        .conversation
        .participants
        .iter()
        .filter_map(|p| p.handle.as_deref())
        .collect();
    assert_eq!(
        roster,
        vec![FRIEND_EMAIL],
        "the owner's email is not a participant of the owner's own chat"
    );
}

/// The exporter's config for `format` rather than JSON Lines.
fn config_for(db_path: &Path, output: &Path, format: OutputFormat) -> ExporterConfig {
    ExporterConfig {
        output_format: format,
        ..config(db_path, output, None)
    }
}

/// The Apple-specific fields the program reads travel into the document:
/// the "Nice" row's body is one text part.
#[test]
fn a_message_keeps_its_apple_fields_in_the_document() {
    helper_binary();
    let dir = tempfile::tempdir().unwrap();
    let db_path = write_chat_db(dir.path());
    let output = dir.path().join("out");

    imessage_ir_exporter::run(&config(&db_path, &output, None)).unwrap();

    let phone_chat = document_for(&output, FRIEND_PHONE);
    let nice = message(&phone_chat, "guid-2");
    let fields = nice.imessage.as_ref().expect("the row has Apple fields");
    assert_eq!(
        fields.parts,
        Some(serde_json::json!([{ "index": 0, "kind": "run", "text": "Nice" }]))
    );
    assert!(!fields.is_reply);
}

/// A CSV export writes one file per conversation and copies the photo
/// under `attachments/`, where the photo's row points.
#[test]
fn a_csv_export_writes_each_conversation_and_copies_the_photo() {
    helper_binary();
    let dir = tempfile::tempdir().unwrap();
    let db_path = write_chat_db(dir.path());
    let output = dir.path().join("out");

    let result =
        imessage_ir_exporter::run(&config_for(&db_path, &output, OutputFormat::Csv)).unwrap();
    assert!(
        result.messages.iter().any(|l| l == "  saved 1 attachments"),
        "{:#?}",
        result.messages
    );

    let csv_files: Vec<PathBuf> = fs::read_dir(&output)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "csv"))
        .collect();
    assert_eq!(
        csv_files.len(),
        3,
        "one file per conversation: {csv_files:?}"
    );

    let staged = walk(&output.join("attachments"));
    assert_eq!(staged.len(), 1, "{staged:?}");
    assert_eq!(fs::read(&staged[0]).unwrap(), PHOTO_BYTES);

    let phone_chat = read_conversation_csv(&output.join(format!("{FRIEND_PHONE}.csv"))).unwrap();
    assert_eq!(phone_chat.messages.len(), 4);
    let photo = message(&phone_chat, "guid-1");
    assert_eq!(photo.attachments.len(), 1);
    let path = photo.attachments[0]
        .path
        .as_deref()
        .expect("the photo's path");
    assert!(path.starts_with("attachments/"), "{path}");
}

/// An MBOX or EML export with attachment copying on carries the photo's
/// bytes as a MIME part, and writes no `attachments/` folder beside it.
#[test]
fn a_mail_archive_export_embeds_the_photo() {
    helper_binary();
    let dir = tempfile::tempdir().unwrap();
    let db_path = write_chat_db(dir.path());

    for format in [OutputFormat::Mbox, OutputFormat::Eml] {
        let output = dir.path().join(format.as_str());
        imessage_ir_exporter::run(&config_for(&db_path, &output, format)).unwrap();

        let phone_chat = if format == OutputFormat::Mbox {
            read_conversation_mbox(&output.join(format!("{FRIEND_PHONE}.mbox"))).unwrap()
        } else {
            read_conversation_eml_dir(&output.join(FRIEND_PHONE)).unwrap()
        };
        let attachments: Vec<_> = phone_chat
            .messages
            .iter()
            .flat_map(|m| &m.attachments)
            .collect();
        assert_eq!(attachments.len(), 1, "{format:?}");
        assert_eq!(
            attachments[0].bytes.as_deref(),
            Some(PHOTO_BYTES),
            "{format:?}"
        );
        assert!(!output.join("attachments").exists(), "{format:?}");
    }
}

#[test]
fn a_cancelled_run_stops_and_kills_the_helper() {
    helper_binary();
    let dir = tempfile::tempdir().unwrap();
    let db_path = write_chat_db(dir.path());
    let output = dir.path().join("out");
    let cancel = Arc::new(AtomicBool::new(true));

    let err = imessage_ir_exporter::run(&config(&db_path, &output, Some(cancel))).unwrap_err();
    assert_eq!(err.to_string(), "cancelled");
    assert!(jsonl_files(&output).is_empty());
}

#[test]
fn an_unreadable_database_is_reported_in_the_helpers_words() {
    helper_binary();
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("chat.db");
    fs::write(&db_path, b"this is not sqlite").unwrap();
    let output = dir.path().join("out");

    let err = imessage_ir_exporter::run(&config(&db_path, &output, None)).unwrap_err();
    let text = format!("{err:#}");
    assert!(
        text.contains("not a database") || text.contains("file is not a database"),
        "{text}"
    );
}

#[test]
fn a_second_run_reuses_the_cancelled_flag_only_when_set() {
    helper_binary();
    let dir = tempfile::tempdir().unwrap();
    let db_path = write_chat_db(dir.path());
    let output = dir.path().join("out");
    let cancel = Arc::new(AtomicBool::new(false));

    imessage_ir_exporter::run(&config(&db_path, &output, Some(cancel.clone()))).unwrap();
    assert_eq!(jsonl_files(&output).len(), 3);
    cancel.store(true, Ordering::Relaxed);
    let err = imessage_ir_exporter::run(&config(&db_path, &output, Some(cancel))).unwrap_err();
    assert_eq!(err.to_string(), "cancelled");
}
