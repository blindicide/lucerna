//! Tiny UTC date formatting, so no calendar crate is needed.

use std::time::{SystemTime, UNIX_EPOCH};

/// Days since 1970-01-01 to `(year, month, day)` in the proleptic Gregorian calendar
/// (Howard Hinnant's `civil_from_days`).
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097); // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if month <= 2 { year + 1 } else { year }, month, day)
}

fn split(unix_secs: i64) -> (i64, u32, u32, i64, i64, i64) {
    let days = unix_secs.div_euclid(86_400);
    let rem = unix_secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    (y, m, d, rem / 3600, (rem % 3600) / 60, rem % 60)
}

/// `2026-09-30T10:00:00Z`
pub fn format_rfc3339(unix_secs: i64) -> String {
    let (y, m, d, hh, mm, ss) = split(unix_secs);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

/// `20260930T100000Z`, safe in file names.
pub fn format_compact(unix_secs: i64) -> String {
    let (y, m, d, hh, mm, ss) = split(unix_secs);
    format!("{y:04}{m:02}{d:02}T{hh:02}{mm:02}{ss:02}Z")
}

pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0))
}

pub fn now_rfc3339() -> String {
    format_rfc3339(now_unix())
}

pub fn now_compact() -> String {
    format_compact(now_unix())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_and_known_dates() {
        assert_eq!(format_rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_rfc3339(951_782_400), "2000-02-29T00:00:00Z"); // leap day
        assert_eq!(format_rfc3339(1_785_405_600), "2026-07-30T10:00:00Z");
        assert_eq!(format_rfc3339(1_782_813_600), "2026-06-30T10:00:00Z");
        assert_eq!(format_compact(1_782_813_600), "20260630T100000Z");
    }

    #[test]
    fn year_boundaries_and_pre_epoch() {
        assert_eq!(format_rfc3339(946_684_799), "1999-12-31T23:59:59Z");
        assert_eq!(format_rfc3339(946_684_800), "2000-01-01T00:00:00Z");
        assert_eq!(format_rfc3339(-1), "1969-12-31T23:59:59Z");
        assert_eq!(format_rfc3339(4_102_444_800), "2100-01-01T00:00:00Z");
    }

    #[test]
    fn civil_from_days_matches_reference_points() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
        assert_eq!(civil_from_days(-719_468), (0, 3, 1));
    }
}
