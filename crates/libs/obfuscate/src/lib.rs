//! Stable, non-reversible obfuscation for exported conversations, in every
//! output format.
//!
//! Fake identities are derived with HMAC-SHA256 over a secret key. The same
//! key always yields the same remaps; fakes do not embed or encrypt the
//! original, and no mapping sidecar is written.

mod names;

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::LazyLock;

use anyhow::{Context, Result, anyhow};
use hmac::{Hmac, KeyInit, Mac};
use phone::split_country_calling_code;
use rand::Rng;
use regex::Regex;
use sha2::{Digest, Sha256};

use names::{FIRST_NAMES, LAST_NAMES};

type HmacSha256 = Hmac<Sha256>;

const PLACEHOLDER_JPG: &[u8] = include_bytes!("../assets/placeholder.jpg");
const PLACEHOLDER_MP4: &[u8] = include_bytes!("../assets/placeholder.mp4");
const PLACEHOLDER_BIN: &[u8] = include_bytes!("../assets/placeholder.bin");

const REL_IMAGE: &str = "attachments/placeholder.jpg";
const REL_VIDEO: &str = "attachments/placeholder.mp4";
const REL_OTHER: &str = "attachments/placeholder.bin";

static EMAIL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}").expect("email re")
});
static URL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)\b(?:https?://|www\.)[^\s<>"'\)\]]+"#).expect("url re"));
static PHONE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\+?\d[\d\-\s().]{4,}\d").expect("phone re"));

/// Trim trailing sentence punctuation often glued to URLs/emails in message text.
fn trim_trailing_glue(s: &str) -> &str {
    s.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '\''])
}

/// Media class for placeholder substitution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaClass {
    /// Image placeholder bucket.
    Image,
    /// Video placeholder bucket.
    Video,
    /// Everything-else placeholder bucket.
    Other,
}

#[derive(Debug, Clone, Copy)]
enum StructuredKind {
    Url,
    Email,
    Phone,
}

/// Keyed obfuscator (in-memory cache only; never writes a real→fake map).
#[derive(Debug)]
pub struct Obfuscator {
    key: [u8; 32],
    /// Digits-only → fake digits-only (same length).
    phone_cache: HashMap<String, String>,
    name_cache: HashMap<String, (String, String)>,
    email_cache: HashMap<String, String>,
    url_cache: HashMap<String, String>,
    text_cache: HashMap<String, String>,
}

impl Obfuscator {
    /// Build an obfuscator from a 32-byte HMAC key.
    pub fn new(key: [u8; 32]) -> Self {
        Self {
            key,
            phone_cache: HashMap::new(),
            name_cache: HashMap::new(),
            email_cache: HashMap::new(),
            url_cache: HashMap::new(),
            text_cache: HashMap::new(),
        }
    }

    fn digest(&self, domain: &str, value: &str) -> [u8; 32] {
        let mut mac = HmacSha256::new_from_slice(&self.key).expect("HMAC accepts any key length");
        mac.update(domain.as_bytes());
        mac.update(b"\0");
        mac.update(value.as_bytes());
        let result = mac.finalize().into_bytes();
        let mut out = [0u8; 32];
        out.copy_from_slice(&result);
        out
    }

    /// Fake phone with the same digit count as `raw`.
    ///
    /// Keeps the country calling code unchanged and remaps only the national-number
    /// digits. Non-digit formatting (`+`, spaces, dashes, parentheses) is preserved
    /// in place. Cache key is digits-only so the same number always maps to the same
    /// fake digits regardless of formatting.
    pub fn obfuscate_phone(&mut self, raw: &str) -> String {
        let trimmed = raw.trim();
        let digits: String = trimmed.chars().filter(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            return trimmed.to_string();
        }
        let fake_digits = if let Some(cached) = self.phone_cache.get(&digits) {
            cached.clone()
        } else {
            let had_plus = trimmed.contains('+');
            let (cc, national) = split_country_calling_code(&digits, had_plus);
            let d = self.digest("phone", &digits);
            let mut fake_national = String::with_capacity(national.len());
            let mut i = 0usize;
            while fake_national.len() < national.len() {
                let b = d[i % d.len()];
                fake_national.push(char::from(b'0' + (b % 10)));
                i += 1;
            }
            // Avoid all-zeros when possible.
            if !fake_national.is_empty() && fake_national.chars().all(|c| c == '0') {
                fake_national = "1".repeat(national.len());
            }
            let fake_digits = format!("{cc}{fake_national}");
            debug_assert_eq!(fake_digits.len(), digits.len());
            self.phone_cache.insert(digits, fake_digits.clone());
            fake_digits
        };
        let mut out = String::with_capacity(trimmed.len());
        let mut di = 0usize;
        for ch in trimmed.chars() {
            if ch.is_ascii_digit() {
                out.push(fake_digits.as_bytes()[di] as char);
                di += 1;
            } else {
                out.push(ch);
            }
        }
        out
    }

