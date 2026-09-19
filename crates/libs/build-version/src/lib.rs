//! The Build string a build script embeds: the Product Version plus the
//! commit it was built from.
//!
//! A Build reads `0.9.0+343fe0d8`. Source with uncommitted changes adds
//! `.dirty`, a build made from a `v*` tag is the Product Version alone, and a
//! build that can learn nothing about its source reads `0.9.0+unknown`. The
//! vault server's and the desktop app's build scripts both call [`emit`], so
//! the two cannot format a Build differently. `web/vite.config.ts` follows the
//! same rules for the SPA and must be changed together with this file.
//!
//! The part after the `+` comes from `MESSAGE_VAULT_BUILD_METADATA` when that
//! variable is set, and from git otherwise. The release Dockerfile sets it,
//! because `.git` is not in the image's build context. Set and empty means a
//! release: the Build is the Product Version alone.

use std::path::Path;
use std::process::Command;

/// Overrides what git would say; see the module docs.
pub const METADATA_ENV: &str = "MESSAGE_VAULT_BUILD_METADATA";

/// The compile-time variable [`emit`] sets; read it with
/// `env!("MESSAGE_VAULT_BUILD")`.
pub const BUILD_ENV: &str = "MESSAGE_VAULT_BUILD";

/// Join a Product Version and build metadata. Empty metadata is a release.
pub fn format_build(product_version: &str, metadata: &str) -> String {
    if metadata.is_empty() {
        product_version.to_string()
    } else {
        format!("{product_version}+{metadata}")
    }
}

/// Run git in `dir` and return its trimmed stdout, or `None` when git is
/// missing, fails, or `dir` is not in a repository.
fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The metadata git reports for the checkout holding `dir`.
fn metadata_from_git(dir: &Path) -> String {
    let Some(commit) = git(dir, &["rev-parse", "--short=8", "HEAD"]) else {
        return "unknown".to_string();
    };
    // Tracked files only: a build leaves untracked output behind.
    let dirty = git(dir, &["status", "--porcelain", "--untracked-files=no"])
        .is_some_and(|changes| !changes.is_empty());
    if dirty {
        return format!("{commit}.dirty");
    }
    let tagged = git(
        dir,
        &[
            "describe",
            "--exact-match",
            "--tags",
            "--match",
            "v*",
            "HEAD",
        ],
    )
    .is_some();
    if tagged { String::new() } else { commit }
}

/// Work out the Build for the crate at `manifest_dir`.
pub fn build_string(product_version: &str, manifest_dir: &Path) -> String {
    let metadata = std::env::var(METADATA_ENV).unwrap_or_else(|_| metadata_from_git(manifest_dir));
    format_build(product_version, metadata.trim())
}

/// Call from a build script: sets `MESSAGE_VAULT_BUILD` for the crate being
/// built, and asks cargo to run the script again when the commit changes.
///
/// # Panics
///
/// Panics when cargo has not set `CARGO_PKG_VERSION` or `CARGO_MANIFEST_DIR`,
/// which means it was not called from a build script.
pub fn emit() {
    let version = std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let manifest_dir = Path::new(&manifest_dir);

    println!("cargo:rerun-if-env-changed={METADATA_ENV}");
    // HEAD moves on a commit or a checkout and the index on `git add`; the
    // crate's own sources are what turn a clean checkout dirty.
    for name in ["HEAD", "logs/HEAD", "index"] {
        if let Some(path) = git(manifest_dir, &["rev-parse", "--git-path", name]) {
            let path = manifest_dir.join(path);
            if path.exists() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=build.rs");

    println!(
        "cargo:rustc-env={BUILD_ENV}={}",
        build_string(&version, manifest_dir)
    );
}

#[cfg(test)]
mod tests {
    use super::format_build;

    // The same cases are asserted in `web/src/lib/build.test.ts`.
    #[test]
    fn a_build_is_the_version_plus_its_metadata() {
        assert_eq!(format_build("0.9.0", "343fe0d8"), "0.9.0+343fe0d8");
        assert_eq!(
            format_build("0.9.0", "343fe0d8.dirty"),
            "0.9.0+343fe0d8.dirty"
        );
        assert_eq!(format_build("0.9.0", "unknown"), "0.9.0+unknown");
    }

    #[test]
    fn a_release_is_the_version_alone() {
        assert_eq!(format_build("0.9.0", ""), "0.9.0");
    }
}
