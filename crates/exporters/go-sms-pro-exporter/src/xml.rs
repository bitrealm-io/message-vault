//! Parse GO SMS Pro `gosms_sys*.xml` SMS backups.

use crate::emit::MAX_SKIP_DETAILS;
use anyhow::{Context, Result, bail};
use go_sms_mms::decode_gosms_emojis;
use phone::Handle;
use quick_xml::Reader;
use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesRef, Event};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone)]
pub(crate) struct XmlMessage {
    /// The other party's address.
    pub other: Handle,
    pub name_alias: Option<String>,
    pub timestamp_secs: f64,
    pub is_from_me: bool,
    pub text: String,
    /// Raw Android `<type>` (`1` received, `2` sent).
    pub android_type: String,
    /// Raw `<date>` milliseconds string.
    pub date_ms: String,
    /// Raw `<contactName>`.
    pub contact_name: String,
    /// Every `<SMS>` child element name → text.
    pub xml_fields: BTreeMap<String, String>,
}

/// Diagnostic row when an XML SMS has a blank `<address>`.
#[derive(Debug, Clone)]
pub(crate) struct SkippedBadAddrDetail {
    pub xml_file: String,
    pub address: String,
    pub contact_name: String,
    pub android_type: String,
    pub date_ms: String,
    pub body: String,
}

#[derive(Debug, Default)]
pub(crate) struct XmlParseStats {
    pub messages: u64,
    pub sent: u64,
    pub received: u64,
    pub skipped_invalid_date: u64,
    pub skipped_unknown_type: u64,
    pub skipped_unknown_address: u64,
    /// Capped at [`MAX_SKIP_DETAILS`] entries; overflow counted here.
    pub skipped_unknown_address_details: Vec<SkippedBadAddrDetail>,
    pub skipped_unknown_address_details_more: u64,
    /// Messages dropped because a reference in one of their fields, such
    /// as `&#55357;`, is not a character.
    pub skipped_unreadable_text: u64,
}

/// The last millisecond of the year 9999, the latest `<date>` read as real.
/// The SBR reader in `crates/libs/sbr` uses the same bound.
const MAX_DATE_MS: i64 = 253_402_300_799_999;

/// Unix seconds from a millisecond `<date>`, or `None` for anything but a
/// whole number of milliseconds from 0 to [`MAX_DATE_MS`], so `NaN`, `inf`,
/// and out-of-range values never become a timestamp.
fn timestamp_secs_from_date_ms(date_ms: &str) -> Option<f64> {
    let millis = date_ms
        .parse::<i64>()
        .ok()
        .filter(|ms| (0..=MAX_DATE_MS).contains(ms))?;
    // Exact: every value up to MAX_DATE_MS fits in an f64 mantissa.
    Some(millis as f64 / 1000.0)
}

/// Parse one GO SMS Pro XML backup file.
///
/// # Errors
///
/// Returns an error when the file cannot be read or the XML cannot be parsed.
pub(crate) fn parse_xml_file(path: &Path) -> Result<(Vec<XmlMessage>, XmlParseStats)> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let (msgs, mut stats) = parse_xml_str(&text)?;
    let xml_file = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    for d in &mut stats.skipped_unknown_address_details {
        d.xml_file.clone_from(&xml_file);
    }
    Ok((msgs, stats))
}

