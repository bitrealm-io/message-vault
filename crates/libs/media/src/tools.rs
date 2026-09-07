use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};

use anyhow::{Context, Result, bail};

struct ToolsState {
    override_dir: Option<PathBuf>,
    generation: u64,
    ffmpeg: Option<PathBuf>,
    ffprobe: Option<PathBuf>,
}

impl ToolsState {
    /// The cached location of `ffmpeg` or `ffprobe`.
    fn cached(&self, name: &str) -> Option<PathBuf> {
        match name {
            "ffmpeg" => self.ffmpeg.clone(),
            "ffprobe" => self.ffprobe.clone(),
            _ => None,
        }
    }

    /// Remember where `ffmpeg` or `ffprobe` was found.
    fn set_cached(&mut self, name: &str, path: Option<PathBuf>) {
        match name {
            "ffmpeg" => self.ffmpeg = path,
            "ffprobe" => self.ffprobe = path,
            _ => {}
        }
    }
}

/// The process-wide tool cache.
fn tools_state() -> &'static Mutex<ToolsState> {
    static STATE: OnceLock<Mutex<ToolsState>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(ToolsState {
            override_dir: None,
            generation: 0,
            ffmpeg: None,
            ffprobe: None,
        })
    })
}

/// Store a folder-only override for ffmpeg/ffprobe discovery and clear cached paths.
pub fn set_tools_dir(dir: Option<PathBuf>) {
    let mut state = tools_state().lock().expect("tools state lock");
    state.override_dir = dir;
    state.generation = state.generation.wrapping_add(1);
    state.ffmpeg = None;
    state.ffprobe = None;
}

/// Current tools-folder override, if any (primarily for tests).
pub fn tools_dir() -> Option<PathBuf> {
    tools_state()
        .lock()
        .expect("tools state lock")
        .override_dir
        .clone()
}

/// Result of locating ffmpeg and ffprobe (the GUI's probe result type).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfmpegToolsProbe {
    /// Whether both tools were found and pass `-version`.
    pub ok: bool,
    /// Resolved ffmpeg path, if found.
    pub ffmpeg_path: Option<PathBuf>,
    /// Resolved ffprobe path, if found.
    pub ffprobe_path: Option<PathBuf>,
    /// Human-readable list of missing tools when `ok` is false.
    pub error: Option<String>,
}

/// True when both ffmpeg and ffprobe resolve from the search path.
pub fn ffmpeg_available() -> bool {
    resolve_tool("ffmpeg").is_some() && resolve_tool("ffprobe").is_some()
}

/// Fail with an installation hint when ffmpeg and ffprobe are not available.
pub(crate) fn require_ffmpeg() -> Result<()> {
    if ffmpeg_available() {
        Ok(())
    } else {
        bail!(
            "ffmpeg and ffprobe are required for --media-mode convert/compress. \
             Keep the release-bundled tools in lib/ next to this program (or ../lib/ from cli/), \
             install ffmpeg on PATH, or set MESSAGE_VAULT_IO_BIN to a directory that contains both."
        )
    }
}

