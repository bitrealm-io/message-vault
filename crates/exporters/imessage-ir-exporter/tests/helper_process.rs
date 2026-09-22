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

use chat_db_fixture::{PHOTO_BYTES, write_chat_db};
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
    assert_eq!(files.len(), 2, "one file per conversation: {files:?}");
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
    assert_eq!(identities, vec!["+15550000001".to_string()]);
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
    assert_eq!(jsonl_files(&output).len(), 2);
    cancel.store(true, Ordering::Relaxed);
    let err = imessage_ir_exporter::run(&config(&db_path, &output, Some(cancel))).unwrap_err();
    assert_eq!(err.to_string(), "cancelled");
}
