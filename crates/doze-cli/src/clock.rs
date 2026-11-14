//! This machine's wall clock, as Go's `time.Local` gives it: the zone `TZ` names when set (an IANA name or a POSIX
//! rule), the system's otherwise, UTC when neither can be read. Log lines and `why` are stamped with it.

use doze_control::Timestamp;
use jiff::tz::TimeZone;

// fuzz:parser fuzz_env
/// The zone for a `TZ` value (`None`: unset).
pub fn zone(tz: Option<&str>) -> TimeZone {
    match tz.map(|z| z.trim_start_matches(':')) {
        Some("" | "UTC" | "UTC0" | "Etc/UTC") => TimeZone::UTC,
        Some(name) => TimeZone::get(name)
            .or_else(|_| TimeZone::posix(name))
            .unwrap_or_else(|_| TimeZone::try_system().unwrap_or(TimeZone::UTC)),
        None => TimeZone::try_system().unwrap_or(TimeZone::UTC),
    }
}

/// `15:04:05` in `zone`; `--:--:--` for a time that is not one.
pub fn wall(t: &Timestamp, zone: &TimeZone) -> String {
    t.unix_nanos()
        .and_then(|n| jiff::Timestamp::from_nanosecond(n).ok())
        .map_or_else(
            || "--:--:--".to_owned(),
            |ts| ts.to_zoned(zone.clone()).strftime("%H:%M:%S").to_string(),
        )
}
