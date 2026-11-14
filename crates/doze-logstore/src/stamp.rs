//! The time at the start of a line: Go's layout `2006-01-02T15:04:05.000000Z`, UTC, microseconds truncated as Go's
//! `AppendFormat` truncates them.

use std::time::{SystemTime, UNIX_EPOCH};

/// `t` as a line's stamp. A time before 1970 is written as the epoch (no clock doze runs on reads that).
pub fn stamp(t: SystemTime) -> String {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs();
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (y, m, day) = civil(days);
    format!(
        "{y:04}-{m:02}-{day:02}T{:02}:{:02}:{:02}.{:06}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        d.subsec_micros()
    )
}

/// Days since 1970-01-01 as (year, month, day) in the proleptic Gregorian calendar (Hinnant's `civil_from_days`).
fn civil(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

/// Go's layout, digit for digit.
pub(crate) fn is_stamp(s: &str) -> bool {
    const LAYOUT: &[u8] = b"dddd-dd-ddTdd:dd:dd.ddddddZ";
    let shaped = s.len() == LAYOUT.len()
        && s.bytes().zip(LAYOUT).all(|(c, l)| match l {
            b'd' => c.is_ascii_digit(),
            l => c == *l,
        });
    // The ranges Go's `time.Parse` checks too (E2 audit 1, m4): month, the day in that month, hour, minute, second.
    let n = |from: usize, to: usize| {
        s.get(from..to)
            .and_then(|d| d.parse::<u32>().ok())
            .unwrap_or(u32::MAX)
    };
    shaped && {
        let (year, month, day) = (n(0, 4), n(5, 7), n(8, 10));
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let days = match month {
            2 if leap => 29,
            2 => 28,
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        };
        (1..=12).contains(&month)
            && (1..=days).contains(&day)
            && n(11, 13) < 24
            && n(14, 16) < 60
            && n(17, 19) < 60
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Go's `time.Parse` refuses what is out of range, and so does this (E2 audit 1, m4).
    #[test]
    fn a_stamp_out_of_range_is_not_a_stamp() {
        assert!(is_stamp("2024-02-29T23:59:59.999999Z"));
        for bad in [
            "2023-02-29T00:00:00.000000Z",
            "2024-13-01T00:00:00.000000Z",
            "2024-00-01T00:00:00.000000Z",
            "2024-04-31T00:00:00.000000Z",
            "2024-01-00T00:00:00.000000Z",
            "2024-01-01T24:00:00.000000Z",
            "2024-01-01T00:60:00.000000Z",
            "2024-01-01T00:00:60.000000Z",
            "2024-01-01T00:00:00.0000000Z",
            "2024-01-01T00:00:00.00000Z",
        ] {
            assert!(!is_stamp(bad), "{bad}");
        }
    }

    #[test]
    fn stamps_are_gos_layout_in_utc_with_microseconds_truncated() {
        let at = |s: u64, n: u32| stamp(UNIX_EPOCH + Duration::new(s, n));
        assert_eq!(at(0, 0), "1970-01-01T00:00:00.000000Z");
        // 2026-10-10T09:15:02.418305999Z (`date -u -r 1791623702`).
        assert_eq!(
            at(1_791_623_702, 418_305_999),
            "2026-10-10T09:15:02.418305Z"
        );
        // A leap day and the end of a century year that is not leap.
        assert_eq!(at(951_782_400, 0), "2000-02-29T00:00:00.000000Z");
        assert_eq!(
            at(4_107_542_399, 999_999_000),
            "2100-02-28T23:59:59.999999Z"
        );
        assert_eq!(at(4_107_542_400, 0), "2100-03-01T00:00:00.000000Z");
        assert!(is_stamp(&at(1_791_623_702, 1)));
    }
}
