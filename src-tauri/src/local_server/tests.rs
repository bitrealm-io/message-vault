//! Tests for the app's own Message Crate. The server here is a shell script
//! or an `httpmock` server, so nothing depends on the real program being
//! built; the script-based ones run on Unix only.

use super::*;
use httpmock::prelude::*;
use std::net::TcpListener;

/// An address nothing listens on: bound once to pick a free port, then let go.
fn free_address() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap()
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

#[cfg(unix)]
mod with_a_script {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// Write an executable shell script standing in for the server.
    fn script(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("fake-server");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
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
}
