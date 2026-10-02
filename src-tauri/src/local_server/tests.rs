//! Tests for the app's own Message Crate. The server here is a shell script
//! or an `httpmock` server, so nothing depends on the real program being
//! built; the script-based ones run on Unix only.

use super::*;
use httpmock::prelude::*;
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};

/// An address nothing listens on: bound once to pick a free port, then let go.
fn free_address() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    wait_until_free(address);
    address
}

/// Wait for a listener this test let go of to be gone. A process another
/// test is starting holds a copy of every open socket until it runs its
/// program, and answers connections on it until then.
fn wait_until_free(address: SocketAddr) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while probe(address) != Probe::Free {
        assert!(Instant::now() < deadline, "{address} is still listened on");
        thread::sleep(Duration::from_millis(20));
    }
}

/// A mock that answers `GET /v1/server` the way a Message Crate does.
fn message_crate() -> MockServer {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/v1/server");
        then.status(200).json_body(serde_json::json!({
            "state": "unclaimed",
            "demo_account": true,
            "version": "0.10.0",
            "schema_fingerprint": 1,
        }));
    });
    server
}

/// Wait for a start to settle, failing the test if it never does.
fn settled(server: &LocalServer) -> Status {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let status = server.status();
        if !matches!(status, Status::Starting { .. }) {
            return status;
        }
        assert!(Instant::now() < deadline, "the start never settled");
        thread::sleep(Duration::from_millis(20));
    }
}

fn launch_at(address: SocketAddr, program: &Path, data_dir: &Path) -> Launch {
    Launch {
        program: program.to_path_buf(),
        data_dir: data_dir.to_path_buf(),
        static_dir: data_dir.join("website"),
        address,
        open_to_network: false,
        cors_origins: Vec::new(),
    }
}

#[test]
fn probe_reports_a_free_port() {
    assert_eq!(probe(free_address()), Probe::Free);
}

#[test]
fn probe_recognises_a_message_crate() {
    let server = message_crate();
    assert_eq!(probe(*server.address()), Probe::MessageCrate);
}

#[test]
fn probe_reports_another_program_on_the_port() {
    // Answers every path, `/v1/server` included, with something that is not
    // a Message Crate's answer.
    let server = MockServer::start();
    server.mock(|when, then| {
        when.any_request();
        then.status(200).body("<html>router admin</html>");
    });
    assert_eq!(probe(*server.address()), Probe::Other);

    let not_found = MockServer::start();
    assert_eq!(probe(*not_found.address()), Probe::Other);
}

#[test]
fn the_server_is_started_on_this_computer_with_no_config_file() {
    let launch = Launch {
        program: PathBuf::from("message-crate-server"),
        data_dir: PathBuf::from("/data"),
        static_dir: PathBuf::from("/site"),
        address: OWN_ADDRESS.parse().unwrap(),
        open_to_network: false,
        cors_origins: Vec::new(),
    };
    assert_eq!(
        Launch {
            cors_origins: vec!["http://localhost:5173".into()],
            ..launch.clone()
        }
        .arguments()[7..],
        ["--cors-origin", "http://localhost:5173"]
    );
    assert_eq!(
        launch.arguments(),
        [
            "serve",
            "--data-dir",
            "/data",
            "--bind",
            "127.0.0.1:8080",
            "--static-dir",
            "/site"
        ]
    );
}

#[test]
fn a_dev_build_keeps_its_data_apart_from_the_installed_app() {
    let app_data = Path::new("/home/someone/.local/share/app.messagecrate.desktop");
    assert_eq!(data_dir_in(app_data, false), app_data.join("data"));
    assert_eq!(data_dir_in(app_data, true), app_data.join("data-dev"));
}

#[test]
fn open_to_the_network_listens_on_every_address_at_the_same_port() {
    let launch = Launch {
        open_to_network: true,
        ..launch_at(
            OWN_ADDRESS.parse().unwrap(),
            Path::new("unused"),
            Path::new("/data"),
        )
    };
    assert_eq!(launch.arguments()[3..5], ["--bind", "0.0.0.0:8080"]);
    // The app still asks this computer's own address what is running.
    assert_eq!(launch.address.to_string(), OWN_ADDRESS);
}

