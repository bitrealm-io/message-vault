//! The client side of the helper protocol, against small fake helpers.
//!
//! Each fake is a shell script that reads the request line and prints the
//! events a broken or mismatched `imessage-reader` would. They run on Unix
//! only; the real program is covered on every platform by
//! `tests/helper_process.rs`.

#[cfg(unix)]
pub(crate) use fake::{fake_helper, source_line, spawn_fake};

#[cfg(unix)]
mod fake {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
    };

    use imessage_reader_protocol::Request;

    use crate::helper::Helper;

    /// Write `body` as an executable `/bin/sh` script in `dir`. The script
    /// reads the request line first, as the real program does.
    pub(crate) fn fake_helper(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("imessage-reader");
        fs::write(&path, format!("#!/bin/sh\nread -r request\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// Start the fake at `path` with `request`. Retries while another test
    /// thread's fork still holds the script open for writing (`ETXTBSY`).
    pub(crate) fn spawn_fake(path: &Path, request: &Request) -> Helper {
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
    pub(crate) fn source_line(version: u32) -> String {
        format!(r#"echo '{{"event":"source","protocol_version":{version},"encrypted":false}}'"#)
    }
}

mod locating {
    use std::{ffi::OsString, fs, path::Path};

    use crate::helper::{HELPER_PATH_ENV, Places, executable_name, locate_in};

    fn nowhere<'a>() -> Places<'a> {
        Places {
            explicit: None,
            exe_dir: None,
            io_bin: None,
            path: None,
        }
    }

    fn put_program(dir: &Path) -> std::path::PathBuf {
        fs::create_dir_all(dir).unwrap();
        let path = dir.join(executable_name());
        fs::write(&path, b"").unwrap();
        path
    }

    #[test]
    fn the_explicit_path_wins_over_everything_else() {
        let root = tempfile::tempdir().unwrap();
        let explicit = put_program(&root.path().join("custom"));
        let app = root.path().join("app");
        put_program(&app);

        let found = locate_in(&Places {
            explicit: Some(explicit.clone()),
            exe_dir: Some(&app),
            ..nowhere()
        })
        .unwrap();
        assert_eq!(found, explicit);
    }

    #[test]
    fn an_explicit_path_that_is_not_a_file_is_an_error_not_a_fallback() {
        let root = tempfile::tempdir().unwrap();
        let app = root.path().join("app");
        put_program(&app);
        let missing = root.path().join("gone").join("imessage-reader");

        let err = locate_in(&Places {
            explicit: Some(missing.clone()),
            exe_dir: Some(&app),
            ..nowhere()
        })
        .unwrap_err()
        .to_string();
        assert_eq!(
            err,
            format!(
                "{HELPER_PATH_ENV} is set but not a file: {}",
                missing.display()
            )
        );
    }

    #[test]
    fn the_program_beside_the_app_is_found() {
        let root = tempfile::tempdir().unwrap();
        let app = root.path().join("app");
        let program = put_program(&app);

        let found = locate_in(&Places {
            exe_dir: Some(&app),
            ..nowhere()
        })
        .unwrap();
        assert_eq!(found, program);
    }

    #[test]
    fn a_test_binary_in_deps_finds_the_program_one_folder_up() {
        let root = tempfile::tempdir().unwrap();
        let program = put_program(&root.path().join("debug"));
        let deps = root.path().join("debug").join("deps");
        fs::create_dir_all(&deps).unwrap();

        let found = locate_in(&Places {
            exe_dir: Some(&deps),
            ..nowhere()
        })
        .unwrap();
        assert_eq!(found, program);
    }

    #[test]
    fn the_io_bin_folder_then_path_are_searched_in_that_order() {
        let root = tempfile::tempdir().unwrap();
        let io_bin = root.path().join("io-bin");
        let in_io_bin = put_program(&io_bin);
        let on_path = root.path().join("on-path");
        let in_path = put_program(&on_path);
        let path = std::env::join_paths([&on_path]).unwrap();

        let found = locate_in(&Places {
            io_bin: Some(io_bin),
            path: Some(path.clone()),
            ..nowhere()
        })
        .unwrap();
        assert_eq!(found, in_io_bin);

        let found = locate_in(&Places {
            io_bin: Some(root.path().join("empty")),
            path: Some(path),
            ..nowhere()
        })
        .unwrap();
        assert_eq!(found, in_path);
    }

    #[test]
    fn a_missing_program_names_the_folders_the_app_looked_in() {
        let root = tempfile::tempdir().unwrap();
        let app = root.path().join("app");
        fs::create_dir_all(&app).unwrap();
        let io_bin = root.path().join("io-bin");
        let path: OsString = std::env::join_paths([root.path().join("on-path")]).unwrap();

        let err = locate_in(&Places {
            exe_dir: Some(&app),
            io_bin: Some(io_bin.clone()),
            path: Some(path),
            ..nowhere()
        })
        .unwrap_err()
        .to_string();
        let executable = executable_name();
        assert!(
            err.starts_with(&format!(
                "Could not find {executable}, the program that reads Apple Messages."
            )),
            "{err}"
        );
        let tried = format!(
            "Tried: {}, {}, {}",
            app.join(&executable).display(),
            root.path().join(&executable).display(),
            io_bin.join(&executable).display()
        );
        assert!(err.ends_with(&tried), "{err}");
    }
}

#[cfg(unix)]
mod faults {
    use imessage_reader_protocol::{Event, PROTOCOL_VERSION, Platform, Request, Source};

    use super::fake::{fake_helper, source_line, spawn_fake};

    /// A request that expects a `source` event first.
    fn identities_request() -> Request {
        Request::Identities(Source {
            db_path: "/nowhere/chat.db".into(),
            platform: Platform::MacOs,
            backup_password: None,
        })
    }

    #[test]
    fn a_helper_on_another_protocol_version_is_refused_by_both_numbers() {
        let dir = tempfile::tempdir().unwrap();
        let path = fake_helper(dir.path(), &source_line(PROTOCOL_VERSION + 41));
        let mut helper = spawn_fake(&path, &identities_request());

        let err = helper.next_event().unwrap_err().to_string();
        assert_eq!(
            err,
            format!(
                "imessage-reader speaks protocol version {}, this app speaks {PROTOCOL_VERSION}; \
                 the two were not built together",
                PROTOCOL_VERSION + 41
            )
        );
    }

    #[test]
    fn a_helper_that_answers_without_a_source_event_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = fake_helper(
            dir.path(),
            r#"echo '{"event":"identities","values":["P:+15550001111"]}'"#,
        );
        let mut helper = spawn_fake(&path, &identities_request());

        let err = helper.next_event().unwrap_err().to_string();
        assert!(err.contains("protocol version"), "{err}");
        assert!(err.contains("not built together"), "{err}");
    }

    #[test]
    fn log_and_progress_lines_may_come_before_the_source_event() {
        let dir = tempfile::tempdir().unwrap();
        let body = format!(
            "echo '{{\"event\":\"log\",\"line\":\"opening\"}}'\n\
             echo '{{\"event\":\"progress\",\"stage\":\"setup\",\"label\":\"keys\",\"step\":1,\"total\":2}}'\n\
             {}",
            source_line(PROTOCOL_VERSION)
        );
        let path = fake_helper(dir.path(), &body);
        let mut helper = spawn_fake(&path, &identities_request());

        assert!(matches!(
            helper.next_event().unwrap(),
            Event::Source {
                encrypted: false,
                ..
            }
        ));
        helper.finish().unwrap();
    }

    #[test]
    fn a_line_that_is_not_json_is_named_in_the_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = fake_helper(dir.path(), "echo 'Segmentation fault (core dumped)'");
        let mut helper = spawn_fake(&path, &identities_request());

        let err = helper.next_event().unwrap_err().to_string();
        assert_eq!(
            err,
            "imessage-reader sent something unexpected: Segmentation fault (core dumped)"
        );
    }

    #[test]
    fn a_helper_that_dies_mid_stream_reports_its_status_and_stderr_tail() {
        let dir = tempfile::tempdir().unwrap();
        let body = format!(
            "{}\necho 'reading chat.db' >&2\necho 'panicked at out of memory' >&2\nexit 3",
            source_line(PROTOCOL_VERSION)
        );
        let path = fake_helper(dir.path(), &body);
        let mut helper = spawn_fake(&path, &identities_request());

        assert!(matches!(helper.next_event().unwrap(), Event::Source { .. }));
        let err = helper.next_event().unwrap_err().to_string();
        assert_eq!(
            err,
            "imessage-reader stopped before finishing (exit status: 3): \
             reading chat.db | panicked at out of memory"
        );
    }

    #[test]
    fn a_helper_that_fails_after_answering_reports_its_status_on_finish() {
        let dir = tempfile::tempdir().unwrap();
        let body = format!(
            "{}\necho '{{\"event\":\"identities\",\"values\":[]}}'\necho 'lost the lock' >&2\nexit 3",
            source_line(PROTOCOL_VERSION)
        );
        let path = fake_helper(dir.path(), &body);
        let mut helper = spawn_fake(&path, &identities_request());

        assert!(matches!(helper.next_event().unwrap(), Event::Source { .. }));
        assert!(matches!(
            helper.next_event().unwrap(),
            Event::Identities { .. }
        ));
        let err = helper.finish().unwrap_err().to_string();
        assert_eq!(
            err,
            "imessage-reader exited with exit status: 3: lost the lock"
        );
    }

    #[test]
    fn an_error_event_becomes_the_error_as_the_helper_worded_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = fake_helper(
            dir.path(),
            r#"echo '{"event":"error","message":"The backup password is wrong."}'; exit 1"#,
        );
        let mut helper = spawn_fake(&path, &identities_request());

        let err = helper.next_event().unwrap_err().to_string();
        assert_eq!(err, "The backup password is wrong.");
    }
}
