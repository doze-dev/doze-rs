//! Go's `time.Duration.String`, because the words the engine writes ("restarting (1/5) in 2s") and the durations the
//! control protocol carries are Go's, byte for byte.

use std::time::Duration;

/// Formats `d` as Go does: `0s`, `750ms`, `1.5µs`, `2s`, `1m30s`, `2h0m0s`.
pub fn go_duration(d: Duration) -> String {
    let ns = d.as_nanos();
    if ns == 0 {
        return "0s".into();
    }
    if ns < 1_000_000_000 {
        let (unit, per) = if ns < 1_000 {
            ("ns", 1)
        } else if ns < 1_000_000 {
            ("µs", 1_000)
        } else {
            ("ms", 1_000_000)
        };
        return format!("{}{unit}", fraction(ns, per));
    }
    let secs = ns / 1_000_000_000;
    let (h, m, s) = (secs / 3600, (secs / 60) % 60, secs % 60);
    let sub = ns % 1_000_000_000;
    let s_text = fraction(s * 1_000_000_000 + sub, 1_000_000_000);
    if h > 0 {
        format!("{h}h{m}m{s_text}s")
    } else if m > 0 {
        format!("{m}m{s_text}s")
    } else {
        format!("{s_text}s")
    }
}

/// `n / per` with the fraction's trailing zeros dropped (Go's `fmtFrac`).
fn fraction(n: u128, per: u128) -> String {
    let (whole, rest) = (n / per, n % per);
    if rest == 0 {
        return whole.to_string();
    }
    let width = per.to_string().len() - 1;
    let digits = format!("{rest:0width$}");
    format!("{whole}.{}", digits.trim_end_matches('0'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_go() {
        let cases = [
            (Duration::ZERO, "0s"),
            (Duration::from_nanos(1), "1ns"),
            (Duration::from_nanos(1_500), "1.5µs"),
            (Duration::from_millis(750), "750ms"),
            (Duration::from_millis(1), "1ms"),
            (Duration::from_secs(1), "1s"),
            (Duration::from_millis(1500), "1.5s"),
            (Duration::from_secs(30), "30s"),
            (Duration::from_secs(60), "1m0s"),
            (Duration::from_secs(90), "1m30s"),
            (Duration::from_secs(300), "5m0s"),
            (Duration::from_secs(7200), "2h0m0s"),
            (Duration::from_millis(3_723_500), "1h2m3.5s"),
        ];
        for (d, want) in cases {
            assert_eq!(go_duration(d), want, "{d:?}");
        }
    }
}
