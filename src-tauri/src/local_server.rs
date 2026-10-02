//! The Message Crate this app starts for itself.
//!
//! The installer carries `message-crate-server` beside the app, the way it
//! carries the Apple Messages helper. When the app is pointed at its own
//! address, [`OWN_ADDRESS`], it first asks that address what it is:
//!
//! - a Message Crate answers (Docker on this computer, or a server left
//!   running): the app uses it and starts nothing;
//! - nothing answers: the app starts the server and waits for it to listen;
//! - something else answers: the port is taken, and the app says so.
//!
//! The server is given its data folder, its address and the website files on
//! its command line, so there is no config file. A database that does not
//! exist yet is created with the Demo Account before the server listens,
//! which is why a first start takes a few seconds longer.
//!
//! The server listens on this computer only, unless the person switched on
//! the setting that opens it to the network; it then listens on every
//! address of this computer, on the same port. Changing the setting restarts
//! the server the app started, at once while it is starting and once no
//! desktop job runs when it is ready. A Message Crate the app only found is
//! not changed, and the setting never starts a server.
//!
//! The app stops the server it started when it closes, and never one it only
//! found. The process is killed, not asked: the server's database survives
//! that, and the only work a kill can interrupt is an import this app was
//! running anyway.
//!
//! What happens next is decided by one pure function, [`step`], from the
//! state the app has, the network setting the person wants, and what just
//! happened. [`LocalServer`] holds that state with the processes and does
//! what [`step`] asks.

use serde::Serialize;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Where the app's own Message Crate listens: the same port as the Docker
/// image, on this computer only.
pub const OWN_ADDRESS: &str = "127.0.0.1:8080";

/// The server program's name, without the platform's suffix.
const SERVER_NAME: &str = "message-crate-server";

/// The database file the server keeps in its data folder. It is missing
/// before the first start.
const DATABASE_FILE: &str = "messagecrate.db";

/// How long a started server may take to listen. A first start generates the
/// Demo Account, which takes seconds; minutes means something is wrong.
const START_TIMEOUT: Duration = Duration::from_secs(300);

/// How often a running server is checked on.
const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// How long connecting to the address may take. Nothing listening on this
/// computer is known at once.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// How long the answer to `GET /v1/server` may take. That handler waits for a
/// database connection, so a Message Crate busy with a long write answers
/// late, and it is still a Message Crate.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(15);

/// How many of the server's last output lines are kept for a failure report.
const OUTPUT_LINES_KEPT: usize = 40;

/// The start of the line the server writes once it holds its address
/// (`crates/server/server/src/server.rs`). Only the process that bound the
/// port writes it, so it tells the app's own server from another Message
/// Crate answering at the same address.
const LISTENING_LINE: &str = "message-crate-server serve listening on ";

/// What the server writes when it exits because another server holds its
/// data folder (`crates/server/server/src/operation_lock.rs`): the server of
/// a second window of this app, started at the same moment.
const LOCKED_OUT: &str = "while reset-demo or another server is active";

/// How many more times the address is asked, [`POLL_INTERVAL`] apart, after
/// the app's server exited because another server holds its data folder.
/// That server may still be creating the Demo Account, so the app waits for
/// it as long as for a start of its own: [`START_TIMEOUT`].
const LOCKED_OUT_PROBES: u32 = 1200;

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
    if TcpStream::connect_timeout(&address, CONNECT_TIMEOUT).is_err() {
        return Probe::Free;
    }
    let answer = message_crate_http::build_client().ok().and_then(|client| {
        client
            .get(format!("http://{address}/v1/server"))
            .timeout(ANSWER_TIMEOUT)
            .send()
            .ok()
    });
    let Some(response) = answer else {
        return Probe::Other;
    };
    let status = response.status().as_u16();
    let body = response
        .text()
        .ok()
        .and_then(|body| serde_json::from_str::<serde_json::Value>(&body).ok());
    if body.is_some_and(|body| is_message_crate_answer(status, &body)) {
        Probe::MessageCrate
    } else {
        Probe::Other
    }
}