    /// A keyed stand-in for an id, as 64 lowercase hex digits: the same id
    /// gives the same stand-in under one key, two ids give two, and nobody
    /// without the key can tell which id it stands for.
    pub fn obfuscate_id(&self, raw: &str) -> String {
        hex::encode(self.digest("id", raw))
    }

    /// Human display name from the name word lists (keyed by normalized original).
    pub fn obfuscate_display_name(&mut self, raw: &str) -> String {
        let key = normalize_name_key(raw);
        if key.is_empty() {
            return String::new();
        }
        let (first, last) = self.name_parts(&key);
        format!("{first} {last}")
    }

    fn name_parts(&mut self, key: &str) -> (String, String) {
        if let Some(cached) = self.name_cache.get(key) {
            return cached.clone();
        }
        let d = self.digest("name", key);
        let fi = u32::from_le_bytes(d[0..4].try_into().unwrap()) as usize % FIRST_NAMES.len();
        let li = u32::from_le_bytes(d[4..8].try_into().unwrap()) as usize % LAST_NAMES.len();
        let pair = (FIRST_NAMES[fi].to_string(), LAST_NAMES[li].to_string());
        self.name_cache.insert(key.to_string(), pair.clone());
        pair
    }

    /// Display name derived from a phone/email handle (keeps person consistent with handle).
    pub fn display_name_for_handle(&mut self, handle: &str) -> String {
        let h = handle.trim();
        if h.is_empty() {
            return String::new();
        }
        if looks_like_email(h) {
            let (first, last) = self.name_parts(&format!("email:{}", h.to_ascii_lowercase()));
            return format!("{first} {last}");
        }
        let digits: String = h.chars().filter(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            let (first, last) = self.name_parts(&format!("phone:{digits}"));
            return format!("{first} {last}");
        }
        self.obfuscate_display_name(h)
    }

    /// Map an email to `first.last@example.invalid`, keyed case-insensitively.
    pub fn obfuscate_email(&mut self, raw: &str) -> String {
        let key = raw.trim().to_ascii_lowercase();
        if key.is_empty() {
            return String::new();
        }
        if let Some(cached) = self.email_cache.get(&key) {
            return cached.clone();
        }
        let (first, last) = self.name_parts(&format!("email:{key}"));
        let fake = format!(
            "{}.{}@example.invalid",
            first.to_ascii_lowercase(),
            last.to_ascii_lowercase()
        );
        self.email_cache.insert(key, fake.clone());
        fake
    }

