//! Go's `time.Time` and `time.Duration` as they cross the wire.
//!
//! A time is an RFC 3339 string with up to nine fraction digits and the daemon's own offset
//! (`2026-11-13T04:19:47.338643+05:30`); the zero time is `0001-01-01T00:00:00Z`. [`Timestamp`] keeps the string
//! as it came, so a frame re-encodes byte for byte, and reads it as an instant on demand. A duration is a count of
//! nanoseconds.

use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How Go writes its zero time.
pub const ZERO_TIME: &str = "0001-01-01T00:00:00Z";

const NANOS_PER_SEC: i128 = 1_000_000_000;
const SECS_PER_DAY: i64 = 86_400;

/// A Go `time.Time` on the wire.
#[derive(Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct Timestamp(String);

impl Default for Timestamp {
    fn default() -> Self {
        Timestamp(ZERO_TIME.to_owned())
    }
}

impl fmt::Debug for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // Go leaves a time it is sent `null` for at zero.
        Ok(Option::<String>::deserialize(d)?.map_or_else(Timestamp::default, Timestamp))
    }
}

impl Timestamp {
    /// The zero time.
    pub fn zero() -> Timestamp {
        Timestamp::default()
    }

    /// Whether this is Go's zero time (which the daemon sends for "never").
    pub fn is_zero(&self) -> bool {
        self.0.is_empty() || self.unix_nanos() == Some(zero_nanos())
    }

    /// An RFC 3339 time, written as Go writes it (UTC, nanoseconds with the trailing zeros trimmed); `None` when the
    /// text is not one.
    pub fn parse(s: &str) -> Option<Timestamp> {
        parse_rfc3339(s).map(Timestamp::from_unix_nanos)
    }

    /// The text as it was sent.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Nanoseconds since the Unix epoch, or `None` if the text is not an RFC 3339 time.
    // fuzz:parser fuzz_text
    pub fn unix_nanos(&self) -> Option<i128> {
        parse_rfc3339(&self.0)
    }

    /// The instant, if it is one a `SystemTime` can hold and not the zero time.
    pub fn to_system_time(&self) -> Option<SystemTime> {
        if self.is_zero() {
            return None;
        }
        let n = self.unix_nanos()?;
        let d = Duration::from_nanos(u64::try_from(n.unsigned_abs()).ok()?);
        if n >= 0 {
            UNIX_EPOCH.checked_add(d)
        } else {
            UNIX_EPOCH.checked_sub(d)
        }
    }

    /// The time `nanos` after the epoch, written in UTC as Go's RFC 3339 with nanoseconds does.
    pub fn from_unix_nanos(nanos: i128) -> Timestamp {
        let secs = nanos.div_euclid(NANOS_PER_SEC);
        let frac = nanos.rem_euclid(NANOS_PER_SEC);
        let days = i64::try_from(secs.div_euclid(i128::from(SECS_PER_DAY))).unwrap_or(0);
        let rest = i64::try_from(secs.rem_euclid(i128::from(SECS_PER_DAY))).unwrap_or(0);
        let (y, m, d) = civil_from_days(days);
        let mut s = format!(
            "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}",
            rest / 3600,
            rest % 3600 / 60,
            rest % 60
        );
        if frac != 0 {
            let digits = format!("{frac:09}");
            s.push('.');
            s.push_str(digits.trim_end_matches('0'));
        }
        s.push('Z');
        Timestamp(s)
    }

    /// A `SystemTime`, written as [`Timestamp::from_unix_nanos`] does.
    pub fn from_system_time(t: SystemTime) -> Timestamp {
        let nanos = match t.duration_since(UNIX_EPOCH) {
            Ok(d) => i128::try_from(d.as_nanos()).unwrap_or(i128::MAX),
            Err(e) => -i128::try_from(e.duration().as_nanos()).unwrap_or(i128::MAX),
        };
        Timestamp::from_unix_nanos(nanos)
    }
}