/// A state with `phase`, the setting `wanted_open`, and no job running.
fn at(phase: Phase, wanted_open: bool) -> State {
    State {
        phase,
        wanted_open,
        jobs: 0,
    }
}

#[test]
fn turning_the_network_off_during_a_start_restarts_the_server_closed() {
    let (state, actions) = step(
        at(Phase::Starting { open: true }, true),
        Event::Requested { open: false },
    );
    assert_eq!(actions, [Action::Kill, Action::Spawn { open: false }]);
    assert_eq!(state.phase, Phase::Starting { open: false });

    // The setting alone does the same.
    let (_, actions) = step(
        at(Phase::Starting { open: true }, true),
        Event::Configured { open: false },
    );
    assert_eq!(actions, [Action::Kill, Action::Spawn { open: false }]);
}

#[test]
fn the_network_setting_starts_nothing_and_leaves_a_found_message_crate_alone() {
    // On another address the app never started its own server.
    for phase in [
        Phase::Idle,
        Phase::Found { checking: false },
        Phase::PortTaken,
    ] {
        let (state, actions) = step(at(phase.clone(), false), Event::Configured { open: true });
        assert_eq!(actions, [], "{phase:?}");
        assert_eq!(state.phase, phase);
        assert!(state.wanted_open);
    }
}

#[test]
fn ticking_the_network_setting_on_another_address_starts_no_server() {
    let server = LocalServer::default();
    assert_eq!(server.set_open_to_network(true), Status::Idle);
    thread::sleep(POLL_INTERVAL * 2);
    assert_eq!(server.status(), Status::Idle);
    assert!(server.lock().child.is_none());
}

#[test]
fn a_ready_server_is_restarted_for_the_network_setting_only_once_no_job_runs() {
    let (state, _) = step(at(Phase::Own { open: false }, false), Event::JobStarted);
    let (state, actions) = step(state, Event::Configured { open: true });
    assert_eq!(actions, [], "the import would be cut off");
    assert_eq!(state.phase, Phase::Own { open: false });

    let (state, actions) = step(state, Event::JobEnded);
    assert_eq!(actions, [Action::Kill, Action::Spawn { open: true }]);
    assert_eq!(state.phase, Phase::Starting { open: true });

    // With no job, at once.
    let (_, actions) = step(
        at(Phase::Own { open: false }, false),
        Event::Configured { open: true },
    );
    assert_eq!(actions, [Action::Kill, Action::Spawn { open: true }]);
}

#[test]
fn a_found_message_crate_is_asked_again_and_replaced_when_it_stops() {
    let found = at(Phase::Found { checking: false }, false);
    let (state, actions) = step(found.clone(), Event::Check);
    assert_eq!(actions, [Action::Probe]);
    // A second look while the first is under way asks nothing more.
    let (state, actions) = step(state, Event::Check);
    assert_eq!(actions, []);
    let (state, actions) = step(state, Event::Probed(Probe::Free));
    assert_eq!(actions, [Action::Spawn { open: false }]);
    assert_eq!(state.phase, Phase::Starting { open: false });

    // Retry asks again too.
    let (state, actions) = step(found, Event::Requested { open: false });
    assert_eq!(actions, [Action::Probe]);
    assert_eq!(state.phase, Phase::Probing);
}

#[test]
fn only_the_apps_own_server_saying_it_listens_makes_the_start_its_own() {
    let starting = at(Phase::Starting { open: false }, false);
    // Another Message Crate answering the port is not the app's server.
    let (state, actions) = step(starting.clone(), Event::Probed(Probe::MessageCrate));
    assert_eq!(state.phase, Phase::Starting { open: false });
    assert_eq!(actions, []);

    let (state, _) = step(starting.clone(), Event::Listening);
    assert_eq!(state.phase, Phase::Own { open: false });

    // A server that exits leaves the port to whichever Message Crate answers.
    let (state, actions) = step(
        starting.clone(),
        Event::ChildExited {
            output: "lost".into(),
        },
    );
    assert_eq!(actions, [Action::Probe]);
    let (state, _) = step(state, Event::Probed(Probe::MessageCrate));
    assert_eq!(state.phase, Phase::Found { checking: false });

    // Locked out by the server of a second window, still being set up: the
    // address is asked again until it answers.
    let locked = format!("Error: cannot start serve for /data {LOCKED_OUT}");
    let (state, _) = step(starting.clone(), Event::ChildExited { output: locked });
    let (state, actions) = step(state, Event::Probed(Probe::Free));
    assert_eq!(actions, [Action::ProbeLater]);
    let (state, _) = step(state, Event::Probed(Probe::MessageCrate));
    assert_eq!(state.phase, Phase::Found { checking: false });

    // Any other exit with nothing answering is a failure.
    let (state, _) = step(
        starting,
        Event::ChildExited {
            output: "database is locked".into(),
        },
    );
    let (state, actions) = step(state, Event::Probed(Probe::Free));
    assert_eq!(actions, []);
    assert!(
        matches!(&state.phase, Phase::Failed { details, .. } if details == "database is locked"),
        "{state:?}"
    );
}

