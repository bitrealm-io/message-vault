//! Stable, non-reversible obfuscation for exporter CSV output.
//!
//! Fake identities are derived with HMAC-SHA256 over a secret key. The same
//! key always yields the same remaps; fakes do not embed or encrypt the
//! original, and no mapping sidecar is written.

mod names;

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::Path;
// Only the `#[cfg(test)]` fixture builders below own paths.
#[cfg(test)]
use std::path::PathBuf;
use std::sync::LazyLock;

use anyhow::{Context, Result, anyhow};
use hmac::{Hmac, KeyInit, Mac};
use rand::Rng;
use regex::Regex;
#[cfg(test)]
use serde_json::{Value, json};
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

/// ITU-T E.164 country calling codes, longest-first for greedy prefix match.
const COUNTRY_CALLING_CODES: &[&str] = &[
    "211", "212", "213", "216", "218", "220", "221", "222", "223", "224", "225", "226", "227",
    "228", "229", "230", "231", "232", "233", "234", "235", "236", "237", "238", "239", "240",
    "241", "242", "243", "244", "245", "246", "248", "249", "250", "251", "252", "253", "254",
    "255", "256", "257", "258", "260", "261", "262", "263", "264", "265", "266", "267", "268",
    "269", "290", "291", "297", "298", "299", "350", "351", "352", "353", "354", "355", "356",
    "357", "358", "359", "370", "371", "372", "373", "374", "375", "376", "377", "378", "380",
    "381", "382", "383", "385", "386", "387", "389", "420", "421", "423", "500", "501", "502",
    "503", "504", "505", "506", "507", "508", "509", "590", "591", "592", "593", "594", "595",
    "596", "597", "598", "599", "670", "672", "673", "674", "675", "676", "677", "678", "679",
    "680", "681", "682", "683", "685", "686", "687", "688", "689", "690", "691", "692", "850",
    "852", "853", "855", "856", "880", "886", "960", "961", "962", "963", "964", "965", "966",
    "967", "968", "970", "971", "972", "973", "974", "975", "976", "977", "992", "993", "994",
    "995", "996", "998", "20", "27", "30", "31", "32", "33", "34", "36", "39", "40", "41", "43",
    "44", "45", "46", "47", "48", "49", "51", "52", "53", "54", "55", "56", "57", "58", "60", "61",
    "62", "63", "64", "65", "66", "81", "82", "84", "86", "90", "91", "92", "93", "94", "95", "98",
    "1", "7",
];

/// Minimum national-number digits required after peeling a country calling code.
/// Keeps short codes (4–6 digits) from being misread as country + stub.
const MIN_NATIONAL_DIGITS: usize = 7;

/// Split digits into `(country_calling_code, national_number)`.
///
/// When `had_plus` is true, uses longest-match ITU calling codes. Without `+`, only
/// recognizes the NANP leading `1` on 11-digit numbers. Returns `("", digits)` when
/// no country code can be identified safely.
fn split_country_calling_code(digits: &str, had_plus: bool) -> (&str, &str) {
    if had_plus {
        for cc in COUNTRY_CALLING_CODES {
            if digits.starts_with(cc) && digits.len() - cc.len() >= MIN_NATIONAL_DIGITS {
                return (cc, &digits[cc.len()..]);
            }
        }
        return ("", digits);
    }
    if digits.len() == 11 && digits.starts_with('1') {
        return ("1", &digits[1..]);
    }
    ("", digits)
}

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
        let fake = self.rewrite_structured_text(raw, true);
        self.text_cache.insert(raw.to_string(), fake.clone());
        fake
    }

    /// Remap URL/email/phone substrings inside a free-form field (e.g. Chat Session).
    pub fn obfuscate_mixed_field(&mut self, raw: &str) -> String {
        if raw.is_empty() {
            return String::new();
        }
        self.rewrite_structured_text(raw, false)
    }

    /// Replace URL → email → phone spans; optionally word-shape the surrounding prose.
    fn rewrite_structured_text(&mut self, raw: &str, shape_prose: bool) -> String {
        let spans = find_structured_spans(raw);
        if spans.is_empty() {
            return if shape_prose {
                let d = self.digest("text", raw);
                shape_preserving_filler(raw, &d, 0)
            } else {
                raw.to_string()
            };
        }
        let mut out = String::with_capacity(raw.len());
        let mut cursor = 0usize;
        for (start, end, kind) in spans {
            let gap = &raw[cursor..start];
            if shape_prose {
                let d = self.digest("text", gap);
                out.push_str(&shape_preserving_filler(gap, &d, 0));
            } else {
                out.push_str(gap);
            }
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
        if shape_prose {
            let d = self.digest("text", gap);
            out.push_str(&shape_preserving_filler(gap, &d, 0));
        } else {
            out.push_str(gap);
        }
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

/// Write the three shared placeholder files under `output_dir/attachments/`.
///
/// # Errors
///
/// Returns an error when the directory cannot be created or a file cannot be written.
pub fn materialize_placeholders(output_dir: &Path) -> Result<()> {
    let dir = output_dir.join("attachments");
    fs::create_dir_all(&dir)?;
    // Remove prior real media.
    if dir.is_dir() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name != "placeholder.jpg"
                    && name != "placeholder.mp4"
                    && name != "placeholder.bin"
                {
                    let _ = fs::remove_file(&path);
                }
            }
        }
    }
    fs::write(dir.join("placeholder.jpg"), PLACEHOLDER_JPG)?;
    fs::write(dir.join("placeholder.mp4"), PLACEHOLDER_MP4)?;
    fs::write(dir.join("placeholder.bin"), PLACEHOLDER_BIN)?;
    Ok(())
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