/// Whether `body`, sent with `status`, is a Message Crate's answer to
/// `GET /v1/server`. Every Message Crate reports its Schema Fingerprint there.
/// One that cannot answer, because it is busy, sends a problem document
/// instead, with the request id every Message Crate puts in its failures.
/// Nothing else that happens to hold the port sends either.
fn is_message_crate_answer(status: u16, body: &serde_json::Value) -> bool {
    match status {
        200..=299 => body.get("schema_fingerprint").is_some(),
        500..=599 => {
            body.get("type").is_some_and(serde_json::Value::is_string)
                && body.get("status").and_then(serde_json::Value::as_u64) == Some(status.into())
                && body
                    .get("request_id")
                    .is_some_and(serde_json::Value::is_string)
        }
        _ => false,
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

/// What the app has at its own address.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    /// Nothing has been asked for.
    Idle,
    /// The address is being asked what it is, before anything is started.
    Probing,
    /// The app's server runs and has not yet said it listens.
    Starting {
        /// Whether it was started open to the network.
        open: bool,
    },
    /// The app's server listens.
    Own {
        /// Whether it was started open to the network.
        open: bool,
    },
    /// A Message Crate the app did not start answers.
    Found {
        /// Whether the address is being asked again whether it still does.
        checking: bool,
    },
    /// The app's server exited while starting, and the address is being
    /// asked whether another Message Crate answers there instead.
    Lost {
        /// What the server last wrote.
        details: String,
        /// How many more times to ask while nothing answers.
        probes_left: u32,
    },
    /// Another program holds the port.
    PortTaken,
    /// No Message Crate answers and the app could not start one.
    Failed {
        /// Which kind of failure.
        reason: FailureReason,
        /// One sentence a person can act on.
        message: String,
        /// The server's own last output.
        details: String,
    },
}

/// What the app has, beside what the person wants.
#[derive(Debug, Clone, PartialEq, Eq)]
struct State {
    /// What is at the address.
    phase: Phase,
    /// Whether the person wants the app's server open to the network.
    wanted_open: bool,
    /// How many desktop jobs run. A ready server is not restarted while any
    /// does, since the job may be an import into it.
    jobs: usize,
}

impl Default for State {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            wanted_open: false,
            jobs: 0,
        }
    }
}

/// Something that happened, for [`step`] to act on.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Event {
    /// The screens want a Message Crate at the address, with the network
    /// setting `open`. Starts one when none answers.
    Requested {
        /// The network setting.
        open: bool,
    },
    /// The person changed the network setting. Starts nothing.
    Configured {
        /// The network setting.
        open: bool,
    },
    /// The address answered a [`Action::Probe`].
    Probed(Probe),
    /// The app's server wrote [`LISTENING_LINE`].
    Listening,
    /// The app's server exited, having written `output`.
    ChildExited {
        /// Its last output.
        output: String,
    },
    /// The app's server has not listened within [`START_TIMEOUT`].
    Deadline {
        /// Its last output.
        output: String,
    },
    /// The server program could not be started.
    SpawnFailed {
        /// Why.
        error: String,
    },
    /// The screens asked for the state: a found Message Crate is asked again
    /// whether it still answers.
    Check,
    /// A desktop job started.
    JobStarted,
    /// A desktop job ended.
    JobEnded,
    /// The app is closing.
    Stopped,
}

/// What [`step`] asks [`LocalServer`] to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// Ask the address what it is, and report it as [`Event::Probed`].
    Probe,
    /// The same, after [`POLL_INTERVAL`].
    ProbeLater,
    /// Start the server program, listening on the network when `open`.
    Spawn {
        /// Whether it listens on every address of this computer.
        open: bool,
    },
    /// Kill the server program the app started.
    Kill,
}

