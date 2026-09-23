//! Truncated and corrupted PDUs must never panic the decoder.
//!
//! Every seed PDU is cut at every length, then mutated many times over with a
//! fixed-seed generator, so a failure names a seed, a case index, and the
//! mutations that produced it, and reruns the same way every time.

use super::*;
use std::panic::{AssertUnwindSafe, catch_unwind};

const CASES_PER_SEED: u64 = 2_000;
const FILENAME_TS: i64 = 1_609_459_200;

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

/// Bytes that mean something to the decoder: header ids, value-length
/// escapes, NUL terminators, and the GO named-part marker.
const INTERESTING: &[u8] = &[
    0x00, 0x01, 0x1e, 0x1f, 0x7f, 0x80, 0x81, 0x83, 0x84, 0x85, 0x89, 0x8c, 0x8e, 0x97, 0xac, 0xc0,
    0xfe, 0xff,
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
    let (owners, primary) = test_owners();
    let path = Path::new("I_1609459200_fuzz.pdu");
    let parsed = catch_unwind(AssertUnwindSafe(|| {
        parse_pdu_bytes(path, data, &owners, &primary)
    }))
    .map_err(|panic| {
        panic
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_string()))
            .unwrap_or_else(|| "panic".into())
    })?;
    let Some(p) = parsed else {
        return if data.len() < 10 {
            Ok(())
        } else {
            Err("None for a well-named file of 10 or more bytes".into())
        };
    };
    if p.timestamp != FILENAME_TS && p.timestamp <= 0 {
        return Err(format!("timestamp {} not positive", p.timestamp));
    }
    for (i, a) in p.attachments.iter().enumerate() {
        if !a.ext.starts_with('.') {
            return Err(format!("attachment {i} extension {:?}", a.ext));
        }
        if !is_subslice(data, &a.data) {
            return Err(format!("attachment {i} bytes are not from the input"));
        }
    }
    let mut seen = HashSet::new();
    for n in &p.participants {
        if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) || !seen.insert(n) {
            return Err(format!("participant {n:?} not unique digits"));
        }
    }
    if p.is_group != (p.participants.len() >= 3) {
        return Err("is_group disagrees with the participant count".into());
    }
    if p.participants.is_empty() && (p.is_sent || !p.sender_number.is_empty()) {
        return Err("a direction with no participants".into());
    }
    if !matches!(p.decode_quality, "structured" | "mixed" | "heuristic") {
        return Err(format!("decode quality {:?}", p.decode_quality));
    }
    Ok(())
}

/// A multipart PDU that reaches the header, part-header, SMIL, and cid paths
/// the small fixtures do not.
fn multipart_seed() -> Vec<u8> {
    let smil =
        b"<smil><body><par><img src=\"cid:img1\"/><text src=\"text_0.txt\"/></par></body></smil>";
    let text = b"Seed body text";
    let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe0];
    jpeg.extend(std::iter::repeat_n(0x11u8, 80));

    let mut data = vec![0x8c, 0x84]; // X-Mms-Message-Type: m-retrieve-conf
    data.extend_from_slice(&[0x98]);
    data.extend_from_slice(b"T123\0"); // Transaction-Id
    data.extend_from_slice(&[0x8d, 0x92]); // MMS-Version 1.2
    data.extend_from_slice(&[0x85, 0x04, 0x5f, 0xee, 0x66, 0x00]); // Date
    data.extend_from_slice(&[0x89, 0x18, 0x80]); // From, address present
    data.extend_from_slice(b"+14075551234/TYPE=PLMN\0");
    data.push(0x97); // To
    data.extend_from_slice(b"+15555550100/TYPE=PLMN\0");
    data.push(0x96); // Subject
    data.extend_from_slice(b"Seed subject\0");
    // Content-Type: general form, multipart.related with a Start parameter.
    let ct_params = b"\x8a<smil>\0";
    data.push(0x84);
    data.push((1 + ct_params.len()) as u8);
    data.push(0xac);
    data.extend_from_slice(ct_params);

    data.push(0x03); // three parts
    let mut part = |headers: Vec<u8>, body: &[u8]| {
        data.push(headers.len() as u8);
        data.push(body.len() as u8);
        data.extend_from_slice(&headers);
        data.extend_from_slice(body);
    };
    let mut smil_headers = b"application/smil\0".to_vec();
    smil_headers.extend_from_slice(b"\xc0<smil>\0");
    part(smil_headers, smil);
    // text/plain; charset=utf-8, Content-Location text_0.txt
    let mut text_headers = vec![0x03, 0x83, 0x88, 0xea];
    text_headers.extend_from_slice(b"\x8etext_0.txt\0");
    part(text_headers, text);
    // image/jpeg, Content-ID <img1>, Content-Disposition attachment; filename
    let mut img_headers = vec![0x97, 0xc0];
    img_headers.extend_from_slice(b"<img1>\0");
    img_headers.extend_from_slice(b"\xae\x0a\x81\x86pic.jpg\0");
    part(img_headers, &jpeg);
    data
}

fn seeds() -> Vec<(String, Vec<u8>)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pdu");
    let mut seeds: Vec<(String, Vec<u8>)> = std::fs::read_dir(&dir)
        .expect("fixture dir")
        .map(|e| e.expect("fixture entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "pdu"))
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            (name, std::fs::read(&p).expect("read fixture"))
        })
        .collect();
    seeds.sort();
    seeds.push(("multipart_seed".into(), multipart_seed()));
    seeds
}

fn hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn multipart_seed_decodes_as_structured() {
    let data = multipart_seed();
    let (owners, primary) = test_owners();
    let p = parse_pdu_bytes(Path::new("I_1609459200_seed.pdu"), &data, &owners, &primary)
        .expect("parsed");
    assert_eq!(p.body, "Seed body text");
    assert_eq!(p.attachments.len(), 1);
    assert_eq!(p.attachments[0].ext, ".jpg");
    assert_eq!(p.sender_number, "4075551234");
    assert!(!p.is_sent);
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