/// Parse `--obfuscate-seed` hex or generate a random seed; print seed to stderr when generated.
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
const EXPORT_IDENTITY_COLS: &[&str] = &[
    "chat_identifier",
    "group_title",
    "participants_json",
    "sender_handle",
    "sender_display_name",
    "owner_handle",
    "owner_display_name",
    "text",
    "subject",
    "attachments_json",
    "announcement",
    "shared_location",
];

#[cfg(test)]
fn rename_chat_csv_files(output_dir: &Path) -> Result<()> {
    let mut csv_paths: Vec<PathBuf> = fs::read_dir(output_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("csv"))
        })
        .collect();
    csv_paths.sort();
    for path in csv_paths {
        let mut rdr = csv::ReaderBuilder::new().flexible(true).from_path(&path)?;
        let headers = rdr.headers()?.clone();
        let Some(chat_idx) = headers.iter().position(|h| h == "chat_identifier") else {
            continue;
        };
        let Some(Ok(first)) = rdr.records().next() else {
            continue;
        };
        let chat_id = first.get(chat_idx).unwrap_or("").trim();
        if chat_id.is_empty() {
            continue;
        }
        let safe: String = chat_id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '+' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let dest = output_dir.join(format!("{safe}.csv"));
        if dest != path && !dest.exists() {
            let _ = fs::rename(&path, &dest);
        }
    }
    Ok(())
}

#[cfg(test)]
fn obfuscate_export_csv_file(input: &Path, output: &Path, anon: &mut Obfuscator) -> Result<()> {
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(input)
        .with_context(|| format!("read {}", input.display()))?;
    let headers = rdr.headers()?.clone();
    let mut rows: Vec<csv::StringRecord> = Vec::new();
    for result in rdr.records() {
        let record = result?;
        rows.push(obfuscate_export_record(&headers, &record, anon));
    }

    let tmp = output.with_extension("csv.tmp");
    {
        let mut wtr =
            csv::Writer::from_path(&tmp).with_context(|| format!("write {}", tmp.display()))?;
        wtr.write_record(&headers)?;
        for row in &rows {
            wtr.write_record(row)?;
        }
        wtr.flush()?;
    }
    fs::rename(&tmp, output)?;
    Ok(())
}

