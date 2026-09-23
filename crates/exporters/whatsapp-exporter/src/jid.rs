//! WhatsApp JID helpers.
//!
//! A JID is WhatsApp's address for a user or group (for example
//! `15551234567@s.whatsapp.net` or `120363042@g.us`).

/// True for `@g.us` group JIDs (WhatsApp's group address suffix).
pub(crate) fn is_group_jid(jid: &str) -> bool {
    jid.trim().ends_with("@g.us")
}

/// The domain of a user JID, the only kind whose local part is a phone number.
const USER_JID_DOMAIN: &str = "s.whatsapp.net";

/// Characters a phone-number sender may carry besides its digits.
const PHONE_PUNCTUATION: [char; 5] = ['+', '-', '(', ')', ' '];

/// Map a user JID or a phone-number sender to E.164 (the international
/// phone-number format that starts with +).
///
/// A user JID's local part is the full international number, country code
/// included, so the result is `+` and its digits:
///
/// - `15551234567@s.whatsapp.net` → `+15551234567`
/// - `6591234567@s.whatsapp.net` → `+6591234567` (Singapore, not `+1 659…`)
/// - `+15551234567` → `+15551234567`
///
/// No regional rule applies, because the country code is already there. The
/// `phone` crate's sanitizers are not used for the same reason: they strip a
/// leading `1` from 11 digits, which is a US rule.
///
/// Returns `None` for group, `@lid`, and `status@broadcast` ids, for anything
/// that is not digits and phone punctuation, for a local part that starts with
/// `0` (no country code does), and outside 8 to 15 digits.
pub(crate) fn jid_to_e164(jid: &str) -> Option<String> {
    let jid = jid.trim();
    let local = match jid.split_once('@') {
        Some((local, USER_JID_DOMAIN)) => local,
        Some(_) => return None,
        None => jid,
    };
    if !local
        .chars()
        .all(|c| c.is_ascii_digit() || PHONE_PUNCTUATION.contains(&c))
    {
        return None;
    }
    let digits: String = local.chars().filter(char::is_ascii_digit).collect();
    if (8..=15).contains(&digits.len()) && !digits.starts_with('0') {
        Some(format!("+{digits}"))
    } else {
        None
    }
}

/// Chat identifier for CSV: E.164 for 1:1 user JIDs; otherwise the raw JID.
pub(crate) fn chat_id_from_jid(jid: &str) -> String {
    if is_group_jid(jid) {
        jid.trim().to_string()
    } else {
        jid_to_e164(jid).unwrap_or_else(|| jid.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A user JID's local part is the full international number, country code
    /// included, so the E.164 form is `+` and those digits. A 10-digit local
    /// part is never a US number, because a US number carries the leading `1`.
    #[test]
    fn jid_to_e164_cases() {
        let cases: &[(&str, Option<&str>)] = &[
            // Singapore, Norway, Denmark: 10 digits that US rules misread.
            ("6591234567@s.whatsapp.net", Some("+6591234567")),
            ("4791234567@s.whatsapp.net", Some("+4791234567")),
            ("4512345678@s.whatsapp.net", Some("+4512345678")),
            ("447911123456@s.whatsapp.net", Some("+447911123456")),
            ("15555550122@s.whatsapp.net", Some("+15555550122")),
            ("+15555550122", Some("+15555550122")),
            ("  15555550122@s.whatsapp.net  ", Some("+15555550122")),
            ("+65 9123-4567", Some("+6591234567")),
            ("+1 (555) 555-0122", Some("+15555550122")),
            // Not a person's phone number.
            ("status@broadcast", None),
            ("STATUS@broadcast", None),
            ("@s.whatsapp.net", None),
            ("", None),
            ("   ", None),
            ("123456789012345@lid", None),
            ("120363042@g.us", None),
            ("Alice", None),
            ("Alice 15555550122", None),
            // No country code starts with 0, so a trunk-zero local part is not
            // an international number and gets no handle at all.
            ("02079460000@s.whatsapp.net", None),
            // E.164 numbers run 8 to 15 digits here; outside that is not one.
            ("1234567@s.whatsapp.net", None),
            ("12345678@s.whatsapp.net", Some("+12345678")),
            ("123456789012345@s.whatsapp.net", Some("+123456789012345")),
            ("1234567890123456@s.whatsapp.net", None),
        ];
        let wrong: Vec<String> = cases
            .iter()
            .filter_map(|(jid, want)| {
                let got = jid_to_e164(jid);
                (got.as_deref() != *want).then(|| format!("{jid:?}: got {got:?}, want {want:?}"))
            })
            .collect();
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    #[test]
    fn group_jid_detection() {
        assert!(is_group_jid("120363042@g.us"));
        assert!(!is_group_jid("15555550122@s.whatsapp.net"));
    }

    #[test]
    fn chat_id_keeps_group_jid() {
        assert_eq!(chat_id_from_jid("120363042@g.us"), "120363042@g.us");
        assert_eq!(
            chat_id_from_jid("15555550122@s.whatsapp.net"),
            "+15555550122"
        );
        assert_eq!(chat_id_from_jid("6591234567@s.whatsapp.net"), "+6591234567");
        assert_eq!(chat_id_from_jid("status@broadcast"), "status@broadcast");
    }
}