/// Decide what follows `event`. The only place the state changes; it does no
/// I/O, so every rule is tested without a process.
fn step(mut state: State, event: Event) -> (State, Vec<Action>) {
    let mut actions = Vec::new();
    match event {
        Event::Requested { open } => {
            state.wanted_open = open;
            match state.phase {
                Phase::Idle | Phase::PortTaken | Phase::Failed { .. } => {
                    state.phase = Phase::Probing;
                    actions.push(Action::Probe);
                }
                // Asked again whether it still answers: Retry on the login
                // card, after Docker was stopped.
                Phase::Found { checking } => {
                    state.phase = Phase::Probing;
                    if !checking {
                        actions.push(Action::Probe);
                    }
                }
                _ => restart_if_wanted(&mut state, &mut actions),
            }
        }
        Event::Configured { open } => {
            state.wanted_open = open;
            restart_if_wanted(&mut state, &mut actions);
        }
        Event::Probed(answer) => match (&state.phase, answer) {
            (
                Phase::Probing | Phase::Found { checking: true } | Phase::Lost { .. },
                Probe::MessageCrate,
            ) => {
                state.phase = Phase::Found { checking: false };
            }
            (Phase::Probing | Phase::Found { checking: true }, Probe::Free) => {
                state.phase = Phase::Starting {
                    open: state.wanted_open,
                };
                actions.push(Action::Spawn {
                    open: state.wanted_open,
                });
            }
            (Phase::Probing | Phase::Found { checking: true }, Probe::Other) => {
                state.phase = Phase::PortTaken;
            }
            (
                Phase::Lost {
                    details,
                    probes_left,
                },
                _,
            ) => {
                state.phase = if *probes_left > 0 && answer == Probe::Free {
                    actions.push(Action::ProbeLater);
                    Phase::Lost {
                        details: details.clone(),
                        probes_left: probes_left - 1,
                    }
                } else {
                    Phase::Failed {
                        reason: FailureReason::StartFailed,
                        message: "Message Crate stopped while starting.".into(),
                        details: details.clone(),
                    }
                };
            }
            _ => {}
        },
        Event::Listening => {
            if let Phase::Starting { open } = state.phase {
                state.phase = Phase::Own { open };
            }
        }
        Event::ChildExited { output } => match state.phase {
            // Another Message Crate may have taken the port first; it is
            // used if it answers.
            Phase::Starting { .. } => {
                let probes_left = if output.contains(LOCKED_OUT) {
                    LOCKED_OUT_PROBES
                } else {
                    0
                };
                state.phase = Phase::Lost {
                    details: output,
                    probes_left,
                };
                actions.push(Action::Probe);
            }
            Phase::Own { .. } => {
                state.phase = Phase::Failed {
                    reason: FailureReason::StartFailed,
                    message: "Message Crate stopped.".into(),
                    details: output,
                };
            }
            _ => {}
        },
        Event::Deadline { output } => {
            if matches!(state.phase, Phase::Starting { .. }) {
                state.phase = Phase::Failed {
                    reason: FailureReason::StartFailed,
                    message: "Message Crate took too long to start.".into(),
                    details: output,
                };
                actions.push(Action::Kill);
            }
        }
        Event::SpawnFailed { error } => {
            if matches!(state.phase, Phase::Starting { .. }) {
                state.phase = Phase::Failed {
                    reason: FailureReason::StartFailed,
                    message: "Message Crate could not be started.".into(),
                    details: error,
                };
            }
        }
        Event::Check => {
            if state.phase == (Phase::Found { checking: false }) {
                state.phase = Phase::Found { checking: true };
                actions.push(Action::Probe);
            }
        }
        Event::JobStarted => state.jobs += 1,
        Event::JobEnded => {
            state.jobs = state.jobs.saturating_sub(1);
            restart_if_wanted(&mut state, &mut actions);
        }
        Event::Stopped => match state.phase {
            Phase::Starting { .. } | Phase::Own { .. } => {
                state.phase = Phase::Failed {
                    reason: FailureReason::StartFailed,
                    message: "Message Crate stopped.".into(),
                    details: String::new(),
                };
                actions.push(Action::Kill);
            }
            // A probe under way must not start a server once the app closes.
            Phase::Probing | Phase::Found { .. } | Phase::Lost { .. } => {
                state.phase = Phase::Idle;
            }
            Phase::Idle | Phase::PortTaken | Phase::Failed { .. } => {}
        },
    }
    (state, actions)
}