/// Parse GO SMS Pro XML from a string.
///
/// # Errors
///
/// Returns an error when the XML cannot be parsed.
pub(crate) fn parse_xml_str(text: &str) -> Result<(Vec<XmlMessage>, XmlParseStats)> {
    let mut stats = XmlParseStats::default();
    let mut out = Vec::new();

    for fields in read_sms_elements(text, &mut stats)? {
        let addr = Handle::parse(fields.get("address").map_or("", String::as_str));
        let contact = fields.get("contactName").cloned().unwrap_or_default();
        let body_raw = fields.get("body").map_or("", String::as_str);
        let body = decode_gosms_emojis(body_raw);
        // A missing `<date>` must not become a fake 1970-01-01 row: all such
        // messages would share timestamp 0 and could falsely deduplicate.
        let Some(date_ms) = fields.get("date").cloned() else {
            stats.skipped_invalid_date += 1;
            continue;
        };
        let Some(timestamp_secs) = timestamp_secs_from_date_ms(&date_ms) else {
            stats.skipped_invalid_date += 1;
            continue;
        };
        let typ = fields
            .get("type")
            .map(|s| s.trim().to_string())
            .unwrap_or_default();

        let msg = match typ.as_str() {
            "2" => {
                let Some(other) = addr else {
                    push_bad_addr(&mut stats, &fields, &contact, &typ, &date_ms, &body);
                    continue;
                };
                stats.sent += 1;
                XmlMessage {
                    other,
                    name_alias: message_ir::nonempty(&contact),
                    timestamp_secs,
                    is_from_me: true,
                    text: body,
                    android_type: typ.clone(),
                    date_ms: date_ms.clone(),
                    contact_name: contact.clone(),
                    xml_fields: fields,
                }
            }
            "1" => {
                let Some(other) = addr else {
                    push_bad_addr(&mut stats, &fields, &contact, &typ, &date_ms, &body);
                    continue;
                };
                stats.received += 1;
                let hint = if contact.is_empty() {
                    None
                } else {
                    Some(contact.clone())
                };
                XmlMessage {
                    other,
                    name_alias: hint,
                    timestamp_secs,
                    is_from_me: false,
                    text: body,
                    android_type: typ.clone(),
                    date_ms: date_ms.clone(),
                    contact_name: contact,
                    xml_fields: fields,
                }
            }
            _ => {
                stats.skipped_unknown_type += 1;
                continue;
            }
        };

        out.push(msg);
    }

    Ok((out, stats))
}

/// Every `<SMS>` element under the root as a map of child element name to
/// its text, with leading and trailing whitespace trimmed.
///
/// Each `<SMS>` is counted in `stats.messages`. One whose fields hold a
/// reference that is not a character or a predefined XML entity, such as
/// `&#55357;` or `&nbsp;`, is left out and counted in
/// `stats.skipped_unreadable_text`, so the reference costs that message
/// and not the file.
///
/// # Errors
///
/// Returns an error when the XML is not well formed.
fn read_sms_elements(
    text: &str,
    stats: &mut XmlParseStats,
) -> Result<Vec<BTreeMap<String, String>>> {
    let mut reader = Reader::from_str(text);
    let mut all = Vec::new();
    let mut depth = 0usize;
    // The `<SMS>` being read, and whether every reference in it resolved.
    let mut sms: Option<(BTreeMap<String, String>, bool)> = None;
    // The child element of `<SMS>` being read: its name and text so far.
    let mut field: Option<(String, String)> = None;
    loop {
        let event = reader.read_event().context("failed to parse GoSms XML")?;
        match event {
            Event::Start(e) => {
                depth += 1;
                let name = e.name().as_ref().to_string();
                match depth {
                    2 if name == "SMS" => sms = Some((BTreeMap::new(), true)),
                    3 if sms.is_some() => field = Some((name, String::new())),
                    _ => {}
                }
            }
            Event::Empty(e) => {
                let name = e.name().as_ref().to_string();
                match (depth, &mut sms) {
                    (1, _) if name == "SMS" => {
                        stats.messages += 1;
                        all.push(BTreeMap::new());
                    }
                    (2, Some((fields, _))) => {
                        fields.insert(name, String::new());
                    }
                    _ => {}
                }
            }
            Event::Text(t) if depth == 3 => {
                if let Some((_, value)) = &mut field {
                    value.push_str(&t.xml10_content());
                }
            }
            Event::CData(c) if depth == 3 => {
                if let Some((_, value)) = &mut field {
                    value.push_str(&c);
                }
            }
            Event::GeneralRef(r) if depth == 3 => {
                if let (Some((_, value)), Some((_, readable))) = (&mut field, &mut sms) {
                    match resolve_reference(&r) {
                        Some(resolved) => value.push_str(&resolved),
                        None => *readable = false,
                    }
                }
            }
            Event::End(_) => {
                match depth {
                    3 => {
                        if let (Some((name, value)), Some((fields, _))) = (field.take(), &mut sms) {
                            let trimmed = value.trim_matches([' ', '\t', '\r', '\n']);
                            fields.insert(name, trimmed.to_string());
                        }
                    }
                    2 => {
                        if let Some((fields, readable)) = sms.take() {
                            stats.messages += 1;
                            if readable {
                                all.push(fields);
                            } else {
                                stats.skipped_unreadable_text += 1;
                            }
                        }
                    }
                    _ => {}
                }
                depth = depth.saturating_sub(1);
            }
            Event::Eof if depth == 0 => break,
            Event::Eof => bail!("failed to parse GoSms XML: the file ends inside an element"),
            _ => {}
        }
    }
    Ok(all)
}

