//! Shared phone-number parsing for message converters.
//!
//! [`normalize_typed_handle`] is the one key a handle is stored and matched
//! under: by the server, by the contacts book, and by [`OwnerHandleSet`].
//! [`Handle::parse`] is where an exporter classifies an address before it
//! keys it: a phone number, an email address, or a sender name.

use std::fmt;

use anyhow::{Context, Result, bail};
use message_ir::HandleType;
use sha2::{Digest, Sha256};

/// Minimum digit length after stripping formatting.
///
/// Allows 4–6 digit short codes (carrier/bank SMS, e.g. AT&T `7535`).
/// Rejects junk like `"4"` or `"06"`.
const MIN_PHONE_DIGITS: usize = 4;

/// Region rules for [`normalize_checked`] and [`normalize_guarded`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhoneRegion {
    /// US NANP: certain only for 10 digits or 11 digits starting with `1`.
    Usa,
    /// International: certain only when the raw value has a leading `+`.
    International,
}

impl fmt::Display for PhoneRegion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PhoneRegion::Usa => write!(f, "usa"),
            PhoneRegion::International => write!(f, "international"),
        }
    }
}

impl PhoneRegion {
    /// Region for a raw value: a value with a `+` before its first digit
    /// names its country, so international rules apply (`+65 9123 4567`,
    /// `(+44) 7700 900123`, `tel:+447700900123`); anything else is treated
    /// as a US national number (this crate's home region).
    pub fn for_raw(raw: &str) -> Self {
        let before_first_digit = raw.split(|c: char| c.is_ascii_digit()).next();
        if before_first_digit.is_some_and(|s| s.contains('+')) {
            Self::International
        } else {
            Self::Usa
        }
    }
}

/// Strip non-digits and a leading US country code `1`.
/// Returns `None` when fewer than the minimum digit count (4) remain.
pub fn sanitize_number(num: &str) -> Option<String> {
    if num.is_empty() {
        return None;
    }
    let mut digits: String = num.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() == 11 && digits.starts_with('1') {
        digits = digits[1..].to_string();
    }
    if digits.len() < MIN_PHONE_DIGITS {
        None
    } else {
        Some(digits)
    }
}

/// Punctuation a written phone number may carry between or around its digits.
///
/// `/` is deliberately absent: it separates two numbers ("home / cell") far
/// more often than it appears inside one.
const PHONE_PUNCTUATION: [char; 7] = ['+', '-', '(', ')', '.', ' ', '\t'];

/// The most digits a phone number can have, from ITU-T E.164.
const MAX_PHONE_DIGITS: usize = 15;

/// Digits of a value that is *written as* a phone number, rather than a value
/// that merely contains enough digits.
///
/// [`sanitize_number`] keeps every ASCII digit it finds and then asks only how
/// many there are, so `met in 2019 at 42 Acacia Avenue` sanitizes to `201942`.
/// That is the right rule for a field known to hold a number and allowed to
/// carry stray formatting; it is the wrong rule for a field that may hold
/// anything, or that may hold two numbers with nothing between them.
///
/// Two conditions are added. The value must be nothing but digits and phone
/// punctuation, so prose is rejected before any digits are collected. And the
/// digits that remain must number no more than [`MAX_PHONE_DIGITS`], so two
/// numbers run together are rejected rather than concatenated into a handle
/// that matches nothing.
///
/// An extension suffix (`555-1234 x99`) is prose by the first rule and is
/// rejected whole, which is deliberate: gluing an extension onto a number
/// produces a handle that matches nothing either.
///
/// Returns `None` when the value carries any other character, when the digits
/// are too many, or when [`sanitize_number`] rejects what remains.
#[must_use]
pub fn sanitize_phone_shaped(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty()
        || !value
            .chars()
            .all(|c| c.is_ascii_digit() || PHONE_PUNCTUATION.contains(&c))
    {
        return None;
    }
    let digits = sanitize_number(value)?;
    (digits.len() <= MAX_PHONE_DIGITS).then_some(digits)
}

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
pub fn split_country_calling_code(digits: &str, had_plus: bool) -> (&str, &str) {
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

/// E.164 when the parse is unambiguous for `region`, else the human-readable
/// reason it is not.
///
/// Unlike [`sanitize_number`], this does **not** accept short codes or
/// ambiguous lengths. [`normalize_guarded`] calls it and keeps the error
/// string as the review note the server shows.
///
/// # Errors
///
/// Returns the reason the value is not certain E.164 (empty, no digits, wrong
/// digit count for the region, missing `+` in international mode, or a
/// country code starting with 0 — 0 is the trunk prefix, so `+020…` would be
/// fabricated).
pub fn normalize_checked(raw: &str, region: PhoneRegion) -> Result<String, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("empty phone".into());
    }
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    match region {
        PhoneRegion::Usa => {
            if digits.len() == 10 {
                Ok(format!("+1{digits}"))
            } else if digits.len() == 11 && digits.starts_with('1') {
                Ok(format!("+{digits}"))
            } else if digits.is_empty() {
                Err("no digits".into())
            } else {
                Err("USA needs 10 digits or 11 starting with 1".into())
            }
        }
        PhoneRegion::International => {
            if !raw.contains('+') {
                Err("international mode requires a leading +".into())
            } else if digits.starts_with('0') {
                // E.164 country codes never start with 0 (0 is the trunk
                // prefix), so a `+020…` value is fabricated, not certain.
                Err("international country code cannot start with 0".into())
            } else if (8..=15).contains(&digits.len()) {
                Ok(format!("+{digits}"))
            } else {
                Err("international needs 8–15 digits after +".into())
            }
        }
    }
}

