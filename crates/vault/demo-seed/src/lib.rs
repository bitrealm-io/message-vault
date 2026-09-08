//! Builds a demo message dataset with three backup sources.
//!
//! Each conversation is a JSON Lines file: one JSON object per line. The three
//! folders under `staging/` look like separate phone backups (iMessage, Android
//! SMS Backup & Restore, and WhatsApp).

mod assets;
mod config;
mod contacts;
mod conversations;
mod corpus;
mod names;
mod personas;
mod phones;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

pub use config::SeedConfig;
pub use conversations::GenStats;

const IMESSAGE_SOURCE: &str = "imessage";
const SBR_SOURCE: &str = "sms-backup-restore";
const WHATSAPP_SOURCE: &str = "whatsapp";
const GENERATED_PATHS: [&str; 3] = ["staging", "config", "README.md"];

/// Turn `total * fraction` into a whole number that still fits in `0..=total`.
fn rounded_fraction(total: usize, fraction: f64) -> usize {
    let count = (total as f64) * fraction;
    count.round().clamp(0.0, total as f64) as usize
}

/// Build the demo dataset under `cfg.out`.
///
/// New files are written in a temporary directory next to the destination.
/// After they look valid, `staging`, `config`, and `README.md` are moved into
/// place. If that move fails partway through, the previous copies are moved
/// back.
///
/// # Errors
///
/// Returns an error if a directory cannot be created, a file cannot be written,
/// the new files fail a check, or they cannot replace the old ones.
pub fn generate(cfg: &SeedConfig) -> Result<GenStats> {
    let out = Path::new(&cfg.out);
    let parent = output_parent_dir(out);
    fs::create_dir_all(parent)
        .with_context(|| format!("create demo output parent {}", parent.display()))?;

    // Write the new bundle in a temporary directory next to the destination.
    // After the new files look valid, they are moved into place. If that move
    // fails partway through, the previous staging, config, and README files
    // can be moved back.
    let prepared = tempfile::Builder::new()
        .prefix(".demo-seed-")
        .tempdir_in(parent)
        .with_context(|| format!("create temporary demo bundle beside {}", out.display()))?;
    let replacement = prepare_and_replace(out, prepared.path(), |root| generate_into(cfg, root));
    let stats = match replacement {
        Ok(stats) => stats,
        Err(error) => return Err(keep_prepared_if_restore_failed(prepared, error)),
    };

    println!("demo-seed: wrote {}", out.display());
    println!("  seed:          {}", cfg.seed);
    println!("  contacts:      {}", stats.contacts);
    println!("  groups:        {}", stats.groups);
    println!("  conversations: {}", stats.conversation_files);
    println!("  messages:      {}", stats.messages);
    println!("  attachments:   {}", stats.attachment_refs);
    Ok(stats)
}

/// Parent directory of `out`, or `.` when `out` has no parent (for example `demo`).
fn output_parent_dir(out: &Path) -> &Path {
    match out.parent() {
        Some(path) if !path.as_os_str().is_empty() => path,
        _ => Path::new("."),
    }
}

/// If putting the new files in place failed and the previous copies are still
/// sitting in the temp directory, leave that directory on disk so nothing is
/// lost. Otherwise let the temp directory be deleted as usual.
fn keep_prepared_if_restore_failed(
    prepared: tempfile::TempDir,
    error: anyhow::Error,
) -> anyhow::Error {
    let previous_copies = prepared.path().join(".previous-active");
    if !previous_copies.exists() {
        return error;
    }
    let kept = prepared.keep();
    error.context(format!(
        "Could not restore the previous demo files. The prepared output and previous copies were left at {}",
        kept.display()
    ))
}