/// Restart the app's own server when it runs the other way from the network
/// setting. One that is starting is restarted at once; one that is ready only
/// while no desktop job runs, since a kill would cut off an import.
fn restart_if_wanted(state: &mut State, actions: &mut Vec<Action>) {
    let wanted = state.wanted_open;
    let restart = match state.phase {
        Phase::Starting { open } => open != wanted,
        Phase::Own { open } => open != wanted && state.jobs == 0,
        _ => false,
    };
    if restart {
        state.phase = Phase::Starting { open: wanted };
        actions.extend([Action::Kill, Action::Spawn { open: wanted }]);
    }
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
    /// The server's address on this computer: where the app asks what is
    /// running, and where the server listens unless it is open to the
    /// network.
    pub address: SocketAddr,
    /// Whether other devices on the network may connect. The server then
    /// listens on every address of this computer, same port, over plain HTTP.
    pub open_to_network: bool,
    /// Websites allowed to call the server besides the installed app, which
    /// always is. `cargo tauri dev` loads the screens from the Vite dev
    /// server, a different origin, and names it here.
    pub cors_origins: Vec<String>,
}

impl Launch {
    /// The address the server is told to listen on.
    pub fn bind(&self) -> SocketAddr {
        if self.open_to_network {
            SocketAddr::from(([0, 0, 0, 0], self.address.port()))
        } else {
            self.address
        }
    }

    /// The arguments the server is started with.
    pub fn arguments(&self) -> Vec<String> {
        let mut arguments = vec![
            "serve".into(),
            "--data-dir".into(),
            self.data_dir.display().to_string(),
            "--bind".into(),
            self.bind().to_string(),
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
    /// What the app has and what the person wants.
    state: State,
    /// How to start the server, from the last request.
    launch: Option<Launch>,
    /// Whether the start under way creates the database.
    first_time: bool,
    /// The server this app started, while it runs.
    child: Option<Child>,
    /// The threads reading that server's output.
    readers: Vec<JoinHandle<()>>,
    /// That server's last output lines.
    output: Output,
    /// Whether that server wrote [`LISTENING_LINE`].
    listened: Arc<AtomicBool>,
    /// When that server has taken too long to listen.
    deadline: Instant,
    /// Counts the servers started and killed, so a thread watching one that
    /// is gone stops.
    generation: u64,
}

impl Inner {
    /// What the screens are told.
    fn status(&self) -> Status {
        match &self.state.phase {
            Phase::Idle => Status::Idle,
            Phase::Probing | Phase::Starting { .. } | Phase::Lost { .. } => Status::Starting {
                first_time: self.first_time,
            },
            Phase::Own { .. } => Status::Ready {
                started_by_app: true,
            },
            Phase::Found { .. } => Status::Ready {
                started_by_app: false,
            },
            Phase::PortTaken => Status::Failed {
                reason: FailureReason::PortTaken,
                message: format!(
                    "Another program is using port {}. Close it, or enter another server address.",
                    self.launch
                        .as_ref()
                        .map_or(0, |launch| launch.address.port())
                ),
                details: String::new(),
            },
            Phase::Failed {
                reason,
                message,
                details,
            } => Status::Failed {
                reason: *reason,
                message: message.clone(),
                details: details.clone(),
            },
        }
    }

    /// Kill the server this app started and wait for it to end.
    fn kill(&mut self) {
        self.generation += 1;
        self.readers.clear();
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
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
                state: State::default(),
                launch: None,
                first_time: false,
                child: None,
                readers: Vec::new(),
                output: Output::default(),
                listened: Arc::default(),
                deadline: Instant::now(),
                generation: 0,
            })),
        }
    }
}