/// Result of [`normalize_guarded`]: the E.164 value when certain, plus a
/// review note when the value is ambiguous.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardedNormalize {
    /// The phone value to store: E.164 (`+1…`) when the parse was certain,
    /// otherwise raw digits without a `+` prefix.
    pub normalized: String,
    /// `Some(reason)` when the value was ambiguous and stored digits-as-is;
    /// `None` when certain.
    pub note: Option<String>,
}

/// E.164 when unambiguous for `region`, else digits-as-is plus a note.
///
/// Rewrite to E.164 only when the parse is certain. Otherwise store the
/// digits without adding a `+` prefix (a trunk-zero national number like
/// `020 7946 0000` would otherwise become the invalid `+02079460000`) and
/// attach a human-readable reason so the server can show it for review.
pub fn normalize_guarded(raw: &str, region: PhoneRegion) -> GuardedNormalize {
    match normalize_checked(raw, region) {
        Ok(e164) => GuardedNormalize {
            normalized: e164,
            note: None,
        },
        Err(reason) => GuardedNormalize {
            normalized: sanitize_number(raw).unwrap_or_default(),
            note: Some(reason),
        },
    }
}

/// Guarded normalization with the region inferred from the raw value: the
/// lenient one-argument policy loaders and exporters share.
///
/// Policy: [`PhoneRegion::for_raw`] picks the region (a `+`-prefixed value
/// uses international rules, anything else US), then [`normalize_guarded`]
/// yields E.164 when the parse is certain and the sanitized digits as-is
/// otherwise — never a fabricated `+0…`. The result is empty when no usable
/// digits survive (fewer than 4). The guard note is dropped; call
/// [`normalize_guarded`] directly when the caller records the note.
pub fn normalize_lenient(raw: &str) -> String {
    normalize_guarded(raw, PhoneRegion::for_raw(raw)).normalized
}

/// Sanitize to US digits, then apply the guarded policy: the one-argument
/// form for values already known to be phone-shaped digit strings.
///
/// Policy: `None` when [`sanitize_number`] finds no usable digits (fewer than
/// 4 after stripping formatting and a leading US `1`). Otherwise the guarded
/// form of the digits under [`PhoneRegion::Usa`]: `+1…` E.164 when the digit
/// count is unambiguous, else the sanitized digits as-is (short codes and
/// trunk-zero locals stay digits — never a fabricated `+0…`).
pub fn normalize_digits_us(raw: &str) -> Option<String> {
    let digits = sanitize_number(raw)?;
    Some(normalize_guarded(&digits, PhoneRegion::Usa).normalized)
}

/// One normalization policy for a typed handle, shared by the server, the
/// contacts book, and the exporters.
///
/// Phone: guarded E.164 via [`normalize_guarded`] with [`PhoneRegion::for_raw`]
/// (a `+`-prefixed value keeps international rules; anything else is treated
/// as US), falling back to the trimmed raw value when no digits survive.
/// Email: lowercased. Username/Other: verbatim (trimmed). The second value is
/// the guard note, when the phone form was uncertain.
pub fn normalize_typed_handle(raw: &str, handle_type: HandleType) -> (String, Option<String>) {
    match handle_type {
        HandleType::Phone => {
            let guarded = normalize_guarded(raw, PhoneRegion::for_raw(raw));
            if guarded.normalized.is_empty() {
                // No usable digits: fall back to the raw, unflagged.
                (raw.trim().to_string(), None)
            } else {
                (guarded.normalized, guarded.note)
            }
        }
        HandleType::Email => (raw.trim().to_lowercase(), None),
        HandleType::Username | HandleType::Other => (raw.trim().to_string(), None),
    }
}