    /// Replace every URL with a deterministic dummy URL.
    ///
    /// The dummy is always `https://{n}.example.invalid/` where `n` is derived
    /// from the original URL via HMAC, so the same URL always maps to the same
    /// dummy without leaking any structure of the original.
    pub fn obfuscate_url(&mut self, raw: &str) -> String {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return String::new();
        }
        let key = trimmed.to_ascii_lowercase();
        if let Some(cached) = self.url_cache.get(&key) {
            return cached.clone();
        }
        let d = self.digest("url", &key);
        let n = u32::from_le_bytes(d[0..4].try_into().unwrap());
        let fake = format!("https://{n}.example.invalid/");
        self.url_cache.insert(key, fake.clone());
        fake
    }

    /// Handle: phone, email, or opaque string → fake of the same kind.
    pub fn obfuscate_handle(&mut self, raw: &str) -> String {
        let t = raw.trim();
        if t.is_empty() {
            return String::new();
        }
        if looks_like_email(t) {
            return self.obfuscate_email(t);
        }
        let digit_count = t.chars().filter(|c| c.is_ascii_digit()).count();
        if digit_count >= 5 {
            return self.obfuscate_phone(t);
        }
        // Name-only chat id / opaque handle
        self.obfuscate_display_name(t)
    }

    /// Word-shape nonsense for prose; emails/URLs/phones stay valid randomized forms.
    pub fn obfuscate_text(&mut self, raw: &str) -> String {
        if raw.is_empty() {
            return String::new();
        }
        if let Some(cached) = self.text_cache.get(raw) {
            return cached.clone();
        }
        let fake = self.rewrite_structured_text(raw);
        self.text_cache.insert(raw.to_string(), fake.clone());
        fake
    }

    /// Replace URL → email → phone spans and word-shape the prose between them.
    fn rewrite_structured_text(&mut self, raw: &str) -> String {
        let spans = find_structured_spans(raw);
        if spans.is_empty() {
            let d = self.digest("text", raw);
            return shape_preserving_filler(raw, &d, 0);
        }
        let mut out = String::with_capacity(raw.len());
        let mut cursor = 0usize;
        for (start, end, kind) in spans {
            let gap = &raw[cursor..start];
            let d = self.digest("text", gap);
            out.push_str(&shape_preserving_filler(gap, &d, 0));
            let piece = &raw[start..end];
            let replacement = match kind {
                StructuredKind::Url => self.obfuscate_url(piece),
                StructuredKind::Email => self.obfuscate_email(piece),
                StructuredKind::Phone => self.obfuscate_phone(piece),
            };
            out.push_str(&replacement);
            cursor = end;
        }
        let gap = &raw[cursor..];
        let d = self.digest("text", gap);
        out.push_str(&shape_preserving_filler(gap, &d, 0));
        out
    }
}

/// Same-length letter/digit filler; whitespace and punctuation kept in place.
///
/// ASCII letters/digits and any Unicode alphabetic (CJK, Cyrillic, accented
/// Latin, Arabic, etc.) are replaced with deterministic ASCII substitutes.
/// Emoji, symbols, combining marks, and other non-structural characters are
/// also replaced so they cannot leak identity or message content.
/// Only whitespace and ASCII punctuation pass through unchanged.
fn shape_preserving_filler(raw: &str, digest: &[u8; 32], digest_offset: usize) -> String {
    let len = raw.chars().count();
    let mut out = String::with_capacity(raw.len());
    let mut i = digest_offset;
    for ch in raw.chars() {
        if ch.is_whitespace() || ch.is_ascii_punctuation() {
            out.push(ch);
        } else if ch.is_ascii_alphabetic() {
            let b = digest[i % digest.len()];
            i += 1;
            let base = if ch.is_ascii_uppercase() { b'A' } else { b'a' };
            out.push(char::from(base + (b % 26)));
        } else if ch.is_ascii_digit() {
            let b = digest[i % digest.len()];
            i += 1;
            out.push(char::from(b'0' + (b % 10)));
        } else {
            // Non-ASCII alphabetic (CJK, Cyrillic, accented Latin, Arabic,
            // etc.), emoji, symbols, combining marks, zero-width characters,
            // and anything else — replace with a deterministic ASCII letter
            // so nothing identifiable leaks through.
            let b = digest[i % digest.len()];
            i += 1;
            out.push(char::from(b'a' + (b % 26)));
        }
    }
    let mut chars: Vec<char> = out.chars().collect();
    while chars.len() < len {
        chars.push('x');
    }
    chars.truncate(len);
    chars.into_iter().collect()
}

fn span_overlaps(covered: &[bool], start: usize, end: usize) -> bool {
    covered[start..end].iter().any(|&c| c)
}

fn mark_covered(covered: &mut [bool], start: usize, end: usize) {
    for slot in &mut covered[start..end] {
        *slot = true;
    }
}

