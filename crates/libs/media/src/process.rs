use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::tools::{probe_video, require_ffmpeg, run_ffmpeg};
use crate::{CompressOptions, MediaMode};

/// Aggregate counts and errors from one media convert/compress pass.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MediaReport {
    /// Number of files converted or compressed.
    pub processed: usize,
    /// Number of files left unchanged.
    pub skipped: usize,
    /// Total bytes under `attachments/` before convert/compress (non-temp files).
    pub bytes_before: u64,
    /// Total bytes under `attachments/` after convert/compress (non-temp files).
    pub bytes_after: u64,
    /// Per-file error messages (`path: error`) from the pass.
    pub errors: Vec<String>,
}

/// How often to write `…n/total` progress lines during convert/compress.
const MEDIA_PROGRESS_EVERY: usize = 100;

/// JPEGs at or under this size are left alone in compress mode: re-encoding
/// them buys nothing.
const JPEG_COMPRESS_FLOOR: u64 = 500 * 1024;
/// MP3s at or under this size are left alone in compress mode.
const MP3_COMPRESS_FLOOR: u64 = 100 * 1024;

/// Convert or compress the given attachment files in place.
///
/// The caller builds `files` (usually via [`collect_media_files`]), so a
/// resumed or scoped pass can name exactly the files it means instead of
/// sweeping the whole directory. Paths must live under `output_dir`'s
/// `attachments/` directory.
///
/// Returns a path remap (`old_rel` → `new_rel`, forward-slash relative to
/// `output_dir`) for callers that update IR / CSV themselves.
///
/// # Errors
///
/// Returns an error when ffmpeg/ffprobe are missing or fail, an input path
/// escapes the output directory, or IO fails.
pub fn process_attachment_files(
    output_dir: &Path,
    files: &[PathBuf],
    mode: MediaMode,
    compress: &CompressOptions,
    mut log: Option<&mut dyn FnMut(&str)>,
) -> Result<(MediaReport, HashMap<String, String>)> {
    if matches!(mode, MediaMode::Clone | MediaMode::Disabled) {
        return Ok((MediaReport::default(), HashMap::new()));
    }
    require_ffmpeg()?;

    let attachments = output_dir.join("attachments");
    if !attachments.is_dir() {
        return Ok((MediaReport::default(), HashMap::new()));
    }

    // Leftovers from a previous failed ffmpeg run.
    remove_msgmedia_temps(&attachments)?;

    let mut report = MediaReport::default();
    let mut remap = HashMap::new();
    let total = files.len();
    if total == 0 {
        return Ok((report, remap));
    }

    report.bytes_before = attachments_dir_bytes(&attachments)?;
    let verb = match mode {
        MediaMode::Compress => "Compressing",
        _ => "Converting",
    };
    emit(&mut log, "");
    emit(
        &mut log,
        &format!(
            "{verb} attachments ({total} file(s), {})…",
            format_bytes(report.bytes_before)
        ),
    );

    let mut done = 0usize;
    for path in files {
        match process_one(output_dir, path, mode, compress) {
            Ok(Outcome::Changed { old_rel, new_rel }) => {
                report.processed += 1;
                remap.insert(old_rel, new_rel);
            }
            Ok(Outcome::Skipped) => report.skipped += 1,
            Err(err) => report.errors.push(format!("{}: {err}", path.display())),
        }
        done += 1;
        if done.is_multiple_of(MEDIA_PROGRESS_EVERY) || done == total {
            emit(&mut log, &format!("  …{done}/{total}"));
        }
    }

    // Always sweep again so a failed convert cannot leave junk behind.
    remove_msgmedia_temps(&attachments)?;
    report.bytes_after = attachments_dir_bytes(&attachments)?;

    let mut summary = format!(
        "Attachment {mode} done: processed={} skipped={} size {} → {}",
        report.processed,
        report.skipped,
        format_bytes(report.bytes_before),
        format_bytes(report.bytes_after),
    );
    if !report.errors.is_empty() {
        summary.push_str(&format!(" errors={}", report.errors.len()));
    }
    emit(&mut log, &summary);

    Ok((report, remap))
}

/// Send one line to the log callback, if there is one.
fn emit(log: &mut Option<&mut dyn FnMut(&str)>, line: &str) {
    if let Some(log) = log.as_mut() {
        log(line);
    }
}

