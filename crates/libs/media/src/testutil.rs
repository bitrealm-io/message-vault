//! Test helpers for code that runs ffmpeg and ffprobe.
//!
//! Compiled for this crate's own tests and, through the `testutil` feature,
//! for the tests of every crate that lists `media` with that feature as a
//! dev-dependency. Every test in the workspace that needs the real tools
//! gates on [`real_ffmpeg_test_guard`], so the rule for a missing ffmpeg
//! lives in one place.
//!
//! The tool location is process-wide ([`crate::set_tools_dir`]), so a test
//! that points it somewhere else and a test that runs the real ffmpeg must
//! not overlap. One lock serializes them: tests that only read the location
//! share it, and a test that changes the location holds it alone.

use std::io::Write;
use std::path::PathBuf;
use std::sync::{OnceLock, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::tools::{ffmpeg_available, set_tools_dir, tools_dir};

/// The lock over the process-wide tool location.
fn tools_lock() -> &'static RwLock<()> {
    static LOCK: OnceLock<RwLock<()>> = OnceLock::new();
    LOCK.get_or_init(|| RwLock::new(()))
}

/// Hold the tool location still, for a test that changes it.
///
/// A test that calls [`crate::set_tools_dir`] must hold this for as long as
/// the location differs from the one it found, and put that one back before
/// releasing it. Otherwise a test that runs the real ffmpeg can resolve the
/// tools in the instant another test has them pointed at an empty or mock
/// directory.
pub fn tools_test_lock() -> RwLockWriteGuard<'static, ()> {
    tools_lock().write().unwrap_or_else(PoisonError::into_inner)
}

/// Whether CI is running this process. GitHub Actions sets `CI=true`.
fn running_in_ci() -> bool {
    std::env::var("CI").is_ok_and(|value| !value.is_empty() && value != "false" && value != "0")
}

/// Hold the tool location still for a test that needs the real ffmpeg, and
/// say whether ffmpeg and ffprobe are there. `None` means they are not, and
/// the test should return.
///
/// Under CI a missing ffmpeg panics instead. CI installs ffmpeg for every job
/// that runs these tests, so a missing one there is a broken job, and a test
/// that returned early would report a pass it never earned. On a developer
/// machine the test skips, and says so on stderr.
///
/// Taking the lock and asking whether ffmpeg is available are one call
/// because doing either without the other lets a test be answered by an
/// empty or mock directory another test has the location pointed at for
/// that instant (#308).
///
/// # Panics
///
/// When `CI` is set and ffmpeg or ffprobe cannot be found.
#[must_use]
pub fn real_ffmpeg_test_guard() -> Option<RwLockReadGuard<'static, ()>> {
    let guard = tools_lock().read().unwrap_or_else(PoisonError::into_inner);
    if ffmpeg_available() {
        return Some(guard);
    }
    drop(guard);
    let current = std::thread::current();
    let test = current.name().unwrap_or("an unnamed test");
    assert!(
        !running_in_ci(),
        "{test} needs ffmpeg and ffprobe, and CI is set but they were not found. \
         Install ffmpeg in this CI job, or put both tools in MESSAGE_VAULT_IO_BIN."
    );
    // Written to the stderr handle rather than through `eprintln!`, which the
    // test harness captures and throws away for a test that passes.
    let _ = writeln!(
        std::io::stderr(),
        "skipped {test}: ffmpeg or ffprobe was not found"
    );
    None
}

/// The tool location made empty for as long as this lives.
///
/// Holds [`tools_test_lock`], points the location at a directory that does
/// not exist, and puts the previous location back when dropped. The override
/// replaces every other place the tools are looked for, `PATH` included, so
/// the tools are missing whether or not the machine has ffmpeg installed.
pub struct ToolsHidden {
    previous: Option<PathBuf>,
    _lock: RwLockWriteGuard<'static, ()>,
}

/// Make ffmpeg and ffprobe unavailable to this process until the returned
/// guard is dropped.
#[must_use]
pub fn hide_ffmpeg() -> ToolsHidden {
    let lock = tools_test_lock();
    let previous = tools_dir();
    let nowhere = std::env::temp_dir()
        .join(format!("message-vault-no-tools-{}", std::process::id()))
        .join("does-not-exist");
    set_tools_dir(Some(nowhere));
    ToolsHidden {
        previous,
        _lock: lock,
    }
}

impl Drop for ToolsHidden {
    fn drop(&mut self) {
        set_tools_dir(self.previous.take());
    }
}