#[test]
fn a_message_crate_already_answering_is_used_and_nothing_is_started() {
    let existing = message_crate();
    let dir = tempfile::tempdir().unwrap();
    // A program that does not exist: starting it would fail the test.
    let launch = launch_at(*existing.address(), &dir.path().join("absent"), dir.path());

    let server = LocalServer::default();
    server.ensure_started(launch);

    assert_eq!(
        settled(&server),
        Status::Ready {
            started_by_app: false
        }
    );
    assert!(server.lock().child.is_none());
}

#[test]
fn another_program_on_the_port_is_reported_by_port_number() {
    let other = MockServer::start();
    let dir = tempfile::tempdir().unwrap();
    let launch = launch_at(*other.address(), &dir.path().join("absent"), dir.path());

    let server = LocalServer::default();
    server.ensure_started(launch);

    let Status::Failed {
        reason, message, ..
    } = settled(&server)
    else {
        panic!("a taken port must fail the start");
    };
    assert_eq!(reason, FailureReason::PortTaken);
    assert!(
        message.contains(&other.address().port().to_string()),
        "{message}"
    );
}

#[test]
fn a_missing_server_program_fails_the_start_and_names_it() {
    let dir = tempfile::tempdir().unwrap();
    let program = dir.path().join("absent");
    let launch = launch_at(free_address(), &program, dir.path());

    let server = LocalServer::default();
    server.ensure_started(launch);

    let Status::Failed {
        reason, details, ..
    } = settled(&server)
    else {
        panic!("a missing program must fail the start");
    };
    assert_eq!(reason, FailureReason::StartFailed);
    assert!(details.contains("absent"), "{details}");
}

#[test]
fn the_first_start_is_the_one_with_no_database() {
    let dir = tempfile::tempdir().unwrap();
    let launch = launch_at(free_address(), Path::new("unused"), dir.path());
    assert!(launch.is_first_start());
    std::fs::write(dir.path().join(DATABASE_FILE), b"").unwrap();
    assert!(!launch.is_first_start());
}

#[test]
fn a_found_message_crate_that_stops_is_not_reported_ready() {
    // A Message Crate that can be stopped: httpmock keeps its servers
    // listening after they are dropped.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let running = Arc::new(AtomicBool::new(true));
    let flag = Arc::clone(&running);
    let existing = thread::spawn(move || {
        use std::io::Write;
        while flag.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    let mut buf = [0u8; 4096];
                    let _ = std::io::Read::read(&mut stream, &mut buf);
                    let body = r#"{"schema_fingerprint":1}"#;
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                }
                Err(_) => thread::sleep(Duration::from_millis(10)),
            }
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let launch = launch_at(address, &dir.path().join("absent"), dir.path());

    let server = LocalServer::default();
    server.ensure_started(launch.clone());
    assert_eq!(
        settled(&server),
        Status::Ready {
            started_by_app: false
        }
    );

    // Docker is stopped, or the other app instance closes.
    running.store(false, Ordering::Relaxed);
    existing.join().unwrap();
    wait_until_free(address);
    // Retry on the login card.
    server.ensure_started(launch);

    assert_ne!(
        settled(&server),
        Status::Ready {
            started_by_app: false
        },
        "nothing answers at the address any more"
    );
}