/// Sum sizes of non-temp files under `attachments/` (folder-level total).
fn attachments_dir_bytes(attachments: &Path) -> Result<u64> {
    let mut total = 0u64;
    let mut stack = vec![attachments.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if !is_msgmedia_temp(&path) {
                total = total.saturating_add(
                    entry
                        .metadata()
                        .with_context(|| format!("stat {}", path.display()))?
                        .len(),
                );
            }
        }
    }
    Ok(total)
}

/// A byte count as KB, MB, or GB with one decimal, for progress lines and
/// reports. Decimal units: that is how people read a file size.
pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1000.0;
    const MB: f64 = KB * 1000.0;
    const GB: f64 = MB * 1000.0;
    let n = bytes as f64;
    if n >= GB {
        format!("{:.1} GB", n / GB)
    } else if n >= MB {
        format!("{:.1} MB", n / MB)
    } else if n >= KB {
        format!("{:.1} KB", n / KB)
    } else {
        format!("{bytes} B")
    }
}

enum Outcome {
    Changed { old_rel: String, new_rel: String },
    Skipped,
}

pub(crate) use crate::mime::Kind;

/// List the files a media pass would touch under `root`.
///
/// Every non-temp file [`classify`] recognizes, recursively, sorted so two
/// runs enumerate in the same order. Callers hand the result — or a subset of
/// it — to [`process_attachment_files`].
///
/// # Errors
///
/// Returns an error when a directory under `root` cannot be read.
pub fn collect_media_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if !is_msgmedia_temp(&path) && classify(&path).is_some() {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Sidecar written by ffmpeg before [`replace_original`] (must never remain on disk).
fn is_msgmedia_temp(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.contains(".msgmedia.tmp."))
}

/// Delete `*.msgmedia.tmp.*` files left by an interrupted run anywhere under `root`.
fn remove_msgmedia_temps(root: &Path) -> Result<()> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if is_msgmedia_temp(&path) {
                let _ = fs::remove_file(&path);
            }
        }
    }
    Ok(())
}

/// Delete ffmpeg scratch left beside `path` by an earlier interrupted run.
///
/// Matched on the exact name a transcode of `path` could have written (see
/// `temp_sibling`), not a stem prefix, and scoped to `path`'s own kind: a
/// given kind only ever writes one scratch extension (`jpg` for images,
/// `mp3` for audio, `mp4` for video). Precision here matters because two
/// source files can share a stem — an iOS Live Photo's `IMG_0001.HEIC` and
/// `IMG_0001.MOV`, for instance — and a coarser, stem-only match would delete
/// one file's in-flight scratch while converting the other.
///
/// `path` itself can never be swept: `classify` treats a `.msgmedia.tmp.`
/// path as having no kind, so a caller that passes scratch as `path` never
/// gets here with a kind to sweep by.
fn remove_temps_beside(path: &Path, kind: Kind) {
    let ext = match kind {
        Kind::Image => "jpg",
        Kind::Audio => "mp3",
        Kind::Video => "mp4",
    };
    let _ = fs::remove_file(temp_sibling(path, ext));
}

/// The temp path a conversion writes to next to `path`.
fn temp_sibling(path: &Path, ext: &str) -> PathBuf {
    path.with_extension(format!("msgmedia.tmp.{ext}"))
}

/// Run work that writes `tmp`. Deletes `tmp` on any error (success must rename it away).
fn with_temp_output<T>(tmp: &Path, f: impl FnOnce() -> Result<T>) -> Result<T> {
    match f() {
        Ok(v) => Ok(v),
        Err(err) => {
            let _ = fs::remove_file(tmp);
            Err(err)
        }
    }
}

/// Remux into MP4 without re-encoding and commit it; `None` when ffmpeg cannot remux the container.
fn try_remux_replace(path: &Path, commit: Commit<'_>) -> Result<Option<PathBuf>> {
    let tmp = temp_sibling(path, "mp4");
    if remux_mp4(path, &tmp).is_err() {
        let _ = fs::remove_file(&tmp);
        return Ok(None);
    }
    match commit_produced(commit, path, &tmp) {
        Ok(p) => Ok(Some(p)),
        Err(err) => {
            let _ = fs::remove_file(&tmp);
            Err(err)
        }
    }
}

