//! Builds phone numbers that look like US and international numbers.

use std::collections::HashSet;

use rand::Rng;
use rand::RngExt;
use rand::seq::IndexedRandom;

/// Demo owner phone number. Matches the value in `crates/server/demo-seed/config/seed.toml`.
pub const OWNER_PHONE: &str = "+14155559000";

/// US area codes that look real. Generated numbers skip the 555 exchange.
const US_AREA: &[u16] = &[
    201, 202, 212, 213, 214, 215, 216, 301, 303, 305, 310, 312, 313, 314, 315, 404, 407, 408, 415,
    416, 503, 512, 516, 617, 619, 626, 650, 702, 703, 713, 718, 773, 786, 801, 818, 832, 858, 901,
    916, 917, 929, 971,
];

#[derive(Clone, Copy)]
struct IntlPattern {
    dial: &'static str,
    national_len: usize,
}

const INTL: &[IntlPattern] = &[
    IntlPattern {
        dial: "44",
        national_len: 10,
    },
    IntlPattern {
        dial: "61",
        national_len: 9,
    },
    IntlPattern {
        dial: "33",
        national_len: 9,
    },
    IntlPattern {
        dial: "49",
        national_len: 10,
    },
    IntlPattern {
        dial: "81",
        national_len: 10,
    },
    IntlPattern {
        dial: "52",
        national_len: 10,
    },
    IntlPattern {
        dial: "91",
        national_len: 10,
    },
];

/// Pick a unique phone number. Most numbers look like US numbers when
/// `us_probability` is high; otherwise they use other country codes.
///
/// The demo owner's number is never returned. After many collisions, a
/// `+1555…` number is used so generation can still finish.
pub fn generate_phone(
    rng: &mut impl Rng,
    us_probability: f64,
    used: &mut HashSet<String>,
) -> String {
    for _ in 0..64 {
        let phone = if rng.random_bool(us_probability) {
            us_phone(rng)
        } else {
            intl_phone(rng)
        };
        if phone != OWNER_PHONE && used.insert(phone.clone()) {
            return phone;
        }
    }
    let fallback = format!("+1555{:07}", rng.random_range(1_000_000..9_000_000));
    used.insert(fallback.clone());
    fallback
}

/// Build a US number as `+1` plus area code, exchange, and line number.
///
/// The 555 exchange is skipped so the numbers do not look like movie placeholders.
fn us_phone(rng: &mut impl Rng) -> String {
    let area = *US_AREA.choose(rng).unwrap_or(&415);
    let mut exchange = rng.random_range(200..1000);
    if exchange == 555 {
        exchange = 556;
    }
    let station = rng.random_range(0..10_000);
    format!("+1{area}{exchange:03}{station:04}")
}

/// Build a non-US number from a country code and a random national number.
fn intl_phone(rng: &mut impl Rng) -> String {
    let pat = *INTL.choose(rng).unwrap_or(&INTL[0]);
    let mut national = String::with_capacity(pat.national_len);
    national.push(char::from_digit(rng.random_range(1..10), 10).unwrap());
    for _ in 1..pat.national_len {
        national.push(char::from_digit(rng.random_range(0..10), 10).unwrap());
    }
    format!("+{}{}", pat.dial, national)
}
