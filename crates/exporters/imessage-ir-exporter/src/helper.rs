//! Find, start, and talk to the `imessage-reader` program.
//!
//! The program is found the way `ffmpeg` is in `crates/libs/media`: an
//! explicit environment variable first, then beside this executable, then
//! `MESSAGE_VAULT_IO_BIN`, then `PATH`. The desktop bundle puts it beside the
//! app (`externalBin` in `src-tauri/tauri.conf.json`), so a person never sets
//! anything. `MESSAGE_VAULT_IMESSAGE_READER` names one file outright, for a
//! build that keeps the program somewhere else.
//!
//! One [`Helper`] is one process and one request. It relays the program's
//! log lines to the run's [`LogSink`] and its counts to the run's
//! [`ProgressSink`], hands back every other event to the caller, and kills
//! the process when dropped, so a cancelled run leaves no orphan behind.

use std::{
    env,
    ffi::OsString,
    io::{BufRead, BufReader, Lines, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    thread::JoinHandle,
};

use anyhow::{Context, Result, anyhow, bail};
use imessage_reader_protocol::{Event, HELPER_NAME, PROTOCOL_VERSION, Progress, Request};
use message_vault_io_core::{LogSink, ProgressEvent, ProgressSink, emit_log, emit_progress};

/// Names the helper executable outright, bypassing the search.
pub(crate) const HELPER_PATH_ENV: &str = "MESSAGE_VAULT_IMESSAGE_READER";

/// The helper's file name on this platform.
fn executable_name() -> String {
    if cfg!(windows) {
        format!("{HELPER_NAME}.exe")
    } else {
        HELPER_NAME.to_string()
    }
}

/// Locate `imessage-reader`: `MESSAGE_VAULT_IMESSAGE_READER`, then beside
/// this executable, then the folder above it (an integration test runs from
/// `target/<profile>/deps/` while the program sits in `target/<profile>/`),
/// then `MESSAGE_VAULT_IO_BIN`, then `PATH`.
///
/// # Errors
///
/// Returns an error naming every path tried when no file is found.
pub(crate) fn locate() -> Result<PathBuf> {
    let current = env::current_exe().ok();
    locate_in(&Places {
        explicit: env::var_os(HELPER_PATH_ENV).map(PathBuf::from),
        exe_dir: current.as_deref().and_then(Path::parent),
        io_bin: env::var_os("MESSAGE_VAULT_IO_BIN").map(PathBuf::from),
        path: env::var_os("PATH"),
    })
}

/// Where [`locate`] looks, read from the environment once so the search
/// itself can be tested without changing the environment.
struct Places<'a> {
    /// `MESSAGE_VAULT_IMESSAGE_READER`.
    explicit: Option<PathBuf>,
    /// The folder of the running executable.
    exe_dir: Option<&'a Path>,
    /// `MESSAGE_VAULT_IO_BIN`.
    io_bin: Option<PathBuf>,
    /// `PATH`.
    path: Option<OsString>,
}

