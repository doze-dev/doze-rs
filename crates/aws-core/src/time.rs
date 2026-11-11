//! Calendar arithmetic and the wire timestamp formats, without a date crate. The current time comes from `doze_runtime::Clock`;
//! these functions only turn seconds into text and back.

/// Days since 1970-01-01 for a proleptic Gregorian date (Hinnant's algorithm).
#[must_use]
pub const fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Inverse of [`days_from_civil`].
#[must_use]
pub const fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Parse `YYYYMMDDTHHMMSSZ` (the `X-Amz-Date` form) to unix seconds.
#[must_use]
pub fn parse_amz_date(s: &str) -> Option<i64> {
    if s.len() != 16 || s.get(8..9)? != "T" || s.get(15..16)? != "Z" {
        return None;
    }
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (
        num(0..4)?,
        num(4..6)?,
        num(6..8)?,
        num(9..11)?,
        num(11..13)?,
        num(13..15)?,
    );
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    Some(days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + se)
}

/// Format unix seconds as `YYYYMMDDTHHMMSSZ`.
#[must_use]
pub fn format_amz_date(secs: i64) -> String {
    let (y, mo, d, h, mi, s) = split(secs);
    format!("{y:04}{mo:02}{d:02}T{h:02}{mi:02}{s:02}Z")
}

/// Format unix seconds as `YYYY-MM-DDTHH:MM:SS.000Z` (query/restXml timestamps).
#[must_use]
pub fn format_iso8601(secs: i64) -> String {
    let (y, mo, d, h, mi, s) = split(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.000Z")
}

/// Format as an HTTP-date, e.g. `Sun, 06 Nov 1994 08:49:37 GMT`.
#[must_use]
pub fn format_http_date(secs: i64) -> String {
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let (y, mo, d, h, mi, s) = split(secs);
    let wd = DAYS
        .get(usize::try_from(secs.div_euclid(86_400).rem_euclid(7)).unwrap_or(0))
        .copied()
        .unwrap_or("Thu");
    let month = MONTHS
        .get(usize::try_from(mo - 1).unwrap_or(0))
        .copied()
        .unwrap_or("Jan");
    format!("{wd}, {d:02} {month} {y:04} {h:02}:{mi:02}:{s:02} GMT")
}

fn split(secs: i64) -> (i64, i64, i64, i64, i64, i64) {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, mo, d) = civil_from_days(days);
    (y, mo, d, rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn amz_date_round_trip() {
        let t = parse_amz_date("20130524T000000Z").unwrap();
        assert_eq!(t, 1_369_353_600);
        assert_eq!(format_amz_date(t), "20130524T000000Z");
        assert_eq!(format_iso8601(t), "2013-05-24T00:00:00.000Z");
        assert_eq!(
            format_http_date(784_111_777),
            "Sun, 06 Nov 1994 08:49:37 GMT"
        );
        assert!(parse_amz_date("2013-05-24").is_none());
        assert!(parse_amz_date("20131324T000000Z").is_none());
    }

    #[test]
    fn leap_years() {
        assert_eq!(civil_from_days(days_from_civil(2024, 2, 29)), (2024, 2, 29));
        assert_eq!(days_from_civil(1970, 1, 1), 0);
    }
}