/// Write contacts, conversations, attachments, and README into `out`.
///
/// # Errors
///
/// Returns an error if a directory or file cannot be created, or if a name list
/// or message-text file cannot be loaded.
fn generate_into(cfg: &SeedConfig, out: &Path) -> Result<GenStats> {
    let mut rng = ChaCha8Rng::seed_from_u64(cfg.seed);

    let imessage_staging = out.join("staging").join(IMESSAGE_SOURCE);
    let sbr_staging = out.join("staging").join(SBR_SOURCE);
    let whatsapp_staging = out.join("staging").join(WHATSAPP_SOURCE);
    let imessage_attachments = imessage_staging.join("attachments");
    let sbr_attachments = sbr_staging.join("attachments");
    let whatsapp_attachments = whatsapp_staging.join("attachments");
    let config_dir = out.join("config");

    fs::create_dir_all(&imessage_staging)?;
    fs::create_dir_all(&sbr_staging)?;
    fs::create_dir_all(&whatsapp_staging)?;
    fs::create_dir_all(&imessage_attachments)?;
    fs::create_dir_all(&sbr_attachments)?;
    fs::create_dir_all(&whatsapp_attachments)?;
    fs::create_dir_all(&config_dir)?;

    let corpus =
        corpus::Corpus::load_pride_and_prejudice().context("load public-domain message corpus")?;
    let names = names::NameBank::load_default().context("load name lists")?;

    let attachment_digests = assets::write_attachment_blobs(&imessage_attachments)?;
    // Copy the same attachment files into the Android and WhatsApp folders so
    // those conversations can point at the same relative paths.
    copy_dir_files(&imessage_attachments, &sbr_attachments)?;
    copy_dir_files(&imessage_attachments, &whatsapp_attachments)?;

    let roster = personas::build_roster(cfg, &names, &mut rng)?;
    contacts::write_vcf(&config_dir, &roster)?;
    contacts::write_config_toml(&config_dir)?;
    contacts::write_seed_toml(&config_dir)?;

    let staging = conversations::StagingDirs {
        imessage: &imessage_staging,
        sbr: &sbr_staging,
        whatsapp: &whatsapp_staging,
    };
    let stats = conversations::write_all(
        &staging,
        &roster,
        cfg,
        &corpus,
        &mut rng,
        &attachment_digests,
    )?;

    write_readme(out, &stats, cfg, corpus.len())?;

    Ok(stats)
}

/// Run `prepare` in `prepared`, check the result, then move it over `active`.
///
/// # Errors
///
/// Returns an error if the two paths are the same, preparation fails, the new
/// files are incomplete, or the move cannot finish.
fn prepare_and_replace<F>(active: &Path, prepared: &Path, prepare: F) -> Result<GenStats>
where
    F: FnOnce(&Path) -> Result<GenStats>,
{
    if active == prepared {
        anyhow::bail!("active and prepared demo roots must differ");
    }
    let stats = prepare(prepared)?;
    validate_generated_bundle(prepared)?;
    replace_generated_paths(active, prepared)?;
    Ok(stats)
}

/// Check that the three backup folders and the expected config files exist.
///
/// # Errors
///
/// Returns an error if a required folder or file is missing, or if a JSON Lines
/// file cannot be read as JSON.
fn validate_generated_bundle(root: &Path) -> Result<()> {
    for source in [IMESSAGE_SOURCE, SBR_SOURCE, WHATSAPP_SOURCE] {
        let staging = root.join("staging").join(source);
        if !staging.is_dir() {
            anyhow::bail!("prepared demo bundle is missing {}", staging.display());
        }
    }
    for relative in [
        Path::new("config/config.toml"),
        Path::new("config/seed.toml"),
        Path::new("config/contacts.vcf"),
        Path::new("README.md"),
    ] {
        let path = root.join(relative);
        if !path.is_file() {
            anyhow::bail!("prepared demo bundle is missing {}", path.display());
        }
    }
    validate_tree_files(root)
}

/// Walk every file under `root`. JSON Lines files must parse as JSON, one object per line.
///
/// # Errors
///
/// Returns an error if a directory cannot be listed, a file cannot be read, or a
/// JSON Lines line is not valid JSON.
fn validate_tree_files(root: &Path) -> Result<()> {
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)
            .with_context(|| format!("read prepared directory {}", directory.display()))?
        {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let bytes = fs::read(&path)
                .with_context(|| format!("read prepared demo file {}", path.display()))?;
            if !is_jsonl_file(&path) {
                continue;
            }
            let text = std::str::from_utf8(&bytes)
                .with_context(|| format!("decode prepared JSONL {}", path.display()))?;
            for (index, line) in text.lines().enumerate() {
                serde_json::from_str::<serde_json::Value>(line)
                    .with_context(|| format!("parse {} line {}", path.display(), index + 1))?;
            }
        }
    }
    Ok(())
}