/// Media [`Kind`] of a file, from its extension via the shared
/// extension table in `crate::mime`; `None` for unrecognized extensions
/// and for this crate's own in-progress temp files.
pub fn classify(path: &Path) -> Option<Kind> {
    if is_msgmedia_temp(path) {
        return None;
    }
    crate::mime::kind_for_ext(path.extension().and_then(|e| e.to_str()).unwrap_or(""))
}

/// Run the media step over one file, committing however `commit` says.
///
/// Returns the produced path, or `None` when the media step leaves this file
/// alone — either because the mode does not touch it, or because a same-format
/// re-encode came out no smaller (decision 44).
fn run_one(
    path: &Path,
    kind: Kind,
    mode: MediaMode,
    compress: &CompressOptions,
    commit: Commit<'_>,
) -> Result<Option<PathBuf>> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match (kind, mode) {
        (Kind::Image, MediaMode::Convert) => {
            // Keep GIF as-is (animation); jpg already in target form.
            if matches!(ext.as_str(), "jpg" | "jpeg" | "gif") {
                return Ok(None);
            }
            convert_image(path, false, false, commit)
        }
        (Kind::Image, MediaMode::Compress) => {
            if ext == "gif" {
                return Ok(None);
            }
            let same_format = matches!(ext.as_str(), "jpg" | "jpeg");
            if same_format && fs::metadata(path)?.len() <= JPEG_COMPRESS_FLOOR {
                return Ok(None);
            }
            convert_image(path, true, same_format, commit)
        }
        (Kind::Audio, MediaMode::Convert) => {
            if ext == "mp3" {
                return Ok(None);
            }
            convert_audio(path, false, false, commit)
        }
        (Kind::Audio, MediaMode::Compress) => {
            let same_format = ext == "mp3";
            if same_format && fs::metadata(path)?.len() <= MP3_COMPRESS_FLOOR {
                return Ok(None);
            }
            convert_audio(path, true, same_format, commit)
        }
        (Kind::Video, MediaMode::Convert) => convert_video(path, commit).map(Some),
        (Kind::Video, MediaMode::Compress) => compress_video(path, compress, commit),
        (_, MediaMode::Clone | MediaMode::Disabled) => Ok(None),
    }
}

/// Convert or compress one file by its media kind and report what changed.
fn process_one(
    output_dir: &Path,
    path: &Path,
    mode: MediaMode,
    compress: &CompressOptions,
) -> Result<Outcome> {
    let old_rel = rel_path(output_dir, path)?;
    let kind = classify(path).context("unknown media kind")?;
    match run_one(path, kind, mode, compress, Commit::InPlace)? {
        Some(new_path) => changed(output_dir, &old_rel, &new_path),
        None => Ok(Outcome::Skipped),
    }
}

/// What [`transcode_file`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscodeOutcome {
    /// Nothing was written: the mode does not touch this file, or a
    /// same-format re-encode came out no smaller.
    Skipped,
    /// A derivative was written to the destination the caller named.
    Produced,
}

/// File name the media step would produce for `src`, or `None` when it leaves
/// the file alone.
///
/// Reads the same decision tree as the pass itself, so the name a caller
/// patches into a conversation file is the name the pass writes when it does
/// write one. For video this is a forecast, not a promise: `derivative_name`
/// cannot see `CompressOptions`, so it always answers `mp4` for a video in
/// either mode, even though `compress_video` may skip a small or
/// already-efficient file and `try_remux_replace` may fall through on a
/// remux failure. Callers must treat [`TranscodeOutcome::Skipped`] from
/// [`transcode_file`] as authoritative over whatever this function predicted.
///
/// Stats `src` for the two size floors (compress-mode same-format JPEG/MP3).
/// When `src` may not exist on disk, use [`derivative_name_for_missing`]
/// instead — a stat failure here silently reads as size 0, which is under
/// both floors and answers `None`, the wrong answer for "is there a
/// candidate name to look for", not "is this live file worth touching".
#[must_use]
pub fn derivative_name(src: &Path, mode: MediaMode) -> Option<String> {
    derivative_name_impl(src, mode, |floor| {
        fs::metadata(src).map(|m| m.len()).unwrap_or(0) <= floor
    })
}

/// Same decision tree as [`derivative_name`], but never stats `src` — for a
/// recorded path already known to be missing from disk.
///
/// The two size floors exist to skip a small file that is still there to
/// measure; a missing file's size is unknowable and irrelevant to the
/// question this variant answers ("what name would a committed derivative of
/// this file carry, if one exists"), so both floors are treated as never
/// crossed and the candidate name is always produced. The caller is
/// expected to check the filesystem for that name itself.
#[must_use]
pub fn derivative_name_for_missing(src: &Path, mode: MediaMode) -> Option<String> {
    derivative_name_impl(src, mode, |_floor| false)
}

