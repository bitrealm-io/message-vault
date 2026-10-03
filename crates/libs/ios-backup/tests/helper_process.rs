//! The identities read through the real `imessage-reader` process.
//!
//! The test writes the small `chat.db` from `chat-db-fixture`, builds the
//! program, and asks it which addresses the device sent from. A test binary
//! runs from `target/<profile>/deps/`, not beside the program in
//! `target/<profile>/`, so the build names the program through
//! `MESSAGE_CRATE_IMESSAGE_READER`.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};

use chat_db_fixture::{OWNER, OWNER_EMAIL, write_chat_db};

/// Build `imessage-reader` once per test binary, into the target directory
/// this test binary came from (cargo-llvm-cov uses its own), and name it in
/// `MESSAGE_CRATE_IMESSAGE_READER`, where [`ios_backup::Helper`] looks.
fn build_helper() {
    static BUILT: OnceLock<()> = OnceLock::new();
    BUILT.get_or_init(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let mut command = Command::new(cargo);
        command
            .args(["build", "-p", "imessage-reader"])
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
        let exe = std::env::current_exe().expect("this test binary's path");
        let profile_dir = exe
            .ancestors()
            .nth(2)
            .expect("a test binary runs from target/<profile>/deps/");
        let program = profile_dir.join(format!("imessage-reader{}", std::env::consts::EXE_SUFFIX));
        // SAFETY: the one test calls `build_helper` before it starts the
        // program, and nothing else reads the environment meanwhile.
        unsafe {
            std::env::set_var("MESSAGE_CRATE_IMESSAGE_READER", program);
        }
    });
}

/// The target directory this test binary was built into: it runs from
/// `<target>/<profile>/deps/`, so three levels up.
fn target_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().nth(3).map(Path::to_path_buf)
}

#[test]
fn identities_come_back_cleaned_from_the_helper_process() {
    build_helper();
    let dir = tempfile::tempdir().unwrap();
    let db_path = write_chat_db(dir.path());
    let scratch_root = tempfile::tempdir().unwrap();

    let mut identities =
        ios_backup::backup_identities(&db_path, false, None, scratch_root.path()).unwrap();
    let left: Vec<_> = fs::read_dir(scratch_root.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .filter(|name| name != ".lock")
        .collect();
    assert!(
        left.is_empty(),
        "the request's scratch folder stays: {left:?}"
    );
    identities.sort();
    assert_eq!(
        identities,
        vec![OWNER.to_string(), OWNER_EMAIL.to_string()],
        "the phone from `P:`, bare and `tel:`-prefixed caller ids, and the \
         email from `E:`, each once, and the NULL caller id on one outgoing \
         row adds nothing"
    );
}