/// True when `path` ends in `.jsonl`.
fn is_jsonl_file(path: &Path) -> bool {
    match path.extension() {
        Some(extension) => extension == "jsonl",
        None => false,
    }
}

/// Move `staging`, `config`, and `README.md` from `prepared` onto `active`.
///
/// Existing copies are set aside first so they can be moved back if the new
/// files cannot be installed.
///
/// # Errors
///
/// Returns an error if a rename fails. If the previous files cannot be fully
/// restored, they are left in the backup folder and the error says so.
fn replace_generated_paths(active: &Path, prepared: &Path) -> Result<()> {
    replace_generated_paths_with(active, prepared, move_path)
}

/// Move `source` onto `destination`: a rename, or, when the paths sit on
/// different mounts (`EXDEV` / `ErrorKind::CrossesDevices`), a copy then a
/// delete of the source. Docker BuildKit overlay layers trigger that error
/// when the demo bundle or the reset-demo work tree moves into place.
///
/// # Errors
///
/// Returns an error if neither rename nor copy-then-remove can finish.
pub fn move_path(source: &Path, destination: &Path) -> Result<()> {
    move_path_with(source, destination, |from, to| fs::rename(from, to))
}

/// Same as [`move_path`], but uses `rename` so tests can return
/// `ErrorKind::CrossesDevices` without two real filesystems.
///
/// # Errors
///
/// Returns an error if `rename` fails for a reason other than a cross-device
/// move, or if the copy-then-remove fallback cannot finish.
pub fn move_path_with<F>(source: &Path, destination: &Path, rename: F) -> Result<()>
where
    F: FnOnce(&Path, &Path) -> io::Result<()>,
{
    match rename(source, destination) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::CrossesDevices => {
            move_across_devices(source, destination).with_context(|| {
                format!(
                    "copy {} to {} after a cross-device rename",
                    source.display(),
                    destination.display()
                )
            })
        }
        Err(error) => Err(error)
            .with_context(|| format!("rename {} to {}", source.display(), destination.display())),
    }
}

/// Copy `source` onto `destination`, then delete `source`.
///
/// # Errors
///
/// Returns an error if a directory cannot be created, a file cannot be copied,
/// or the source cannot be removed.
fn move_across_devices(source: &Path, destination: &Path) -> Result<()> {
    if source.is_dir() {
        copy_dir_recursive(source, destination)?;
        fs::remove_dir_all(source)
            .with_context(|| format!("remove copied directory {}", source.display()))?;
    } else {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create parent {}", parent.display()))?;
        }
        fs::copy(source, destination)
            .with_context(|| format!("copy {} to {}", source.display(), destination.display()))?;
        fs::remove_file(source)
            .with_context(|| format!("remove copied file {}", source.display()))?;
    }
    Ok(())
}

