//! Inclusive start / exclusive end day filters (`YYYY-MM-DD`).

use anyhow::{Result, bail};
use chrono::{Local, NaiveDate, TimeZone};

/// Message timestamp window: `[start, end)` in Unix seconds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DateRange {
    /// Inclusive lower bound (local/tz midnight), if set.
    pub start_secs: Option<i64>,
    /// Exclusive upper bound (local/tz midnight), if set.
    pub end_secs: Option<i64>,
}

impl DateRange {
    /// Parse optional `YYYY-MM-DD` bounds using the host local timezone.
    pub fn parse(start: Option<&str>, end: Option<&str>) -> Result<Self> {
        Self::parse_with(
            |date| {
                Local
                    .from_local_datetime(&date.and_hms_opt(0, 0, 0).expect("midnight"))
                    .single()
                    .map(|dt| dt.timestamp())
                    .ok_or_else(|| {
                        anyhow::anyhow!("ambiguous or invalid local midnight for {date}")
                    })
            },
            start,
            end,
        )
    }

    /// Parse optional `YYYY-MM-DD` bounds in a named zone.
    pub fn parse_in_zone(
        start: Option<&str>,
        end: Option<&str>,
        zone: chrono_tz::Tz,
    ) -> Result<Self> {
        Self::parse_with(
            |date| {
                zone.from_local_datetime(&date.and_hms_opt(0, 0, 0).expect("midnight"))
                    .single()
                    .map(|dt| dt.timestamp())
                    .ok_or_else(|| {
                        anyhow::anyhow!("ambiguous or invalid midnight for {date} in {zone}")
                    })
            },
            start,
            end,
        )
    }

    /// Parse bounds in `zone` when one is given; otherwise in the host zone.
    pub fn parse_optional_tz(
        start: Option<&str>,
        end: Option<&str>,
        zone: Option<chrono_tz::Tz>,
    ) -> Result<Self> {
        match zone {
            None => Self::parse(start, end),
            Some(zone) => Self::parse_in_zone(start, end, zone),
        }
    }

    /// Parse the optional start and end dates into a range; `midnight_secs` decides which zone midnight is in.
    fn parse_with(
        midnight_secs: impl Fn(NaiveDate) -> Result<i64>,
        start: Option<&str>,
        end: Option<&str>,
    ) -> Result<Self> {
        let start_secs = match start.and_then(message_ir::trimmed) {
            None => None,
            Some(s) => Some(midnight_secs(parse_ymd(s)?)?),
        };
        let end_secs = match end.and_then(message_ir::trimmed) {
            None => None,
            Some(s) => Some(midnight_secs(parse_ymd(s)?)?),
        };
        if let (Some(s), Some(e)) = (start_secs, end_secs)
            && s >= e
        {
            bail!("start-date must be before end-date (end is exclusive)");
        }
        Ok(Self {
            start_secs,
            end_secs,
        })
    }

    /// True when neither bound is set.
    pub fn is_unbounded(&self) -> bool {
        self.start_secs.is_none() && self.end_secs.is_none()
    }

    /// True when `secs` falls inside `[start, end)`.
    pub fn contains_secs(&self, secs: i64) -> bool {
        if let Some(start) = self.start_secs
            && secs < start
        {
            return false;
        }
        if let Some(end) = self.end_secs
            && secs >= end
        {
            return false;
        }
        true
    }

    /// Like `contains_secs`, flooring a float timestamp first; non-finite
    /// values are outside the range.
    pub fn contains_secs_f64(&self, secs: f64) -> bool {
        if !secs.is_finite() {
            return false;
        }
        self.contains_secs(secs.floor() as i64)
    }
}

/// A `YYYY-MM-DD` date.
fn parse_ymd(value: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| anyhow::anyhow!("invalid date '{value}' (expected YYYY-MM-DD)"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_is_unbounded() {
        let range = DateRange::parse(None, None).unwrap();
        assert!(range.is_unbounded());
        assert!(range.contains_secs(0));
        assert!(range.contains_secs(i64::MAX / 2));
    }

    #[test]
    fn inclusive_start_exclusive_end_utc() {
        let range =
            DateRange::parse_in_zone(Some("2020-01-01"), Some("2020-01-03"), chrono_tz::UTC)
                .unwrap();
        // 2020-01-01 00:00:00 UTC
        assert!(range.contains_secs(1_577_836_800));
        // 2020-01-02 12:00:00 UTC
        assert!(range.contains_secs(1_577_966_400));
        // 2020-01-03 00:00:00 UTC — exclusive end
        assert!(!range.contains_secs(1_578_009_600));
        // 2019-12-31 23:59:59 UTC
        assert!(!range.contains_secs(1_577_836_799));
    }

    #[test]
    fn start_must_precede_end() {
        let err = DateRange::parse_in_zone(Some("2020-01-02"), Some("2020-01-02"), chrono_tz::UTC)
            .unwrap_err();
        assert!(err.to_string().contains("before end-date"));
    }

    #[test]
    fn rejects_bad_date() {
        assert!(DateRange::parse(Some("2020/01/01"), None).is_err());
        assert!(DateRange::parse_optional_tz(None, Some("nope"), Some(chrono_tz::UTC)).is_err());
    }

    #[test]
    fn a_named_zone_is_accepted() {
        // The old helper rejected IANA names and took only `UTC-05:00`. A zone
        // is what a multi-year backup needs, so the name is now the input.
        assert!(
            DateRange::parse_optional_tz(
                Some("2020-01-01"),
                None,
                Some(chrono_tz::America::New_York)
            )
            .is_ok()
        );
    }

    #[test]
    fn f64_floors_toward_contains() {
        let range =
            DateRange::parse_in_zone(Some("2020-01-01"), Some("2020-01-02"), chrono_tz::UTC)
                .unwrap();
        assert!(range.contains_secs_f64(1_577_836_800.9));
        assert!(!range.contains_secs_f64(1_577_923_200.0)); // 2020-01-02 00:00 UTC
    }

    #[test]
    fn a_zone_midnight_is_not_a_utc_midnight() {
        // The point of naming a zone: the day starts when it starts there.
        let range = DateRange::parse_optional_tz(
            Some("2020-01-01"),
            Some("2020-01-02"),
            Some(chrono_tz::America::New_York),
        )
        .unwrap();
        assert_eq!(range.start_secs, Some(1_577_854_800)); // 2020-01-01 00:00 EST
        assert!(!range.is_unbounded());
    }
}
