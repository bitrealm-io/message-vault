//! The client side of the helper protocol, against small fake helpers.
//!
//! Each fake is a shell script that reads the request line and prints the
//! events a broken or mismatched `imessage-reader` would. They run on Unix
//! only; the real program is covered on every platform by
//! `tests/helper_process.rs`.

mod locating {
    use std::{fs, path::Path};

    use crate::helper::{HELPER_PATH_ENV, Places, executable_name, locate_in};

    fn nowhere<'a>() -> Places<'a> {
        Places {
            explicit: None,
            exe_dir: None,
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

    /// The program is looked for beside the app and nowhere else: a copy in
    /// the folder above is not used.
    #[test]
    fn a_program_in_the_folder_above_the_app_is_not_found() {
        let root = tempfile::tempdir().unwrap();
        put_program(root.path());
        let app = root.path().join("app");
        fs::create_dir_all(&app).unwrap();

        let err = locate_in(&Places {
            exe_dir: Some(&app),
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
        assert!(
            err.ends_with(&format!("Tried: {}", app.join(&executable).display())),
            "{err}"
        );
    }
}

#[cfg(unix)]
mod faults {
    use imessage_reader_protocol::{
        Event, IdentitiesRequest, PROTOCOL_VERSION, Platform, Request, Source,
    };

    use crate::testutil::{fake_helper, source_line, spawn_fake};

    /// A request that expects a `source` event first.
    fn identities_request() -> Request {
        Request::Identities(IdentitiesRequest {
            source: Source {
                db_path: "/nowhere/chat.db".into(),
                platform: Platform::MacOs,
                backup_password: None,
            },
            scratch_dir: "/nowhere/scratch".into(),
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
            r#"echo '{"event":"identities","values":["P:+15555550110"]}'"#,
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