/// Copy every file and subdirectory under `source` into `destination`.
///
/// # Errors
///
/// Returns an error if a directory cannot be created or a file cannot be copied.
fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination).with_context(|| format!("create {}", destination.display()))?;
    for entry in fs::read_dir(source).with_context(|| format!("read {}", source.display()))? {
        let entry = entry?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            fs::copy(&from, &to)
                .with_context(|| format!("copy {} to {}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

/// Same as [`replace_generated_paths`], but uses `rename` so tests can fail a move on purpose.
///
/// # Errors
///
/// Returns an error if `rename` fails. Tries to put the previous files back.
fn replace_generated_paths_with<F>(active: &Path, prepared: &Path, mut rename: F) -> Result<()>
where
    F: FnMut(&Path, &Path) -> Result<()>,
{
    fs::create_dir_all(active)
        .with_context(|| format!("create active demo root {}", active.display()))?;
    let backup = prepared.join(".previous-active");
    fs::create_dir(&backup)
        .with_context(|| format!("create demo replacement backup {}", backup.display()))?;

    let mut backed_up = Vec::<PathBuf>::new();
    let mut installed = Vec::<PathBuf>::new();
    let replacement = install_generated_paths(
        active,
        prepared,
        &backup,
        &mut rename,
        &mut backed_up,
        &mut installed,
    );

    if let Err(error) = replacement {
        return restore_previous_paths(active, &backup, &mut rename, &backed_up, &installed, error);
    }

    if let Err(cleanup_error) = fs::remove_dir_all(&backup) {
        eprintln!(
            "warning: installed the generated demo bundle but could not remove backup {}: {cleanup_error}",
            backup.display()
        );
    }
    Ok(())
}

/// Set aside the current `staging`, `config`, and `README.md`, then move the new copies in.
///
/// # Errors
///
/// Returns an error if `rename` fails for any of those paths.
fn install_generated_paths<F>(
    active: &Path,
    prepared: &Path,
    backup: &Path,
    rename: &mut F,
    backed_up: &mut Vec<PathBuf>,
    installed: &mut Vec<PathBuf>,
) -> Result<()>
where
    F: FnMut(&Path, &Path) -> Result<()>,
{
    for name in GENERATED_PATHS {
        let destination = active.join(name);
        if !destination.exists() {
            continue;
        }
        let backup_path = backup.join(name);
        rename(&destination, &backup_path).with_context(|| {
            format!(
                "move existing demo path {} into backup",
                destination.display()
            )
        })?;
        backed_up.push(PathBuf::from(name));
    }

    for name in GENERATED_PATHS {
        let source = prepared.join(name);
        let destination = active.join(name);
        rename(&source, &destination).with_context(|| {
            format!(
                "install prepared demo path {} at {}",
                source.display(),
                destination.display()
            )
        })?;
        installed.push(PathBuf::from(name));
    }
    Ok(())
}

/// Remove the new files that were installed, then move the previous copies back.
///
/// Every restore step is attempted even if an earlier one fails. If any step
/// fails, the backup folder is left on disk.
///
/// # Errors
///
/// Always returns `error`, with extra context if the previous files could not
/// all be restored.
fn restore_previous_paths<F>(
    active: &Path,
    backup: &Path,
    rename: &mut F,
    backed_up: &[PathBuf],
    installed: &[PathBuf],
    error: anyhow::Error,
) -> Result<()>
where
    F: FnMut(&Path, &Path) -> Result<()>,
{
    let mut restore_errors = Vec::new();
    for name in installed.iter().rev() {
        let installed_path = active.join(name);
        if let Err(restore_error) = remove_path_if_exists(&installed_path) {
            restore_errors.push(format!(
                "remove installed {}: {restore_error:#}",
                installed_path.display()
            ));
        }
    }
    for name in backed_up.iter().rev() {
        let previous_path = backup.join(name);
        let restore_path = active.join(name);
        if let Err(restore_error) = rename(&previous_path, &restore_path) {
            restore_errors.push(format!(
                "restore previous demo path {}: {restore_error:#}",
                restore_path.display()
            ));
        }
    }
    if restore_errors.is_empty() {
        if let Err(cleanup_error) = fs::remove_dir_all(backup) {
            eprintln!(
                "warning: restored the previous demo bundle but could not remove backup {}: {cleanup_error}",
                backup.display()
            );
        }
        return Err(error.context("replace generated demo bundle"));
    }
    Err(anyhow::anyhow!(
        "replace generated demo bundle: {error:#}; could not fully restore the previous files; copies were kept at {}: {}",
        backup.display(),
        restore_errors.join("; ")
    ))
}

/// Delete `path` if it exists. Directories are removed with their contents.
///
/// # Errors
///
/// Returns an error if the file or directory cannot be deleted.
fn remove_path_if_exists(path: &Path) -> Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path).with_context(|| format!("remove {}", path.display()))?;
    } else if path.exists() {
        fs::remove_file(path).with_context(|| format!("remove {}", path.display()))?;
    }
    Ok(())
}

