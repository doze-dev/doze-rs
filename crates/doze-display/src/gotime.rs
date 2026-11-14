//! Time written the way the Go surfaces write it, so a figure reads the same in the CLI and the app: a signed span
//! in nanoseconds (`now - then` can be negative when clocks disagree), rounding half away from zero, the
//! `1h2m3.5s` form, and whole units taken by truncating the floating-point figure.

use std::time::{Duration, SystemTime};

pub(crate) const MILLISECOND: i64 = 1_000_000;
pub(crate) const SECOND: i64 = 1_000_000_000;
pub(crate) const MINUTE: i64 = 60 * SECOND;
pub(crate) const HOUR: i64 = 60 * MINUTE;

/// A span in nanoseconds, saturating at the ends of `i64` as Go's does.
pub(crate) fn nanos(d: Duration) -> i64 {
    i64::try_from(d.as_nanos()).unwrap_or(i64::MAX)
}

/// `now - then` in nanoseconds, saturating. A missing `then` is the start of time, long enough ago to saturate.
pub(crate) fn since(now: SystemTime, then: Option<SystemTime>) -> i64 {
    then.map_or(i64::MAX, |then| between(now, then))
}

/// `then - now` in nanoseconds, saturating. A missing `then` is the start of time.
pub(crate) fn until(then: Option<SystemTime>, now: SystemTime) -> i64 {
    then.map_or(i64::MIN, |then| between(then, now))
}

fn between(a: SystemTime, b: SystemTime) -> i64 {
    let n = match a.duration_since(b) {
        Ok(d) => i128::try_from(d.as_nanos()).unwrap_or(i128::MAX),
        Err(e) => -i128::try_from(e.duration().as_nanos()).unwrap_or(i128::MAX),
    };
    i64::try_from(n).unwrap_or(if n < 0 { i64::MIN } else { i64::MAX })
}

/// `d` rounded to a multiple of `m`, halves away from zero; saturating where the result would not fit.
pub(crate) fn round(d: i64, m: i64) -> i64 {
    if m <= 0 {
        return d;
    }
    let less_than_half =
        |r: i64| r.unsigned_abs().wrapping_add(r.unsigned_abs()) < m.unsigned_abs();
    let r = d % m;
    if d < 0 {
        let r = -r;
        if less_than_half(r) {
            return d + r;
        }
        let d1 = d.wrapping_sub(m).wrapping_add(r);
        return if d1 < d { d1 } else { i64::MIN };
    }
    if less_than_half(r) {
        return d - r;
    }
    let d1 = d.wrapping_add(m).wrapping_sub(r);
    if d1 > d { d1 } else { i64::MAX }
}

/// Whole seconds, minutes and hours as the floating-point figure truncated (Go's `int(d.Seconds())`), which for
/// very long spans can differ from integer division by one.
pub(crate) fn whole_seconds(d: i64) -> i64 {
    whole(d, SECOND)
}

pub(crate) fn whole_minutes(d: i64) -> i64 {
    whole(d, MINUTE)
}

pub(crate) fn whole_hours(d: i64) -> i64 {
    whole(d, HOUR)
}

/// Hours as a figure, for a count of days.
#[allow(clippy::cast_precision_loss)] // WHY: the figure is the float Go computes, precision loss and all
pub(crate) fn hours(d: i64) -> f64 {
    (d / HOUR) as f64 + (d % HOUR) as f64 / HOUR as f64
}

#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
// WHY: the float and the truncation toward zero are what Go's int(d.Unit()) does
fn whole(d: i64, unit: i64) -> i64 {
    ((d / unit) as f64 + (d % unit) as f64 / unit as f64) as i64
}

/// A span as Go writes one: `0s`, `1.5µs`, `250ms`, `4m30s`, `1h0m5s`.
pub(crate) fn duration_string(d: i64) -> String {
    let u = d.unsigned_abs();
    let sign = if d < 0 { "-" } else { "" };
    if u == 0 {
        return "0s".to_owned();
    }
    if u < 1_000_000_000 {
        let (unit, prec) = match u {
            0..1_000 => ("ns", 0),
            1_000..1_000_000 => ("µs", 3),
            _ => ("ms", 6),
        };
        let scale = 10_u64.pow(prec);
        return format!("{sign}{}{}{unit}", u / scale, fraction(u % scale, prec));
    }
    let frac = fraction(u % 1_000_000_000, 9);
    let secs = u / 1_000_000_000;
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    match (h, secs / 60) {
        (0, 0) => format!("{sign}{s}{frac}s"),
        (0, _) => format!("{sign}{m}m{s}{frac}s"),
        _ => format!("{sign}{h}h{m}m{s}{frac}s"),
    }
}

/// `.digits` of `v` as a fraction of `10^prec`, trailing zeros dropped; empty when it is zero.
fn fraction(v: u64, prec: u32) -> String {
    if v == 0 {
        return String::new();
    }
    let digits = format!("{v:0width$}", width = prec as usize);
    format!(".{}", digits.trim_end_matches('0'))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Go's own Duration.String examples, and the rounding at both saturating ends.
    #[test]
    fn duration_strings_and_rounding_match_go() {
        for (d, want) in [
            (0, "0s"),
            (1, "1ns"),
            (1_100, "1.1µs"),
            (2_200_000, "2.2ms"),
            (3_300_000_000, "3.3s"),
            (-(4 * MINUTE + 5 * SECOND), "-4m5s"),
            (4 * MINUTE + 5_001_000_000, "4m5.001s"),
            (5 * HOUR + 6 * MINUTE + 7_001_000_000, "5h6m7.001s"),
            (8 * MINUTE + 1, "8m0.000000001s"),
            (i64::MAX, "2562047h47m16.854775807s"),
            (i64::MIN, "-2562047h47m16.854775808s"),
        ] {
            assert_eq!(duration_string(d), want);
        }
        assert_eq!(round(1_500 * MILLISECOND, SECOND), 2 * SECOND);
        assert_eq!(round(-1_500 * MILLISECOND, SECOND), -2 * SECOND);
        assert_eq!(round(i64::MAX, SECOND), i64::MAX);
        assert_eq!(round(i64::MIN, SECOND), i64::MIN);
    }
}