/// The search [`locate`] runs, over the places given.
fn locate_in(places: &Places<'_>) -> Result<PathBuf> {
    if let Some(path) = &places.explicit {
        if path.is_file() {
            return Ok(path.clone());
        }
        bail!(
            "{HELPER_PATH_ENV} is set but not a file: {}",
            path.display()
        );
    }

    let executable = executable_name();
    let mut tried = Vec::new();

    if let Some(dir) = places.exe_dir {
        let beside = dir.join(&executable);
        let above = dir.parent().map(|p| p.join(&executable));
        for candidate in std::iter::once(beside).chain(above) {
            if candidate.is_file() {
                return Ok(candidate);
            }
            tried.push(candidate);
        }
    }

    if let Some(extra) = &places.io_bin {
        let candidate = extra.join(&executable);
        if candidate.is_file() {
            return Ok(candidate);
        }
        tried.push(candidate);
    }

    if let Some(paths) = &places.path {
        for directory in env::split_paths(paths) {
            let candidate = directory.join(&executable);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }

    bail!(
        "Could not find {executable}, the program that reads Apple Messages. \
         The desktop installer places it beside the app; a source build gets it from \
         `cargo build -p imessage-reader`. Tried: {}",
        tried
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// A running `imessage-reader` serving one request.
pub(crate) struct Helper {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Lines<BufReader<ChildStdout>>,
    stderr: Option<JoinHandle<String>>,
    log: Option<LogSink>,
    progress: Option<ProgressSink>,
    /// The request was an export or an identities read, and the program has
    /// not yet sent the [`Event::Source`] that says which protocol version it
    /// speaks.
    awaiting_source: bool,
}

impl Helper {
    /// Start the program and send it `request`.
    ///
    /// # Errors
    ///
    /// Returns an error when the program cannot be found or started, or the
    /// request cannot be written.
    pub fn spawn(
        request: &Request,
        log: Option<LogSink>,
        progress: Option<ProgressSink>,
    ) -> Result<Self> {
        let path = locate()?;
        Self::spawn_at(&path, request, log, progress)
    }

    /// Start the program at `path` and send it `request`.
    ///
    /// # Errors
    ///
    /// Returns an error when the program cannot be started or the request
    /// cannot be written.
    pub fn spawn_at(
        path: &Path,
        request: &Request,
        log: Option<LogSink>,
        progress: Option<ProgressSink>,
    ) -> Result<Self> {
        let mut child = Command::new(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("start {}", path.display()))?;
        let mut stdin = child.stdin.take().expect("stdin was requested");
        let stdout = child.stdout.take().expect("stdout was requested");
        let stderr = child.stderr.take().expect("stderr was requested");
        // Drain stderr on its own thread so a chatty program cannot block on
        // a full pipe while this side waits for its stdout.
        let stderr = std::thread::spawn(move || {
            let mut text = String::new();
            let _ = std::io::Read::read_to_string(&mut BufReader::new(stderr), &mut text);
            text
        });

        let mut line = serde_json::to_string(request)?;
        line.push('\n');
        stdin
            .write_all(line.as_bytes())
            .and_then(|()| stdin.flush())
            .context("send the request to imessage-reader")?;

        Ok(Self {
            child,
            stdin: Some(stdin),
            stdout: BufReader::new(stdout).lines(),
            stderr: Some(stderr),
            log,
            progress,
            awaiting_source: !matches!(request, Request::Attachment { .. }),
        })
    }

    /// The next event that is not a log line or a progress count. Those two
    /// are relayed to the run's sinks as they arrive.
    ///
    /// # Errors
    ///
    /// Returns an error when the program reports one, sends something that is
    /// not an event, or exits before answering.
    pub fn next_event(&mut self) -> Result<Event> {
        loop {
            let Some(line) = self.stdout.next() else {
                return Err(self.exited_early());
            };
            let line = line.context("read from imessage-reader")?;
            let event: Event = serde_json::from_str(&line)
                .with_context(|| format!("imessage-reader sent something unexpected: {line}"))?;
            match event {
                Event::Log { line } => emit_log(self.log.as_ref(), line),
                Event::Progress(progress) => {
                    emit_progress(self.progress.as_ref(), progress_event(progress));
                }
                Event::Error { message } => return Err(anyhow!(message)),
                Event::Source {
                    protocol_version, ..
                } if protocol_version != PROTOCOL_VERSION => {
                    bail!(
                        "imessage-reader speaks protocol version {protocol_version}, \
                         this app speaks {PROTOCOL_VERSION}; the two were not built together"
                    );
                }
                source @ Event::Source { .. } => {
                    self.awaiting_source = false;
                    return Ok(source);
                }
                other if self.awaiting_source => bail!(
                    "imessage-reader answered without saying which protocol version it speaks \
                     (this app speaks {PROTOCOL_VERSION}), so the two were not built together. \
                     It sent {other:?}"
                ),
                other => return Ok(other),
            }
        }
    }

    /// Ask the program to decrypt one attachment of the streamed export.
    /// `None` means the backup does not hold it.
    ///
    /// # Errors
    ///
    /// Returns an error when the program fails or answers with the wrong
    /// event.
    pub fn decrypt_attachment(&mut self, path: &Path) -> Result<Option<PathBuf>> {
        self.send(&Request::Attachment {
            path: path.to_path_buf(),
        })?;
        match self.next_event()? {
            Event::Attachment { path } => Ok(path),
            other => bail!("expected an attachment answer, got {other:?}"),
        }
    }

    /// Close the program's stdin and wait for it to exit.
    ///
    /// # Errors
    ///
    /// Returns an error when the program exits with a failure status.
    pub fn finish(mut self) -> Result<()> {
        drop(self.stdin.take());
        let status = self.child.wait().context("wait for imessage-reader")?;
        if !status.success() {
            let stderr = self.stderr_text();
            bail!("imessage-reader exited with {status}{}", tail(&stderr));
        }
        Ok(())
    }

    /// Write one more request line.
    fn send(&mut self, request: &Request) -> Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow!("imessage-reader's stdin is already closed"))?;
        let mut line = serde_json::to_string(request)?;
        line.push('\n');
        stdin
            .write_all(line.as_bytes())
            .and_then(|()| stdin.flush())
            .context("send a request to imessage-reader")
    }

    /// The error for a program that stopped talking before it was done.
    fn exited_early(&mut self) -> anyhow::Error {
        let status = self
            .child
            .wait()
            .map_or_else(|e| format!("unknown status ({e})"), |s| s.to_string());
        let stderr = self.stderr_text();
        anyhow!(
            "imessage-reader stopped before finishing ({status}){}",
            tail(&stderr)
        )
    }

    /// Everything the program wrote to stderr, once it has exited.
    fn stderr_text(&mut self) -> String {
        self.stderr
            .take()
            .and_then(|handle| handle.join().ok())
            .unwrap_or_default()
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        // Already exited: kill is a no-op error and wait reaps it. Still
        // running (a cancelled or failed run): kill so no orphan keeps the
        // backup open.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The program's count as the event the desktop's progress bar consumes.
fn progress_event(progress: Progress) -> ProgressEvent {
    let count = |n: u64| usize::try_from(n).unwrap_or(usize::MAX);
    match progress {
        Progress::Setup { label, step, total } => ProgressEvent::Setup {
            label,
            step: count(step),
            total: count(total),
        },
        Progress::Parse { done, total } => ProgressEvent::Parse {
            done: count(done),
            total: count(total),
        },
    }
}

/// The last few lines of stderr, formatted to follow an error sentence.
fn tail(stderr: &str) -> String {
    let trimmed = stderr.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let lines: Vec<&str> = trimmed.lines().collect();
    let keep = lines.len().saturating_sub(8);
    format!(": {}", lines[keep..].join(" | "))
}

#[cfg(test)]
pub(crate) mod tests;