/// The text a reference stands for: a character reference, or one of the
/// five predefined XML entities. `None` for anything else.
fn resolve_reference(r: &BytesRef<'_>) -> Option<String> {
    if r.is_char_ref() {
        return r.resolve_char_ref().ok().flatten().map(String::from);
    }
    resolve_predefined_entity(r).map(String::from)
}

/// Record an XML SMS whose `<address>` is blank.
fn push_bad_addr(
    stats: &mut XmlParseStats,
    fields: &BTreeMap<String, String>,
    contact: &str,
    typ: &str,
    date_ms: &str,
    body: &str,
) {
    stats.skipped_unknown_address += 1;
    let body_preview: String = body.chars().take(160).collect();
    if stats.skipped_unknown_address_details.len() < MAX_SKIP_DETAILS {
        stats
            .skipped_unknown_address_details
            .push(SkippedBadAddrDetail {
                xml_file: String::new(),
                address: fields.get("address").cloned().unwrap_or_default(),
                contact_name: contact.to_string(),
                android_type: typ.to_string(),
                date_ms: date_ms.to_string(),
                body: body_preview,
            });
    } else {
        stats.skipped_unknown_address_details_more += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use message_ir::HandleType;

    #[test]
    fn parses_sent_and_received() {
        let xml = r#"<?xml version="1.0"?>
<GoSms>
  <SMSCount>2</SMSCount>
  <SMS>
    <address>+14075550107</address>
    <contactName>Alice</contactName>
    <date>1400773261000</date>
    <type>1</type>
    <body>hello +g1f602</body>
  </SMS>
  <SMS>
    <address>+14075550107</address>
    <contactName>Alice</contactName>
    <date>1400773321000</date>
    <type>2</type>
    <body>hi back</body>
  </SMS>
</GoSms>"#;
        let (msgs, stats) = parse_xml_str(xml).unwrap();
        assert_eq!(stats.messages, 2);
        assert_eq!(stats.received, 1);
        assert_eq!(stats.sent, 1);
        assert_eq!(msgs.len(), 2);
        assert!(!msgs[0].is_from_me);
        assert_eq!(msgs[0].text, "hello 😂");
        assert_eq!(msgs[0].other.key(), "+14075550107");
        assert!(msgs[1].is_from_me);
    }

    #[test]
    fn missing_date_skips_message() {
        let xml = r#"<?xml version="1.0"?>
<GoSms>
  <SMS>
    <address>+14075550107</address>
    <contactName>Alice</contactName>
    <type>1</type>
    <body>no date here</body>
  </SMS>
  <SMS>
    <address>+14075550107</address>
    <contactName>Alice</contactName>
    <date>1400773261000</date>
    <type>1</type>
    <body>dated</body>
  </SMS>
</GoSms>"#;
        let (msgs, stats) = parse_xml_str(xml).unwrap();
        assert_eq!(stats.messages, 2);
        assert_eq!(stats.skipped_invalid_date, 1);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].text, "dated");
        assert!(msgs[0].date_ms.starts_with("1400773261"));
    }

    #[test]
    fn preserves_extra_xml_fields() {
        let xml = r#"<?xml version="1.0"?>
<GoSms>
  <SMS>
    <address>+14075550107</address>
    <contactName>Alice</contactName>
    <date>1400773261000</date>
    <type>1</type>
    <body>hello</body>
    <read>1</read>
    <status>-1</status>
  </SMS>
</GoSms>"#;
        let (msgs, _) = parse_xml_str(xml).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(
            msgs[0].xml_fields.get("read").map(String::as_str),
            Some("1")
        );
        assert_eq!(
            msgs[0].xml_fields.get("status").map(String::as_str),
            Some("-1")
        );
        assert_eq!(msgs[0].android_type, "1");
        assert_eq!(msgs[0].date_ms, "1400773261000");
        assert_eq!(msgs[0].contact_name, "Alice");
    }

    fn received_from(address: &str) -> (Vec<XmlMessage>, XmlParseStats) {
        let xml = format!(
            "<GoSms><SMS><address>{address}</address><date>1400773261000</date><type>1</type><body>hi</body></SMS></GoSms>"
        );
        parse_xml_str(&xml).unwrap()
    }

    #[test]
    fn a_number_with_its_country_keeps_it() {
        let (msgs, _) = received_from("+6595550100");
        assert_eq!(msgs[0].other.key(), "+6595550100");
    }

    #[test]
    fn an_email_address_and_a_sender_name_are_not_numbers() {
        let (msgs, stats) = received_from("ann2020@example.com");
        assert_eq!(
            (msgs[0].other.kind(), msgs[0].other.key()),
            (HandleType::Email, "ann2020@example.com")
        );
        assert_eq!(stats.skipped_unknown_address, 0);
        let (msgs, stats) = received_from("AMAZON");
        assert_eq!(
            (msgs[0].other.kind(), msgs[0].other.key()),
            (HandleType::Other, "AMAZON")
        );
        assert_eq!(stats.skipped_unknown_address, 0);
    }

    fn sms_with_date(date: &str) -> (Vec<XmlMessage>, XmlParseStats) {
        let xml = format!(
            "<GoSms><SMS><address>+14075550107</address><date>{date}</date><type>1</type><body>hi</body></SMS></GoSms>"
        );
        parse_xml_str(&xml).unwrap()
    }

    #[test]
    fn unreadable_dates_are_skipped() {
        for date in [
            "NaN",
            "nan",
            "inf",
            "-inf",
            "infinity",
            "1e400",
            "1.4e12",
            "1400773261000.5",
            "",
            "-1",
            "abc",
            "99999999999999999999999",
            "9223372036854775807",
        ] {
            let (msgs, stats) = sms_with_date(date);
            assert!(msgs.is_empty(), "date {date:?} was accepted");
            assert_eq!(stats.skipped_invalid_date, 1, "date {date:?}");
        }
    }

    #[test]
    fn millisecond_dates_parse_exactly() {
        for (date, secs) in [
            ("0", 0.0),
            ("1", 0.001),
            ("1400773261000", 1_400_773_261.0),
            ("1400773261123", 1_400_773_261_123.0 / 1000.0),
            ("253402300799999", 253_402_300_799_999.0 / 1000.0),
        ] {
            let (msgs, stats) = sms_with_date(date);
            assert_eq!(stats.skipped_invalid_date, 0, "date {date:?}");
            assert_eq!(msgs[0].timestamp_secs, secs, "date {date:?}");
            assert_eq!(msgs[0].date_ms, date);
        }
    }

    #[test]
    fn a_refused_reference_costs_only_its_own_message() {
        let xml = "<GoSms>\
            <SMS><address>+14075550101</address><date>1400773261000</date><type>1</type><body>On my way &#55357;&#56832;</body></SMS>\
            <SMS><address>+14075550101</address><date>1400773321000</date><type>2</type><body>a &amp; b &lt;c&gt;</body></SMS>\
            </GoSms>";
        let (msgs, stats) = parse_xml_str(xml).unwrap();
        assert_eq!(stats.messages, 2);
        assert_eq!(stats.skipped_unreadable_text, 1);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].text, "a & b <c>");
    }
}
