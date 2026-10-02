//! Builds the Demo Account's phone numbers, all from ranges set aside for fiction.
//!
//! The Demo Account ships with every Message Crate, so a number that could be
//! dialled might belong to a real person, who would then appear beside made-up
//! names and messages. Two ranges are reserved so that cannot happen:
//!
//! - North American numbers 555-0100 to 555-0199, in every area code (NANPA).
//! - UK mobile numbers 07700 900000 to 07700 900999, which Ofcom keeps for drama.

use std::collections::HashSet;

use anyhow::{Result, bail};
use rand::Rng;
use rand::RngExt;
use rand::seq::IndexedRandom;

/// Demo owner phone number. Matches the value in `crates/server/demo-seed/config/seed.toml`.
pub const OWNER_PHONE: &str = "+14155550100";

/// North American area codes. Each holds the 100 fictional numbers
/// 555-0100 to 555-0199, so the list gives 4,200.
const US_AREA: &[u16] = &[
    201, 202, 212, 213, 214, 215, 216, 301, 303, 305, 310, 312, 313, 314, 315, 404, 407, 408, 415,
    416, 503, 512, 516, 617, 619, 626, 650, 702, 703, 713, 718, 773, 786, 801, 818, 832, 858, 901,
    916, 917, 929, 971,
];

/// Area codes for the phone-number chat identifiers of group conversations.
/// None of them is in [`US_AREA`], so a group never shares its number with a
/// person, and the import keeps the two conversations apart.
const GROUP_AREA: &[u16] = &[206, 307, 406, 505];

/// The line numbers after the 555 exchange that are reserved for fiction.
const US_LINES: std::ops::Range<u16> = 100..200;

/// Ofcom's drama range is this prefix followed by three digits.
const UK_DRAMA_PREFIX: &str = "+447700900";

/// The three digits after [`UK_DRAMA_PREFIX`].
const UK_LINES: std::ops::Range<u16> = 0..1000;

/// Pick a phone number that is not in `used`, and add it there. A share of
/// `us_probability` of them are North American; the rest are from the UK.
///
/// The caller puts the numbers the generator writes by name, such as
/// [`OWNER_PHONE`], into `used` first, so none of them is handed to a contact.
/// When the number drawn is taken, the next free one after it in the
/// fictional ranges is used, without drawing again.
///
/// Every call takes the same values from `rng`, whatever `used` holds. The
/// names, groups and messages drawn after it are therefore the ones the
/// generator drew before its numbers moved into the fictional ranges, and
/// the Demo Data keeps the people that searches and the user guide name.
///
/// # Errors
///
/// Returns an error when every number in the fictional ranges is in `used`.
pub fn generate_phone(
    rng: &mut impl Rng,
    us_probability: f64,
    used: &mut HashSet<String>,
) -> Result<String> {
    let phone = if rng.random_bool(us_probability) {
        us_phone(rng)
    } else {
        uk_phone(rng)
    };
    if used.insert(phone.clone()) {
        return Ok(phone);
    }
    let all: Vec<String> = all_fictional_phones().collect();
    let start = all.iter().position(|taken| *taken == phone).unwrap_or(0);
    let Some(free) = all[start..]
        .iter()
        .chain(&all[..start])
        .find(|candidate| !used.contains(*candidate))
        .cloned()
    else {
        bail!(
            "the demo needs more phone numbers than the {} in the fictional ranges",
            all.len()
        );
    };
    used.insert(free.clone());
    Ok(free)
}

/// The phone-number chat identifier of the group at `index`: a different
/// number for each index, or `None` once the 400 numbers of [`GROUP_AREA`]
/// are used up.
pub fn group_chat_phone(index: usize) -> Option<String> {
    let area = *GROUP_AREA.get(index / US_LINES.len())?;
    let line = US_LINES.start + u16::try_from(index % US_LINES.len()).ok()?;
    Some(format_us(area, line))
}

/// Build a North American number in the 555-0100 to 555-0199 range.
///
/// It draws an exchange and four digits, as the generator did when its
/// numbers could be dialled, so the draws after it stay where they were. The
/// exchange is not used, and the last two digits pick the line.
fn us_phone(rng: &mut impl Rng) -> String {
    let area = *US_AREA.choose(rng).unwrap_or(&415);
    // `i32`, the type the draws had then: another type can take other values
    // from `rng`.
    let _exchange: i32 = rng.random_range(200..1000);
    let station: i32 = rng.random_range(0..10_000);
    let line = u16::try_from(station % 100).unwrap_or(0);
    format_us(area, US_LINES.start + line)
}

/// The national number lengths of the seven countries the generator once
/// drew from (UK, Australia, France, Germany, Japan, Mexico, India). Only how
/// many digits [`uk_phone`] draws depends on them.
const OLD_INTL_LENGTHS: [usize; 7] = [10, 9, 9, 10, 10, 10, 10];

/// Build a UK mobile number in Ofcom's drama range.
///
/// It draws a country and that country's number of digits, as the generator
/// did when its numbers could be dialled, so the draws after it stay where
/// they were. The last three digits pick the line.
fn uk_phone(rng: &mut impl Rng) -> String {
    let national_len = *OLD_INTL_LENGTHS.choose(rng).unwrap_or(&10);
    // `u32`, the type the draws had then: another type can take other values
    // from `rng`.
    let mut line: u32 = rng.random_range(1..10);
    for _ in 1..national_len {
        line = (line * 10 + rng.random_range(0..10u32)) % 1000;
    }
    format_uk(u16::try_from(line).unwrap_or(0))
}

fn format_us(area: u16, line: u16) -> String {
    format!("+1{area}555{line:04}")
}

fn format_uk(line: u16) -> String {
    format!("{UK_DRAMA_PREFIX}{line:03}")
}

/// Every number [`generate_phone`] can return, North American first.
fn all_fictional_phones() -> impl Iterator<Item = String> {
    US_AREA
        .iter()
        .flat_map(|&area| US_LINES.map(move |line| format_us(area, line)))
        .chain(UK_LINES.map(format_uk))
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;

    use super::*;

    #[test]
    fn a_full_draw_falls_back_to_the_first_free_number_and_errors_only_when_none_is_left() {
        let mut rng = ChaCha8Rng::seed_from_u64(1);
        let mut used: HashSet<String> = all_fictional_phones().collect();
        let total = used.len();
        assert_eq!(total, 5200, "42 area codes x 100 lines, and 1,000 UK lines");

        used.remove("+12165550142");
        let phone = generate_phone(&mut rng, 0.9, &mut used).expect("one number is free");
        assert_eq!(phone, "+12165550142");
        assert_eq!(used.len(), total);

        assert!(generate_phone(&mut rng, 0.9, &mut used).is_err());
    }

    #[test]
    fn group_chat_phones_are_distinct_and_never_a_number_a_person_can_draw() {
        let people: HashSet<String> = all_fictional_phones().collect();
        let groups: Vec<String> = (0..400)
            .map(|index| group_chat_phone(index).expect("400 group numbers"))
            .collect();
        let distinct: HashSet<&String> = groups.iter().collect();
        assert_eq!(distinct.len(), 400);
        assert!(groups.iter().all(|phone| !people.contains(phone)));
        assert_eq!(group_chat_phone(400), None);
    }
}
