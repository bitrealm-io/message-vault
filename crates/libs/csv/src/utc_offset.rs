//! Fixed UTC offset parsing (`UTC±HH:MM`).

use anyhow::{Result, bail};
use chrono::FixedOffset;

/// Parse a fixed UTC offset string into [`FixedOffset`].
///
/// Accepted forms:
/// - `UTC` → +00:00
/// - `UTC+00:00`, `UTC-05:00`, `UTC+05:30`, `UTC+05:45`
///
/// IANA names are rejected here; [`crate::Zone::parse`] accepts both.
pub fn parse_utc_offset(raw: &str) -> Result<FixedOffset> {
    let s = raw.trim();
    if s.is_empty() {
        bail!("empty UTC offset");
    }
    let upper = s.to_ascii_uppercase();
    if upper == "UTC" || upper == "Z" {
        return FixedOffset::east_opt(0).ok_or_else(|| anyhow::anyhow!("invalid UTC offset"));
    }
    let rest = upper
        .strip_prefix("UTC")
        .ok_or_else(|| anyhow::anyhow!("expected UTC offset like UTC-05:00, got {raw:?}"))?;
    if rest.is_empty() {
        return FixedOffset::east_opt(0).ok_or_else(|| anyhow::anyhow!("invalid UTC offset"));
    }
    let (sign, body) = match rest.chars().next() {
        Some('+') => (1i32, &rest[1..]),
        Some('-') => (-1i32, &rest[1..]),
        _ => {
            bail!("expected UTC±HH:MM (e.g. UTC-05:00), got {raw:?}");
        }
    };
    let (hours, minutes) = parse_hh_mm(body)?;
    if hours > 14 || (hours == 14 && minutes > 0) {
        bail!("UTC offset out of range: {raw:?}");
    }
    if minutes >= 60 {
        bail!("invalid minutes in UTC offset: {raw:?}");
    }
    let secs = sign * (hours * 3600 + minutes * 60);
    FixedOffset::east_opt(secs).ok_or_else(|| anyhow::anyhow!("invalid UTC offset: {raw:?}"))
}

/// Hours and minutes from `HH` or `HH:MM`.
fn parse_hh_mm(body: &str) -> Result<(i32, i32)> {
    let parts: Vec<&str> = body.split(':').collect();
    match parts.as_slice() {
        [hh] => {
            let hours: i32 = hh
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid hours in UTC offset: {body:?}"))?;
            Ok((hours, 0))
        }
        [hh, mm] => {
            let hours: i32 = hh
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid hours in UTC offset: {body:?}"))?;
            let minutes: i32 = mm
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid minutes in UTC offset: {body:?}"))?;
            Ok((hours, minutes))
        }
        _ => bail!("expected HH or HH:MM in UTC offset, got {body:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_utc_and_offsets() {
        assert_eq!(parse_utc_offset("UTC").unwrap().local_minus_utc(), 0);
        assert_eq!(parse_utc_offset("UTC+00:00").unwrap().local_minus_utc(), 0);
        assert_eq!(
            parse_utc_offset("UTC-05:00").unwrap().local_minus_utc(),
            -5 * 3600
        );
        assert_eq!(
            parse_utc_offset("UTC+05:30").unwrap().local_minus_utc(),
            5 * 3600 + 30 * 60
        );
        assert_eq!(
            parse_utc_offset("UTC+05:45").unwrap().local_minus_utc(),
            5 * 3600 + 45 * 60
        );
        assert_eq!(
            parse_utc_offset("UTC+14:00").unwrap().local_minus_utc(),
            14 * 3600
        );
    }

    #[test]
    fn rejects_iana() {
        assert!(parse_utc_offset("America/New_York").is_err());
        assert!(parse_utc_offset("Not/AZone").is_err());
    }

    #[test]
    fn parses_every_accepted_shape() {
        for (raw, secs) in [
            ("UTC", 0),
            ("utc", 0),
            ("Z", 0),
            ("z", 0),
            ("UTC-5", -5 * 3600),
            ("UTC+9", 9 * 3600),
            ("UTC+05:30", 5 * 3600 + 30 * 60),
            ("UTC-14:00", -14 * 3600),
            ("UTC+14:00", 14 * 3600),
        ] {
            assert_eq!(
                parse_utc_offset(raw).unwrap().local_minus_utc(),
                secs,
                "{raw}"
            );
        }
    }

    #[test]
    fn refuses_offsets_past_fourteen_hours_and_bad_minutes() {
        for raw in [
            "UTC+14:30",
            "UTC+14:01",
            "UTC+15",
            "UTC-15:00",
            "UTC+05:60",
            "UTC+5:5:5",
            "UTC5",
            "",
        ] {
            assert!(parse_utc_offset(raw).is_err(), "{raw}");
        }
    }
}
