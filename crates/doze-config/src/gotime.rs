//! Go's `time.ParseDuration` and `Duration.String`, which doze.hcl's durations are written in and
//! its messages print.

use std::time::Duration;

const UNITS: [(&str, u128); 9] = [
    ("ns", 1),
    ("us", 1_000),
    ("\u{b5}s", 1_000),
    ("\u{3bc}s", 1_000),
    ("ms", 1_000_000),
    ("s", 1_000_000_000),
    ("m", 60_000_000_000),
    ("h", 3_600_000_000_000),
    ("", 0),
];

/// Go's `time.ParseDuration`, in nanoseconds (negative allowed, as Go's).
pub fn parse(s: &str) -> Option<i64> {
    parse_go(s).ok()
}

/// Go's `time.ParseDuration` with Go's error text.
pub fn parse_go(orig: &str) -> Result<i64, String> {
    let q = doze_hcl::value::quote::go_quote;
    let invalid = || format!("time: invalid duration {}", q(orig));
    let (neg, mut rest) = match orig.as_bytes().first() {
        Some(b'-' | b'+') => (orig.starts_with('-'), orig.get(1..).unwrap_or("")),
        _ => (false, orig),
    };
    if rest == "0" {
        return Ok(0);
    }
    if rest.is_empty() {
        return Err(invalid());
    }
    let mut total: u128 = 0;
    while !rest.is_empty() {
        let (n, r) = match one(rest) {
            Step::Ok(n, r) => (n, r),
            Step::Invalid => return Err(invalid()),
            Step::NoUnit => return Err(format!("time: missing unit in duration {}", q(orig))),
            Step::Unknown(u) => {
                return Err(format!(
                    "time: unknown unit {} in duration {}",
                    q(u),
                    q(orig)
                ));
            }
        };
        total = total.checked_add(n).ok_or_else(invalid)?;
        rest = r;
    }
    let max = u128::from(i64::MAX.unsigned_abs()) + u128::from(neg);
    if total > max {
        return Err(invalid());
    }
    let total = i128::try_from(total).map_err(|_| invalid())?;
    i64::try_from(if neg { -total } else { total }).map_err(|_| invalid())
}

enum Step<'s> {
    Ok(u128, &'s str),
    Invalid,
    NoUnit,
    Unknown(&'s str),
}

/// One `<number>[.<fraction>]<unit>` step, in nanoseconds, and what follows it.
fn one(s: &str) -> Step<'_> {
    match one_step(s) {
        Ok(Some(x)) => Step::Ok(x.0, x.1),
        Ok(None) => Step::Invalid,
        Err("") => Step::NoUnit,
        Err(u) => Step::Unknown(u),
    }
}

/// `Err(unit)` for a unit that is missing ("") or unknown.
fn one_step(s: &str) -> Result<Option<(u128, &str)>, &str> {
    let int_len = s.bytes().take_while(u8::is_ascii_digit).count();
    let (int, rest) = s.split_at(int_len);
    let (frac, rest) = match rest.strip_prefix('.') {
        Some(r) => {
            let n = r.bytes().take_while(u8::is_ascii_digit).count();
            r.split_at(n)
        }
        None => ("", rest),
    };
    if int.is_empty() && frac.is_empty() {
        return Ok(None);
    }
    let unit_len = rest
        .char_indices()
        .find(|(_, c)| *c == '.' || c.is_ascii_digit())
        .map_or(rest.len(), |(i, _)| i);
    let (unit, rest) = rest.split_at(unit_len);
    if unit.is_empty() {
        return Err("");
    }
    let Some(&(_, scale)) = UNITS.iter().find(|(u, _)| !u.is_empty() && *u == unit) else {
        return Err(unit);
    };
    let Ok(whole) = (if int.is_empty() {
        Ok(0u128)
    } else {
        int.parse::<u128>()
    }) else {
        return Ok(None);
    };
    let Some(mut n) = whole.checked_mul(scale) else {
        return Ok(None);
    };
    if !frac.is_empty() {
        let mut f = 0u128;
        let mut div = 1u128;
        for d in frac.bytes().take(30) {
            f = f * 10 + u128::from(d - b'0');
            div *= 10;
        }
        let Some(m) = n.checked_add(f.saturating_mul(scale) / div) else {
            return Ok(None);
        };
        n = m;
    }
    Ok(Some((n, rest)))
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
