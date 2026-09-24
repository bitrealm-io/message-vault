//! Locate and run the external `wtsexporter` CLI.

use anyhow::{Context, Result, bail};
use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const PINNED_HINT: &str = "whatsapp-chat-exporter>=0.13";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Platform {
    Android,
    Ios,
}

impl Platform {
    /// The wtsexporter command-line flag for this platform.
    pub fn as_flag(self) -> &'static str {
        match self {
            Self::Android => "-a",
            Self::Ios => "-i",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct WtsexporterArgs {
    pub platform: Platform,
    /// Search root for relative defaults (`msgstore.db`, `wa.db`, …). Not the process cwd.
    pub input: PathBuf,
    /// Scratch directory for wtsexporter (media extract + JSON). Must outlive convert.
    pub work_dir: PathBuf,
    /// Key file path or crypt15 hex string (`-k`).
    pub key: Option<String>,
    pub backup: Option<PathBuf>,
    pub wa: Option<PathBuf>,
    pub media: Option<PathBuf>,
    pub db: Option<PathBuf>,
    pub business: bool,
}

/// Locate `wtsexporter` (the Python WhatsApp export tool this crate shells out to):
/// `WTSEXPORTER` → sibling of this exe → `cli/` next to the GUI →
/// legacy parent dir → `MESSAGE_VAULT_IO_BIN` → `PATH`.
///
/// # Errors
///
/// Returns an error when no usable binary is found.
pub(crate) fn resolve_wtsexporter() -> Result<PathBuf> {
    if let Some(explicit) = env::var_os("WTSEXPORTER") {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return Ok(path);
        }
        bail!(
            "WTSEXPORTER is set but not a file: {}. Install with \
             pip install '{PINNED_HINT}' or place the release binary in cli/ next to this tool.",
            path.display()
        );
    }

    let executable = if cfg!(windows) {
        "wtsexporter.exe"
    } else {
        "wtsexporter"
    };
    let mut tried = Vec::new();

    if let Ok(current) = env::current_exe()
        && let Some(dir) = current.parent()
    {
        let candidates = [
            dir.join(executable),
            dir.join("cli").join(executable),
            // Legacy flat-root archives.
            dir.parent().map(|p| p.join(executable)).unwrap_or_default(),
        ];
        for candidate in candidates {
            if candidate.as_os_str().is_empty() {
                continue;
            }
            tried.push(candidate.clone());
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }

    if let Some(extra) = env::var_os("MESSAGE_VAULT_IO_BIN") {
        let candidate = PathBuf::from(extra).join(executable);
        tried.push(candidate.clone());
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    if let Some(paths) = env::var_os("PATH") {
        for directory in env::split_paths(&paths) {
            let candidate = directory.join(executable);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }

    bail!(
        "Could not find {executable}. Install with: pip install '{PINNED_HINT}' \
         (or pip install 'whatsapp-chat-exporter[android_backup,crypt15]'), \
         put the KnugiHK release binary in cli/ next to this tool / in MESSAGE_VAULT_IO_BIN, \
         or set WTSEXPORTER. Tried: {}",
        tried
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
}

/// Run wtsexporter in `args.work_dir`; write JSON to `json_out`.
/// Returns stderr+stdout for logging.
///
/// # Errors
///
/// Returns an error when the work dir is missing, the process cannot start, or
/// wtsexporter exits with a non-zero status.
pub(crate) fn run_wtsexporter(
    bin: &Path,
    args: &WtsexporterArgs,
    json_out: &Path,
) -> Result<String> {
    if !args.work_dir.is_dir() {
        bail!("work dir does not exist: {}", args.work_dir.display());
    }
    let out_dir = json_out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(out_dir).with_context(|| format!("create {}", out_dir.display()))?;

    let output = wtsexporter_command(bin, args, out_dir, json_out)?
        .output()
        .map_err(|err| {
            let hint = if err.kind() == std::io::ErrorKind::NotFound {
                " (often a broken pipx/venv shim: the script exists but its Python interpreter does not — try `pipx reinstall whatsapp-chat-exporter` or set WTSEXPORTER to a working binary)"
            } else {
                ""
            };
            anyhow::anyhow!("spawn {}: {err}{hint}", bin.display())
        })?;
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        bail!(
            "wtsexporter failed ({}){}\n{}",
            output.status,
            if combined.trim().is_empty() { "" } else { ":" },
            combined.trim()
        );
    }
    if !json_out.is_file() {
        bail!(
            "wtsexporter finished but JSON missing at {}. Output:\n{}",
            json_out.display(),
            combined.trim()
        );
    }
    Ok(combined)
}

/// The wtsexporter command for `args`, writing media to `out_dir` and JSON
/// to `json_out`. A hex key is written to a file in the work dir here.
///
/// # Errors
///
/// Returns an error when a forwarded path cannot be resolved or the key
/// file cannot be written.
fn wtsexporter_command(
    bin: &Path,
    args: &WtsexporterArgs,
    out_dir: &Path,
    json_out: &Path,
) -> Result<Command> {
    let paths = resolve_forwarded_paths(args)?;
    let mut cmd = Command::new(bin);
    // Scratch cwd so iOS/Android extract does not pollute the GUI launch directory.
    cmd.current_dir(&args.work_dir)
        .arg(args.platform.as_flag())
        .arg("--no-html")
        .arg("--no-banner")
        .arg("-o")
        .arg(out_dir)
        .arg("-j")
        .arg(json_out)
        // wtsexporter uses tqdm; without this, progress bars spam piped capture
        // (GUI only shows the dump after the process exits).
        .env("TQDM_DISABLE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(db) = &paths.db {
        cmd.arg("-d").arg(db);
    }
    match paths.key.as_deref() {
        // Key file path — the path itself is not secret, forward as-is.
        Some(key) if looks_like_path(key) => {
            cmd.arg("-k").arg(key);
        }
        // Hex key material — write the decoded bytes to a 0600 file in the
        // scratch work dir and pass the path, so the secret never appears in
        // the process command line (/proc/<pid>/cmdline).
        Some(key) => {
            let key_path = write_key_file(&args.work_dir, key)?;
            cmd.arg("-k").arg(&key_path);
        }
        None => {}
    }
    push_opt(&mut cmd, "-b", paths.backup.as_deref());
    push_opt(&mut cmd, "-w", paths.wa.as_deref());
    push_opt(&mut cmd, "-m", paths.media.as_deref());
    if args.business {
        cmd.arg("--business");
    }
    // Never pass `-c` (--move-media): wtsexporter would shutil.move the user's
    // media directory into the scratch work dir, which is deleted when the run
    // finishes — permanently destroying the original media. Always copy.
    Ok(cmd)
}

struct ForwardedPaths {
    key: Option<String>,
    backup: Option<PathBuf>,
    wa: Option<PathBuf>,
    media: Option<PathBuf>,
    db: Option<PathBuf>,
}

/// Absolutize user paths and fill Android/iOS defaults from `input` when missing.
fn resolve_forwarded_paths(args: &WtsexporterArgs) -> Result<ForwardedPaths> {
    let search = input_search_root(&args.input)?;

    let db = match &args.db {
        Some(p) => Some(absolutize(p)?),
        None if args.input.is_file() => Some(absolutize(&args.input)?),
        None => first_existing(&[
            search.join("msgstore.db"),
            search.join("ChatStorage.sqlite"),
        ]),
    };

    let wa = match &args.wa {
        Some(p) => Some(absolutize(p)?),
        None => first_existing(&[
            search.join("wa.db"),
            search.join("ContactsV2.sqlite"),
            search.join("AppDomainGroup-group.net.whatsapp.WhatsApp.shared/ContactsV2.sqlite"),
            search.join("AppDomainGroup-group.net.whatsapp.WhatsAppSMB.shared/ContactsV2.sqlite"),
        ]),
    };

    let media = match &args.media {
        Some(p) => Some(absolutize(p)?),
        None => first_existing(&[
            search.join("WhatsApp"),
            search.join("AppDomainGroup-group.net.whatsapp.WhatsApp.shared"),
            search.join("AppDomainGroup-group.net.whatsapp.WhatsAppSMB.shared"),
        ]),
    };

    let backup = match &args.backup {
        Some(p) => Some(absolutize(p)?),
        None if args.platform == Platform::Android => android_crypt_backup(&search),
        None => None,
    };

    // Do not pass `-k` when no backup is forwarded.
    let key = if backup.is_none() {
        None
    } else {
        match args.key.as_deref().and_then(message_ir::trimmed) {
            Some(k) if looks_like_path(k) => {
                let path = absolutize(Path::new(k))?;
                Some(path.to_string_lossy().into_owned())
            }
            Some(k) => Some(k.to_string()),
            None => None,
        }
    };

    Ok(ForwardedPaths {
        key,
        backup,
        wa,
        media,
        db,
    })
}

/// Directory used to resolve relative wtsexporter defaults (`msgstore.db`, and similar).
///
/// # Errors
///
/// Returns an error when `input` does not exist.
fn input_search_root(input: &Path) -> Result<PathBuf> {
    if input.is_dir() {
        return absolutize(input);
    }
    if input.is_file() {
        let parent = input
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        return absolutize(parent);
    }
    bail!("input path does not exist: {}", input.display());
}

/// First candidate path that exists on disk.
fn first_existing(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.iter().find(|p| p.exists()).cloned()
}

/// Android crypt file in `search` for `-b`, or `None` so wtsexporter defaults apply.
///
/// Prefers a decrypted `msgstore.db` file over any crypt name. Crypt names are
/// checked at the folder root only, in order: crypt12, crypt14, crypt15.
/// Directories with those names are ignored (`is_file`), matching the form probe.
pub(crate) fn android_crypt_backup(search: &Path) -> Option<PathBuf> {
    if search.join("msgstore.db").is_file() {
        return None;
    }
    [
        search.join("msgstore.db.crypt12"),
        search.join("msgstore.db.crypt14"),
        search.join("msgstore.db.crypt15"),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

/// True when `s` looks like a filesystem path rather than a hex key string.
fn looks_like_path(s: &str) -> bool {
    s.contains('/') || s.contains('\\') || s.ends_with(".key") || Path::new(s).exists()
}

/// Make `path` absolute relative to the current working directory.
///
/// # Errors
///
/// Returns an error when the current working directory cannot be read.
fn absolutize(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    let cwd = env::current_dir().context("resolve current working directory")?;
    Ok(cwd.join(path))
}

/// Append `flag` and `path` to `cmd` when `path` is `Some`.
fn push_opt(cmd: &mut Command, flag: &str, path: Option<&Path>) {
    if let Some(p) = path {
        cmd.arg(flag).arg(p);
    }
}

/// Write hex-encoded decryption key bytes to a 0600 file in the scratch work dir.
///
/// wtsexporter's `-k` accepts a hex string or a key file path (there is no
/// stdin key support upstream), so the file path is forwarded instead of the
/// hex string itself. The file lives in the disposable scratch dir and is
/// removed with it when the run finishes.
///
/// # Errors
///
/// Returns an error when the hex is invalid or the file cannot be written.
fn write_key_file(work_dir: &Path, hex_key: &str) -> Result<PathBuf> {
    let cleaned: String = hex_key.chars().filter(|c| !c.is_whitespace()).collect();
    // Deliberately do not echo the key material in the error message.
    let raw =
        hex::decode(&cleaned).with_context(|| "decryption key is not a hex string".to_string())?;
    let path = work_dir.join("decryption.key");
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts
        .open(&path)
        .with_context(|| format!("create {}", path.display()))?;
    file.write_all(&raw)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::{
        Platform, WtsexporterArgs, android_crypt_backup, resolve_forwarded_paths,
        wtsexporter_command,
    };
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    fn android_args(input: &Path, key: Option<&str>) -> WtsexporterArgs {
        WtsexporterArgs {
            platform: Platform::Android,
            input: input.to_path_buf(),
            work_dir: input.to_path_buf(),
            key: key.map(str::to_string),
            backup: None,
            wa: None,
            media: None,
            db: None,
            business: false,
        }
    }

    #[test]
    fn prefers_msgstore_db_over_crypt() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("msgstore.db"), b"db").unwrap();
        fs::write(dir.path().join("msgstore.db.crypt15"), b"crypt").unwrap();
        assert_eq!(android_crypt_backup(dir.path()), None);
    }

    #[test]
    fn finds_crypt15_when_msgstore_missing() {
        let dir = tempdir().unwrap();
        let crypt = dir.path().join("msgstore.db.crypt15");
        fs::write(&crypt, b"crypt").unwrap();
        assert_eq!(
            android_crypt_backup(dir.path()).as_deref(),
            Some(crypt.as_path())
        );
    }

    #[test]
    fn prefers_crypt12_over_crypt15() {
        let dir = tempdir().unwrap();
        let crypt12 = dir.path().join("msgstore.db.crypt12");
        fs::write(&crypt12, b"c12").unwrap();
        fs::write(dir.path().join("msgstore.db.crypt15"), b"c15").unwrap();
        assert_eq!(
            android_crypt_backup(dir.path()).as_deref(),
            Some(crypt12.as_path())
        );
    }

    #[test]
    fn ignores_crypt15_directory() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("msgstore.db.crypt15")).unwrap();
        assert_eq!(android_crypt_backup(dir.path()), None);
    }

    #[test]
    fn drops_key_when_msgstore_db_is_present() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("msgstore.db"), b"db").unwrap();
        fs::write(dir.path().join("msgstore.db.crypt15"), b"crypt").unwrap();
        let paths = resolve_forwarded_paths(&android_args(dir.path(), Some("deadbeef"))).unwrap();
        assert!(paths.backup.is_none());
        assert!(paths.key.is_none());
    }

