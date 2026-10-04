use super::*;

fn key(n: u8) -> [u8; 32] {
    [n; 32]
}

/// An obfuscated export's message ids are made from these stand-ins, so
/// two ids must stay two and one key must always give the same one.
#[test]
fn an_id_stand_in_is_stable_under_one_key_and_tells_ids_apart() {
    let anon = Obfuscator::new(key(1));
    let a = anon.obfuscate_id("guid-a");
    assert_eq!(a, anon.obfuscate_id("guid-a"));
    assert_ne!(a, anon.obfuscate_id("guid-b"));
    assert_ne!(a, Obfuscator::new(key(2)).obfuscate_id("guid-a"));
    assert_eq!(a.len(), 64);
    assert!(!a.contains("guid-a"));
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
/// same fake data, which is what makes it shareable and comparable — so
/// the mapping is a contract, not an implementation detail. Regenerate
/// deliberately if it ever has to change, and say why in the commit.
/// The fake numbers on the right are the key's output, not chosen
/// numbers, so they can fall outside the reserved test ranges (#1521).
#[test]
fn known_answers_for_a_fixed_key() {
    const KEY: u8 = 7;

    let mut phones = Obfuscator::new(key(KEY));
    for (raw, expected) in [
        ("+15555550100", "+10490970433"),
        ("+44 20 7183 8750", "+44 39 2603 3624"),
        ("7535", "7032"),
        ("(555) 555-0102", "(075) 458-7648"),
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
        text.obfuscate_text("Call me at +1 555 555 0119 or alice@example.com"),
        "Vmrq sm hl +1 060 199 4055 pt hugo.white@example.invalid"
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
    let raw = "see https://mail.example.com/u/alice@example.com?id=5555550119 now";
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
    assert!(!out.contains("5555550119"), "{out}");
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

/// Run `materialize_placeholders` on `output_dir` with `folder` read-only,
/// so a delete inside it fails on Unix, and put the permissions back.
/// `None` when the user can delete from a read-only folder (root): such a
/// user cannot exercise the failure, so the test has nothing to check.
#[cfg(unix)]
fn materialize_with_folder_read_only(folder: &Path, output_dir: &Path) -> Option<Result<()>> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(folder, fs::Permissions::from_mode(0o555)).expect("read-only");
    let probe = folder.join("probe");
    let result = if fs::write(&probe, b"").is_ok() {
        let _ = fs::remove_file(&probe);
        None
    } else {
        Some(materialize_placeholders(output_dir))
    };
    fs::set_permissions(folder, fs::Permissions::from_mode(0o755)).expect("restore");
    result
}

/// A real attachment the pass cannot delete stays in the obfuscated
/// export, and that export exists to be shared without the real content.
/// The pass must fail and name the file, not report success (#1139).
#[cfg(unix)]
#[test]
fn a_real_attachment_that_cannot_be_removed_fails_the_pass_and_names_the_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let attachments = dir.path().join("attachments");
    fs::create_dir_all(&attachments).expect("attachments dir");
    let photo = attachments.join("IMG_0001.jpg");
    fs::write(&photo, b"real photo bytes").expect("write");

    let Some(result) = materialize_with_folder_read_only(&attachments, dir.path()) else {
        return;
    };

    let err = result.expect_err("a file that cannot be removed must fail the pass");
    let message = format!("{err:#}");
    assert!(
        message.contains(&photo.display().to_string()),
        "the error must name the file it could not remove: {message}"
    );
    assert!(
        photo.is_file(),
        "the test's premise: the file is still there"
    );
}

/// A real attachment in a subfolder that the pass cannot delete fails the
/// pass, and the error names that file, not only its folder (#1406).
#[cfg(unix)]
#[test]
fn a_file_in_a_subfolder_that_cannot_be_removed_is_the_one_the_error_names() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sub = dir.path().join("attachments").join("sub");
    fs::create_dir_all(&sub).expect("subfolder");
    let photo = sub.join("photo.jpg");
    fs::write(&photo, b"real photo bytes").expect("write");

    let Some(result) = materialize_with_folder_read_only(&sub, dir.path()) else {
        return;
    };

    let message = format!("{:#}", result.expect_err("the pass must fail"));
    assert!(
        message.contains(&photo.display().to_string()),
        "the error must name the file that stayed: {message}"
    );
}

