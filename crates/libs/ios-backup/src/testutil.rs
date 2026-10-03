//! Fake `imessage-reader` programs, for tests here and in
//! `imessage-ir-exporter`.
//!
//! Each fake is a `/bin/sh` script that reads the request line and prints
//! the events a real, broken, or mismatched program would. They run on Unix
//! only; the real program is covered on every platform by the tests that
//! build it (`tests/helper_process.rs` here and in the exporter).

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use imessage_reader_protocol::Request;

use crate::Helper;

/// Write `body` as an executable `/bin/sh` script in `dir`. The script
/// reads the request line first, as the real program does.
///
/// # Panics
///
/// Panics when the script cannot be written.
pub fn fake_helper(dir: &Path, body: &str) -> PathBuf {
    let path = dir.join("imessage-reader");
    fs::write(&path, format!("#!/bin/sh\nread -r request\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// Start the fake at `path` with `request`. Retries while another test
/// thread's fork still holds the script open for writing (`ETXTBSY`).
///
/// # Panics
///
/// Panics when the fake cannot be started.
pub fn spawn_fake(path: &Path, request: &Request) -> Helper {
    for _ in 0..50 {
        match Helper::spawn_at(path, request, None, None) {
            Ok(helper) => return helper,
            Err(e)
                if e.downcast_ref::<std::io::Error>()
                    .is_some_and(|io| io.raw_os_error() == Some(26)) =>
            {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(e) => panic!("start the fake helper: {e:#}"),
        }
    }
    panic!("the fake helper stayed busy");
}

/// The shell line that prints a `source` event with `version`.
#[must_use]
pub fn source_line(version: u32) -> String {
    format!(r#"echo '{{"event":"source","protocol_version":{version},"encrypted":false}}'"#)
}