/// An address as a source wrote it, classified once: what kind of address
/// it is and the key it is stored and matched under.
///
/// [`Handle::parse`] is the one place an address is classified, from its raw
/// value with its `+` intact. The raw value travels with the handle, because
/// [`OwnerHandleSet::is_owner`] needs to know whether the source wrote a
/// number with its country or in national form.
#[derive(Debug, Clone)]
pub struct Handle {
    kind: HandleType,
    key: String,
    raw: String,
}

impl Handle {
    /// Classify an address:
    ///
    /// - one with `@` is an email address, keyed in lower case;
    /// - one written as a phone number (digits and phone punctuation, after
    ///   an optional `tel:`) is a phone number, keyed by
    ///   [`normalize_typed_handle`]: a `+` before the first digit keeps the
    ///   country, a number without one is read by US rules, and a short code
    ///   keeps its digits;
    /// - anything else is a sender name such as `AMAZON`, an identity of type
    ///   `other` keyed by the name as written.
    ///
    /// `None` when the value is blank.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        let value = raw.trim();
        if value.is_empty() {
            return None;
        }
        let kind = if value.contains('@') {
            HandleType::Email
        } else if is_written_as_a_number(value) {
            HandleType::Phone
        } else {
            HandleType::Other
        };
        Some(Self::typed(value, kind))
    }

    /// A handle of a kind the caller already knows.
    fn typed(raw: &str, kind: HandleType) -> Self {
        Self {
            kind,
            key: normalize_typed_handle(raw, kind).0,
            raw: raw.trim().to_string(),
        }
    }

    /// What kind of address this is.
    #[must_use]
    pub fn kind(&self) -> HandleType {
        self.kind
    }

    /// The key the address is stored and matched under.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The key, taken out of the handle.
    #[must_use]
    pub fn into_key(self) -> String {
        self.key
    }
}

/// Whether a value is written as a phone number: after an optional `tel:`,
/// at least one digit and nothing but digits and the punctuation a dialled
/// number carries.
fn is_written_as_a_number(value: &str) -> bool {
    let number = value
        .get(..4)
        .filter(|scheme| scheme.eq_ignore_ascii_case("tel:"))
        .map_or(value, |_| &value[4..]);
    number.chars().any(|c| c.is_ascii_digit())
        && number
            .chars()
            .all(|c| c.is_ascii_digit() || PHONE_PUNCTUATION.contains(&c) || c == '#' || c == '*')
}

/// All configured owner handles, each stored under its handle key, in the
/// order they were given.
///
/// The key is [`normalize_typed_handle`], the same function the server uses
/// for its `handles` rows and the contacts book uses for its entries, so an
/// owner phone written `+44 7700 900123` is `+447700900123` here too.
#[derive(Debug, Clone)]
pub struct OwnerHandleSet {
    handles: Vec<(String, HandleType)>,
}

impl OwnerHandleSet {
    /// Build the set from raw `(value, HandleType)` pairs; errors when the list
    /// is empty or a phone has no usable digits. The order is kept, and a
    /// handle given twice is kept once.
    ///
    /// # Errors
    ///
    /// Returns an error when `handles` is empty, or when a `HandleType::Phone`
    /// value sanitizes to no usable digits.
    pub fn new(handles: &[(String, HandleType)]) -> Result<Self> {
        if handles.is_empty() {
            bail!("the backup device's phone number or email address is required");
        }
        let mut keyed: Vec<(String, HandleType)> = Vec::new();
        for (raw, handle_type) in handles {
            if *handle_type == HandleType::Phone {
                sanitize_number(raw)
                    .with_context(|| format!("owner phone has no usable digits: {raw}"))?;
            }
            let entry = (normalize_typed_handle(raw, *handle_type).0, *handle_type);
            if !keyed.contains(&entry) {
                keyed.push(entry);
            }
        }
        Ok(Self { handles: keyed })
    }

