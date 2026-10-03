//! The real `imessage-reader`, built for the tests that start it, here and
//! in `imessage-ir-exporter`. The fakes in `testutil` stand in for it on
//! Unix; these tests run the program itself on every platform.

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};

use crate::helper::HELPER_PATH_ENV;

/// Build `imessage-reader` once per test binary, name it in
/// `MESSAGE_CRATE_IMESSAGE_READER` for [`crate::Helper`], and return its
/// path. Every test calls this before it starts the program.
///
/// A test binary runs from `target/<profile>/deps/`, not beside the program,
/// so the path comes from cargo's own report of what it built. The build
/// goes to the target directory the test binary came from, so a run under
/// another one (cargo-llvm-cov uses `target/llvm-cov-target/`) uses the
/// program built for it.
///
/// # Panics
///
/// Panics when cargo cannot build the program or reports no executable.
pub fn build_imessage_reader() -> &'static Path {
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
        let program = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|message| message["reason"] == "compiler-artifact")
            .filter(|message| message["target"]["name"] == "imessage-reader")
            .find_map(|message| message["executable"].as_str().map(PathBuf::from))
            .expect("cargo reported the imessage-reader executable");
        // SAFETY: every test calls this first, and the others wait on this
        // `OnceLock` while it runs, so no test thread reads the environment
        // until the variable is set.
        unsafe {
            std::env::set_var(HELPER_PATH_ENV, &program);
        }
        program
    })
}

/// The target directory the running test binary was built into, taken as
/// three levels above it: `<target>/<profile>/deps/`.
fn target_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().nth(3).map(Path::to_path_buf)
}
