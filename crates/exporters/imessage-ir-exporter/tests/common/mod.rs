//! What every test that runs the real `imessage-reader` shares: the build
//! of the program and the exporter's config for the `chat.db` fixture.
//!
//! The build is a no-op once the program is built. The exporter finds the
//! program in `target/<profile>/` because a test binary runs from
//! `target/<profile>/deps/`. The build is sent to the same target directory
//! the test binary came from, so a run under another one (cargo-llvm-cov
//! uses `target/llvm-cov-target/`) still puts the program where the
//! exporter looks.

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, OnceLock, atomic::AtomicBool},
};

use message_crate_core::{
    AppleConfig, ApplePlatform, ExporterConfig, MediaConfig, OutputFormat, SourceConfig,
};

/// Build `imessage-reader` once per test binary and return its path.
pub fn helper_binary() -> &'static Path {
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

/// A JSON Lines export of the Mac `chat.db` at `db_path` into `output`.
pub fn config(db_path: &Path, output: &Path, cancel: Option<Arc<AtomicBool>>) -> ExporterConfig {
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
