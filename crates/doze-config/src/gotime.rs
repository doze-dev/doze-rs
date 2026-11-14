//! Go's `time.ParseDuration` and `Duration.String`, which doze.hcl's durations are written in and
//! its messages print.

use std::time::Duration;

const UNITS: [(&str, u64); 8] = [
    ("ns", 1),
    ("us", 1_000),
    ("\u{b5}s", 1_000),
    ("\u{3bc}s", 1_000),
    ("ms", 1_000_000),
    ("s", 1_000_000_000),
    ("m", 60_000_000_000),
    ("h", 3_600_000_000_000),
];

const LIMIT: u64 = 1 << 63;

/// Go's `time.ParseDuration`, in nanoseconds (negative allowed, as Go's).
pub fn parse(s: &str) -> Option<i64> {
    parse_go(s).ok()
}

/// Go's `time.ParseDuration` with Go's error text, step for step (its integer and fraction
/// readers, its float multiply for the fraction, and the order of its checks).
pub fn parse_go(orig: &str) -> Result<i64, String> {
    let q = doze_hcl::value::quote::go_quote;
    let invalid = || format!("time: invalid duration {}", q(orig));
    let (neg, mut s) = match orig.as_bytes().first() {
        Some(b'-' | b'+') => (orig.starts_with('-'), orig.get(1..).unwrap_or("")),
        _ => (false, orig),
    };
    if s == "0" {
        return Ok(0);
    }
    if s.is_empty() {
        return Err(invalid());
    }
    let mut d: u64 = 0;
    while !s.is_empty() {
        if !s.starts_with(|c: char| c == '.' || c.is_ascii_digit()) {
            return Err(invalid());
        }
        let (v, rest) = leading_int(s).ok_or_else(invalid)?;
        let pre = rest.len() != s.len();
        s = rest;
        let (mut f, mut scale, mut post) = (0u64, 1f64, false);
        if let Some(r) = s.strip_prefix('.') {
            let (x, sc, rest) = leading_fraction(r);
            (f, scale, post) = (x, sc, rest.len() != r.len());
            s = rest;
        }
        if !pre && !post {
            return Err(invalid());
        }
        let i = s
            .find(|c: char| c == '.' || c.is_ascii_digit())
            .unwrap_or(s.len());
        if i == 0 {
            return Err(format!("time: missing unit in duration {}", q(orig)));
        }
        let (u, rest) = s.split_at(i);
        s = rest;
        let Some(&(_, unit)) = UNITS.iter().find(|(name, _)| *name == u) else {
            return Err(format!(
                "time: unknown unit {} in duration {}",
                q(u),
                q(orig)
            ));
        };
        if v > LIMIT / unit {
            return Err(invalid());
        }
        let mut v = v * unit;
        if f > 0 {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss
            )]
            let add = (f as f64 * (unit as f64 / scale)) as u64;
            v = v.saturating_add(add);
            if v > LIMIT {
                return Err(invalid());
            }
        }
        d = d.saturating_add(v);
        if d > LIMIT {
            return Err(invalid());
        }
    }
    if neg {
        return Ok(0i64.wrapping_sub_unsigned(d));
    }
    i64::try_from(d).map_err(|_| invalid())
}

/// Go's `leadingInt`: the digits, `None` on overflow.
fn leading_int(s: &str) -> Option<(u64, &str)> {
    let n = s.bytes().take_while(u8::is_ascii_digit).count();
    let mut x: u64 = 0;
    for b in s.bytes().take(n) {
        if x > LIMIT / 10 {
            return None;
        }
        x = x * 10 + u64::from(b - b'0');
        if x > LIMIT {
            return None;
        }
    }
    Some((x, s.get(n..).unwrap_or("")))
}

/// Go's `leadingFraction`: the digits it can hold, the scale they are at, the rest.
fn leading_fraction(s: &str) -> (u64, f64, &str) {
    let n = s.bytes().take_while(u8::is_ascii_digit).count();
    let (mut x, mut scale, mut overflow) = (0u64, 1f64, false);
    for b in s.bytes().take(n) {
        if overflow {
            continue;
        }
        if x > (LIMIT - 1) / 10 {
            overflow = true;
            continue;
        }
        let y = x * 10 + u64::from(b - b'0');
        if y > LIMIT {
            overflow = true;
            continue;
        }
        x = y;
        scale *= 10.0;
    }
    (x, scale, s.get(n..).unwrap_or(""))
}

/// A duration from doze.hcl that must not be negative.
pub fn parse_non_negative(s: &str) -> Option<Duration> {
    let n = parse(s)?;
    u64::try_from(n).ok().map(Duration::from_nanos)
}

/// Go's `Duration.String()`: `1h2m3.5s`, `150ms`, `0s`.
pub fn format(d: Duration) -> String {
    let u = d.as_nanos();
    if u == 0 {
        return "0s".into();
    }
    if u < 1_000_000_000 {
        let (prec, unit) = match u {
            0..1_000 => (0, "ns"),
            1_000..1_000_000 => (3, "\u{b5}s"),
            _ => (6, "ms"),
        };
        return format!("{}{unit}", frac(u, prec));
    }
    let secs = u / 1_000_000_000;
    let mut out = format!("{}s", frac(u % 60_000_000_000, 9));
    let mins = secs / 60;
    if mins > 0 {
        out = format!("{}m{out}", mins % 60);
        let hours = mins / 60;
        if hours > 0 {
            out = format!("{hours}h{out}");
        }
    }
    out
}

/// `v / 10^prec`, with the fraction's trailing zeros (and a bare point) left out.
fn frac(v: u128, prec: u32) -> String {
    let p = 10u128.pow(prec);
    let (whole, f) = (v / p, v % p);
    if f == 0 {
        return whole.to_string();
    }
    let digits = format!("{f:0width$}", width = usize::try_from(prec).unwrap_or(0));
    format!("{whole}.{}", digits.trim_end_matches('0'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_as_go() {
        assert_eq!(parse("1h30m"), Some(5_400_000_000_000));
        assert_eq!(parse("1.5s"), Some(1_500_000_000));
        assert_eq!(parse("-2m"), Some(-120_000_000_000));
        assert_eq!(parse("0"), Some(0));
        assert_eq!(parse(".5ms"), Some(500_000));
        // Go's float multiply for a long fraction, and its overflow check before the unit.
        assert_eq!(parse("1.9999999999999999999ns"), Some(2));
        assert_eq!(parse("1.99999999999999999999s"), Some(2_000_000_000));
        assert_eq!(parse("0.333333333333333333333h"), Some(1_200_000_000_000));
        assert_eq!(
            parse_go("99999999999999999999x"),
            Err("time: invalid duration \"99999999999999999999x\"".into())
        );
        assert_eq!(
            parse_go("5x"),
            Err("time: unknown unit \"x\" in duration \"5x\"".into())
        );
        assert_eq!(
            parse_go("10"),
            Err("time: missing unit in duration \"10\"".into())
        );
        for bad in ["", "10", "5x", "s", ".s", "1h-2m", "never"] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn formats_as_go() {
        let f = |n: u64| format(Duration::from_nanos(n));
        assert_eq!(f(0), "0s");
        assert_eq!(f(1_500), "1.5\u{b5}s");
        assert_eq!(f(150_000_000), "150ms");
        assert_eq!(f(300_000_000_000), "5m0s");
        assert_eq!(f(3_723_500_000_000), "1h2m3.5s");
    }
}