/// A Message Crate that is busy is still a Message Crate (ADR 0018). Neither
/// a slow answer nor a 503 may make the app say another program holds the
/// port.
#[test]
fn a_slow_or_busy_message_crate_is_not_another_program() {
    let slow = MockServer::start();
    slow.mock(|when, then| {
        when.method(GET).path("/v1/server");
        then.status(200)
            .delay(Duration::from_secs(3))
            .json_body(serde_json::json!({ "schema_fingerprint": 1 }));
    });
    let busy = MockServer::start();
    busy.mock(|when, then| {
        when.method(GET).path("/v1/server");
        then.status(503).json_body(serde_json::json!({
            "type": "about:blank", "status": 503, "request_id": "x"
        }));
    });
    let slow_probe = probe(*slow.address());
    let busy_probe = probe(*busy.address());
    assert_eq!(
        (slow_probe, busy_probe),
        (Probe::MessageCrate, Probe::MessageCrate),
        "slow: {slow_probe:?}, 503: {busy_probe:?}"
    );
}

#[cfg(unix)]
mod with_a_script {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// Write an executable shell script standing in for the server.
    ///
    /// A process another test is starting holds a copy of the file as it was
    /// open for writing, and until it runs its own program, running the
    /// script fails with "Text file busy". So the script is run once with
    /// `ready`, which only exits, until that works.
    fn script(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("fake-server");
        std::fs::write(
            &path,
            format!("#!/bin/sh\n[ \"$1\" = ready ] && exit 0\n{body}\n"),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while Command::new(&path).arg("ready").status().is_err() {
            assert!(
                Instant::now() < deadline,
                "the script never became runnable"
            );
            thread::sleep(Duration::from_millis(20));
        }
        path
    }

    #[test]
    fn a_server_that_stops_while_starting_fails_with_what_it_wrote() {
        let dir = tempfile::tempdir().unwrap();
        let program = script(dir.path(), "echo 'database is locked' >&2\nexit 3");
        let launch = launch_at(free_address(), &program, dir.path());

        let server = LocalServer::default();
        server.ensure_started(launch);

        let Status::Failed {
            reason, details, ..
        } = settled(&server)
        else {
            panic!("a server that exits must fail the start");
        };
        assert_eq!(reason, FailureReason::StartFailed);
        assert!(details.contains("database is locked"), "{details}");
    }

    #[test]
    fn stop_ends_the_server_the_app_started() {
        let dir = tempfile::tempdir().unwrap();
        let program = script(dir.path(), "exec sleep 600");
        let launch = launch_at(free_address(), &program, dir.path());

        let server = LocalServer::default();
        server.ensure_started(launch);
        // The script never answers, so the start stays under way; wait for
        // the process to exist.
        let deadline = Instant::now() + Duration::from_secs(20);
        let pid = loop {
            if let Some(child) = server.lock().child.as_ref() {
                break child.id();
            }
            assert!(Instant::now() < deadline, "the server was never started");
            thread::sleep(Duration::from_millis(20));
        };

        server.stop();

        assert!(server.lock().child.is_none());
        // `stop` waited for the process, so its id is no longer a process.
        assert!(!Path::new(&format!("/proc/{pid}")).exists() || !cfg!(target_os = "linux"));
        // The start that was waiting on it now reports the failure.
        assert!(matches!(settled(&server), Status::Failed { .. }));
    }

    #[test]
    fn turning_the_network_setting_off_during_a_start_is_applied() {
        let dir = tempfile::tempdir().unwrap();
        let binds = dir.path().join("binds");
        // Records the address it was told to listen on, then never answers.
        let program = script(
            dir.path(),
            &format!("echo \"$5\" >> {}\nexec sleep 600", binds.display()),
        );
        let address = free_address();
        let server = LocalServer::default();
        server.ensure_started(Launch {
            open_to_network: true,
            ..launch_at(address, &program, dir.path())
        });
        let deadline = Instant::now() + Duration::from_secs(20);
        while server.lock().child.is_none() {
            assert!(Instant::now() < deadline, "the server was never started");
            thread::sleep(Duration::from_millis(20));
        }

        // The person unticks the setting while the start is under way.
        server.ensure_started(launch_at(address, &program, dir.path()));
        thread::sleep(Duration::from_secs(1));

        let recorded = std::fs::read_to_string(&binds).unwrap();
        let last = recorded.lines().last().unwrap().to_string();
        server.stop();
        assert_eq!(
            last,
            address.to_string(),
            "the running server listens where the setting no longer says"
        );
    }
}