    #[test]
    fn forwards_crypt15_and_key_when_msgstore_missing() {
        let dir = tempdir().unwrap();
        let crypt = dir.path().join("msgstore.db.crypt15");
        fs::write(&crypt, b"crypt").unwrap();
        let paths = resolve_forwarded_paths(&android_args(dir.path(), Some("deadbeef"))).unwrap();
        assert_eq!(paths.backup.as_deref(), Some(crypt.as_path()));
        assert_eq!(paths.key.as_deref(), Some("deadbeef"));
    }

    #[test]
    fn forwards_crypt15_without_key_when_key_omitted() {
        let dir = tempdir().unwrap();
        let crypt = dir.path().join("msgstore.db.crypt15");
        fs::write(&crypt, b"crypt").unwrap();
        let paths = resolve_forwarded_paths(&android_args(dir.path(), None)).unwrap();
        assert_eq!(paths.backup.as_deref(), Some(crypt.as_path()));
        assert!(paths.key.is_none());
    }

    /// The arguments of the command built for `args`, as strings.
    fn command_args(args: &WtsexporterArgs, out_dir: &Path, json_out: &Path) -> Vec<String> {
        let cmd = wtsexporter_command(Path::new("wtsexporter"), args, out_dir, json_out).unwrap();
        assert_eq!(cmd.get_current_dir(), Some(args.work_dir.as_path()));
        cmd.get_args()
            .map(|arg| arg.to_str().unwrap().to_string())
            .collect()
    }

