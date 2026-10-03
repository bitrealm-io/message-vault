//! What every test that runs the real `imessage-reader` shares: the build
//! of the program (`ios_backup::reader_build`) and the exporter's config
//! for the `chat.db` fixture.

use std::{
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};

use message_crate_core::{
    AppleConfig, ApplePlatform, ExporterConfig, MediaConfig, OutputFormat, SourceConfig,
};

/// Build `imessage-reader` once per test binary, name it in
/// `MESSAGE_CRATE_IMESSAGE_READER` for the exporter, and return its path.
/// Every test calls this before it runs the exporter.
pub fn helper_binary() -> &'static Path {
    ios_backup::reader_build::build_imessage_reader()
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