/// A desktop job that is running. While one is, the app's ready server is
/// not restarted for the network setting; the restart follows when the last
/// one is dropped.
#[derive(Debug)]
pub struct Job {
    server: LocalServer,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.server.apply_now(Event::JobEnded);
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

    /// The current state. A Message Crate the app only found is asked again,
    /// in the background, whether it still answers; when it no longer does,
    /// the app starts its own.
    pub fn status(&self) -> Status {
        let mut inner = self.lock();
        self.apply(&mut inner, Event::Check);
        inner.status()
    }

    /// Make sure a Message Crate answers at `launch.address`, starting the
    /// server when nothing does. Returns at once; [`Self::status`] reports
    /// how it went. Safe to call on every launch: a start under way, or the
    /// app's own ready server, is left alone, a Message Crate the app found
    /// is asked again whether it still answers, and a failed start is tried
    /// again. The app's own server is restarted when `launch.open_to_network`
    /// differs from how it was started, as [`Self::set_open_to_network`]
    /// does.
    pub fn ensure_started(&self, launch: Launch) {
        let mut inner = self.lock();
        let open = launch.open_to_network;
        if matches!(
            inner.state.phase,
            Phase::Idle | Phase::PortTaken | Phase::Failed { .. } | Phase::Found { .. }
        ) {
            inner.first_time = launch.is_first_start();
        }
        inner.launch = Some(launch);
        self.apply(&mut inner, Event::Requested { open });
    }

    /// Take the person's network setting. The app's own server is restarted
    /// to match it: at once while starting, and while ready once no desktop
    /// job runs. Nothing is started, and a Message Crate the app only found
    /// is left as it is.
    pub fn set_open_to_network(&self, open: bool) -> Status {
        let mut inner = self.lock();
        self.apply(&mut inner, Event::Configured { open });
        inner.status()
    }

    /// Record a desktop job until the returned [`Job`] is dropped.
    pub fn job_started(&self) -> Job {
        self.apply_now(Event::JobStarted);
        Job {
            server: self.clone(),
        }
    }

    /// Stop the server this app started, and start nothing more. One the app
    /// only found is left running.
    pub fn stop(&self) {
        self.apply_now(Event::Stopped);
    }

    /// [`Self::apply`] with the lock taken here.
    fn apply_now(&self, event: Event) {
        let mut inner = self.lock();
        self.apply(&mut inner, event);
    }

    /// Run `event` through [`step`] and do what it asks. Killing and
    /// starting the process are quick and done here; asking the address can
    /// take seconds and is done on a thread of its own.
    fn apply(&self, inner: &mut Inner, event: Event) {
        let mut events = VecDeque::from([event]);
        while let Some(event) = events.pop_front() {
            let (state, actions) = step(inner.state.clone(), event);
            inner.state = state;
            for action in actions {
                match action {
                    Action::Probe => self.probe_in_background(inner, Duration::ZERO),
                    Action::ProbeLater => self.probe_in_background(inner, POLL_INTERVAL),
                    Action::Kill => inner.kill(),
                    Action::Spawn { open } => {
                        if let Err(error) = self.spawn(inner, open) {
                            events.push_back(Event::SpawnFailed { error });
                        }
                    }
                }
            }
        }
    }

    /// Ask the address what it is after `wait`, and report the answer.
    fn probe_in_background(&self, inner: &Inner, wait: Duration) {
        let Some(address) = inner.launch.as_ref().map(|launch| launch.address) else {
            return;
        };
        let server = self.clone();
        thread::spawn(move || {
            thread::sleep(wait);
            let answer = probe(address);
            server.apply_now(Event::Probed(answer));
        });
    }