#[cfg(test)]
fn obfuscate_export_record(
    headers: &csv::StringRecord,
    record: &csv::StringRecord,
    anon: &mut Obfuscator,
) -> csv::StringRecord {
    let mut out = csv::StringRecord::new();
    let mut sender_handle_original = String::new();
    for (i, header) in headers.iter().enumerate() {
        let val = record.get(i).unwrap_or("");
        let new_val = match header {
            "chat_identifier" => anon.obfuscate_handle(val),
            "group_title" => {
                if val.is_empty() {
                    String::new()
                } else {
                    anon.obfuscate_mixed_field(val)
                }
            }
            "participants_json" => obfuscate_participants_json(val, anon),
            "sender_handle" | "owner_handle" => {
                if header == "sender_handle" {
                    sender_handle_original = val.to_string();
                }
                anon.obfuscate_handle(val)
            }
            "sender_display_name" | "owner_display_name" => {
                if val.is_empty() {
                    String::new()
                } else if header == "sender_display_name" && !sender_handle_original.is_empty() {
                    anon.display_name_for_handle(&sender_handle_original)
                } else {
                    anon.obfuscate_display_name(val)
                }
            }
            "text" | "subject" | "announcement" => anon.obfuscate_text(val),
            "attachments_json" => obfuscate_attachments_json(val),
            "shared_location" => {
                if val.is_empty() {
                    String::new()
                } else {
                    anon.obfuscate_text(val)
                }
            }
            _ => {
                if EXPORT_IDENTITY_COLS.contains(&header) {
                    anon.obfuscate_text(val)
                } else {
                    val.to_string()
                }
            }
        };
        out.push_field(&new_val);
    }
    out
}

#[cfg(test)]
fn obfuscate_participants_json(raw: &str, anon: &mut Obfuscator) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "null" || trimmed == "[]" {
        return trimmed.to_string();
    }
    let Ok(mut value) = serde_json::from_str::<Value>(trimmed) else {
        return anon.obfuscate_mixed_field(raw);
    };
    if let Some(arr) = value.as_array_mut() {
        for item in arr.iter_mut() {
            if let Some(obj) = item.as_object_mut() {
                if let Some(h) = obj.get("handle").and_then(|v| v.as_str()) {
                    let fake_n = anon.display_name_for_handle(h);
                    let fake_h = anon.obfuscate_handle(h);
                    obj.insert("handle".into(), json!(fake_h));
                    if obj.contains_key("display_name") {
                        obj.insert("display_name".into(), json!(fake_n));
                    }
                }
            } else if let Some(s) = item.as_str() {
                *item = json!(anon.obfuscate_handle(s));
            }
        }
    }
    serde_json::to_string(&value).unwrap_or_else(|_| "[]".into())
}