/// The file name a conversion would produce, or `None` when the file would be left as-is;
/// `under_floor` says whether a size is too small to bother with.
fn derivative_name_impl(
    src: &Path,
    mode: MediaMode,
    under_floor: impl Fn(u64) -> bool,
) -> Option<String> {
    let kind = classify(src)?;
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let stem = src.file_stem().and_then(|s| s.to_str())?;
    let target = match (kind, mode) {
        (_, MediaMode::Clone | MediaMode::Disabled) => return None,
        (Kind::Image, MediaMode::Convert) => {
            if matches!(ext.as_str(), "jpg" | "jpeg" | "gif") {
                return None;
            }
            "jpg"
        }
        (Kind::Image, MediaMode::Compress) => {
            if ext == "gif" {
                return None;
            }
            if matches!(ext.as_str(), "jpg" | "jpeg") && under_floor(JPEG_COMPRESS_FLOOR) {
                return None;
            }
            "jpg"
        }
        (Kind::Audio, MediaMode::Convert) => {
            if ext == "mp3" {
                return None;
            }
            "mp3"
        }
        (Kind::Audio, MediaMode::Compress) => {
            if ext == "mp3" && under_floor(MP3_COMPRESS_FLOOR) {
                return None;
            }
            "mp3"
        }
        // Forecast only: whether a video is actually rewritten depends on
        // CompressOptions and probed efficiency, neither visible here. See
        // the function doc.
        (Kind::Video, _) => "mp4",
    };
    Some(format!("{stem}.{target}"))
}

/// Transcode `src` and write the derivative to exactly `dest`.
///
/// `src` is never modified or deleted: committing is the caller's, because it
/// has to patch whatever points at the original first (decision 28). Scratch
/// left beside `src` by an interrupted run is cleared; scratch belonging to
/// other files, and any `.in_progress` marker, is left alone.
///
/// # Errors
///
/// Returns an error when ffmpeg/ffprobe are missing or fail, or IO fails.
pub fn transcode_file(
    src: &Path,
    dest: &Path,
    mode: MediaMode,
    compress: &CompressOptions,
) -> Result<TranscodeOutcome> {
    let kind = classify(src);
    // Clear this file's own scratch before checking the mode: an interrupted
    // run can leave scratch beside a file regardless of what mode retries it.
    if let Some(kind) = kind {
        remove_temps_beside(src, kind);
    }
    if matches!(mode, MediaMode::Clone | MediaMode::Disabled) {
        return Ok(TranscodeOutcome::Skipped);
    }
    let kind = kind.context("unknown media kind")?;
    transcode_as(src, kind, dest, mode, compress)
}

/// [`transcode_file`] for a source whose kind the caller already knows,
/// because the file carries no extension to read it from: the vault stores
/// originals under their SHA-256 alone, and knows the kind from the MIME type
/// the import declared. Use [`kind_for_mime`](crate::kind_for_mime) for that.
///
/// A source with no extension is never "already in the target format", so in
/// `Convert` mode every image becomes a JPEG, every video an MP4, and every
/// audio file an MP3; the same-format floors only apply when the extension
/// says so.
///
/// # Errors
///
/// Returns an error when ffmpeg/ffprobe are missing or fail, or IO fails.
pub fn transcode_file_as(
    src: &Path,
    kind: Kind,
    dest: &Path,
    mode: MediaMode,
    compress: &CompressOptions,
) -> Result<TranscodeOutcome> {
    remove_temps_beside(src, kind);
    if matches!(mode, MediaMode::Clone | MediaMode::Disabled) {
        return Ok(TranscodeOutcome::Skipped);
    }
    transcode_as(src, kind, dest, mode, compress)
}

/// The shared tail of the two `transcode_file` entry points.
fn transcode_as(
    src: &Path,
    kind: Kind,
    dest: &Path,
    mode: MediaMode,
    compress: &CompressOptions,
) -> Result<TranscodeOutcome> {
    require_ffmpeg()?;
    match run_one(src, kind, mode, compress, Commit::To(dest))? {
        Some(_) => Ok(TranscodeOutcome::Produced),
        None => Ok(TranscodeOutcome::Skipped),
    }
}