    /// Whether an address is one of the owner's.
    ///
    /// Every handle matches by handle key. A phone also matches by its
    /// [`sanitize_number`] digits, for sources that record numbers with the
    /// `+` already gone.
    /// Without the `+`, `6591234567` could be Singapore or the US; the
    /// source has already thrown that information away.
    ///
    /// A value written without `+` also matches an owner number given with
    /// `+` when it is that number in national form: the national number
    /// alone, or the trunk prefix `0` followed by it. A carrier often lists
    /// the owner that way in an MMS, so `07700900123` is the owner
    /// `+447700900123`.
    pub fn is_owner(&self, handle: &Handle) -> bool {
        let Handle { kind, key, raw } = handle;
        if *kind != HandleType::Phone {
            return self.handles.iter().any(|(v, t)| t == kind && v == key);
        }
        let Some(digits) = sanitize_number(raw) else {
            return false;
        };
        let national_form = !raw.contains('+');
        self.phone_keys().any(|owner| {
            owner == key
                || sanitize_number(owner).is_some_and(|d| d == digits)
                || (national_form && is_national_form_of(&digits, owner))
        })
    }

    /// Convenience for exporters that only know about phone numbers.
    pub fn from_phones(phones: &[String]) -> Result<Self> {
        let handles: Vec<(String, HandleType)> = phones
            .iter()
            .map(|p| (p.clone(), HandleType::Phone))
            .collect();
        Self::new(&handles)
    }

    /// The handle key of the first owner phone given, for callers that need
    /// a single owner value (e.g. `owner_handle` in export metadata).
    ///
    /// `None` only when the set holds no phone-typed handles. A set built
    /// with [`OwnerHandleSet::from_phones`] always returns `Some`: that
    /// constructor rejects an empty list and types every entry as a phone.
    pub fn primary_owner_handle(&self) -> Option<String> {
        self.phone_keys().next().map(str::to_string)
    }

    /// The handle keys of the owner's phones, in the order given.
    fn phone_keys(&self) -> impl Iterator<Item = &str> {
        self.handles
            .iter()
            .filter(|(_, t)| *t == HandleType::Phone)
            .map(|(v, _)| v.as_str())
    }
}

/// Whether `digits` are the national form of the E.164 key `owner`: its
/// national number, alone or after the trunk prefix `0`. An owner key
/// without `+` has no country to strip, so nothing is its national form.
fn is_national_form_of(digits: &str, owner: &str) -> bool {
    let Some(owner_digits) = owner.strip_prefix('+') else {
        return false;
    };
    let (country, national) = split_country_calling_code(owner_digits, true);
    !country.is_empty() && (digits == national || digits.strip_prefix('0') == Some(national))
}

/// A group chat's id and display title from the [`Handle`] keys of its
/// non-owner participants. The id is `prefix` plus a length-prefixed slug of
/// the sorted, deduplicated keys, so `["12","34"]` and `["123","4"]` cannot
/// collide, hashed when it would pass 180 bytes so it stays a safe file
/// name. The title names up to four of the keys.
pub fn group_chat_id(prefix: &str, others: &[String]) -> (String, String) {
    let mut sorted = others.to_vec();
    sorted.sort();
    sorted.dedup();
    let title = if sorted.is_empty() {
        "Group".to_string()
    } else if sorted.len() <= 4 {
        format!("Group: {}", sorted.join(", "))
    } else {
        format!(
            "Group: {}, and {} others",
            sorted[..4].join(", "),
            sorted.len() - 4
        )
    };
    let id = format!("{prefix}{}", group_id_slug(&sorted));
    let id = if id.len() > 180 {
        let digest = hex::encode(Sha256::digest(id.as_bytes()));
        format!("{prefix}{}", &digest[..16])
    } else {
        id
    };
    (id, title)
}