    fn text(path: &Path) -> String {
        path.to_str().unwrap().to_string()
    }

    /// An Android backup with every option found: an encrypted database,
    /// a hex key (passed as a key file, never on the command line),
    /// contacts, media, and the business app. The whole command line is
    /// pinned, so a flag added in any form (`-c`, which moves the user's
    /// media into the scratch folder, above all) fails here.
    #[test]
    fn an_android_command_forwards_every_found_path_and_nothing_else() {
        let dir = tempdir().unwrap();
        let crypt = dir.path().join("msgstore.db.crypt15");
        fs::write(&crypt, b"crypt").unwrap();
        let wa = dir.path().join("wa.db");
        fs::write(&wa, b"wa").unwrap();
        let media = dir.path().join("WhatsApp");
        fs::create_dir(&media).unwrap();
        let work = tempdir().unwrap();
        let out = work.path().join("out");
        let json = out.join("result.json");
        let mut args = android_args(dir.path(), Some("deadbeef"));
        args.work_dir = work.path().to_path_buf();
        args.business = true;

        let key_file = work.path().join("decryption.key");
        assert_eq!(
            command_args(&args, &out, &json),
            [
                "-a".to_string(),
                "--no-html".to_string(),
                "--no-banner".to_string(),
                "-o".to_string(),
                text(&out),
                "-j".to_string(),
                text(&json),
                "-k".to_string(),
                text(&key_file),
                "-b".to_string(),
                text(&crypt),
                "-w".to_string(),
                text(&wa),
                "-m".to_string(),
                text(&media),
                "--business".to_string(),
            ]
        );
        assert_eq!(fs::read(&key_file).unwrap(), [0xde, 0xad, 0xbe, 0xef]);
    }