/// A `Changed` outcome naming the new relative path. Always Changed, even when the path
/// is the same: callers must refresh digests after an in-place rewrite.
fn changed(output_dir: &Path, old_rel: &str, new_path: &Path) -> Result<Outcome> {
    let new_rel = rel_path(output_dir, new_path)?;
    // Always report Changed — even when the relative path is unchanged (e.g. JPG
    // recompressed in place). Callers must invalidate digest_sha256 for remapped
    // paths; treating same-path rewrites as Skipped left stale fingerprints in
    // JSON Lines files and caused vault-push sha256 mismatches after upload.
    Ok(Outcome::Changed {
        old_rel: old_rel.to_string(),
        new_rel,
    })
}

/// The path under `output_dir` with forward slashes.
fn rel_path(output_dir: &Path, path: &Path) -> Result<String> {
    let rel = path
        .strip_prefix(output_dir)
        .with_context(|| format!("{} not under {}", path.display(), output_dir.display()))?;
    Ok(rel.to_string_lossy().replace('\\', "/"))
}

/// `path` with its extension replaced by `ext`; unchanged when that would be the same path.
fn sibling_with_ext(path: &Path, ext: &str) -> PathBuf {
    let stem = path.file_stem().unwrap_or_default();
    let mut dest = path.with_file_name(stem);
    dest.set_extension(ext);
    if dest == path {
        return dest;
    }
    if !dest.exists() {
        return dest;
    }
    // collision: stem_converted.ext
    let mut n = 1u32;
    loop {
        let name = format!("{}_{n}.{ext}", stem.to_string_lossy());
        let candidate = path.with_file_name(name);
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

/// Where a freshly produced derivative goes.
#[derive(Debug, Clone, Copy)]
enum Commit<'a> {
    /// Replace the original in place, deleting it. The directory pass's
    /// behaviour, unchanged.
    InPlace,
    /// Move the derivative to exactly this path and leave the original alone.
    ///
    /// The caller commits: it patches whatever points at the original, renames
    /// this file into its final name, and only then deletes the original
    /// (decision 28).
    To(&'a Path),
}

/// Move the produced file to where the commit mode says: over the original, or to a named destination.
fn commit_produced(commit: Commit<'_>, original: &Path, produced: &Path) -> Result<PathBuf> {
    match commit {
        Commit::InPlace => replace_original(original, produced),
        Commit::To(dest) => {
            if dest == original {
                // The whole point of Commit::To is that the final name never
                // exists until the caller has patched whatever points at the
                // original and renamed this derivative into place itself
                // (decision 28). A destination equal to the original would
                // overwrite it here, before any of that has happened — for
                // example `derivative_name` returning the source's own name
                // (a same-format compress) joined onto the source's directory
                // without a caller-added suffix like `.in_progress`.
                bail!(
                    "transcode destination {} is the original file: write the \
                     derivative to a distinct temporary name (e.g. suffixed with \
                     `.in_progress`) and rename it into place only after patching \
                     whatever points at {}",
                    dest.display(),
                    original.display()
                );
            }
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("create {}", parent.display()))?;
            }
            move_file(produced, dest)?;
            Ok(dest.to_path_buf())
        }
    }
}

/// Move `from` to `to`: a rename when both sit on one filesystem, else a copy
/// and a delete. The scratch file is written beside the source, and a caller's
/// destination (a temp folder, say) need not share a device with it.
fn move_file(from: &Path, to: &Path) -> Result<()> {
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    fs::copy(from, to).with_context(|| format!("copy {} to {}", from.display(), to.display()))?;
    fs::remove_file(from).with_context(|| format!("remove {}", from.display()))?;
    Ok(())
}

/// Is `produced` actually smaller than `original`?
///
/// Only meaningful for a same-format re-encode. Where the format changes the
/// user asked for the target format, and a smaller file in the source format
/// is not a substitute for it.
fn is_smaller(produced: &Path, original: &Path) -> Result<bool> {
    Ok(fs::metadata(produced)?.len() < fs::metadata(original)?.len())
}

