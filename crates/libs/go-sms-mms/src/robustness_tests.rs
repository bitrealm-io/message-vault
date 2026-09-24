//! Truncated and corrupted PDUs must never panic the decoder, and whatever
//! it does return must hold the parser's invariants.
//!
//! Every seed PDU is cut at every length, then mutated many times over with a
//! fixed-seed generator, so a failure names a seed, a case index, and the
//! mutations that produced it, and reruns the same way every time.

use crate::pdu::parse_pdu_bytes;
use crate::testutil::{BuildPart, PduBuilder};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

const CASES_PER_SEED: u64 = 2_000;

/// A small linear congruential generator: deterministic, no dependency.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    /// A number in `0..n`; `n` must be above zero.
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Bytes that mean something to the decoder: header codes, value-length
/// escapes, NUL terminators, quotes, and the multipart media id.
const INTERESTING: &[u8] = &[
    0x00, 0x01, 0x1e, 0x1f, 0x22, 0x7f, 0x80, 0x81, 0x83, 0x84, 0x85, 0x89, 0x8c, 0x8e, 0x97, 0xb3,
    0xc0, 0xea, 0xfe, 0xff,
];

/// Apply one random mutation and describe it.
fn mutate(rng: &mut Lcg, data: &mut Vec<u8>) -> String {
    if data.is_empty() {
        data.push(INTERESTING[rng.below(INTERESTING.len())]);
        return "push to empty".into();
    }
    let at = rng.below(data.len());
    match rng.below(8) {
        0 => {
            let len = rng.below(data.len() + 1);
            data.truncate(len);
            format!("truncate to {len}")
        }
        1 => {
            let bit = rng.below(8);
            data[at] ^= 1 << bit;
            format!("flip bit {bit} at {at}")
        }
        2 => {
            let b = rng.next() as u8;
            data[at] = b;
            format!("set {at} to {b:#04x}")
        }
        3 => {
            let b = INTERESTING[rng.below(INTERESTING.len())];
            data[at] = b;
            format!("set {at} to {b:#04x}")
        }
        4 => {
            let n = 1 + rng.below(8);
            let bytes: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
            data.splice(at..at, bytes);
            format!("insert {n} bytes at {at}")
        }
        5 => {
            let n = 1 + rng.below(16.min(data.len() - at));
            data.drain(at..at + n);
            format!("delete {n} bytes at {at}")
        }
        6 => {
            // A value-length that claims far more than is there.
            let run = 1 + rng.below(4);
            data.splice(
                at..at,
                [0x1f].into_iter().chain(std::iter::repeat_n(0xff, run)),
            );
            format!("insert 0x1f + {run}x0xff at {at}")
        }
        _ => {
            let n = 1 + rng.below(32.min(data.len() - at));
            let copy = data[at..at + n].to_vec();
            let to = rng.below(data.len() + 1);
            data.splice(to..to, copy);
            format!("copy {n} bytes from {at} to {to}")
        }
    }
}

fn is_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    needle.is_empty() || haystack.windows(needle.len()).any(|w| w == needle)
}

/// Run the parser on `data`. `Err` carries a panic message or a broken invariant.
fn check(data: &[u8]) -> Result<(), String> {
    let path = Path::new("I_1609459200_fuzz.pdu");
    let parsed =
        catch_unwind(AssertUnwindSafe(|| parse_pdu_bytes(path, data))).map_err(|panic| {
            panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_string()))
                .unwrap_or_else(|| "panic".into())
        })?;
    let Ok(p) = parsed else {
        return Ok(());
    };
    if p.timestamp <= 0 {
        return Err(format!("timestamp {} not positive", p.timestamp));
    }
    for (i, a) in p.attachments.iter().enumerate() {
        if !is_subslice(data, &a.data) {
            return Err(format!("attachment {i} bytes are not from the input"));
        }
        if a.content_type.is_empty() {
            return Err(format!("attachment {i} has no content type"));
        }
    }
    let mut seen = std::collections::HashSet::new();
    for n in p.sender.iter().chain(&p.recipients) {
        if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
            return Err(format!("number {n:?} is not digits"));
        }
    }
    for n in &p.recipients {
        if !seen.insert(n) {
            return Err(format!("recipient {n:?} listed twice"));
        }
    }
    if p.is_sent && p.sender.is_some() {
        return Err("a sent message with a sender".into());
    }
    Ok(())
}

fn seeds() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        (
            "received with picture",
            PduBuilder::received("+14075551234")
                .to("+15555550100")
                .subject("Seed subject")
                .text("Seed body text")
                .part(BuildPart::jpeg("pic.jpg", 84))
                .build(),
        ),
        (
            "sent to a group",
            PduBuilder::sent()
                .to("+14075551234")
                .to("+14075559876")
                .cc("+15555550100")
                .text("hello all")
                .build(),
        ),
        ("stub", b"application/smil\0".to_vec()),
    ]
}

fn hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn every_seed_parses_before_it_is_mutated() {
    for (name, seed) in seeds() {
        let result = parse_pdu_bytes(Path::new("I_1609459200_seed.pdu"), &seed);
        if name == "stub" {
            assert!(
                matches!(result, Err(crate::pdu::PduError::Stub)),
                "{name}: {result:?}"
            );
        } else {
            assert!(result.is_ok(), "{name}: {result:?}");
        }
    }
}

#[test]
fn truncated_and_corrupted_pdus_never_panic() {
    let mut failures = Vec::new();
    let mut cases = 0u64;
    for (seed_index, (name, seed)) in seeds().into_iter().enumerate() {
        for len in 0..=seed.len() {
            cases += 1;
            if let Err(e) = check(&seed[..len]) {
                failures.push(format!("{name}: truncated to {len}: {e}"));
            }
        }
        for case in 0..CASES_PER_SEED {
            let mut rng = Lcg(((seed_index as u64) << 32) ^ case ^ 0x9e37_79b9_7f4a_7c15);
            let mut data = seed.clone();
            let steps = 1 + rng.below(4);
            let applied: Vec<String> = (0..steps).map(|_| mutate(&mut rng, &mut data)).collect();
            cases += 1;
            if let Err(e) = check(&data) {
                failures.push(format!(
                    "{name}: case {case} [{}]: {e}\n  input {}",
                    applied.join(", "),
                    hex(&data)
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {cases} cases failed:\n{}",
        failures.len(),
        failures
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
