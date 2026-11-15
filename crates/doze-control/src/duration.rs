//! Go's duration text (`time.Duration.String` and `time.ParseDuration`): how a subscribe request names its stats
//! interval (`"1s"`, `"500ms"`, `"1m30s"`).

use std::time::Duration;

const UNITS: [(&str, u128); 8] = [
    ("ns", 1),
    ("us", 1_000),
    ("µs", 1_000),
    ("μs", 1_000),
    ("ms", 1_000_000),
    ("s", 1_000_000_000),
    ("m", 60_000_000_000),
    ("h", 3_600_000_000_000),
];

/// `v` in `unit`s, with the fraction Go shows and no trailing zeros.
fn with_fraction(v: u64, unit: u64) -> String {
    let whole = v / unit;
    let frac = v % unit;
    if frac == 0 {
        return whole.to_string();
    }
    let width = unit.ilog10() as usize;
    let digits = format!("{frac:0width$}");
    format!("{whole}.{}", digits.trim_end_matches('0'))
}

/// A duration written as Go's `Duration.String` writes it: `0s`, `500ms`, `1.5µs`, `1m30s`, `2h0m0s`.
pub fn format_go_duration(d: Duration) -> String {
    let u = u64::try_from(d.as_nanos()).unwrap_or(i64::MAX.unsigned_abs());
    if u == 0 {
        return "0s".to_owned();
    }
    if u < 1_000_000_000 {
        let (unit, suffix) = match u {
            0..1_000 => (1, "ns"),
            1_000..1_000_000 => (1_000, "µs"),
            _ => (1_000_000, "ms"),
        };
        return format!("{}{suffix}", with_fraction(u, unit));
    }
    let h = u / 3_600_000_000_000;
    let m = u % 3_600_000_000_000 / 60_000_000_000;
    let secs = format!("{}s", with_fraction(u % 60_000_000_000, 1_000_000_000));
    match (h, m) {
        (0, 0) => secs,
        (0, m) => format!("{m}m{secs}"),
        (h, m) => format!("{h}h{m}m{secs}"),
    }
}

/// Go's `time.ParseDuration` for durations that are not negative: a sequence of decimal numbers, each with an
/// optional fraction and a unit (`ns`, `us`/`µs`, `ms`, `s`, `m`, `h`). `None` for anything else, or a negative or
/// overflowing duration.
// fuzz:parser fuzz_text
pub fn parse_go_duration(s: &str) -> Option<Duration> {
    let mut rest = s.strip_prefix('+').unwrap_or(s);
    if rest == "0" {
        return Some(Duration::ZERO);
    }
    if rest.is_empty() {
        return None;
    }
    let mut total: u128 = 0;
    while !rest.is_empty() {
        let int_len = rest.bytes().take_while(u8::is_ascii_digit).count();
        let (int, after) = rest.split_at_checked(int_len)?;
        let (frac, after) = match after.strip_prefix('.') {
            Some(f) => f.split_at_checked(f.bytes().take_while(u8::is_ascii_digit).count())?,
            None => ("", after),
        };
        if int.is_empty() && frac.is_empty() {
            return None;
        }
        let unit_len = after
            .find(|c: char| c == '.' || c.is_ascii_digit())
            .unwrap_or(after.len());
        let (unit, next) = after.split_at_checked(unit_len)?;
        let scale = UNITS.iter().find(|(name, _)| *name == unit)?.1;
        total = total.checked_add(number(int, frac, scale)?)?;
        rest = next;
    }
    let nanos = u64::try_from(total)
        .ok()
        .filter(|n| i64::try_from(*n).is_ok())?;
    Some(Duration::from_nanos(nanos))
}

/// `int.frac` units of `scale` nanoseconds, truncated to a whole nanosecond.
fn number(int: &str, frac: &str, scale: u128) -> Option<u128> {
    let whole: u128 = if int.is_empty() { 0 } else { int.parse().ok()? };
    let mut v = whole.checked_mul(scale)?;
    // Past 30 digits a fraction no longer changes a whole nanosecond.
    let frac = frac.get(..frac.len().min(30))?;
    if !frac.is_empty() {
        let f: u128 = frac.parse().ok()?;
        v = v.checked_add(
            f.checked_mul(scale)? / 10u128.checked_pow(u32::try_from(frac.len()).ok()?)?,
        )?;
    }
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_as_go_does() {
        let cases = [
            (0, "0s"),
            (1, "1ns"),
            (1_500, "1.5µs"),
            (500_000_000, "500ms"),
            (1_000_000_000, "1s"),
            (1_500_000_000, "1.5s"),
            (90_000_000_000, "1m30s"),
            (3_600_000_000_000, "1h0m0s"),
            (3_723_000_000_000, "1h2m3s"),
        ];
        for (n, s) in cases {
            assert_eq!(format_go_duration(Duration::from_nanos(n)), s);
            assert_eq!(parse_go_duration(s), Some(Duration::from_nanos(n)), "{s}");
        }
    }

    #[test]
    fn reads_as_go_does() {
        assert_eq!(parse_go_duration("1.5h"), Some(Duration::from_secs(5_400)));
        assert_eq!(parse_go_duration(".5s"), Some(Duration::from_millis(500)));
        assert_eq!(parse_go_duration("1us"), Some(Duration::from_micros(1)));
        assert_eq!(parse_go_duration("+2m"), Some(Duration::from_secs(120)));
        for bad in [
            "",
            "1",
            "s",
            "1x",
            "-1s",
            ".s",
            "1..2s",
            "9999999999999h",
            "1s2",
        ] {
            assert_eq!(parse_go_duration(bad), None, "{bad}");
        }
    }
}
