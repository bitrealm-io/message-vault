//! The Message Crate this app starts for itself.
//!
//! The installer carries `message-crate-server` beside the app, the way it
//! carries the Apple Messages helper. When the app is pointed at its own
//! address, [`OWN_ADDRESS`], it first asks that address what it is:
//!
//! - a Message Crate answers (Docker on this computer, or a server left
//!   running): the app uses it and starts nothing;
//! - nothing answers: the app starts the server and waits for it to answer;
//! - something else answers: the port is taken, and the app says so.
//!
//! The server is given its data folder, its address and the website files on
//! its command line, so there is no config file. A database that does not
//! exist yet is created with the Demo Account before the server listens,
//! which is why a first start takes a few seconds longer.
//!
//! The app stops the server it started when it closes, and never one it only
//! found. The process is killed, not asked: the server's database survives
//! that, and the only work a kill can interrupt is an import this app was
//! running anyway.

use serde::Serialize;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

/// Where the app's own Message Crate listens: the same port as the Docker
/// image, on this computer only.
pub const OWN_ADDRESS: &str = "127.0.0.1:8080";

/// The server program's name, without the platform's suffix.
const SERVER_NAME: &str = "message-crate-server";

/// The database file the server keeps in its data folder. It is missing
/// before the first start.
const DATABASE_FILE: &str = "messagecrate.db";

/// How long a started server may take to answer. A first start generates the
/// Demo Account, which takes seconds; minutes means something is wrong.
const START_TIMEOUT: Duration = Duration::from_secs(300);

/// How often a starting server is asked whether it is up.
const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// How long one question to the address may take.
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// How many of the server's last output lines are kept for a failure report.
const OUTPUT_LINES_KEPT: usize = 40;

/// What is at an address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    /// A Message Crate answered `GET /v1/server`.
    MessageCrate,
    /// Nothing is listening.
    Free,
    /// Something is listening and it is not a Message Crate.
    Other,
}

/// Ask `address` what it is.
pub fn probe(address: SocketAddr) -> Probe {
    if TcpStream::connect_timeout(&address, PROBE_TIMEOUT).is_err() {
        return Probe::Free;
    }
    let answer = message_crate_http::build_client().ok().and_then(|client| {
        client
            .get(format!("http://{address}/v1/server"))
            .timeout(PROBE_TIMEOUT)
            .send()
            .ok()
    });
    let Some(response) = answer else {
        return Probe::Other;
    };
    if !response.status().is_success() {
        return Probe::Other;
    }
    // Every Message Crate reports its Schema Fingerprint here; nothing else
    // that happens to hold the port does.
    let is_message_crate = response
        .text()
        .ok()
        .and_then(|body| serde_json::from_str::<serde_json::Value>(&body).ok())
        .is_some_and(|json| json.get("schema_fingerprint").is_some());
    if is_message_crate {
        Probe::MessageCrate
    } else {
        Probe::Other
    }
}

/// Why the app's own Message Crate is not running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureReason {
    /// Another program holds the port.
    PortTaken,
    /// The server could not be started, stopped while starting, or stopped
    /// after it was running.
    StartFailed,
}

/// The state of the app's own Message Crate, as the screens are told it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Status {
    /// The app has not been asked to start it.
    Idle,
    /// The server is starting. `first_time` is true when its database did not
    /// exist, so it is also being set up with the Demo Account.
    Starting {
        /// Whether this start creates the database.
        first_time: bool,
    },
    /// A Message Crate answers at the address. `started_by_app` is false when
    /// one was already there.
    Ready {
        /// Whether this app started the server that answers.
        started_by_app: bool,
    },
    /// No Message Crate answers and the app could not start one.
    Failed {
        /// Which kind of failure, for the screen to choose its wording.
        reason: FailureReason,
        /// One sentence a person can act on.
        message: String,
        /// The server's own last output, for a bug report. Empty when the
        /// server never ran.
        details: String,
    },
}