/// Record every match of `re` as a span of `kind` with trailing punctuation
/// trimmed off, skipping a match that overlaps a span already taken.
fn push_trimmed_spans(
    re: &Regex,
    kind: StructuredKind,
    raw: &str,
    covered: &mut [bool],
    spans: &mut Vec<(usize, usize, StructuredKind)>,
) {
    for m in re.find_iter(raw) {
        let trimmed = trim_trailing_glue(m.as_str());
        if trimmed.is_empty() {
            continue;
        }
        let start = m.start();
        let end = start + trimmed.len();
        if span_overlaps(covered, start, end) {
            continue;
        }
        mark_covered(covered, start, end);
        spans.push((start, end, kind));
    }
}

fn find_structured_spans(raw: &str) -> Vec<(usize, usize, StructuredKind)> {
    let mut covered = vec![false; raw.len()];
    let mut spans = Vec::new();

    push_trimmed_spans(&URL_RE, StructuredKind::Url, raw, &mut covered, &mut spans);
    push_trimmed_spans(
        &EMAIL_RE,
        StructuredKind::Email,
        raw,
        &mut covered,
        &mut spans,
    );

    for m in PHONE_RE.find_iter(raw) {
        let p = m.as_str();
        let digit_count = p.chars().filter(|c| c.is_ascii_digit()).count();
        if digit_count < 5 {
            continue;
        }
        let start = m.start();
        let end = m.end();
        if span_overlaps(&covered, start, end) {
            continue;
        }
        mark_covered(&mut covered, start, end);
        spans.push((start, end, StructuredKind::Phone));
    }

    spans.sort_by_key(|(start, _, _)| *start);
    spans
}

fn normalize_name_key(raw: &str) -> String {
    raw.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn looks_like_email(s: &str) -> bool {
    s.contains('@') && s.contains('.')
}

/// Classify attachment by MIME and/or file extension.
///
/// Extension recognition delegates to [`media::classify`] so both crates
/// agree on what counts as an image or a video. Audio maps to
/// [`MediaClass::Other`]: obfuscation has no audio placeholder, so audio
/// files get the generic `placeholder.bin` (as they always have).
pub fn classify_attachment(mime: Option<&str>, path: Option<&str>) -> MediaClass {
    if let Some(m) = mime {
        let m = m.to_ascii_lowercase();
        if m.starts_with("image/") {
            return MediaClass::Image;
        }
        if m.starts_with("video/") {
            return MediaClass::Video;
        }
    }
    if let Some(p) = path {
        match media::classify(Path::new(p)) {
            Some(media::Kind::Image) => return MediaClass::Image,
            Some(media::Kind::Video) => return MediaClass::Video,
            Some(media::Kind::Audio) | None => {}
        }
    }
    MediaClass::Other
}

/// Shared placeholder relative path for a `MediaClass`
/// (`attachments/placeholder.jpg|.mp4|.bin`).
pub fn placeholder_rel_path(class: MediaClass) -> &'static str {
    match class {
        MediaClass::Image => REL_IMAGE,
        MediaClass::Video => REL_VIDEO,
        MediaClass::Other => REL_OTHER,
    }
}

/// Delete everything under `output_dir/attachments/`, in subfolders too,
/// and write the three shared placeholder files there.
///
/// Each entry's type is read without following a symlink. A folder is
/// emptied one entry at a time and then removed, so a failure names the
/// file that stayed. A symlink is removed, never followed, so a link to a
/// folder outside the export leaves that folder alone, and a link with a
/// placeholder's name cannot carry the placeholder out of the export. The
/// placeholders are written afresh each time.
///
/// # Errors
///
/// Returns an error when the directory cannot be created or read, an entry
/// cannot be removed, or a placeholder cannot be written. Each names its
/// path.
pub fn materialize_placeholders(output_dir: &Path) -> Result<()> {
    let dir = output_dir.join("attachments");
    fs::create_dir_all(&dir).with_context(|| format!("could not create {}", dir.display()))?;
    // Anything left behind is content the obfuscated export exists to leave
    // out, so a failed delete fails the pass.
    remove_folder_contents(&dir)?;
    for (rel, bytes) in [
        (REL_IMAGE, PLACEHOLDER_JPG),
        (REL_VIDEO, PLACEHOLDER_MP4),
        (REL_OTHER, PLACEHOLDER_BIN),
    ] {
        let path = output_dir.join(rel);
        fs::write(&path, bytes).with_context(|| format!("could not write {}", path.display()))?;
    }
    Ok(())
}