/// True when running `bin` with `args` exits successfully.
fn command_runs(bin: &Path, args: &[&str]) -> bool {
    Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Resolve `ffmpeg` / `ffprobe`: tools-dir override, then sibling of current exe,
/// `lib/` next to the GUI, `../lib/` from `cli/`, legacy parent dir,
/// `MESSAGE_VAULT_IO_BIN`, then PATH.
fn resolve_tool(name: &str) -> Option<PathBuf> {
    if !matches!(name, "ffmpeg" | "ffprobe") {
        let override_dir = tools_state()
            .lock()
            .expect("tools state lock")
            .override_dir
            .clone();
        return find_tool_with_override(name, override_dir.as_deref());
    }

    loop {
        let (generation, override_dir) = {
            let state = tools_state().lock().expect("tools state lock");
            if let Some(cached) = state.cached(name) {
                return Some(cached);
            }
            (state.generation, state.override_dir.clone())
        };

        let resolved = find_tool_with_override(name, override_dir.as_deref());

        let mut state = tools_state().lock().expect("tools state lock");
        if state.generation != generation {
            continue;
        }
        if state.cached(name).is_none() {
            state.set_cached(name, resolved.clone());
        }
        return resolved;
    }
}

/// The tool under `dir` if it exists and runs.
fn find_tool_in_dir(dir: &Path, name: &str) -> Option<PathBuf> {
    let candidate = dir.join(executable_name(name));
    if candidate.is_file() && command_runs(&candidate, &["-version"]) {
        Some(candidate)
    } else {
        None
    }
}

/// Probe both tools in an explicit directory, or fall back to the default
/// resolution path (tools-dir override, beside the executable, `MESSAGE_VAULT_IO_BIN`, PATH).
pub fn probe_ffmpeg_tools(dir: Option<&Path>) -> FfmpegToolsProbe {
    let (ffmpeg, ffprobe) = match dir {
        Some(d) => (
            find_tool_in_dir(d, "ffmpeg"),
            find_tool_in_dir(d, "ffprobe"),
        ),
        None => (resolve_tool("ffmpeg"), resolve_tool("ffprobe")),
    };
    match (ffmpeg, ffprobe) {
        (Some(f), Some(p)) => FfmpegToolsProbe {
            ok: true,
            ffmpeg_path: Some(f),
            ffprobe_path: Some(p),
            error: None,
        },
        (f, p) => {
            let mut parts = Vec::new();
            if f.is_none() {
                parts.push("ffmpeg not found or failed -version");
            }
            if p.is_none() {
                parts.push("ffprobe not found or failed -version");
            }
            FfmpegToolsProbe {
                ok: false,
                ffmpeg_path: f,
                ffprobe_path: p,
                error: Some(parts.join("; ")),
            }
        }
    }
}

/// Locate a tool: in the override folder when set, else beside the program, in `lib/`,
/// in `MESSAGE_VAULT_IO_BIN`, or on PATH.
fn find_tool_with_override(name: &str, override_dir: Option<&Path>) -> Option<PathBuf> {
    if let Some(dir) = override_dir {
        return find_tool_in_dir(dir, name);
    }

    let executable = executable_name(name);

    if let Ok(current) = std::env::current_exe()
        && let Some(dir) = current.parent()
    {
        let candidates = [
            dir.join(&executable),
            dir.join("lib").join(&executable),
            dir.parent()
                .map(|p| p.join("lib").join(&executable))
                .unwrap_or_default(),
            // Legacy flat-root archives.
            dir.parent()
                .map(|p| p.join(&executable))
                .unwrap_or_default(),
        ];
        for candidate in candidates {
            if candidate.as_os_str().is_empty() {
                continue;
            }
            if candidate.is_file() && command_runs(&candidate, &["-version"]) {
                return Some(candidate);
            }
        }
    }

    if let Some(extra) = std::env::var_os("MESSAGE_VAULT_IO_BIN") {
        let candidate = PathBuf::from(extra).join(&executable);
        if candidate.is_file() && command_runs(&candidate, &["-version"]) {
            return Some(candidate);
        }
    }

    if let Some(paths) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&paths) {
            let candidate = directory.join(&executable);
            if candidate.is_file() && command_runs(&candidate, &["-version"]) {
                return Some(candidate);
            }
        }
    }

    // Last resort: bare name (PATH lookup by the OS / shell semantics).
    let bare = PathBuf::from(&executable);
    if command_runs(&bare, &["-version"]) {
        return Some(bare);
    }

    None
}

