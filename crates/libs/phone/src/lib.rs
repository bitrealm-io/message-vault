//! Shared US-centric phone-number parsing for message converters.

use std::collections::HashSet;
use std::fmt;

use anyhow::{Context, Result, bail};
use message_ir::HandleType;
use sha2::{Digest, Sha256};

/// Minimum digit length after stripping formatting.
///
/// Allows 4–6 digit short codes (carrier/bank SMS, e.g. AT&T `7535`).
/// Rejects junk like `"4"` or `"06"`.
const MIN_PHONE_DIGITS: usize = 4;

/// Region rules for [`normalize_checked`] (contacts validation only).
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
    /// Region for a loader-side raw value: a `+`-prefixed value is already
    /// unambiguous E.164 (international rules apply); anything else is treated
    /// as a US national number (this crate's home region).
    pub fn for_raw(raw: &str) -> Self {
        if raw.trim().starts_with('+') {
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

/// E.164 when the parse is unambiguous for `region`, else the human-readable
/// reason it is not.
///
/// Unlike [`sanitize_number`], this does **not** accept short codes or
/// ambiguous lengths. Contacts validation uses this before rewriting files.
/// The error strings are fixed so the validate log groups failures under one
/// header per reason.
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
/// attach a human-readable reason so the vault can show it for review.
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

/// One normalization policy for a typed handle, shared by the vault, the
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

/// All configured owner handles (normalized, typed).
#[derive(Debug, Clone)]
pub struct OwnerHandleSet {
    handles: HashSet<(String, HandleType)>,
}

impl OwnerHandleSet {
    /// Build the set from raw `(value, HandleType)` pairs; errors when the list
    /// is empty or a phone has no usable digits.
    ///
    /// # Errors
    ///
    /// Returns an error when `handles` is empty, or when a `HandleType::Phone`
    /// value sanitizes to no usable digits.
    pub fn new(handles: &[(String, HandleType)]) -> Result<Self> {
        if handles.is_empty() {
            bail!("owner handle required: pass --owner-phone or --owner-handle");
        }
        let mut set = HashSet::new();
        for (raw, handle_type) in handles {
            let normalized = match handle_type {
                HandleType::Phone => {
                    let d = sanitize_number(raw)
                        .with_context(|| format!("owner phone has no usable digits: {raw}"))?;
                    // Guarded policy on the sanitized digits (same shape both
                    // sides), so matching stays consistent and trunk-zero
                    // values are never fabricated into `+0…`.
                    normalize_guarded(&d, PhoneRegion::Usa).normalized
                }
                HandleType::Email => raw.trim().to_lowercase(),
                HandleType::Username | HandleType::Other => raw.trim().to_string(),
            };
            set.insert((normalized, *handle_type));
        }
        Ok(Self { handles: set })
    }

    /// Whether a raw handle value plus type matches an owner in the set after
    /// the same normalization.
    pub fn is_owner(&self, raw: &str, handle_type: HandleType) -> bool {
        let normalized = match handle_type {
            HandleType::Phone => {
                let Some(d) = sanitize_number(raw) else {
                    return false;
                };
                normalize_guarded(&d, PhoneRegion::Usa).normalized
            }
            HandleType::Email => raw.trim().to_lowercase(),
            HandleType::Username | HandleType::Other => raw.trim().to_string(),
        };
        self.handles.contains(&(normalized, handle_type))
    }

    /// Convenience for exporters that only know about phone numbers.
    pub fn from_phones(phones: &[String]) -> Result<Self> {
        let handles: Vec<(String, HandleType)> = phones
            .iter()
            .map(|p| (p.clone(), HandleType::Phone))
            .collect();
        Self::new(&handles)
    }

    /// All sanitized phone digits in the set. Other handle types are skipped.
    ///
    /// The values are raw digit strings (no `+` prefix, no formatting) so they
    /// compare correctly against `sanitize_number` output in callers such as
    /// `sbr::parse_mms`.
    pub fn all_phone_digits(&self) -> HashSet<String> {
        self.handles
            .iter()
            .filter_map(|(v, t)| phone_digits_if_phone(v, *t))
            .collect()
    }

    /// First sanitized phone digit, for callers that need a single
    /// representative owner phone (e.g. guarded E.164 normalization).
    pub fn primary_phone_digit(&self) -> Option<&str> {
        self.handles
            .iter()
            .find(|(_, t)| *t == HandleType::Phone)
            .map(|(v, _)| v.as_str())
    }

    /// The guarded-normalized primary owner phone, for callers that need one
    /// representative owner value (e.g. `owner_handle` in export metadata).
    ///
    /// `None` only when the set holds no phone-typed handles. A set built
    /// with [`OwnerHandleSet::from_phones`] always returns `Some`: that
    /// constructor rejects an empty list and types every entry as a phone.
    pub fn primary_owner_handle(&self) -> Option<String> {
        self.primary_phone_digit()
            .map(|d| normalize_guarded(d, PhoneRegion::Usa).normalized)
    }
}

/// Strip a stored phone handle back to digits for comparison with [`sanitize_number`].
fn phone_digits_if_phone(value: &str, handle_type: HandleType) -> Option<String> {
    if handle_type != HandleType::Phone {
        return None;
    }
    Some(sanitize_number(value).unwrap_or_else(|| value.to_string()))
}

/// A group chat's id and display title from the digits of its non-owner
/// participants. The id is `prefix` plus a length-prefixed slug of the
/// sorted, deduplicated numbers, so `["12","34"]` and `["123","4"]` cannot
/// collide, hashed when it would pass 180 bytes so it stays a safe file
/// name. The title names up to four numbers in E.164 when unambiguous.
pub fn group_chat_id(prefix: &str, others: &[String]) -> (String, String) {
    let mut sorted = others.to_vec();
    sorted.sort();
    sorted.dedup();
    let title = if sorted.is_empty() {
        "Group".to_string()
    } else if sorted.len() <= 4 {
        format!("Group: {}", join_e164_phones(&sorted))
    } else {
        format!(
            "Group: {}, and {} others",
            join_e164_phones(&sorted[..4]),
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

/// Digit strings as E.164 when unambiguous, joined with `", "`.
fn join_e164_phones(digits: &[String]) -> String {
    digits
        .iter()
        .map(|d| normalize_digits_us(d).unwrap_or_default())
        .collect::<Vec<_>>()
        .join(", ")
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
    fn typed_handle_policy_matches_the_vault_for_international_numbers() {
        // The contacts book and the vault must key this identically:
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
        assert!(OwnerHandleSet::new(&[]).is_err());
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
        assert!(owners.is_owner("+15555550100", HandleType::Phone));
        assert!(owners.is_owner("5555550100", HandleType::Phone));
        assert!(!owners.is_owner("5555550199", HandleType::Phone));
        assert!(owners.is_owner("person@example.com", HandleType::Email));
        assert!(!owners.is_owner("other@example.com", HandleType::Email));
        // A phone-shaped handle is not treated as an email or username handle.
        assert!(!owners.is_owner("(555) 555-0100", HandleType::Email));
        assert!(!owners.is_owner("Person@Example.COM", HandleType::Username));
        // all_phone_digits returns sanitized (digits-only) form so callers
        // that compare against sanitize_number output match consistently.
        let digits = owners.all_phone_digits();
        assert!(digits.contains("5555550100"));
        assert!(
            !digits.contains("+15555550100"),
            "must be digits-only, not E.164"
        );
    }

    #[test]
    fn owner_handle_set_guards_trunk_zero() {
        let owners =
            OwnerHandleSet::new(&[("020 7946 0000".to_string(), HandleType::Phone)]).unwrap();
        assert!(owners.is_owner("02079460000", HandleType::Phone));
        assert!(owners.is_owner("020 7946 0000", HandleType::Phone));
        // The digits-as-is identity is never fabricated into +02079460000, so
        // a +0… message handle matches through the same digit stripping.
        assert!(owners.is_owner("+02079460000", HandleType::Phone));
        assert!(!owners.is_owner("+02079469999", HandleType::Phone));
    }
}
