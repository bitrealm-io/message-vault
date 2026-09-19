//! Build the `imessage-reader` sidecar, then run the Tauri build helper.
//!
//! The desktop app reads Apple Messages through a separate program because
//! that program links GPL code and the app is under the Fair Core License
//! (`docs/agents/licences.md`). Tauri ships such a program as an
//! `externalBin`: it expects `binaries/imessage-reader-<target triple>` to
//! exist when this script runs, copies it beside the app binary for `cargo
//! tauri dev`, and bundles it beside the app in every installer. This script
//! produces that file by building the helper crate from the workspace, so
//! `cargo check`, `cargo tauri dev` and `cargo tauri build` all work from a
//! plain checkout with nothing to remember.
//!
//! The helper is never a dependency of this crate. It is built by a nested
//! `cargo build` into its own target folder, so `cargo tree` on this manifest
//! shows no GPL crate.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

/// The helper's package and binary name.
const HELPER: &str = "imessage-reader";

fn main() {
    // The desktop app's Build, which it sends to the vault with every request.
    build_version::emit();
    build_sidecar();
    tauri_build::build();
}

/// Build the helper for this build's target and place it where Tauri looks.
fn build_sidecar() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace = manifest_dir.parent().unwrap().to_path_buf();
    let target_triple = env::var("TARGET").unwrap();
    let profile = env::var("PROFILE").unwrap();

    // Rebuild when the helper or its protocol changes.
    for dir in [
        "crates/helpers/imessage-reader",
        "crates/helpers/imessage-reader-protocol",
    ] {
        println!("cargo:rerun-if-changed={}", workspace.join(dir).display());
    }
    println!(
        "cargo:rerun-if-changed={}",
        workspace.join("Cargo.lock").display()
    );

    // A folder of its own under this build's target dir. The cargo running
    // this script holds the lock on `<target>/<profile>`, and the workspace's
    // own target dir may be locked by another cargo, so neither can be reused;
    // a sibling folder shares neither lock. OUT_DIR is
    // `<target>/<profile>/build/<pkg>-<hash>/out`, four levels down.
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target_dir = out_dir
        .ancestors()
        .nth(4)
        .expect("OUT_DIR sits four levels under the target dir")
        .join("sidecar");
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .current_dir(&workspace)
        .args(["build", "-p", HELPER, "--target", &target_triple])
        .arg("--target-dir")
        .arg(&target_dir)
        // The outer cargo's flags describe this crate's build, not the
        // helper's; a shared target dir would also invite a deadlock.
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTFLAGS");
    if profile == "release" {
        command.arg("--release");
    }
    let status = command
        .status()
        .unwrap_or_else(|e| panic!("start cargo to build {HELPER}: {e}"));
    assert!(
        status.success(),
        "cargo build -p {HELPER} failed ({status})"
    );

    let exe_suffix = if target_triple.contains("windows") {
        ".exe"
    } else {
        ""
    };
    let built = target_dir
        .join(&target_triple)
        .join(if profile == "release" {
            "release"
        } else {
            "debug"
        })
        .join(format!("{HELPER}{exe_suffix}"));
    let binaries = manifest_dir.join("binaries");
    fs::create_dir_all(&binaries).unwrap();
    let sidecar = binaries.join(format!("{HELPER}-{target_triple}{exe_suffix}"));
    copy_if_changed(&built, &sidecar);
}

/// Copy `from` over `to` unless `to` already has the same bytes, so an
/// unchanged helper does not make Tauri re-copy on every build.
fn copy_if_changed(from: &Path, to: &Path) {
    let same = fs::read(to).is_ok_and(|existing| fs::read(from).is_ok_and(|new| existing == new));
    if same {
        return;
    }
    fs::copy(from, to)
        .unwrap_or_else(|e| panic!("copy {} to {}: {e}", from.display(), to.display()));
}
