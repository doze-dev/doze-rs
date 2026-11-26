//! This machine's wall clock, as Go's `time.Local` gives it: the zone `TZ` names when set (an IANA name or a POSIX
//! rule, or a zone file by its path), the system's when `TZ` is unset, UTC when what it names cannot be read (as Go).
//! Log lines and `why` are stamped with it.

use doze_control::Timestamp;
use jiff::tz::TimeZone;

// fuzz:parser fuzz_env
/// The zone for a `TZ` value (`None`: unset).
pub fn zone(tz: Option<&str>) -> TimeZone {
    match tz.map(|z| z.trim_start_matches(':')) {
        Some("" | "UTC" | "UTC0" | "Etc/UTC") => TimeZone::UTC,
        // Only a regular file of a zone file's size: `TZ=/dev/zero` must not read for ever.
        Some(path) if path.starts_with('/') => std::fs::metadata(path)
            .ok()
            .filter(|m| m.is_file() && m.len() < 1 << 20)
            .and_then(|_| std::fs::read(path).ok())
            .and_then(|b| TimeZone::tzif(path, &b).ok())
            .unwrap_or(TimeZone::UTC),
        Some(name) => TimeZone::get(name)
            .or_else(|_| TimeZone::posix(name))
            .unwrap_or(TimeZone::UTC),
        None => TimeZone::try_system().unwrap_or(TimeZone::UTC),
    }
}

/// `15:04:05` in `zone`; `--:--:--` for a time that is not one.
///
/// The time of day is the floored second plus the zone's offset, written by hand: jiff's `strftime` was about 20 KB of
/// `doze` for three fields (docs/port/weight.md, "Headroom for doze").
pub fn wall(t: &Timestamp, zone: &TimeZone) -> String {
    let Some((n, ts)) = t
        .unix_nanos()
        .and_then(|n| Some((n, jiff::Timestamp::from_nanosecond(n).ok()?)))
    else {
        return "--:--:--".to_owned();
    };
    let local = n.div_euclid(1_000_000_000) + i128::from(zone.to_offset(ts).seconds());
    let day = local.rem_euclid(86_400);
    format!("{:02}:{:02}:{:02}", day / 3600, day / 60 % 60, day % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hand-written time of day is jiff's `strftime("%H:%M:%S")`, which `wall` used before, for every zone kind and
    /// across the epoch, a second's fraction before it, offsets of half and quarter hours, and both DST edges.
    #[test]
    fn wall_is_strftime() {
        let zones = [
            TimeZone::UTC,
            TimeZone::posix("EST5EDT,M3.2.0,M11.1.0").unwrap(),
            TimeZone::posix("IST-5:30").unwrap(),
            TimeZone::posix("<+0545>-5:45").unwrap(),
            TimeZone::posix("<-0930>9:30").unwrap(),
        ];
        let mut nanos: Vec<i128> = vec![
            0,
            -1,
            -999_999_999,
            -1_000_000_000,
            -1_500_000_000,
            1_500_000_000,
        ];
        // A spread from 1900 to 2100 with odd fractions, and the US 2026 DST changes, two seconds either side.
        let mut n: i128 = -2_208_988_800_000_000_000;
        while n < 4_102_444_800_000_000_000 {
            nanos.push(n);
            n += 7_919_876_543_210_123;
        }
        for edge in [1_773_039_600i128, 1_793_599_200] {
            for d in -2..=2 {
                nanos.push((edge + d) * 1_000_000_000 + 250_000_000);
            }
        }
        for zone in &zones {
            for &n in &nanos {
                let t = Timestamp::from_unix_nanos(n);
                let ts = jiff::Timestamp::from_nanosecond(t.unix_nanos().unwrap()).unwrap();
                let want = ts.to_zoned(zone.clone()).strftime("%H:%M:%S").to_string();
                assert_eq!(wall(&t, zone), want, "{n} in {zone:?}");
            }
        }
    }
}