/// Replace the original with the produced file, renaming to the new extension and removing the old file.
fn replace_original(original: &Path, produced: &Path) -> Result<PathBuf> {
    if produced == original {
        return Ok(original.to_path_buf());
    }
    let target_ext = produced
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin");
    let final_path = if original.extension().and_then(|e| e.to_str()) == Some(target_ext) {
        // overwrite same extension via temp
        let tmp = original.with_extension(format!("{target_ext}.tmp"));
        if tmp.exists() {
            let _ = fs::remove_file(&tmp);
        }
        fs::rename(produced, &tmp)?;
        let _ = fs::remove_file(original);
        fs::rename(&tmp, original)?;
        original.to_path_buf()
    } else {
        let dest = sibling_with_ext(original, target_ext);
        if dest.exists() && dest != produced {
            let _ = fs::remove_file(&dest);
        }
        fs::rename(produced, &dest)?;
        let _ = fs::remove_file(original);
        dest
    };
    Ok(final_path)
}

/// Convert an image to JPEG (compressing when asked), keeping the original when the result
/// is not smaller and `keep_smaller` is set.
fn convert_image(
    path: &Path,
    compress: bool,
    keep_smaller: bool,
    commit: Commit<'_>,
) -> Result<Option<PathBuf>> {
    let tmp = temp_sibling(path, "jpg");
    let quality = if compress { "5" } else { "2" }; // ffmpeg -q:v (2 best … 31 worst for mjpeg)
    // `-frames:v 1 -update 1`: animated GIF/WebP must write a single still, not an
    // image2 sequence (otherwise ffmpeg leaves a partial tmp and exits non-zero).
    let args = vec![
        "-y".into(),
        "-i".into(),
        path_str(path),
        "-frames:v".into(),
        "1".into(),
        "-update".into(),
        "1".into(),
        "-q:v".into(),
        quality.into(),
        path_str(&tmp),
    ];
    with_temp_output(&tmp, || {
        run_ffmpeg(&args).with_context(|| format!("convert image {}", path.display()))?;
        if keep_smaller && !is_smaller(&tmp, path)? {
            let _ = fs::remove_file(&tmp);
            return Ok(None);
        }
        commit_produced(commit, path, &tmp).map(Some)
    })
}

/// Convert audio to MP3 (compressing when asked), keeping the original when the result
/// is not smaller and `keep_smaller` is set.
fn convert_audio(
    path: &Path,
    compress: bool,
    keep_smaller: bool,
    commit: Commit<'_>,
) -> Result<Option<PathBuf>> {
    let tmp = temp_sibling(path, "mp3");
    let mut args = vec![
        "-y".into(),
        "-i".into(),
        path_str(path),
        "-vn".into(),
        "-acodec".into(),
        "libmp3lame".into(),
    ];
    if compress {
        args.extend(["-ac".into(), "1".into(), "-b:a".into(), "96k".into()]);
    } else {
        args.extend(["-q:a".into(), "4".into()]);
    }
    args.push(path_str(&tmp));
    with_temp_output(&tmp, || {
        run_ffmpeg(&args).with_context(|| format!("convert audio {}", path.display()))?;
        if keep_smaller && !is_smaller(&tmp, path)? {
            let _ = fs::remove_file(&tmp);
            return Ok(None);
        }
        commit_produced(commit, path, &tmp).map(Some)
    })
}

/// Convert a video to MP4: remux when the codecs allow, else re-encode.
fn convert_video(path: &Path, commit: Commit<'_>) -> Result<PathBuf> {
    let tmp = temp_sibling(path, "mp4");

    with_temp_output(&tmp, || {
        // Prefer remux into mp4 when already a video file.
        if remux_mp4(path, &tmp).is_ok() {
            return commit_produced(commit, path, &tmp);
        }
        let _ = fs::remove_file(&tmp);

        // Light standardize encode (H.264, 30fps, no aggressive downscale).
        let args = vec![
            "-y".into(),
            "-i".into(),
            path_str(path),
            "-vf".into(),
            "fps=30".into(),
            "-c:v".into(),
            "libx264".into(),
            "-crf".into(),
            "23".into(),
            "-preset".into(),
            "medium".into(),
            "-c:a".into(),
            "aac".into(),
            "-b:a".into(),
            "128k".into(),
            "-movflags".into(),
            "+faststart".into(),
            path_str(&tmp),
        ];
        run_ffmpeg(&args).with_context(|| format!("convert video {}", path.display()))?;
        commit_produced(commit, path, &tmp)
    })
}