    /// A key given as a file path is passed to wtsexporter as that path,
    /// made absolute, rather than read as hex key material. A path is told
    /// from hex by a slash or by the `.key` ending.
    #[test]
    fn a_key_file_path_is_forwarded_as_a_path() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("msgstore.db.crypt15"), b"crypt").unwrap();
        let work = tempdir().unwrap();
        let out = work.path().join("out");
        let json = out.join("result.json");
        let cwd = std::env::current_dir().unwrap();

        let absolute = dir.path().join("backup.key");
        for (key, expected) in [
            (text(&absolute), absolute.clone()),
            ("x.key".to_string(), cwd.join("x.key")),
            ("keys/key".to_string(), cwd.join("keys/key")),
        ] {
            let mut args = android_args(dir.path(), Some(&key));
            args.work_dir = work.path().to_path_buf();
            let command = command_args(&args, &out, &json);
            let k = command.iter().position(|a| a == "-k").expect("a -k flag");
            assert_eq!(command[k + 1], text(&expected), "{key}");
            assert!(
                !work.path().join("decryption.key").exists(),
                "{key} was read as hex"
            );
        }
    }

    /// A database file chosen as the input is passed with `-d`, whatever it
    /// is called.
    #[test]
    fn a_database_file_given_as_the_input_is_passed_as_the_database() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("phone-msgstore.db");
        fs::write(&db, b"db").unwrap();
        let mut args = android_args(&db, None);
        args.work_dir = dir.path().to_path_buf();
        let out = dir.path().join("out");
        let command = command_args(&args, &out, &out.join("result.json"));
        let d = command.iter().position(|a| a == "-d").expect("a -d flag");
        assert_eq!(command[d + 1], text(&db));
    }

    /// Only Android has crypt backup files, so an iOS folder that happens to
    /// hold one passes no `-b`.
    #[test]
    fn an_ios_folder_forwards_no_android_backup() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("msgstore.db.crypt15"), b"crypt").unwrap();
        let args = WtsexporterArgs {
            platform: Platform::Ios,
            ..android_args(dir.path(), None)
        };
        let paths = resolve_forwarded_paths(&args).unwrap();
        assert!(paths.backup.is_none());
    }

    /// An iOS backup forwards its database, contacts and media found under
    /// the input folder; with no backup file there is no `-b`, so the key
    /// is dropped too.
    #[test]
    fn an_ios_command_forwards_the_found_database_contacts_and_media() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("ChatStorage.sqlite");
        fs::write(&db, b"db").unwrap();
        let shared = dir
            .path()
            .join("AppDomainGroup-group.net.whatsapp.WhatsApp.shared");
        fs::create_dir(&shared).unwrap();
        let contacts = shared.join("ContactsV2.sqlite");
        fs::write(&contacts, b"contacts").unwrap();
        let out = dir.path().join("out");
        let json = out.join("result.json");
        let args = WtsexporterArgs {
            platform: Platform::Ios,
            key: Some("deadbeef".to_string()),
            ..android_args(dir.path(), None)
        };

        assert_eq!(
            command_args(&args, &out, &json),
            [
                "-i".to_string(),
                "--no-html".to_string(),
                "--no-banner".to_string(),
                "-o".to_string(),
                text(&out),
                "-j".to_string(),
                text(&json),
                "-d".to_string(),
                text(&db),
                "-w".to_string(),
                text(&contacts),
                "-m".to_string(),
                text(&shared),
            ]
        );
    }
}