/// A real attachment in a subfolder of `attachments/` is real content
/// too, and the obfuscated export exists to leave it out (#1406).
#[test]
fn materializing_placeholders_removes_real_media_in_a_subfolder() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sub = dir.path().join("attachments").join("sub");
    fs::create_dir_all(&sub).expect("subfolder");
    fs::write(sub.join("photo.jpg"), b"real photo bytes").expect("write");

    materialize_placeholders(dir.path()).expect("materialize");

    assert!(
        !sub.exists(),
        "the subfolder and the photo in it must be gone"
    );
}

/// A symlink under `attachments/` is removed, never followed: the folder
/// it points at, outside the export, keeps its files.
#[cfg(unix)]
#[test]
fn a_symlink_to_a_folder_outside_the_export_is_removed_not_followed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let outside = tempfile::tempdir().expect("outside");
    fs::write(outside.path().join("keep.jpg"), b"not the export's").expect("write");
    let attachments = dir.path().join("attachments");
    fs::create_dir_all(&attachments).expect("attachments dir");
    let link = attachments.join("linked");
    std::os::unix::fs::symlink(outside.path(), &link).expect("symlink");

    materialize_placeholders(dir.path()).expect("materialize");

    assert!(
        fs::symlink_metadata(&link).is_err(),
        "the symlink must be gone"
    );
    assert_eq!(
        fs::read(outside.path().join("keep.jpg")).expect("read"),
        b"not the export's",
        "the folder the symlink pointed at must be untouched"
    );
}

/// A symlink with a placeholder's name is not kept as a placeholder:
/// writing the placeholder through it would overwrite a file outside the
/// export.
#[cfg(unix)]
#[test]
fn a_symlink_named_like_a_placeholder_is_replaced_by_a_real_placeholder() {
    let dir = tempfile::tempdir().expect("tempdir");
    let outside = tempfile::tempdir().expect("outside");
    let target = outside.path().join("elsewhere.jpg");
    fs::write(&target, b"not the export's").expect("write");
    let attachments = dir.path().join("attachments");
    fs::create_dir_all(&attachments).expect("attachments dir");
    let placeholder = attachments.join("placeholder.jpg");
    std::os::unix::fs::symlink(&target, &placeholder).expect("symlink");

    materialize_placeholders(dir.path()).expect("materialize");

    let meta = fs::symlink_metadata(&placeholder).expect("placeholder");
    assert!(
        meta.file_type().is_file(),
        "the placeholder must be a real file"
    );
    assert_eq!(fs::read(&placeholder).expect("read"), PLACEHOLDER_JPG);
    assert_eq!(
        fs::read(&target).expect("read target"),
        b"not the export's",
        "the file the symlink pointed at must be untouched"
    );
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

/// Letters outside ASCII take their filler from the digest too, and a
/// message can hold far more of them than the digest has bytes.
#[test]
fn a_long_message_in_cyrillic_is_filled_to_its_own_length() {
    let raw = "привет".repeat(184); // 1,104 letters
    let mut anon = Obfuscator::new(key(13));
    let out = anon.obfuscate_text(&raw);

    assert_eq!(out.chars().count(), raw.chars().count());
    assert!(out.chars().all(|c| c.is_ascii_lowercase()), "{out}");
    let distinct: std::collections::HashSet<char> = out.chars().collect();
    assert!(distinct.len() > 1, "the filler is one letter repeated");
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
        ("call 5555550119 now", "nbxh 6667863530 uvx"),
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
    let us = a.obfuscate_phone("+1 (555) 555-0119");
    assert!(us.starts_with("+1"));
    let us_digits: String = us.chars().filter(|c| c.is_ascii_digit()).collect();
    assert!(us_digits.starts_with('1'));
    assert_eq!(us_digits.len(), 11);
    assert_ne!(&us_digits[1..], "5555550119");

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
    let src =
        "Email alice@example.com or https://secret.example/path?x=1 call +1 (555) 555-0119 thanks";
    let fake = a.obfuscate_text(src);
    assert!(!fake.contains("alice@example.com"));
    assert!(!fake.contains("secret.example"));
    assert!(!fake.contains("555) 555-0119"));
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
    let src = "+1 (555) 555-0119";
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
    assert_ne!(digits, "15555550119");
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
