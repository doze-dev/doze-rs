//! Times as the pages write them: a clock in the person's zone (`19:20:40`), a date for another day (`Oct 10`), how
//! far back a log line is (`-3m 05s`), and a duration as Go prints one (`1.2s`, `400ms`). The zone is handed in, so
//! the words do not read the machine's settings and the goldens (written at +05:30) read the same anywhere.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A fixed offset from UTC, in seconds east: the person's zone at the moment a page is drawn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Zone(pub i32);

impl Zone {
    /// UTC.
    pub const UTC: Zone = Zone(0);

    /// Seconds since the epoch, in this zone.
    fn local(self, t: SystemTime) -> i64 {
        secs(t) + i64::from(self.0)
    }

    /// `15:04:05`.
    pub fn clock(self, t: SystemTime) -> String {
        let s = self.local(t).rem_euclid(86_400);
        format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    }

    /// `Jan 2`.
    pub fn date(self, t: SystemTime) -> String {
        let (_, m, d) = civil(self.local(t).div_euclid(86_400));
        let month = MONTHS.get(m.saturating_sub(1) as usize).unwrap_or(&"");
        format!("{month} {d}")
    }
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Whole seconds since the epoch, negative before it.
pub(crate) fn secs(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
        Err(e) => -i64::try_from(e.duration().as_secs()).unwrap_or(i64::MAX),
    }
}

/// The UTC day of `t`, as days since the epoch.
pub(crate) fn utc_day(t: SystemTime) -> i64 {
    secs(t).div_euclid(86_400)
}

/// Year, month (1–12) and day (1–31) of a day counted from 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let m = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// A clock time today (by UTC day, as Go's view compared them), else the date. (Go `when`.)
pub fn when(t: SystemTime, now: SystemTime, zone: Zone) -> String {
    if utc_day(t) == utc_day(now) {
        zone.clock(t)
    } else {
        zone.date(t)
    }
}

/// A time before now as the logs write it: `-3m 05s` within the hour, `-2h` and `-3d` past it. (Go `rel`.)
pub fn rel(t: Option<SystemTime>, now: SystemTime) -> String {
    let d = t
        .and_then(|t| now.duration_since(t).ok())
        .unwrap_or(Duration::ZERO);
    let s = (d.as_millis() + 500) / 1000;
    if s < 60 {
        format!("-{s}s")
    } else if s < 3600 {
        format!("-{}m {:02}s", s / 60, s % 60)
    } else {
        format!(
            "-{}",
            doze_display::ago(Duration::from_secs(u64::try_from(s).unwrap_or(u64::MAX)))
        )
    }
}

/// A duration rounded to a tenth of a second, as Go's `Duration.String` prints it: `400ms`, `1.2s`, `2m3.5s`.
pub fn tenths(d: Duration) -> String {
    let ms = (d.as_millis() + 50) / 100 * 100;
    if ms == 0 {
        return "0s".to_owned();
    }
    if ms < 1000 {
        return format!("{ms}ms");
    }
    let (h, m) = (ms / 3_600_000, ms / 60_000 % 60);
    let s = ms % 60_000;
    let sec = if s.is_multiple_of(1000) {
        format!("{}s", s / 1000)
    } else {
        format!("{}.{}s", s / 1000, s % 1000 / 100)
    };
    match (h, m) {
        (0, 0) => sec,
        (0, m) => format!("{m}m{sec}"),
        (h, m) => format!("{h}h{m}m{sec}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{Zone, rel, tenths, when};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn at(s: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(s)
    }

    #[test]
    fn clocks_and_dates_in_a_zone() {
        // 2026-10-11T14:04:40Z at +05:30.
        let t = at(1_791_727_480);
        let ist = Zone(19_800);
        assert_eq!(ist.clock(t), "19:34:40");
        assert_eq!(Zone::UTC.clock(t), "14:04:40");
        assert_eq!(ist.date(t - Duration::from_secs(26 * 3600)), "Oct 10");
        assert_eq!(when(t - Duration::from_secs(120), t, ist), "19:32:40");
        assert_eq!(Zone(-3600).date(at(0)), "Dec 31");
    }

    #[test]
    fn relative_times() {
        let now = at(10_000);
        assert_eq!(rel(Some(now - Duration::from_secs(840)), now), "-14m 00s");
        assert_eq!(rel(Some(now - Duration::from_secs(5)), now), "-5s");
        assert_eq!(rel(Some(now + Duration::from_secs(5)), now), "-0s");
        assert_eq!(rel(None, now), "-0s");
        assert_eq!(rel(Some(now - Duration::from_secs(7200)), now), "-2h");
    }

    #[test]
    fn durations_as_go_prints_them() {
        assert_eq!(tenths(Duration::from_millis(1234)), "1.2s");
        assert_eq!(tenths(Duration::from_millis(400)), "400ms");
        assert_eq!(tenths(Duration::from_millis(60_000)), "1m0s");
        assert_eq!(tenths(Duration::from_millis(123_450)), "2m3.5s");
        assert_eq!(tenths(Duration::ZERO), "0s");
    }
}