/// What the app needs to start its server.
#[derive(Debug, Clone)]
pub struct Launch {
    /// The server program.
    pub program: PathBuf,
    /// The folder holding the database and attachments.
    pub data_dir: PathBuf,
    /// The folder holding the built website.
    pub static_dir: PathBuf,
    /// Where the server listens.
    pub address: SocketAddr,
    /// Websites allowed to call the server besides the installed app, which
    /// always is. `cargo tauri dev` loads the screens from the Vite dev
    /// server, a different origin, and names it here.
    pub cors_origins: Vec<String>,
}

impl Launch {
    /// The arguments the server is started with.
    pub fn arguments(&self) -> Vec<String> {
        let mut arguments = vec![
            "serve".into(),
            "--data-dir".into(),
            self.data_dir.display().to_string(),
            "--bind".into(),
            self.address.to_string(),
            "--static-dir".into(),
            self.static_dir.display().to_string(),
        ];
        for origin in &self.cors_origins {
            arguments.push("--cors-origin".into());
            arguments.push(origin.clone());
        }
        arguments
    }

    /// Whether the server has no database yet.
    fn is_first_start(&self) -> bool {
        !self.data_dir.join(DATABASE_FILE).exists()
    }
}

/// The server program beside the running app, where the installer and
/// `cargo tauri dev` both place it.
///
/// # Errors
///
/// Returns an error naming the path when the program is not there.
pub fn locate_server() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("find the running app: {e}"))?;
    let dir = exe
        .parent()
        .ok_or_else(|| "the running app has no folder".to_string())?;
    let name = if cfg!(windows) {
        format!("{SERVER_NAME}.exe")
    } else {
        SERVER_NAME.to_string()
    };
    let path = dir.join(name);
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!("The server program is missing: {}", path.display()))
    }
}

/// The last lines a running server wrote.
type Output = Arc<Mutex<VecDeque<String>>>;

/// What [`LocalServer`] guards.
#[derive(Debug)]
struct Inner {
    /// What the screens are told.
    status: Status,
    /// The server this app started, while it runs.
    child: Option<Child>,
    /// That server's last output lines.
    output: Output,
}

/// The app's own Message Crate: its state, and the process when the app
/// started one. Cheap to clone; every clone is the same server.
#[derive(Debug, Clone)]
pub struct LocalServer {
    inner: Arc<Mutex<Inner>>,
}

impl Default for LocalServer {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                status: Status::Idle,
                child: None,
                output: Output::default(),
            })),
        }
    }
}

impl LocalServer {
    /// Lock the state. A panic while it was held leaves nothing half-written
    /// worth refusing over, so a poisoned lock is used as it is.
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The current state. A server that was running and has since stopped is
    /// reported as failed, with what it last wrote.
    pub fn status(&self) -> Status {
        let mut inner = self.lock();
        if matches!(
            inner.status,
            Status::Ready {
                started_by_app: true
            }
        ) {
            let exited = inner
                .child
                .as_mut()
                .is_none_or(|child| !matches!(child.try_wait(), Ok(None)));
            if exited {
                inner.child = None;
                inner.status = Status::Failed {
                    reason: FailureReason::StartFailed,
                    message: "Message Crate stopped.".into(),
                    details: joined(&inner.output),
                };
            }
        }
        inner.status.clone()
    }

    /// Make sure a Message Crate answers at `launch.address`, starting the
    /// server when nothing does. Returns at once; [`Self::status`] reports
    /// how it went. Does nothing while a start is under way or a Message
    /// Crate is ready, so it is safe to call on every launch and is also how
    /// a failed start is tried again.
    pub fn ensure_started(&self, launch: Launch) {
        if matches!(
            self.status(),
            Status::Starting { .. } | Status::Ready { .. }
        ) {
            return;
        }
        self.lock().status = Status::Starting {
            first_time: launch.is_first_start(),
        };
        let server = self.clone();
        thread::spawn(move || {
            let status = server.start(&launch);
            server.lock().status = status;
        });
    }