#[cfg(test)]
fn obfuscate_attachments_json(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "null" || trimmed == "[]" {
        return trimmed.to_string();
    }
    let Ok(mut value) = serde_json::from_str::<Value>(trimmed) else {
        return "[]".into();
    };
    if let Some(arr) = value.as_array_mut() {
        for item in arr.iter_mut() {
            if let Some(obj) = item.as_object_mut() {
                let mime = obj
                    .get("mime_type")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let path = obj
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let class = classify_attachment(mime.as_deref(), path.as_deref());
                let rel = placeholder_rel_path(class);
                obj.insert("path".into(), json!(rel));
                if let Some(orig) = obj.get("original_name").and_then(|v| v.as_str()) {
                    let ext = Path::new(rel)
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("bin");
                    let stem = Path::new(orig)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("file");
                    // Keep extension class only; drop original basename PII lightly.
                    let _ = stem;
                    obj.insert("original_name".into(), json!(format!("attachment.{ext}")));
                }
                if let Some(t) = obj.get_mut("transcription")
                    && t.as_str().is_some_and(|s| !s.is_empty())
                {
                    *t = json!("[redacted]");
                }
            }
        }
    }
    serde_json::to_string(&value).unwrap_or_else(|_| "[]".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(n: u8) -> [u8; 32] {
        [n; 32]
    }

    /// Known-answer vectors for a fixed key.
    ///
    /// Every other test here checks a shape — the right number of digits, the
    /// country code kept, the original absent — and a shape survives almost any
    /// change to the arithmetic underneath. Mutation testing found that:
    /// swapping `%` for `/` and `+=` for `*=` inside `shape_preserving_filler`
    /// and `obfuscate_phone` changed every value produced and failed nothing,
    /// because the results were still digits of the right length.
    ///
    /// These pin what the key actually produces. The obfuscated export is
    /// meant to be reproducible — the same seed on the same backup gives the
    /// same fake vault, which is what makes it shareable and comparable — so
    /// the mapping is a contract, not an implementation detail. Regenerate
    /// deliberately if it ever has to change, and say why in the commit.
    #[test]
    fn known_answers_for_a_fixed_key() {
        const KEY: u8 = 7;

        let mut phones = Obfuscator::new(key(KEY));
        for (raw, expected) in [
            ("+15555550100", "+10490970433"),
            ("+44 20 7183 8750", "+44 39 2603 3624"),
            ("7535", "7032"),
            ("(555) 123-4567", "(972) 642-7627"),
        ] {
            assert_eq!(phones.obfuscate_phone(raw), expected, "phone {raw:?}");
        }

        let mut names = Obfuscator::new(key(KEY));
        for (raw, expected) in [
            ("Sam Example", "Joel Snow"),
            ("alice", "Ula Nelson"),
            ("Dr. Jane Q. Public", "Ida Vogel"),
        ] {
            assert_eq!(names.obfuscate_display_name(raw), expected, "name {raw:?}");
        }

        let mut emails = Obfuscator::new(key(KEY));
        for (raw, expected) in [
            ("alice@example.com", "hugo.white@example.invalid"),
            (
                "sam.doe+tag@mail.example.co.uk",
                "rory.thorne@example.invalid",
            ),
        ] {
            assert_eq!(emails.obfuscate_email(raw), expected, "email {raw:?}");
        }

        let mut urls = Obfuscator::new(key(KEY));
        for (raw, expected) in [
            (
                "https://example.com/a/b?c=1",
                "https://3121646218.example.invalid/",
            ),
            ("http://x.test", "https://2992837897.example.invalid/"),
        ] {
            assert_eq!(urls.obfuscate_url(raw), expected, "url {raw:?}");
        }

        // A handle takes the shape of whatever it is: a number stays a number,
        // an address stays an address, and anything else becomes a name.
        let mut handles = Obfuscator::new(key(KEY));
        for (raw, expected) in [
            ("+15555550100", "+10490970433"),
            ("alice@example.com", "hugo.white@example.invalid"),
            ("chat123", "Uma Thorne"),
        ] {
            assert_eq!(handles.obfuscate_handle(raw), expected, "handle {raw:?}");
        }

        // Message text keeps its structured spans recognisable — the phone is
        // still a phone, the address still an address — and scrambles the
        // prose around them.
        let mut text = Obfuscator::new(key(KEY));
        assert_eq!(
            text.obfuscate_text("Call me at +1 555 123 4567 or alice@example.com"),
            "Vmrq sm hl +1 839 910 9347 pt hugo.white@example.invalid"
        );

        // A handle with no name of its own still gets a stable one, and a real
        // name maps to the same fake name it would anywhere else.
        let mut display = Obfuscator::new(key(KEY));
        assert_eq!(
            display.display_name_for_handle("+15555550100"),
            "Hannah Carter"
        );
        assert_eq!(display.display_name_for_handle("Sam Example"), "Joel Snow");
    }

    /// The span finder must not obfuscate a piece of text twice.
    ///
    /// Spans are found by three regexes over the same string, so an address
    /// inside a URL, or a number inside an address, matches more than one.
    /// `span_overlaps` and `mark_covered` are what stop the second match from
    /// rewriting bytes the first already replaced, and mutation testing found
    /// both could be neutered — `span_overlaps` always false, `mark_covered`
    /// doing nothing — with every test still green.
    #[test]
    fn overlapping_structured_spans_are_each_rewritten_once() {
        // The address sits inside the URL, and the digits sit inside the
        // address: three regexes, one region of text.
        let raw = "see https://mail.example.com/u/alice@example.com?id=5551234567 now";
        let spans = find_structured_spans(raw);

        let mut sorted: Vec<(usize, usize)> = spans.iter().map(|(s, e, _)| (*s, *e)).collect();
        sorted.sort_unstable();
        for pair in sorted.windows(2) {
            assert!(
                pair[0].1 <= pair[1].0,
                "spans {:?} and {:?} overlap in {raw:?}",
                pair[0],
                pair[1]
            );
        }

        // And the rewrite leaves nothing of the original behind.
        let mut anon = Obfuscator::new(key(11));
        let out = anon.obfuscate_text(raw);
        assert!(!out.contains("alice@example.com"), "{out}");
        assert!(!out.contains("mail.example.com"), "{out}");
        assert!(!out.contains("5551234567"), "{out}");
    }

    /// A run of prose longer than the digest is the case that exercises the
    /// filler's wrap-around, which is where the modulo lives. A shorter string
    /// never reaches the end of the digest, so an arithmetic change there is
    /// invisible.
    #[test]
    fn a_long_run_of_prose_is_filled_to_its_own_length() {
        let raw = "The quick brown fox jumps over the lazy dog again and again and again,                    and keeps on jumping well past the length of any digest we might use here.";
        let mut anon = Obfuscator::new(key(13));
        let out = anon.obfuscate_text(raw);

        assert_eq!(
            out.chars().count(),
            raw.chars().count(),
            "length is the shape"
        );
        for (a, b) in raw.chars().zip(out.chars()) {
            if a.is_ascii_alphabetic() {
                assert!(b.is_ascii_alphabetic(), "{a:?} became {b:?}");
                assert_eq!(a.is_uppercase(), b.is_uppercase(), "case of {a:?}");
            } else {
                assert_eq!(a, b, "punctuation and spacing are kept");
            }
        }
        assert_ne!(out, raw, "the prose must actually change");
        // Deterministic: the same key and input give the same filler, which is
        // what makes an obfuscated export reproducible.
        let mut again = Obfuscator::new(key(13));
        assert_eq!(again.obfuscate_text(raw), out);
    }

    /// `looks_like_email` decides whether a bare handle is rewritten as an
    /// address or as a name, and it needs both an `@` and a dot. Mutation
    /// testing found the `&&` could become `||` with nothing failing, which
    /// would turn every handle containing a dot — every version string, every
    /// file name — into a fake email address.
    #[test]
    fn a_handle_is_an_address_only_with_both_an_at_and_a_dot() {
        assert!(looks_like_email("alice@example.com"));
        assert!(looks_like_email("a@b.c"));

        assert!(!looks_like_email("alice@example"), "no dot");
        assert!(!looks_like_email("example.com"), "no at");
        assert!(!looks_like_email("chat123"), "neither");
        assert!(!looks_like_email(""), "empty");
    }

    /// The placeholder pass replaces real media with three stand-in files and
    /// deletes everything else, which is the whole point: an obfuscated export
    /// must not ship the photographs. Mutation testing found the two `&&`
    /// guards on the keep-list could each become `||`, which deletes the
    /// placeholders it has just written and keeps nothing.
    #[test]
    fn materializing_placeholders_removes_the_real_media_and_keeps_the_three() {
        let dir = tempfile::tempdir().expect("tempdir");
        let attachments = dir.path().join("attachments");
        fs::create_dir_all(&attachments).expect("attachments dir");
        fs::write(attachments.join("IMG_0001.jpg"), b"real photo bytes").expect("write");
        fs::write(attachments.join("clip.mp4"), b"real video bytes").expect("write");
        fs::write(attachments.join("notes.bin"), b"real other bytes").expect("write");

        materialize_placeholders(dir.path()).expect("materialize");

        let mut names: Vec<String> = fs::read_dir(&attachments)
            .expect("read")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        assert_eq!(
            names,
            ["placeholder.bin", "placeholder.jpg", "placeholder.mp4"],
            "the real media must be gone and the three placeholders present"
        );
        assert_eq!(
            fs::read(attachments.join("placeholder.jpg")).expect("read jpg"),
            PLACEHOLDER_JPG,
            "the placeholder must be the committed bytes, not a leftover file"
        );
    }

    /// Running it twice must be a no-op, not a pass that deletes what the
    /// first one wrote.
    #[test]
    fn materializing_placeholders_twice_keeps_them() {
        let dir = tempfile::tempdir().expect("tempdir");
        materialize_placeholders(dir.path()).expect("first");
        materialize_placeholders(dir.path()).expect("second");

        let attachments = dir.path().join("attachments");
        assert!(attachments.join("placeholder.jpg").is_file());
        assert!(attachments.join("placeholder.mp4").is_file());
        assert!(attachments.join("placeholder.bin").is_file());
    }

    /// The filler's own arithmetic, pinned.
    ///
    /// `a_long_run_of_prose_is_filled_to_its_own_length` above checks the
    /// shape, and a shape survives any change to the digest arithmetic: swap
    /// `%` for `/` inside `shape_preserving_filler` and every letter changes
    /// while the length, the case and the punctuation all still line up. This
    /// pins what the filler produces for a run longer than the digest, which
    /// is the only input that reaches the wrap-around.
    #[test]
    fn a_long_run_of_prose_has_a_pinned_filler() {
        let raw = "The quick brown fox jumps over the lazy dog again and again and again, \
                   and keeps on jumping well past the length of any digest we might use here.";
        let mut anon = Obfuscator::new(key(13));
        assert_eq!(
            anon.obfuscate_text(raw),
            "Mkv iuujb zampc rum gqbfm wrgu kpf ujhf mkv iuujb zam pcrum gqb fmwrg, \
             ukp fujhf mk viuujbz ampc rumg qbf mwrguk pf ujh fmkviu uj bzamp cru mgqb."
        );
    }

    /// Five digits is where a run of numbers starts being treated as a phone
    /// number rather than as ordinary text, and the two paths produce
    /// different results. Nothing tested the boundary, so `< 5` could become
    /// `<= 5` or `== 5` and every test still passed.
    #[test]
    fn a_number_becomes_a_phone_at_five_digits() {
        for (raw, expected) in [
            ("call 1234 now", "shhw 8610 bsg"),
            ("call 12345 now", "fscs 25657 uto"),
            ("call 5551234567 now", "nbxh 3189880900 uvx"),
        ] {
            let mut anon = Obfuscator::new(key(13));
            assert_eq!(anon.obfuscate_text(raw), expected, "for {raw:?}");
        }
    }

    #[test]
    fn phone_stable_same_key() {
        let mut a = Obfuscator::new(key(1));
        let mut b = Obfuscator::new(key(1));
        assert_eq!(
            a.obfuscate_phone("+15555550100"),
            b.obfuscate_phone("+15555550100")
        );
    }

    #[test]
    fn phone_differs_other_key() {
        let mut a = Obfuscator::new(key(1));
        let mut b = Obfuscator::new(key(2));
        assert_ne!(
            a.obfuscate_phone("+15555550100"),
            b.obfuscate_phone("+15555550100")
        );
    }

    #[test]
    fn phone_preserves_digit_length_and_plus() {
        let mut a = Obfuscator::new(key(3));
        let fake = a.obfuscate_phone("+15555550100");
        assert!(fake.starts_with("+1"));
        assert_eq!(fake.chars().filter(|c| c.is_ascii_digit()).count(), 11);
        assert!(!fake.contains("5555550100"));
    }

    #[test]
    fn phone_keeps_country_calling_code() {
        let mut a = Obfuscator::new(key(3));
        let us = a.obfuscate_phone("+1 (555) 123-4567");
        assert!(us.starts_with("+1"));
        let us_digits: String = us.chars().filter(|c| c.is_ascii_digit()).collect();
        assert!(us_digits.starts_with('1'));
        assert_eq!(us_digits.len(), 11);
        assert_ne!(&us_digits[1..], "5551234567");

        let uk = a.obfuscate_phone("+44 20 7183 8750");
        assert!(uk.starts_with("+44"));
        let uk_digits: String = uk.chars().filter(|c| c.is_ascii_digit()).collect();
        assert!(uk_digits.starts_with("44"));
        assert_eq!(uk_digits.len(), 12);
        assert_ne!(&uk_digits[2..], "2071838750");

        // Short codes have no country code to preserve; length still matches.
        let short = a.obfuscate_phone("7535");
        assert_eq!(short.chars().filter(|c| c.is_ascii_digit()).count(), 4);
    }

    #[test]
    fn name_is_human() {
        let mut a = Obfuscator::new(key(4));
        let name = a.obfuscate_display_name("Secret Person");
        let parts: Vec<_> = name.split_whitespace().collect();
        assert_eq!(parts.len(), 2);
        assert!(FIRST_NAMES.contains(&parts[0]));
        assert!(LAST_NAMES.contains(&parts[1]));
    }

    #[test]
    fn text_same_length_not_original() {
        let mut a = Obfuscator::new(key(5));
        let src = "Hello, call me at dinner!";
        let fake = a.obfuscate_text(src);
        assert_eq!(fake.chars().count(), src.chars().count());
        assert_ne!(fake, src);
        assert!(!fake.contains("dinner"));
        // Word shape: non-letters stay; letter/digit runs keep length.
        let shape = |s: &str| {
            s.chars()
                .map(|c| {
                    if c.is_ascii_alphabetic() {
                        'L'
                    } else if c.is_ascii_digit() {
                        'D'
                    } else {
                        c
                    }
                })
                .collect::<String>()
        };
        assert_eq!(shape(&fake), shape(src));
        assert!(fake.starts_with(|c: char| c.is_ascii_uppercase()));
        assert_eq!(&fake[5..7], ", ");
        assert!(fake.ends_with('!'));
    }

    #[test]
    fn text_keeps_valid_email_url_phone() {
        let mut a = Obfuscator::new(key(6));
        let src = "Email alice@secret.com or https://secret.example/path?x=1 call +1 (555) 123-4567 thanks";
        let fake = a.obfuscate_text(src);
        assert!(!fake.contains("alice@secret.com"));
        assert!(!fake.contains("secret.example"));
        assert!(!fake.contains("555) 123-4567"));
        assert!(fake.contains("@example.invalid"));
        assert!(fake.contains("https://") && fake.contains(".example.invalid"));
        assert!(fake.contains('+') && fake.contains('(') && fake.contains('-'));
        let phone_digits: String = fake
            .chars()
            .skip_while(|c| *c != '+')
            .take_while(|c| c.is_ascii_digit() || matches!(c, '+' | ' ' | '(' | ')' | '-'))
            .filter(|c| c.is_ascii_digit())
            .collect();
        assert_eq!(phone_digits.len(), 11);
    }

    #[test]
    fn phone_preserves_formatting() {
        let mut a = Obfuscator::new(key(7));
        let src = "+1 (555) 123-4567";
        let fake = a.obfuscate_phone(src);
        assert_eq!(fake.len(), src.len());
        let shape = |s: &str| {
            s.chars()
                .map(|c| if c.is_ascii_digit() { 'D' } else { c })
                .collect::<String>()
        };
        assert_eq!(shape(&fake), shape(src));
        let digits: String = fake.chars().filter(|c| c.is_ascii_digit()).collect();
        assert_eq!(digits.len(), 11);
        assert_ne!(digits, "15551234567");
    }

    #[test]
    fn export_dir_smoke() {
        let dir = tempfile::tempdir().unwrap();
        let csv_path = dir.path().join("+15555550100.csv");
        let mut wtr = csv::Writer::from_path(&csv_path).unwrap();
        wtr.write_record([
            "chat_identifier",
            "sender_handle",
            "sender_display_name",
            "text",
            "attachments_json",
            "export_source",
        ])
        .unwrap();
        wtr.write_record([
            "+15555550100",
            "+15555550100",
            "Alice Secret",
            "Meet at 9",
            r#"[{"path":"attachments/photo.jpg","mime_type":"image/jpeg","original_name":"photo.jpg","is_sticker":false}]"#,
            "go-sms-pro",
        ])
        .unwrap();
        wtr.flush().unwrap();
        fs::create_dir_all(dir.path().join("attachments")).unwrap();
        fs::write(dir.path().join("attachments/photo.jpg"), b"REAL").unwrap();

        let mut anon = Obfuscator::new(key(9));
        materialize_placeholders(dir.path()).unwrap();
        obfuscate_export_csv_file(&csv_path, &csv_path, &mut anon).unwrap();
        rename_chat_csv_files(dir.path()).unwrap();

        assert!(dir.path().join("attachments/placeholder.jpg").is_file());
        assert!(!dir.path().join("attachments/photo.jpg").exists());

        let mut found_original = false;
        for entry in fs::read_dir(dir.path()).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("csv") {
                continue;
            }
            let text = fs::read_to_string(&path).unwrap();
            if text.contains("15555550100")
                || text.contains("Alice Secret")
                || text.contains("Meet at 9")
            {
                found_original = true;
            }
            assert!(text.contains("attachments/placeholder.jpg"));
        }
        assert!(!found_original);
    }

    #[test]
    fn seed_resolves_and_is_stable() {
        let mut a = resolve_obfuscator(Some(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        ))
        .unwrap();
        let mut b = resolve_obfuscator(Some(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        ))
        .unwrap();
        assert_eq!(
            a.obfuscate_phone("+15555550100"),
            b.obfuscate_phone("+15555550100")
        );
        assert_ne!(
            a.obfuscate_phone("+15555550100"),
            resolve_obfuscator(Some(
                "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"
            ))
            .unwrap()
            .obfuscate_phone("+15555550100")
        );
    }

    #[test]
    fn seed_must_be_exactly_64_hex_chars() {
        // The old 8-character seed is refused.
        assert!(resolve_obfuscator(Some("01234567")).is_err());
        assert!(resolve_obfuscator(Some(&"a".repeat(63))).is_err());
        assert!(resolve_obfuscator(Some(&"a".repeat(65))).is_err());
        // Right length, not hex.
        assert!(resolve_obfuscator(Some(&"g".repeat(64))).is_err());
        assert!(resolve_obfuscator(Some(&"a".repeat(64))).is_ok());
        // Surrounding whitespace is trimmed; upper-case hex is fine.
        assert!(resolve_obfuscator(Some(&format!(" {} ", "A".repeat(64)))).is_ok());
    }

    #[test]
    fn unicode_emoji_scrambled() {
        let mut a = Obfuscator::new(key(1));
        // Emoji pass through is_alphabetic = false and would have survived
        // the old else branch.
        let fake = a.obfuscate_text("Hello 🎉🥳💃");
        assert!(!fake.contains('🎉'));
        assert!(!fake.contains('🥳'));
        assert!(!fake.contains('💃'));
        assert_eq!(fake.chars().count(), "Hello 🎉🥳💃".chars().count());
    }

    #[test]
    fn unicode_combining_marks_scrambled() {
        let mut a = Obfuscator::new(key(1));
        // NFD "café" = "cafe" + U+0301 combining acute.
        // The 'e' gets replaced, but U+0301 would have survived the old else.
        let src = "cafe\u{0301}";
        let fake = a.obfuscate_text(src);
        assert!(!fake.contains('\u{0301}'), "combining mark survived");
        assert_eq!(fake.chars().count(), src.chars().count());
    }

    #[test]
    fn unicode_math_symbols_scrambled() {
        let mut a = Obfuscator::new(key(1));
        let fake = a.obfuscate_text("x ∑ y → z");
        assert!(!fake.contains('∑'));
        assert!(!fake.contains('→'));
        // '+' and '>' are ASCII punctuation so they survive.
        assert_eq!(fake.chars().count(), "x ∑ y → z".chars().count());
    }

    #[test]
    fn unicode_zero_width_scrambled() {
        let mut a = Obfuscator::new(key(1));
        // Zero-width joiner (U+200D) used in emoji sequences.
        let src = "\u{200D}";
        let fake = a.obfuscate_text(src);
        assert!(!fake.contains('\u{200D}'), "zero-width joiner survived");
        assert_eq!(fake.chars().count(), 1);
    }

    #[test]
    fn ascii_punctuation_and_whitespace_preserved() {
        let mut a = Obfuscator::new(key(1));
        let fake = a.obfuscate_text("Hello, world! How are you?");
        assert!(fake.contains(", "));
        assert!(fake.contains('!'));
        assert!(fake.contains('?'));
        assert!(fake.contains(' '));
        // The words themselves should be scrambled.
        assert!(!fake.contains("Hello"));
        assert!(!fake.contains("world"));
    }

    #[test]
    fn unicode_cjk_still_scrambled() {
        let mut a = Obfuscator::new(key(1));
        // CJK was handled before and should still be.
        let fake = a.obfuscate_text("你好世界");
        assert!(!fake.contains('你'));
        assert!(!fake.contains('世'));
        assert_eq!(fake.chars().count(), 4);
        // Should all be ASCII letters now.
        assert!(fake.chars().all(|c| c.is_ascii_alphabetic()));
    }
}