/// Length-prefix each number so `["12","34"]` and `["123","4"]` cannot both
/// become `12_34`.
fn group_id_slug(digits: &[String]) -> String {
    digits
        .iter()
        .map(|d| format!("{}:{}", d.len(), d))
        .collect::<Vec<_>>()
        .join("_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_handle_policy_matches_the_server_for_international_numbers() {
        // The contacts book and the server must key this identically:
        // for_raw keeps the + signal, so the E.164 form survives.
        let (uk, note) = normalize_typed_handle("+44 20 7946 0000", HandleType::Phone);
        assert_eq!(uk, "+442079460000");
        assert!(note.is_none());
        let (us, _) = normalize_typed_handle("(555) 555-0100", HandleType::Phone);
        assert_eq!(us, "+15555550100");
        let (email, _) = normalize_typed_handle(" Bob@Example.COM ", HandleType::Email);
        assert_eq!(email, "bob@example.com");
        let (wordy, _) = normalize_typed_handle("no digits here", HandleType::Phone);
        assert_eq!(wordy, "no digits here");
    }

    #[test]
    fn sanitize_strips_plus_one() {
        assert_eq!(
            sanitize_number("+15555550100").as_deref(),
            Some("5555550100")
        );
        assert_eq!(
            sanitize_number("(555) 555-0101").as_deref(),
            Some("5555550101")
        );
        assert_eq!(sanitize_number(""), None);
        assert_eq!(sanitize_number("4"), None);
        assert_eq!(sanitize_number("06"), None);
    }

    /// The strict form, which is what a field of unknown content is measured
    /// against. See [`sanitize_phone_shaped`] for why the two differ.
    #[test]
    fn phone_shaped_rejects_prose_and_run_together_numbers() {
        // Written as a number, in every punctuation style a person uses.
        for written in [
            "+1 (555) 123-4567",
            "555.123.4567",
            "(555) 123-4567",
            "  5551234567  ",
        ] {
            assert_eq!(
                sanitize_phone_shaped(written).as_deref(),
                Some("5551234567"),
                "{written} is written as a number"
            );
        }

        // Short codes still pass, as they do for `sanitize_number`.
        assert_eq!(sanitize_phone_shaped("7535").as_deref(), Some("7535"));

        // Prose is rejected before any digits are collected, which is the
        // whole point: `sanitize_number` answers `201942` here.
        assert_eq!(
            sanitize_number("met in 2019 at 42 Acacia Avenue").as_deref(),
            Some("201942")
        );
        assert_eq!(
            sanitize_phone_shaped("met in 2019 at 42 Acacia Avenue"),
            None
        );
        assert_eq!(
            sanitize_phone_shaped("+15551234567 (see also +15557654321)"),
            None
        );
        assert_eq!(
            sanitize_phone_shaped("555-1234 x99"),
            None,
            "an extension is prose"
        );
        assert_eq!(sanitize_phone_shaped("home/cell"), None);

        // Two numbers with nothing but permitted punctuation between them are
        // caught by the digit ceiling instead.
        assert_eq!(sanitize_phone_shaped("+15551234567 +15557654321"), None);
        assert_eq!(
            sanitize_phone_shaped("123456789012345").as_deref(),
            Some("123456789012345"),
            "fifteen digits is the E.164 maximum, and is allowed"
        );
        assert_eq!(
            sanitize_phone_shaped("1234567890123456"),
            None,
            "sixteen is not"
        );

        // Too few digits, and nothing at all.
        assert_eq!(sanitize_phone_shaped("123"), None);
        assert_eq!(sanitize_phone_shaped(""), None);
        assert_eq!(sanitize_phone_shaped("   "), None);
        assert_eq!(sanitize_phone_shaped("()-."), None);
    }

    #[test]
    fn sanitize_keeps_short_codes() {
        assert_eq!(sanitize_number("7535").as_deref(), Some("7535"));
        assert_eq!(sanitize_number("73737").as_deref(), Some("73737"));
    }

    #[test]
    fn certain_usa() {
        assert_eq!(
            normalize_checked("(542).341-2398", PhoneRegion::Usa)
                .ok()
                .as_deref(),
            Some("+15423412398")
        );
        assert_eq!(
            normalize_checked("1-555-456-7890", PhoneRegion::Usa)
                .ok()
                .as_deref(),
            Some("+15554567890")
        );
        assert!(
            normalize_checked("1555-4567", PhoneRegion::Usa).is_err(),
            "too short for USA certainty"
        );
        assert!(normalize_checked("+442071838750", PhoneRegion::Usa).is_err());
    }

    #[test]
    fn guarded_certain_usa() {
        let g = normalize_guarded("5555550100", PhoneRegion::Usa);
        assert_eq!(g.normalized, "+15555550100");
        assert_eq!(g.note, None);
        let g = normalize_guarded("15555550100", PhoneRegion::Usa);
        assert_eq!(g.normalized, "+15555550100");
        assert_eq!(g.note, None);
    }

    #[test]
    fn guarded_certain_plus_prefixed() {
        let g = normalize_guarded("+44 20 7183 8750", PhoneRegion::International);
        assert_eq!(g.normalized, "+442071838750");
        assert_eq!(g.note, None);
    }

    #[test]
    fn guarded_rejects_fabricated_plus_zero() {
        // E.164 country codes never start with 0: `+020…` must not be trusted
        // as certain even when it has a plausible digit count.
        assert!(normalize_checked("+02079460000", PhoneRegion::International).is_err());
        let g = normalize_guarded("+02079460000", PhoneRegion::International);
        assert_eq!(g.normalized, "02079460000");
        assert_eq!(
            g.note.as_deref(),
            Some("international country code cannot start with 0")
        );
    }

    #[test]
    fn guarded_trunk_zero_stays_digits_with_note() {
        // `020 7946 0000` (11 digits not starting with 1): never `+02079460000`.
        let g = normalize_guarded("020 7946 0000", PhoneRegion::Usa);
        assert_eq!(g.normalized, "02079460000");
        assert_eq!(
            g.note.as_deref(),
            Some("USA needs 10 digits or 11 starting with 1")
        );
        // `442079460000` without `+`: ambiguous in both regions.
        let g = normalize_guarded("442079460000", PhoneRegion::International);
        assert_eq!(g.normalized, "442079460000");
        assert_eq!(
            g.note.as_deref(),
            Some("international mode requires a leading +")
        );
    }

    #[test]
    fn guarded_region_for_raw() {
        assert_eq!(
            PhoneRegion::for_raw("+15555550100"),
            PhoneRegion::International
        );
        assert_eq!(
            PhoneRegion::for_raw("  +44 20 7183 8750 "),
            PhoneRegion::International
        );
        assert_eq!(PhoneRegion::for_raw("5555550100"), PhoneRegion::Usa);
        assert_eq!(PhoneRegion::for_raw("020 7946 0000"), PhoneRegion::Usa);
        assert_eq!(PhoneRegion::for_raw(""), PhoneRegion::Usa);
    }

    #[test]
    fn certain_international() {
        assert_eq!(
            normalize_checked("+44 20 7183 8750", PhoneRegion::International)
                .ok()
                .as_deref(),
            Some("+442071838750")
        );
        assert!(
            normalize_checked("(542).341-2398", PhoneRegion::International).is_err(),
            "no leading +"
        );
        assert_eq!(
            normalize_checked("+1-542-341-2398", PhoneRegion::International)
                .ok()
                .as_deref(),
            Some("+15423412398")
        );
    }

    #[test]
    fn checked_rejects_with_fixed_reasons() {
        assert_eq!(
            normalize_checked("+12", PhoneRegion::International),
            Err("international needs 8–15 digits after +".into())
        );
        assert_eq!(
            normalize_checked("442079460000", PhoneRegion::International),
            Err("international mode requires a leading +".into())
        );
        assert_eq!(
            normalize_checked("", PhoneRegion::Usa),
            Err("empty phone".into())
        );
        assert_eq!(
            normalize_checked("abc", PhoneRegion::Usa),
            Err("no digits".into())
        );
    }

    #[test]
    fn lenient_one_arg_matches_guarded_for_raw() {
        assert_eq!(normalize_lenient("(555) 555-0100"), "+15555550100");
        assert_eq!(normalize_lenient("+44 20 7183 8750"), "+442071838750");
        assert_eq!(normalize_lenient("020 7946 0000"), "02079460000");
        assert_eq!(normalize_lenient("+02079460000"), "02079460000");
        assert_eq!(normalize_lenient("7535"), "7535");
        assert_eq!(normalize_lenient("no digits here"), "");
    }

    #[test]
    fn digits_us_one_arg_sanitizes_then_guards() {
        assert_eq!(
            normalize_digits_us("+1 (555) 555-0100").as_deref(),
            Some("+15555550100")
        );
        assert_eq!(normalize_digits_us("7535").as_deref(), Some("7535"));
        assert_eq!(
            normalize_digits_us("020 7946 0000").as_deref(),
            Some("02079460000")
        );
        assert_eq!(normalize_digits_us("06"), None);
        assert_eq!(normalize_digits_us(""), None);
    }

    #[test]
    fn primary_owner_handle_present_for_phone_sets() {
        let owners = OwnerHandleSet::from_phones(&["(555) 555-0100".into()]).unwrap();
        assert_eq!(
            owners.primary_owner_handle().as_deref(),
            Some("+15555550100")
        );
        let trunk = OwnerHandleSet::from_phones(&["020 7946 0000".into()]).unwrap();
        assert_eq!(trunk.primary_owner_handle().as_deref(), Some("02079460000"));
        // Only a phone-free set has no primary owner handle.
        let email_only =
            OwnerHandleSet::new(&[("a@example.com".into(), HandleType::Email)]).unwrap();
        assert_eq!(email_only.primary_owner_handle(), None);
    }

    #[test]
    fn owner_set_rejects_empty() {
        // The exporters have no command line, so the message names no flag.
        assert_eq!(
            OwnerHandleSet::new(&[]).unwrap_err().to_string(),
            "the backup device's phone number or email address is required"
        );
        assert!(OwnerHandleSet::from_phones(&[]).is_err());
        assert!(OwnerHandleSet::from_phones(&["not-a-phone".into()]).is_err());
    }

    #[test]
    fn owner_handle_set_matches_typed_handles() {
        let owners = OwnerHandleSet::new(&[
            ("(555) 555-0100".into(), HandleType::Phone),
            ("Person@Example.COM".into(), HandleType::Email),
        ])
        .unwrap();
        assert!(owners.is_owner(&Handle::typed("+15555550100", HandleType::Phone)));
        assert!(owners.is_owner(&Handle::typed("5555550100", HandleType::Phone)));
        assert!(!owners.is_owner(&Handle::typed("5555550199", HandleType::Phone)));
        assert!(owners.is_owner(&Handle::typed("person@example.com", HandleType::Email)));
        assert!(!owners.is_owner(&Handle::typed("other@example.com", HandleType::Email)));
        // A phone-shaped handle is not treated as an email or username handle.
        assert!(!owners.is_owner(&Handle::typed("(555) 555-0100", HandleType::Email)));
        assert!(!owners.is_owner(&Handle::typed("Person@Example.COM", HandleType::Username)));
    }

    #[test]
    fn the_first_owner_phone_given_is_the_primary_owner_handle() {
        // Nine numbers: a set's order would pick the first given only by chance.
        let phones: Vec<String> = [9, 0, 1, 2, 3, 4, 5, 6, 7]
            .iter()
            .map(|i| format!("+1555555010{i}"))
            .collect();
        let owners = OwnerHandleSet::from_phones(&phones).unwrap();
        assert_eq!(
            owners.primary_owner_handle().as_deref(),
            Some("+15555550109")
        );
    }

    #[test]
    fn an_owner_given_with_plus_matches_its_national_form() {
        let owners = OwnerHandleSet::from_phones(&["+447700900123".into()]).unwrap();
        assert!(owners.is_owner(&Handle::typed("07700900123", HandleType::Phone)));
        assert!(owners.is_owner(&Handle::typed("07700 900123", HandleType::Phone)));
        assert!(owners.is_owner(&Handle::typed("7700900123", HandleType::Phone)));
        assert!(!owners.is_owner(&Handle::typed("07700900124", HandleType::Phone)));
        assert!(!owners.is_owner(&Handle::typed("0447700900123", HandleType::Phone)));
        // A value with `+` is in international form and names its country.
        assert!(!owners.is_owner(&Handle::typed("+7700900123", HandleType::Phone)));
        // An owner given without `+` has no country to strip.
        let local = OwnerHandleSet::from_phones(&["020 7946 0000".into()]).unwrap();
        assert!(!local.is_owner(&Handle::typed("2079460000", HandleType::Phone)));
    }

    #[test]
    fn an_international_owner_keeps_its_country() {
        let owners = OwnerHandleSet::from_phones(&["+44 7700 900123".into()]).unwrap();
        assert_eq!(
            owners.primary_owner_handle().as_deref(),
            Some("+447700900123")
        );
        assert!(owners.is_owner(&Handle::typed("+447700900123", HandleType::Phone)));
        // A source that has already dropped the `+` matches by digits.
        assert!(owners.is_owner(&Handle::typed("447700900123", HandleType::Phone)));
        assert!(!owners.is_owner(&Handle::typed("447700900999", HandleType::Phone)));
        assert!(!owners.is_owner(&Handle::typed("06", HandleType::Phone)));
    }

    #[test]
    fn owner_handle_set_guards_trunk_zero() {
        let owners =
            OwnerHandleSet::new(&[("020 7946 0000".to_string(), HandleType::Phone)]).unwrap();
        assert!(owners.is_owner(&Handle::typed("02079460000", HandleType::Phone)));
        assert!(owners.is_owner(&Handle::typed("020 7946 0000", HandleType::Phone)));
        // The digits-as-is identity is never fabricated into +02079460000, so
        // a +0… message handle matches through the same digit stripping.
        assert!(owners.is_owner(&Handle::typed("+02079460000", HandleType::Phone)));
        assert!(!owners.is_owner(&Handle::typed("+02079469999", HandleType::Phone)));
    }

    #[test]
    fn a_plus_before_the_first_digit_keeps_the_country() {
        for raw in ["(+44) 7700 900123", "tel:+447700900123"] {
            assert_eq!(
                normalize_typed_handle(raw, HandleType::Phone).0,
                "+447700900123",
                "{raw}"
            );
        }
    }

    fn parsed(raw: &str) -> (HandleType, String) {
        let handle = Handle::parse(raw).unwrap();
        (handle.kind(), handle.key().to_string())
    }

    #[test]
    fn a_number_written_with_plus_keeps_its_country() {
        assert_eq!(
            parsed("+6591234567"),
            (HandleType::Phone, "+6591234567".into())
        );
        assert_eq!(
            parsed("+447700900123"),
            (HandleType::Phone, "+447700900123".into())
        );
        assert_eq!(
            parsed("tel:+447700900123"),
            (HandleType::Phone, "+447700900123".into())
        );
    }

    #[test]
    fn a_number_without_plus_is_read_as_a_us_number_and_a_short_code_keeps_its_digits() {
        assert_eq!(
            parsed("(555) 555-0100"),
            (HandleType::Phone, "+15555550100".into())
        );
        assert_eq!(
            parsed("15555550100"),
            (HandleType::Phone, "+15555550100".into())
        );
        assert_eq!(
            parsed("020 7946 0000"),
            (HandleType::Phone, "02079460000".into())
        );
        assert_eq!(parsed("7535"), (HandleType::Phone, "7535".into()));
    }

    #[test]
    fn an_address_with_at_is_an_email_address_not_the_digits_inside_it() {
        assert_eq!(
            parsed(" John1985@Example.com "),
            (HandleType::Email, "john1985@example.com".into())
        );
    }

    #[test]
    fn an_address_that_is_neither_a_number_nor_an_email_is_a_sender_name() {
        assert_eq!(parsed("AMAZON"), (HandleType::Other, "AMAZON".into()));
        assert_eq!(parsed(" Bank 24 "), (HandleType::Other, "Bank 24".into()));
        assert!(Handle::parse("   ").is_none());
    }

    #[test]
    fn a_handle_key_parses_to_itself() {
        for raw in [
            "+6591234567",
            "5555550100",
            "7535",
            "jo@example.com",
            "AMAZON",
        ] {
            let (kind, key) = parsed(raw);
            assert_eq!(parsed(&key), (kind, key.clone()), "{raw}");
        }
    }

    #[test]
    fn the_owner_test_takes_a_handle() {
        let owners = OwnerHandleSet::from_phones(&["+447700900123".into()]).unwrap();
        assert!(owners.is_owner(&Handle::parse("07700 900123").unwrap()));
        assert!(owners.is_owner(&Handle::parse("7700900123").unwrap()));
        assert!(!owners.is_owner(&Handle::parse("AMAZON").unwrap()));
    }

    fn keys(count: usize) -> Vec<String> {
        (0..count).map(|i| format!("+155555501{i:02}")).collect()
    }

    fn numbers(count: usize) -> Vec<String> {
        (0..count).map(|i| format!("55555501{i:02}")).collect()
    }

    #[test]
    fn group_chat_id_names_up_to_four_numbers() {
        let others = vec![
            "+15555550102".to_string(),
            "+15555550101".to_string(),
            "+15555550102".to_string(),
        ];
        assert_eq!(
            group_chat_id("grp-", &others),
            (
                "grp-12:+15555550101_12:+15555550102".to_string(),
                "Group: +15555550101, +15555550102".to_string()
            ),
            "sorted, deduplicated, length-prefixed id; the keys in the title"
        );
        assert_eq!(group_chat_id("grp-", &[]).1, "Group");
        assert_eq!(
            group_chat_id("grp-", &keys(4)).1,
            "Group: +15555550100, +15555550101, +15555550102, +15555550103"
        );
    }

    #[test]
    fn a_group_title_keeps_each_number_s_country() {
        let others = ["+6591234567".to_string(), "+447700900123".to_string()];
        assert_eq!(
            group_chat_id("grp-", &others).1,
            "Group: +447700900123, +6591234567"
        );
    }

    #[test]
    fn group_chat_id_counts_the_numbers_past_four() {
        assert_eq!(
            group_chat_id("grp-", &keys(5)).1,
            "Group: +15555550100, +15555550101, +15555550102, +15555550103, and 1 others"
        );
    }

    #[test]
    fn group_chat_id_hashes_an_id_past_180_bytes() {
        // 20 numbers make a 283-byte id. The expected value is the first 16
        // hex digits of SHA-256 over that id, computed with Python's hashlib.
        let mut others = numbers(20);
        assert_eq!(group_chat_id("grp-", &others).0, "grp-03ecb51fee88d135");
        others.reverse();
        assert_eq!(
            group_chat_id("grp-", &others).0,
            "grp-03ecb51fee88d135",
            "the order the numbers arrive in doesn't change the id"
        );

        // 12 numbers behind a 13-byte prefix make exactly 180 bytes: kept.
        let (id, _) = group_chat_id("sms-backup-g-", &numbers(12));
        assert_eq!(id.len(), 180);
        assert!(id.starts_with("sms-backup-g-10:5555550100_"), "{id}");
    }
}