    /// The work behind [`Self::ensure_started`]: ask the address, start the
    /// server if it is free, and wait for an answer.
    fn start(&self, launch: &Launch) -> Status {
        match probe(launch.address) {
            Probe::MessageCrate => {
                return Status::Ready {
                    started_by_app: false,
                };
            }
            Probe::Other => {
                return Status::Failed {
                    reason: FailureReason::PortTaken,
                    message: format!(
                        "Another program is using port {}. Close it, or enter another server address.",
                        launch.address.port()
                    ),
                    details: String::new(),
                };
            }
            Probe::Free => {}
        }

        let output = Output::default();
        let child = match spawn_server(launch, &output) {
            Ok(child) => child,
            Err(error) => {
                return Status::Failed {
                    reason: FailureReason::StartFailed,
                    message: "Message Crate could not be started.".into(),
                    details: error,
                };
            }
        };
        {
            let mut inner = self.lock();
            inner.child = Some(child);
            inner.output = Arc::clone(&output);
        }

        let deadline = Instant::now() + START_TIMEOUT;
        loop {
            let exited = self
                .lock()
                .child
                .as_mut()
                .is_none_or(|child| !matches!(child.try_wait(), Ok(None)));
            if exited {
                self.lock().child = None;
                return Status::Failed {
                    reason: FailureReason::StartFailed,
                    message: "Message Crate stopped while starting.".into(),
                    details: joined(&output),
                };
            }
            if probe(launch.address) == Probe::MessageCrate {
                return Status::Ready {
                    started_by_app: true,
                };
            }
            if Instant::now() >= deadline {
                self.stop();
                return Status::Failed {
                    reason: FailureReason::StartFailed,
                    message: "Message Crate took too long to start.".into(),
                    details: joined(&output),
                };
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    /// Stop the server this app started. One the app only found is left
    /// running.
    pub fn stop(&self) {
        let child = self.lock().child.take();
        if let Some(mut child) = child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Start the server program with its output kept for a failure report.
fn spawn_server(launch: &Launch, output: &Output) -> Result<Child, String> {
    std::fs::create_dir_all(&launch.data_dir)
        .map_err(|e| format!("create {}: {e}", launch.data_dir.display()))?;
    let mut command = Command::new(&launch.program);
    command
        .args(launch.arguments())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console(&mut command);
    let mut child = command
        .spawn()
        .map_err(|e| format!("start {}: {e}", launch.program.display()))?;
    if let Some(stdout) = child.stdout.take() {
        keep_output(stdout, Arc::clone(output));
    }
    if let Some(stderr) = child.stderr.take() {
        keep_output(stderr, Arc::clone(output));
    }
    Ok(child)
}

/// Keep a console window from opening beside the app on Windows.
#[cfg(windows)]
fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    /// `CREATE_NO_WINDOW`.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

/// Other systems open no window for a child process.
#[cfg(not(windows))]
fn hide_console(_command: &mut Command) {}

/// Read `stream` until it closes, keeping its last lines in `output`. The
/// pipe has to be drained for as long as the server runs, or the server
/// blocks once the pipe is full.
fn keep_output(stream: impl Read + Send + 'static, output: Output) {
    thread::spawn(move || {
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            let mut lines = output
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if lines.len() == OUTPUT_LINES_KEPT {
                lines.pop_front();
            }
            lines.push_back(line);
        }
    });
}

/// The kept output as one block of text.
fn joined(output: &Output) -> String {
    let lines = output
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    lines.iter().cloned().collect::<Vec<_>>().join("\n")
}

/// The folder inside the app's own data folder that the server keeps its
/// database and attachments in. It is the whole Message Crate: copying it is
/// the backup.
pub fn data_dir_in(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("data")
}

#[cfg(test)]
mod tests;