/// Remove every entry of `dir`, which stays, reading each type without
/// following a symlink.
fn remove_folder_contents(dir: &Path) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("could not read {}", dir.display()))? {
        let entry = entry.with_context(|| format!("could not read {}", dir.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("could not read {}", path.display()))?;
        if file_type.is_dir() {
            remove_folder_contents(&path)?;
            fs::remove_dir(&path)
        } else if file_type.is_symlink() {
            remove_symlink(&path, file_type)
        } else {
            fs::remove_file(&path)
        }
        .with_context(|| {
            format!(
                "could not remove {} from the obfuscated export",
                path.display()
            )
        })?;
    }
    Ok(())
}

/// Remove the symlink at `path` itself. Windows removes a link to a folder
/// (or a junction) as a folder, and any other link as a file.
#[cfg(windows)]
fn remove_symlink(path: &Path, file_type: fs::FileType) -> std::io::Result<()> {
    use std::os::windows::fs::FileTypeExt;
    if file_type.is_symlink_dir() {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    }
}

/// Remove the symlink at `path` itself. Unix removes every link as a file.
#[cfg(not(windows))]
fn remove_symlink(path: &Path, _file_type: fs::FileType) -> std::io::Result<()> {
    fs::remove_file(path)
}

/// Seed length: 32 bytes → 64 hex characters (2^256 key space).
pub const OBFUSCATE_SEED_BYTES: usize = 32;
/// Hex length of a 32-byte seed.
pub const OBFUSCATE_SEED_HEX_LEN: usize = OBFUSCATE_SEED_BYTES * 2;

/// Check that `seed_hex` (already trimmed) is exactly
/// [`OBFUSCATE_SEED_HEX_LEN`] hex digits. Every caller that accepts a seed
/// runs this one check, so the form and the run agree on what a seed is.
///
/// # Errors
///
/// Returns the message to show when the seed has the wrong length or holds
/// a non-hex character.
pub fn check_seed_hex(seed_hex: &str) -> std::result::Result<(), String> {
    if seed_hex.len() != OBFUSCATE_SEED_HEX_LEN {
        return Err(format!(
            "obfuscate seed must be exactly {OBFUSCATE_SEED_HEX_LEN} hex characters, got {}",
            seed_hex.len()
        ));
    }
    if !seed_hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("obfuscate seed must contain only hex characters (0-9, a-f)".to_string());
    }
    Ok(())
}

fn key_from_seed_bytes(bytes: &[u8]) -> [u8; 32] {
    let dig = Sha256::digest(bytes);
    let mut key = [0u8; 32];
    key.copy_from_slice(&dig);
    key
}

/// Parse the `seed_hex` obfuscation seed or generate a random seed; print seed to stderr when generated.
///
/// # Errors
///
/// Returns an error when `seed_hex` is not exactly [`OBFUSCATE_SEED_HEX_LEN`] hex digits.
pub fn resolve_obfuscator(seed_hex: Option<&str>) -> Result<Obfuscator> {
    resolve_obfuscator_with_log(seed_hex, None)
}

/// Like [`resolve_obfuscator`], but print the generated-seed notice through `log` when set.
pub fn resolve_obfuscator_with_log(
    seed_hex: Option<&str>,
    log: Option<&dyn Fn(&str)>,
) -> Result<Obfuscator> {
    let key = if let Some(s) = seed_hex {
        let s = s.trim();
        check_seed_hex(s).map_err(|e| anyhow!(e))?;
        let bytes = hex::decode(s).context("invalid obfuscate seed (expected hex)")?;
        key_from_seed_bytes(&bytes)
    } else {
        let mut seed = [0u8; OBFUSCATE_SEED_BYTES];
        rand::rng().fill_bytes(&mut seed);
        let hex_key = hex::encode(seed);
        let msg = format!("obfuscate-seed: {hex_key}  (save to reproduce; not written to output)");
        match log {
            Some(emit) => emit(&msg),
            None => {
                let _ = writeln!(std::io::stderr(), "{msg}");
            }
        }
        key_from_seed_bytes(&seed)
    };
    Ok(Obfuscator::new(key))
}

#[cfg(test)]
mod tests;