/// Copy the streams into an MP4 container without re-encoding.
fn remux_mp4(path: &Path, tmp: &Path) -> Result<()> {
    let args = vec![
        "-y".into(),
        "-i".into(),
        path_str(path),
        "-c".into(),
        "copy".into(),
        "-movflags".into(),
        "+faststart".into(),
        path_str(tmp),
    ];
    run_ffmpeg(&args)
}

/// Leave an MP4 as it is; remux anything else into one, so every video the
/// vault keeps shares a container.
fn keep_or_remux(path: &Path, commit: Commit<'_>) -> Result<Option<PathBuf>> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "mp4" {
        return Ok(None);
    }
    try_remux_replace(path, commit)
}

/// Re-encode a video under the resolution, frame-rate, and quality caps; `None` when it is already within them.
fn compress_video(
    path: &Path,
    opts: &CompressOptions,
    commit: Commit<'_>,
) -> Result<Option<PathBuf>> {
    let meta = fs::metadata(path)?;
    if meta.len() < opts.min_size_bytes {
        // Still remux non-mp4 small files for container consistency.
        return keep_or_remux(path, commit);
    }

    // A probe that fails yields empty codec and zero dimensions, which never
    // count as efficient, so the file goes through compression like any other.
    let probe = probe_video(path).unwrap_or_default();
    if opts.skip_efficient
        && is_efficient(&probe.codec, probe.width, probe.height, probe.bitrate, opts)
    {
        return keep_or_remux(path, commit);
    }

    let max_edge = opts.max_resolution.max_long_edge();
    let fps = if opts.max_fps > 0.0 {
        opts.max_fps
    } else {
        30.0
    };
    let vf = format!(
        "scale='if(gt(iw,ih),min({max_edge},iw),-2)':'if(gt(iw,ih),-2,min({max_edge},ih))',fps={fps}"
    );
    let tmp = temp_sibling(path, "mp4");

    with_temp_output(&tmp, || {
        // Prefer libx265; fall back to libx264.
        let mut hevc_args = base_video_args(path, &tmp, &vf);
        hevc_args.extend([
            "-c:v".into(),
            "libx265".into(),
            "-crf".into(),
            "22".into(),
            "-preset".into(),
            "medium".into(),
            "-tag:v".into(),
            "hvc1".into(),
            "-c:a".into(),
            "aac".into(),
            "-b:a".into(),
            "128k".into(),
            "-movflags".into(),
            "+faststart".into(),
            path_str(&tmp),
        ]);
        if run_ffmpeg(&hevc_args).is_err() {
            let _ = fs::remove_file(&tmp);
            let mut avc_args = base_video_args(path, &tmp, &vf);
            avc_args.extend([
                "-c:v".into(),
                "libx264".into(),
                "-crf".into(),
                "28".into(),
                "-preset".into(),
                "medium".into(),
                "-c:a".into(),
                "aac".into(),
                "-b:a".into(),
                "96k".into(),
                "-movflags".into(),
                "+faststart".into(),
                path_str(&tmp),
            ]);
            run_ffmpeg(&avc_args).with_context(|| format!("compress video {}", path.display()))?;
        }
        Ok(Some(commit_produced(commit, path, &tmp)?))
    })
}

/// The ffmpeg arguments shared by every video re-encode: input, filter graph, and output settings.
fn base_video_args(path: &Path, _tmp: &Path, vf: &str) -> Vec<String> {
    vec![
        "-y".into(),
        "-i".into(),
        path_str(path),
        "-vf".into(),
        vf.into(),
    ]
}

/// Would `compress_video` skip re-encoding this stream and only remux it?
///
/// Takes plain fields rather than [`crate::tools::Probe`] so the size
/// forecast in `estimate.rs` (which has its own [`crate::MediaProbe`] from a
/// public ffprobe call, not this module's private `Probe`) can call the exact
/// predicate `compress_video` uses instead of copying its thresholds — one
/// place decides what counts as "already efficient enough."
pub(crate) fn is_efficient(
    codec: &str,
    width: u32,
    height: u32,
    bitrate: u64,
    opts: &CompressOptions,
) -> bool {
    let hevc = matches!(codec, "hevc" | "h265");
    if !hevc {
        return false;
    }
    let long = width.max(height);
    if long > opts.max_resolution.max_long_edge() {
        return false;
    }
    // ~12 Mbps threshold (archive-tools style)
    if bitrate > 12_000_000 {
        return false;
    }
    true
}

/// A path as a string for a command line.
fn path_str(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests;