fn zero_nanos() -> i128 {
    i128::from(days_from_civil(1, 1, 1)) * i128::from(SECS_PER_DAY) * NANOS_PER_SEC
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// A run of ASCII digits as a number.
fn digits(s: &str) -> Option<i64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// `YYYY-MM-DDTHH:MM:SS[.fff…](Z|±HH:MM)` as nanoseconds since the epoch.
fn parse_rfc3339(s: &str) -> Option<i128> {
    let date = s.get(..10)?;
    let (y, m, d) = (
        digits(date.get(..4)?)?,
        digits(date.get(5..7)?)?,
        digits(date.get(8..10)?)?,
    );
    let sep = |i: usize, c: &[u8]| s.as_bytes().get(i).is_some_and(|b| c.contains(b));
    if !(sep(4, b"-") && sep(7, b"-") && sep(10, b"Tt") && sep(13, b":") && sep(16, b":")) {
        return None;
    }
    let (h, mi, sec) = (
        digits(s.get(11..13)?)?,
        digits(s.get(14..16)?)?,
        digits(s.get(17..19)?)?,
    );
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || h > 23 || mi > 59 || sec > 60 {
        return None;
    }
    let mut rest = s.get(19..)?;
    let mut frac: i128 = 0;
    if let Some(f) = rest.strip_prefix('.') {
        let n = f.bytes().take_while(u8::is_ascii_digit).count();
        if n == 0 || n > 9 {
            return None;
        }
        frac = i128::from(digits(f.get(..n)?)?) * 10i128.pow(u32::try_from(9 - n).ok()?);
        rest = f.get(n..)?;
    }
    let offset = match rest {
        "Z" | "z" => 0,
        _ => {
            let sign = match rest.get(..1)? {
                "+" => 1,
                "-" => -1,
                _ => return None,
            };
            if rest.len() != 6 || rest.as_bytes().get(3) != Some(&b':') {
                return None;
            }
            let (oh, om) = (digits(rest.get(1..3)?)?, digits(rest.get(4..6)?)?);
            sign * (oh * 3600 + om * 60)
        }
    };
    let secs = days_from_civil(y, m, d) * SECS_PER_DAY + h * 3600 + mi * 60 + sec - offset;
    Some(i128::from(secs) * NANOS_PER_SEC + frac)
}

/// A Go `time.Duration` on the wire: nanoseconds.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct GoDuration(pub i64);

impl GoDuration {
    /// Whether it is zero (Go leaves a zero duration out of the frame).
    pub fn is_zero(&self) -> bool {
        self.0 == 0
    }

    /// As a std duration; a negative one is zero.
    pub fn to_std(self) -> Duration {
        Duration::from_nanos(u64::try_from(self.0).unwrap_or(0))
    }

    /// From a std duration, saturating at Go's largest.
    pub fn from_std(d: Duration) -> GoDuration {
        GoDuration(i64::try_from(d.as_nanos()).unwrap_or(i64::MAX))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_go_times_with_offsets_and_fractions() {
        let utc = Timestamp("2026-11-12T22:49:47.34441Z".into());
        let ist = Timestamp("2026-11-13T04:19:47.34441+05:30".into());
        assert_eq!(utc.unix_nanos(), ist.unix_nanos());
        assert_eq!(utc.unix_nanos(), Some(1_794_523_787_344_410_000));
        assert!(Timestamp::zero().is_zero());
        assert_eq!(Timestamp::zero().to_system_time(), None);
        assert!(!utc.is_zero());
        for bad in [
            "",
            "2026-11-12",
            "2026-11-12T22:49:47",
            "2026-11-12T22:49:47.Z",
            "2026-13-12T22:49:47Z",
            "x",
        ] {
            assert_eq!(Timestamp(bad.into()).unix_nanos(), None, "{bad}");
        }
    }

    #[test]
    fn writes_times_as_go_does() {
        for s in [
            "2026-11-12T22:49:47.34441Z",
            "0001-01-01T00:00:00Z",
            "1969-12-31T23:59:59.999999999Z",
            "2026-10-11T14:04:40Z",
        ] {
            let t = Timestamp(s.into());
            let n = t.unix_nanos();
            assert_eq!(n.map(Timestamp::from_unix_nanos), Some(t), "{s}");
        }
        let now = SystemTime::now();
        assert_eq!(Timestamp::from_system_time(now).to_system_time(), Some(now));
    }
}