/// The tool's file name, with `.exe` on Windows.
fn executable_name(name: &str) -> String {
    if cfg!(windows) && !name.ends_with(".exe") {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// Run ffmpeg with `args`, failing with its stderr when it exits non-zero.
pub(crate) fn run_ffmpeg(args: &[String]) -> Result<()> {
    let ffmpeg = resolve_tool("ffmpeg").ok_or_else(|| {
        anyhow::anyhow!(
            "ffmpeg not found in lib/ (or beside this program), in MESSAGE_VAULT_IO_BIN, or on PATH"
        )
    })?;
    let status = Command::new(ffmpeg)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        bail!("ffmpeg failed ({status})")
    }
}

#[derive(Debug, Default, Clone)]
pub(crate) struct Probe {
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub bitrate: u64,
}

/// Build a `Command` for ffprobe, resolved the same way as every other tool
/// lookup in this module. The one place that decides where ffprobe lives, so
/// [`probe_video`] and the public [`crate::probe_media`] agree with each
/// other and report the same error when the tool is missing.
///
/// # Errors
///
/// Returns a named "ffprobe not found…" error when ffprobe cannot be
/// resolved anywhere this module looks — deliberately, rather than falling
/// back to a bare `ffprobe` command name: a bare name that then fails to
/// spawn surfaces as an IO error pointing at the *input file* ("No such file
/// or directory"), which reads as a problem with the user's file rather than
/// a missing tool.
pub(crate) fn ffprobe_command() -> Result<Command> {
    let ffprobe = resolve_tool("ffprobe").ok_or_else(|| {
        anyhow::anyhow!(
            "ffprobe not found in lib/ (or beside this program), in MESSAGE_VAULT_IO_BIN, or on PATH"
        )
    })?;
    let mut cmd = Command::new(ffprobe);
    cmd.stdin(Stdio::null());
    Ok(cmd)
}

/// Width, height, and frame rate of a video from ffprobe.
pub(crate) fn probe_video(path: &std::path::Path) -> Result<Probe> {
    let mut cmd = ffprobe_command()?;
    cmd.args([
        "-v",
        "error",
        "-select_streams",
        "v:0",
        "-show_entries",
        "stream=codec_name,width,height,bit_rate",
        "-of",
        "csv=p=0",
        path.to_str().unwrap_or(""),
    ]);
    let output = cmd
        .output()
        .with_context(|| format!("run ffprobe on {}", path.display()))?;
    if !output.status.success() {
        bail!("ffprobe failed for {}", path.display());
    }
    let line = String::from_utf8_lossy(&output.stdout);
    let parts: Vec<&str> = line.trim().split(',').collect();
    let codec = parts.first().copied().unwrap_or("").to_ascii_lowercase();
    let width = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let height = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    let bitrate = parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
    Ok(Probe {
        codec,
        width,
        height,
        bitrate,
    })
}

/// Serialize every test that reads or changes the process-global tools
/// directory. The tests in this module point it at empty and mock
/// directories; a test elsewhere in the crate that needs the real ffmpeg
/// must hold this lock too, or it can observe a directory with no tools
/// in it for the instant between a set and its restore.
#[cfg(test)]
pub(crate) fn tools_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Hold the tools lock for a test that needs the real ffmpeg, and say whether
/// it is there. `None` means it is not, and the test should return.
///
/// Taking the lock and asking whether ffmpeg is available are one call because
/// doing either without the other is the bug this exists to prevent. A test
/// that asks first can be answered by whatever directory another test has the
/// override pointed at for that instant: an empty one makes it skip silently
/// and report a pass it never earned, and a mock one leaves a tool that exits
/// 0 and writes nothing cached process-wide, so the real work later fails on
/// an output file that was never produced. Both were seen on CI (#308).
#[cfg(test)]
#[must_use]
pub(crate) fn real_ffmpeg_test_guard() -> Option<std::sync::MutexGuard<'static, ()>> {
    let guard = tools_test_lock();
    ffmpeg_available().then_some(guard)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    struct RestoreToolsDir(Option<PathBuf>);

    impl RestoreToolsDir {
        fn capture() -> Self {
            Self(tools_dir())
        }
    }

    impl Drop for RestoreToolsDir {
        fn drop(&mut self) {
            set_tools_dir(self.0.clone());
        }
    }

    fn write_mock_tool(path: &Path) {
        fs::write(path, "#!/bin/sh\nexit 0\n").unwrap();
        let mut perms = fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).unwrap();
    }

    #[test]
    fn executable_name_matches_platform() {
        let name = executable_name("ffmpeg");
        if cfg!(windows) {
            assert_eq!(name, "ffmpeg.exe");
        } else {
            assert_eq!(name, "ffmpeg");
        }
    }

    #[cfg(unix)]
    #[test]
    fn probe_folder_requires_both_tools() {
        let _guard = tools_test_lock();
        let _restore = RestoreToolsDir::capture();
        let dir = tempfile::tempdir().unwrap();
        write_mock_tool(&dir.path().join("ffmpeg"));

        let probe = probe_ffmpeg_tools(Some(dir.path()));
        assert!(!probe.ok);
        assert!(probe.ffmpeg_path.is_some());
        assert!(probe.ffprobe_path.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn set_tools_dir_overrides_and_clears_cache() {
        let _guard = tools_test_lock();
        let _restore = RestoreToolsDir::capture();
        let dir = tempfile::tempdir().unwrap();
        for name in ["ffmpeg", "ffprobe"] {
            write_mock_tool(&dir.path().join(name));
        }
        set_tools_dir(Some(dir.path().to_path_buf()));
        assert_eq!(tools_dir(), Some(dir.path().to_path_buf()));
        assert!(ffmpeg_available());
        set_tools_dir(None);
        assert_eq!(tools_dir(), None);
    }

    #[cfg(unix)]
    #[test]
    fn missing_ffprobe_names_the_tool_not_the_input_file() {
        // A `Command` built from a bare, unresolved "ffprobe" would fail to
        // spawn with an IO error naming whatever path was passed as the
        // *input* — "No such file or directory" on `/path/to/IMG_0001.HEIC"
        // — which reads as a problem with the user's file, not the missing
        // tool. `ffprobe_command` must fail before that, with a message that
        // names ffprobe.
        let _guard = tools_test_lock();
        let _restore = RestoreToolsDir::capture();
        let empty = tempfile::tempdir().unwrap();
        set_tools_dir(Some(empty.path().to_path_buf()));

        let err = ffprobe_command().expect_err("no ffprobe in an empty override dir");
        let message = err.to_string();
        assert!(
            message.contains("ffprobe not found"),
            "message was {message:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn probe_candidate_folder_does_not_change_override() {
        let _guard = tools_test_lock();
        let _restore = RestoreToolsDir::capture();
        let live = tempfile::tempdir().unwrap();
        for name in ["ffmpeg", "ffprobe"] {
            write_mock_tool(&live.path().join(name));
        }
        set_tools_dir(Some(live.path().to_path_buf()));

        let candidate = tempfile::tempdir().unwrap();
        write_mock_tool(&candidate.path().join("ffmpeg"));

        let _probe = probe_ffmpeg_tools(Some(candidate.path()));
        assert_eq!(tools_dir(), Some(live.path().to_path_buf()));
    }

    #[cfg(unix)]
    #[test]
    fn find_tool_prefers_message_vault_io_bin() {
        let _guard = tools_test_lock();
        let _restore = RestoreToolsDir::capture();
        set_tools_dir(None);
        let dir = tempfile::tempdir().unwrap();
        write_mock_tool(&dir.path().join("ffmpeg"));

        // SAFETY: test-only env mutation; this test holds tools_state_lock so no
        // concurrent resolve_tool calls run. In production, set_tools_dir override
        // is checked before MESSAGE_VAULT_IO_BIN; job threads share the same override.
        unsafe {
            std::env::set_var("MESSAGE_VAULT_IO_BIN", dir.path());
        }
        let found = resolve_tool("ffmpeg").expect("ffmpeg from MESSAGE_VAULT_IO_BIN");
        assert_eq!(found, dir.path().join("ffmpeg"));
        unsafe {
            std::env::remove_var("MESSAGE_VAULT_IO_BIN");
        }
    }
}
