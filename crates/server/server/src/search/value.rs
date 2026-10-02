//! Typed values a word can take. Dates are spans, so `date:2019` is the
//! year and `date:>2019` is after it ends; `today` is an input, never the clock.

use chrono::TimeZone;
use chrono::{DateTime, Datelike, Days, Duration, NaiveDate, NaiveDateTime, Utc};

/// Relative spans further back than this are refused.
const MAX_LOOKBACK_DAYS: u64 = 3_650;

/// A comparison or range on an ordered scalar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cmp<T> {
    Eq(T),
    Gt(T),
    Gte(T),
    Lt(T),
    Lte(T),
    /// Inclusive on both ends.
    Range(T, T),
}

/// The days a date value names: `start` inclusive, `end` exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DateSpan {
    pub start: NaiveDate,
    pub end: NaiveDate,
}

/// A date filter with its bounds already resolved to calendar days.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DateCmp {
    /// Inside the span.
    In(DateSpan),
    /// On or after this day.
    Gte(NaiveDate),
    /// On or after this day (the span's end, so "after the span").
    Gt(NaiveDate),
    /// Before this day.
    Lt(NaiveDate),
    /// Before this day (the span's end, so "up to the span's last day").
    Lte(NaiveDate),
}

/// One parsed value, typed by the word it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Value {
    Text(String),
    /// `pre*` on a text word.
    Prefix(String),
    /// `#12`.
    Id(i64),
    /// A universal keyword: `none`, `any`, `me`, `unknown`, `last`.
    Keyword(&'static str),
    /// One of a word's fixed choices, already lower-cased and checked.
    Choice(&'static str),
    Date(DateCmp),
    Count(Cmp<i64>),
    Size(Cmp<i64>),
}

/// The instant `day` begins in `zone`, as the RFC 3339 UTC text the server
/// stores (`2024-01-01T05:00:00Z`), so a day or a year in the account's time
/// zone compares against `messages.timestamp` as text. A day whose midnight
/// falls in a daylight-saving gap starts at the first instant after the gap.
/// A day the zone skipped whole (Pacific/Apia, 30 December 2011) starts where
/// the next day starts, so it holds no instant.
///
/// `None` when the instant falls after year 9999: RFC 3339 text for it has a
/// sign and five digits (`+10000-01-01T00:00:00Z`), and `+` sorts before
/// every digit, so as text it would come before every stored timestamp
/// instead of after them all.
pub(crate) fn utc_instant(zone: chrono_tz::Tz, day: NaiveDate) -> Option<String> {
    let midnight = day.and_hms_opt(0, 0, 0).expect("midnight is a valid time");
    let instant = first_instant_at_or_after(zone, midnight);
    (instant.year() <= 9999).then(|| instant.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

/// The first instant whose clock reading in `zone` is `local` or later.
/// When `local` exists, that is its earliest occurrence. When it falls in a
/// gap of any length, it is the instant the gap ends.
fn first_instant_at_or_after(zone: chrono_tz::Tz, local: NaiveDateTime) -> DateTime<Utc> {
    if let Some(t) = zone.from_local_datetime(&local).earliest() {
        return t.with_timezone(&Utc);
    }
    // Every UTC offset is shorter than a day, so a day before `local` read as
    // UTC the clock shows an earlier time, and a day after it a later one.
    // The gap's end lies between. A binary search over whole seconds finds
    // it, because every transition falls on a whole second.
    let mut before = local.and_utc() - Duration::days(1);
    let mut after = local.and_utc() + Duration::days(1);
    while after - before > Duration::seconds(1) {
        let mid = before + (after - before) / 2;
        if mid.with_timezone(&zone).naive_local() >= local {
            after = mid;
        } else {
            before = mid;
        }
    }
    after
}

/// The first day of the month after `(y, m)`.
fn first_of_next_month(y: i32, m: u32) -> Option<NaiveDate> {
    if m == 12 {
        NaiveDate::from_ymd_opt(y + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(y, m + 1, 1)
    }
}

/// `today` moved back `months` months, clamped to the last day of the target month.
fn shift_months_back(today: NaiveDate, months: u32) -> Option<NaiveDate> {
    let total = i64::from(today.year()) * 12 + i64::from(today.month()) - 1 - i64::from(months);
    let year = i32::try_from(total.div_euclid(12)).ok()?;
    let month = u32::try_from(total.rem_euclid(12) + 1).ok()?;
    let last = first_of_next_month(year, month)?.pred_opt()?.day();
    NaiveDate::from_ymd_opt(year, month, today.day().min(last))
}

/// True for a non-empty string of ASCII digits.
fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// A bare date value as a span. Accepts `YYYY`, `YYYY-MM`, `YYYY-MM-DD`,
/// `today`, `yesterday`, and `Nd`/`Nw`/`Nm`/`Ny` (the last N units ending today).
pub(crate) fn parse_date_span(raw: &str, today: NaiveDate) -> Option<DateSpan> {
    let t = raw.trim().to_ascii_lowercase();
    let tomorrow = today.checked_add_days(Days::new(1))?;
    match t.as_str() {
        "" => return None,
        "today" => {
            return Some(DateSpan {
                start: today,
                end: tomorrow,
            });
        }
        "yesterday" => {
            return Some(DateSpan {
                start: today.checked_sub_days(Days::new(1))?,
                end: today,
            });
        }
        _ => {}
    }
    if let Some(unit) = t.chars().last()
        && matches!(unit, 'd' | 'w' | 'm' | 'y')
        && all_digits(&t[..t.len() - 1])
    {
        let n: u32 = t[..t.len() - 1].parse().ok()?;
        let days = match unit {
            'd' => u64::from(n),
            'w' => u64::from(n) * 7,
            'm' => u64::from(n) * 31,
            _ => u64::from(n) * 365,
        };
        if days > MAX_LOOKBACK_DAYS {
            return None;
        }
        let start = match unit {
            'd' => today.checked_sub_days(Days::new(u64::from(n)))?,
            'w' => today.checked_sub_days(Days::new(u64::from(n) * 7))?,
            'm' => shift_months_back(today, n)?,
            _ => shift_months_back(today, n * 12)?,
        };
        return Some(DateSpan {
            start,
            end: tomorrow,
        });
    }
    let parts: Vec<&str> = t.split('-').collect();
    match parts.as_slice() {
        [y] if y.len() == 4 && all_digits(y) => {
            let y: i32 = y.parse().ok()?;
            Some(DateSpan {
                start: NaiveDate::from_ymd_opt(y, 1, 1)?,
                end: NaiveDate::from_ymd_opt(y + 1, 1, 1)?,
            })
        }
        [y, m] if y.len() == 4 && all_digits(y) && m.len() == 2 && all_digits(m) => {
            let (y, m): (i32, u32) = (y.parse().ok()?, m.parse().ok()?);
            Some(DateSpan {
                start: NaiveDate::from_ymd_opt(y, m, 1)?,
                end: first_of_next_month(y, m)?,
            })
        }
        [y, m, d]
            if y.len() == 4
                && all_digits(y)
                && m.len() == 2
                && all_digits(m)
                && d.len() == 2
                && all_digits(d) =>
        {
            let start = NaiveDate::from_ymd_opt(y.parse().ok()?, m.parse().ok()?, d.parse().ok()?)?;
            Some(DateSpan {
                start,
                end: start.checked_add_days(Days::new(1))?,
            })
        }
        _ => None,
    }
}

/// A date value with its optional comparison or range.
pub(crate) fn parse_date(raw: &str, today: NaiveDate) -> Option<DateCmp> {
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    if let Some((a, b)) = t.split_once("..") {
        let (a, b) = (parse_date_span(a, today)?, parse_date_span(b, today)?);
        if b.end <= a.start {
            return None;
        }
        return Some(DateCmp::In(DateSpan {
            start: a.start,
            end: b.end,
        }));
    }
    if let Some(rest) = t.strip_prefix(">=") {
        return Some(DateCmp::Gte(parse_date_span(rest, today)?.start));
    }
    if let Some(rest) = t.strip_prefix("<=") {
        return Some(DateCmp::Lte(parse_date_span(rest, today)?.end));
    }
    if let Some(rest) = t.strip_prefix('>') {
        return Some(DateCmp::Gt(parse_date_span(rest, today)?.end));
    }
    if let Some(rest) = t.strip_prefix('<') {
        return Some(DateCmp::Lt(parse_date_span(rest, today)?.start));
    }
    Some(DateCmp::In(parse_date_span(t, today)?))
}

/// `>3`, `>=3`, `<10`, `<=10`, `1..10`, or a bare scalar meaning equals.
pub(crate) fn parse_cmp<T: Copy + PartialOrd>(
    raw: &str,
    scalar: impl Fn(&str) -> Option<T>,
) -> Option<Cmp<T>> {
    let t = raw.trim();
    if let Some((a, b)) = t.split_once("..") {
        let (a, b) = (scalar(a)?, scalar(b)?);
        return if a <= b { Some(Cmp::Range(a, b)) } else { None };
    }
    if let Some(rest) = t.strip_prefix(">=") {
        return Some(Cmp::Gte(scalar(rest)?));
    }
    if let Some(rest) = t.strip_prefix("<=") {
        return Some(Cmp::Lte(scalar(rest)?));
    }
    if let Some(rest) = t.strip_prefix('>') {
        return Some(Cmp::Gt(scalar(rest)?));
    }
    if let Some(rest) = t.strip_prefix('<') {
        return Some(Cmp::Lt(scalar(rest)?));
    }
    Some(Cmp::Eq(scalar(t.strip_prefix('=').unwrap_or(t))?))
}

/// `500k`, `1M`, `2G` (1024-based, case-insensitive), or bare bytes.
pub(crate) fn parse_size_bytes(raw: &str) -> Option<i64> {
    let t = raw.trim().to_ascii_lowercase();
    let end = t
        .bytes()
        .position(|b| !(b.is_ascii_digit() || b == b'.'))
        .unwrap_or(t.len());
    if end == 0 {
        return None;
    }
    let n: f64 = t[..end].parse().ok()?;
    let mult = match t[end..].trim().trim_end_matches('b') {
        "" => 1.0,
        "k" => 1024.0,
        "m" => 1024.0_f64.powi(2),
        "g" => 1024.0_f64.powi(3),
        _ => return None,
    };
    let bytes = (n * mult).round();
    if !bytes.is_finite() || bytes < 0.0 || bytes > i64::MAX as f64 {
        return None;
    }
    Some(bytes as i64)
}

/// A non-negative integer.
pub(crate) fn parse_count(raw: &str) -> Option<i64> {
    let t = raw.trim();
    if !all_digits(t) {
        return None;
    }
    t.parse().ok()
}

/// `#12`: the row with that id.
pub(crate) fn parse_id(raw: &str) -> Option<i64> {
    let digits = raw.trim().strip_prefix('#')?;
    if !all_digits(digits) {
        return None;
    }
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }
    const TODAY: fn() -> NaiveDate = || d(2026, 9, 2);

    #[test]
    fn a_partial_date_names_its_whole_span() {
        assert_eq!(
            parse_date_span("2019", TODAY()).unwrap(),
            DateSpan {
                start: d(2019, 1, 1),
                end: d(2020, 1, 1)
            }
        );
        assert_eq!(
            parse_date_span("2024-02", TODAY()).unwrap(),
            DateSpan {
                start: d(2024, 2, 1),
                end: d(2024, 3, 1)
            }
        );
        assert_eq!(
            parse_date_span("2024-02-29", TODAY()).unwrap(),
            DateSpan {
                start: d(2024, 2, 29),
                end: d(2024, 3, 1)
            }
        );
        assert!(parse_date_span("2019-13", TODAY()).is_none());
        assert!(parse_date_span("2023-02-30", TODAY()).is_none());
    }

    #[test]
    fn relative_spans_end_tomorrow() {
        assert_eq!(
            parse_date_span("7d", TODAY()).unwrap(),
            DateSpan {
                start: d(2026, 8, 26),
                end: d(2026, 9, 3)
            }
        );
        assert_eq!(
            parse_date_span("2w", TODAY()).unwrap().start,
            d(2026, 8, 19)
        );
        assert_eq!(parse_date_span("3m", TODAY()).unwrap().start, d(2026, 6, 2));
        assert_eq!(parse_date_span("1y", TODAY()).unwrap().start, d(2025, 9, 2));
        assert_eq!(
            parse_date_span("today", TODAY()).unwrap(),
            DateSpan {
                start: d(2026, 9, 2),
                end: d(2026, 9, 3)
            }
        );
        assert_eq!(
            parse_date_span("yesterday", TODAY()).unwrap(),
            DateSpan {
                start: d(2026, 9, 1),
                end: d(2026, 9, 2)
            }
        );
        // A month shift lands on the last day when the month is shorter.
        assert_eq!(
            parse_date_span("1m", d(2026, 3, 31)).unwrap().start,
            d(2026, 2, 28)
        );
        // More than ten years back is refused.
        assert!(parse_date_span("11y", TODAY()).is_none());
    }

    /// Each unit is measured against the ten-year limit in its own days: a
    /// span of 30 days, 12 weeks or 18 months is well inside it.
    #[test]
    fn spans_of_many_days_weeks_or_months_are_inside_the_ten_year_limit() {
        assert_eq!(
            parse_date_span("30d", TODAY()).unwrap().start,
            d(2026, 8, 3)
        );
        assert_eq!(
            parse_date_span("12w", TODAY()).unwrap().start,
            d(2026, 6, 10)
        );
        assert_eq!(
            parse_date_span("18m", TODAY()).unwrap().start,
            d(2025, 3, 2)
        );
    }

    #[test]
    fn december_ends_on_the_first_of_january() {
        assert_eq!(
            parse_date_span("2024-12", TODAY()).unwrap(),
            DateSpan {
                start: d(2024, 12, 1),
                end: d(2025, 1, 1)
            }
        );
    }

    #[test]
    fn comparisons_resolve_against_the_span_edges() {
        assert_eq!(
            parse_date(">=2019", TODAY()).unwrap(),
            DateCmp::Gte(d(2019, 1, 1))
        );
        assert_eq!(
            parse_date(">2019", TODAY()).unwrap(),
            DateCmp::Gt(d(2020, 1, 1))
        );
        assert_eq!(
            parse_date("<2019", TODAY()).unwrap(),
            DateCmp::Lt(d(2019, 1, 1))
        );
        assert_eq!(
            parse_date("<=2019", TODAY()).unwrap(),
            DateCmp::Lte(d(2020, 1, 1))
        );
        assert_eq!(
            parse_date("2019..2021", TODAY()).unwrap(),
            DateCmp::In(DateSpan {
                start: d(2019, 1, 1),
                end: d(2022, 1, 1)
            })
        );
        assert_eq!(
            parse_date("<1m", TODAY()).unwrap(),
            DateCmp::Lt(d(2026, 8, 2))
        );
        assert!(parse_date("2021..2019", TODAY()).is_none());
        assert!(parse_date("", TODAY()).is_none());
    }

    #[test]
    fn sizes_are_1024_based() {
        assert_eq!(parse_size_bytes("500k").unwrap(), 512_000);
        assert_eq!(parse_size_bytes("1M").unwrap(), 1_048_576);
        assert_eq!(parse_size_bytes("2g").unwrap(), 2_147_483_648);
        assert_eq!(parse_size_bytes("12345").unwrap(), 12_345);
        assert_eq!(parse_size_bytes("1.5M").unwrap(), 1_572_864);
        assert!(parse_size_bytes("big").is_none());
    }

    #[test]
    fn a_size_of_zero_is_zero_bytes() {
        assert_eq!(parse_size_bytes("0").unwrap(), 0);
    }

    #[test]
    fn comparisons_and_ranges_on_counts() {
        assert_eq!(parse_cmp(">3", parse_count).unwrap(), Cmp::Gt(3));
        assert_eq!(parse_cmp(">=3", parse_count).unwrap(), Cmp::Gte(3));
        assert_eq!(parse_cmp("<10", parse_count).unwrap(), Cmp::Lt(10));
        assert_eq!(parse_cmp("<=10", parse_count).unwrap(), Cmp::Lte(10));
        assert_eq!(parse_cmp("0", parse_count).unwrap(), Cmp::Eq(0));
        assert_eq!(parse_cmp("1..10", parse_count).unwrap(), Cmp::Range(1, 10));
        assert!(parse_cmp("10..1", parse_count).is_none());
        assert!(parse_cmp("many", parse_count).is_none());
    }

    #[test]
    fn ids_need_the_hash() {
        assert_eq!(parse_id("#12").unwrap(), 12);
        assert!(parse_id("12").is_none());
        assert!(parse_id("#").is_none());
        assert!(parse_id("#-1").is_none());
    }

    /// Pacific/Apia skipped 30 December 2011: the clock went from the end of
    /// the 29th at -10:00 straight to the 31st at +14:00. The skipped day
    /// begins where the 31st begins, so it holds no instant (#1204).
    #[test]
    fn a_skipped_day_begins_where_the_next_day_begins() {
        let zone = chrono_tz::Pacific::Apia;
        assert_eq!(
            utc_instant(zone, d(2011, 12, 29)).as_deref(),
            Some("2011-12-29T10:00:00Z")
        );
        assert_eq!(
            utc_instant(zone, d(2011, 12, 30)).as_deref(),
            Some("2011-12-30T10:00:00Z")
        );
        assert_eq!(
            utc_instant(zone, d(2011, 12, 31)).as_deref(),
            Some("2011-12-30T10:00:00Z")
        );
        assert_eq!(
            utc_instant(zone, d(2012, 1, 1)).as_deref(),
            Some("2011-12-31T10:00:00Z")
        );
    }

    /// Kwajalein skipped 21 August 1993, and Kiritimati 31 December 1994.
    #[test]
    fn every_skipped_day_begins_where_the_next_day_begins() {
        for (zone, skipped) in [
            (chrono_tz::Pacific::Kwajalein, d(1993, 8, 21)),
            (chrono_tz::Pacific::Kiritimati, d(1994, 12, 31)),
            (chrono_tz::Pacific::Fakaofo, d(2011, 12, 30)),
        ] {
            let next = skipped.succ_opt().unwrap();
            assert_eq!(
                utc_instant(zone, skipped),
                utc_instant(zone, next),
                "{zone} {skipped}"
            );
        }
    }

    /// São Paulo moved its clocks from midnight to 01:00 on 4 November 2018,
    /// so that day begins at 01:00 -02:00.
    #[test]
    fn a_midnight_in_a_one_hour_gap_starts_the_day_after_the_gap() {
        assert_eq!(
            utc_instant(chrono_tz::America::Sao_Paulo, d(2018, 11, 4)).as_deref(),
            Some("2018-11-04T03:00:00Z")
        );
    }
}