    /// Start the server program from the last request, listening on the
    /// network when `open`, and watch it.
    fn spawn(&self, inner: &mut Inner, open: bool) -> Result<(), String> {
        let Some(launch) = inner.launch.as_ref() else {
            return Err("The app was not told how to start its server.".into());
        };
        let launch = Launch {
            open_to_network: open,
            ..launch.clone()
        };
        inner.first_time = launch.is_first_start();
        let output = Output::default();
        let listened = Arc::new(AtomicBool::new(false));
        let (child, readers) = spawn_server(&launch, &output, &listened)?;
        inner.generation += 1;
        inner.child = Some(child);
        inner.readers = readers;
        inner.output = output;
        inner.listened = listened;
        inner.deadline = Instant::now() + START_TIMEOUT;
        self.watch(inner.generation);
        Ok(())
    }

    /// Check on the server of `generation` until it ends or is replaced:
    /// whether it listens, whether it exited, and whether it has taken too
    /// long.
    fn watch(&self, generation: u64) {
        let server = self.clone();
        thread::spawn(move || {
            loop {
                thread::sleep(POLL_INTERVAL);
                let mut inner = server.lock();
                if inner.generation != generation {
                    return;
                }
                let exited = inner
                    .child
                    .as_mut()
                    .is_none_or(|child| !matches!(child.try_wait(), Ok(None)));
                if exited {
                    inner.child = None;
                    let readers = std::mem::take(&mut inner.readers);
                    let output = Arc::clone(&inner.output);
                    drop(inner);
                    // The readers end once the pipes close. Until then, the
                    // last lines the server wrote may not be in `output`.
                    for reader in readers {
                        let _ = reader.join();
                    }
                    let mut inner = server.lock();
                    if inner.generation == generation {
                        let output = joined(&output);
                        server.apply(&mut inner, Event::ChildExited { output });
                    }
                    return;
                }
                if inner.listened.load(Ordering::Relaxed) {
                    server.apply(&mut inner, Event::Listening);
                }
                if matches!(inner.state.phase, Phase::Starting { .. })
                    && Instant::now() >= inner.deadline
                {
                    let output = joined(&inner.output);
                    server.apply(&mut inner, Event::Deadline { output });
                }
            }
        });
    }
}

/// Start the server program with its output kept for a failure report.
/// Returns the process and the threads reading its output.
fn spawn_server(
    launch: &Launch,
    output: &Output,
    listened: &Arc<AtomicBool>,
) -> Result<(Child, Vec<JoinHandle<()>>), String> {
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
    let mut readers = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        readers.push(keep_output(
            stdout,
            Arc::clone(output),
            Arc::clone(listened),
        ));
    }
    if let Some(stderr) = child.stderr.take() {
        readers.push(keep_output(
            stderr,
            Arc::clone(output),
            Arc::clone(listened),
        ));
    }
    Ok((child, readers))
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

/// Read `stream` until it closes, keeping its last lines in `output` and
/// setting `listened` when the server says it listens. The pipe has to be
/// drained for as long as the server runs, or the server blocks once the
/// pipe is full.
fn keep_output(
    stream: impl Read + Send + 'static,
    output: Output,
    listened: Arc<AtomicBool>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            if line.starts_with(LISTENING_LINE) {
                listened.store(true, Ordering::Relaxed);
            }
            let mut lines = output
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if lines.len() == OUTPUT_LINES_KEPT {
                lines.pop_front();
            }
            lines.push_back(line);
        }
    })
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
///
/// A dev build (`cargo tauri dev`) uses `data-dev` beside it. Both builds
/// share one app-data folder, because `tauri.conf.json` has one identifier,
/// and a dev build on a branch with another Schema Fingerprint would
/// otherwise rebuild the installed app's database empty.
pub fn data_dir_in(app_data_dir: &Path, dev: bool) -> PathBuf {
    app_data_dir.join(if dev { "data-dev" } else { "data" })
}

#[cfg(test)]
mod tests;
