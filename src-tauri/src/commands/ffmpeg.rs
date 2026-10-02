//! Commands that find ffmpeg and ffprobe, and remember which folder they live in.
//!
//! Attachment convert and compress need those programs. The WebView cannot
//! search the disk or set process environment variables, so this process does
//! both. This process never *writes* environment variables: the tools-folder
//! override lives in media-crate process state, and `MESSAGE_CRATE_BIN`
//! stays a user-set fallback that is only ever read here (by the media and
//! whatsapp-exporter resolution paths, which is sound because nothing in
//! this process writes the environment).

use std::path::{Path, PathBuf};

/// Paths the Settings screen shows after looking for ffmpeg and ffprobe.
#[derive(Debug, serde::Serialize)]
pub struct FfmpegToolsProbeDto {
    /// Whether both tools were found and pass `-version`.
    pub ok: bool,
    /// Resolved ffmpeg path, when found.
    pub ffmpeg_path: Option<String>,
    /// Resolved ffprobe path, when found.
    pub ffprobe_path: Option<String>,
    /// What was missing, when `ok` is false.
    pub error: Option<String>,
}

/// Copy probe results into the JSON shape the UI expects.
fn probe_to_dto(probe: media::FfmpegToolsProbe) -> FfmpegToolsProbeDto {
    let ffmpeg_path = probe.ffmpeg_path.map(|path| path.display().to_string());
    let ffprobe_path = probe.ffprobe_path.map(|path| path.display().to_string());
    FfmpegToolsProbeDto {
        ok: probe.ok,
        ffmpeg_path,
        ffprobe_path,
        error: probe.error,
    }
}

/// Treat a blank folder string as "use the default search instead".
fn optional_tools_dir(dir: Option<&str>) -> Option<&Path> {
    let dir = dir.and_then(message_ir::trimmed)?;
    Some(Path::new(dir))
}

/// Ask this process whether ffmpeg and ffprobe are available.
///
/// When `dir` is set, look in that folder. When it is empty, use the default
/// search: the saved override folder, beside the program, `lib/`,
/// `MESSAGE_CRATE_BIN`, then the process PATH.
#[tauri::command]
pub fn probe_ffmpeg_tools(dir: Option<String>) -> FfmpegToolsProbeDto {
    let tools_dir = optional_tools_dir(dir.as_deref());
    let probe = media::probe_ffmpeg_tools(tools_dir);
    probe_to_dto(probe)
}

/// Ask this process to remember where ffmpeg and ffprobe live for this session.
///
/// An empty `dir` clears the override and goes back to the default search
/// path. This process never writes environment variables: the override
/// lives in media-crate process state, and `MESSAGE_CRATE_BIN` stays a
/// user-set fallback that is only ever read here.
///
/// # Errors
///
/// Returns an error if `dir` is set but ffmpeg or ffprobe cannot be found
/// there.
#[tauri::command]
pub fn set_ffmpeg_tools_dir(dir: Option<String>) -> Result<FfmpegToolsProbeDto, String> {
    let trimmed = dir.as_deref().and_then(message_ir::trimmed);
    match trimmed {
        None => {
            media::set_tools_dir(None);
            Ok(probe_ffmpeg_tools(None))
        }
        Some(dir) => {
            let path = PathBuf::from(dir);
            let probe = media::probe_ffmpeg_tools(Some(path.as_path()));
            if !probe.ok {
                return Err(probe
                    .error
                    .unwrap_or_else(|| "ffmpeg tools not found".into()));
            }
            media::set_tools_dir(Some(path));
            Ok(probe_to_dto(probe))
        }
    }
}
