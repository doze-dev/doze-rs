//! Calendar arithmetic on the proleptic Gregorian calendar (Howard Hinnant's algorithms): days since 1970-01-01 to and from a date.

/// A date and time of day, to the minute, in UTC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Civil {
    pub(crate) year: i64,
    /// 1 to 12.
    pub(crate) month: u32,
    /// 1 to 31.
    pub(crate) day: u32,
    pub(crate) hour: u32,
    pub(crate) minute: u32,
}

/// Days from 1970-01-01 to the date (negative before it).
pub(crate) fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date `days` after 1970-01-01.
pub(crate) fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let month = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Whether `year` has a 29th of February.
pub(crate) const fn leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// The number of days in the month.
pub(crate) const fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        2 if leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// The day of the week, 1 (Sunday) to 7 (Saturday), as cron numbers them.
pub(crate) fn weekday(days: i64) -> u32 {
    // 1970-01-01 was a Thursday, which is 5 when Sunday is 1.
    u32::try_from((days + 4).rem_euclid(7) + 1).unwrap_or(1)
}

impl Civil {
    /// The minute containing `ms` (milliseconds since the epoch).
    pub(crate) fn of_ms(ms: i64) -> Self {
        let minutes = ms.div_euclid(60_000);
        let days = minutes.div_euclid(1440);
        let in_day = minutes.rem_euclid(1440);
        let (year, month, day) = civil_from_days(days);
        Self {
            year,
            month,
            day,
            hour: u32::try_from(in_day / 60).unwrap_or(0),
            minute: u32::try_from(in_day % 60).unwrap_or(0),
        }
    }

    /// Milliseconds since the epoch at the start of this minute.
    pub(crate) fn to_ms(self) -> i64 {
        let days = days_from_civil(self.year, self.month, self.day);
        (days * 1440 + i64::from(self.hour) * 60 + i64::from(self.minute)) * 60_000
    }

    pub(crate) fn days(self) -> i64 {
        days_from_civil(self.year, self.month, self.day)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_round_trip_through_days() {
        for days in [-800_000, -1, 0, 1, 365, 10_957, 19_000, 2_000_000] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
    }

    #[test]
    fn weekdays_count_from_sunday_as_one() {
        assert_eq!(weekday(0), 5, "1970-01-01 was a Thursday");
        assert_eq!(weekday(days_from_civil(2026, 11, 13)), 6, "a Friday");
        assert_eq!(weekday(days_from_civil(2026, 11, 15)), 1, "a Sunday");
    }

    #[test]
    fn a_minute_is_found_and_put_back() {
        let ms = 1_700_000_000_000;
        let c = Civil::of_ms(ms);
        assert_eq!(
            (c.year, c.month, c.day, c.hour, c.minute),
            (2023, 11, 14, 22, 13)
        );
        assert_eq!(c.to_ms(), ms - ms % 60_000);
    }
}