/// Load the settings at `seed_file`, then generate into `out`.
///
/// The seed and every other setting come from the file. `reset-demo` calls
/// this with the checked-in `demo_seed.toml`.
///
/// # Errors
///
/// Returns an error if the settings file cannot be read, `out` is not valid
/// UTF-8, or generation fails.
pub fn generate_to(seed_file: &Path, out: &Path) -> Result<GenStats> {
    let mut cfg = SeedConfig::load(seed_file)?;
    cfg.out = out
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("demo out path is not UTF-8: {}", out.display()))?
        .to_string();
    generate(&cfg)
}

/// Copy each file in `from` into `to`. Subdirectories are skipped.
///
/// # Errors
///
/// Returns an error if a directory cannot be listed or a file cannot be copied.
fn copy_dir_files(from: &Path, to: &Path) -> Result<()> {
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            let name = entry.file_name();
            fs::copy(&path, to.join(&name))
                .with_context(|| format!("copy {} → {}", path.display(), to.display()))?;
        }
    }
    Ok(())
}

/// Write `README.md` with counts and how to regenerate the dataset.
///
/// # Errors
///
/// Returns an error if the file cannot be written.
fn write_readme(
    out: &Path,
    stats: &GenStats,
    cfg: &SeedConfig,
    corpus_sentences: usize,
) -> Result<()> {
    let path = out.join("README.md");
    let body = format!(
        r"# Message Vault demo dataset

Generated message-ir JSONL bundle for local browsing without a real phone backup.
`staging/` is written by `demo-seed` / `reset-demo` and is not stored in git.

Three staging trees simulate separate backups:

- `staging/imessage/` — Apple Messages-style export
- `staging/sms-backup-restore/` — Android SMS Backup & Restore–style export
- `staging/whatsapp/` — WhatsApp-style export for ~{whatsapp_pct}% of contacts (same phone, platform `whatsapp`)

Most conversations are single-source. A small set appears in both iMessage and Android so the
Sources panel and cross-source dedupe can be exercised. WhatsApp threads share the phone number
with Text message handles so the contact drawer shows both platforms.

Regenerate + import in one step:

```bash
cargo run --release -p message-vault-server -- reset-demo
```

Or regenerate the bundle only:

```bash
cargo run -p demo-seed
```

Config knobs live in `crates/vault/demo-seed/demo_seed.toml` (seed, contact count, rate/span
distributions, group membership, dual-source split, `whatsapp_contact_fraction`,
`apple_fallback_transport_fraction`). Message bodies are sampled from Pride and
Prejudice ({corpus_sentences} sentences) under `crates/vault/demo-seed/data/corpus/`. Names come from
`crates/vault/demo-seed/data/names/`.

## Contents (seed {seed})

| Item | Count |
|------|------:|
| Contacts (VCF) | {contact_count} |
| Groups | {group_count} |
| Conversation files | {conversation_count} |
| Messages | {message_count} |
| Attachment references | {attachment_count} |

## Exercises

- **Triple sources** — `imessage` vs `sms-backup-restore` vs `whatsapp`
- **Platform handles** — Text message + WhatsApp rows on the same contact
- **Transport mix** — SMS/RCS mixed into iMessage threads (~20% by default)
- **Contacts / groups / No Messages** — group memberships and zero-message rows
- **Unassigned** — handles with messages but no VCF row (phone + email)
- **Rate skew** — most 1:1 threads ~200–300 msgs/year (bursty days); rare whales up to ~12k/year
- **History** — typical first contact ~3–5 years ago; longest ~14 years; newest ~1 week
- **Group Chats** — membership mean ~5 groups/contact; size mean ~4; at least 10 groups with 8–20 participants; bursty days (several / none / a lot)
- **Replies, tapbacks, attachments** — including one intentionally missing file
- **orphaned.jsonl** — synthetic orphaned conversation
",
        seed = cfg.seed,
        corpus_sentences = corpus_sentences,
        contact_count = stats.contacts,
        group_count = stats.groups,
        conversation_count = stats.conversation_files,
        message_count = stats.messages,
        attachment_count = stats.attachment_refs,
        whatsapp_pct = (cfg.sources.whatsapp_contact_fraction * 100.0).round() as i64,
    );
    fs::write(&path, body).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests;
