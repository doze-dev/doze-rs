//! How figures and times are written on every surface: a size, a countdown, an age, a count and a path. They are
//! here, once, so a size in the CLI and the same size in the app read the same.

use std::borrow::Cow;
use std::time::Duration;

use crate::gonum::fixed;
use crate::gotime::{self, HOUR, MINUTE, SECOND};

const MIB: i64 = 1 << 20;
const GIB: i64 = 1 << 30;

/// A size as the board writes it: `84 MB`, `1.2 GB`. Nothing is written as a fraction of a megabyte; under half of
/// one is `0 MB`, and nothing at all is `0`. (Go: `MB`.)
#[must_use]
pub fn mb(bytes: i64) -> String {
    if bytes <= 0 {
        "0".to_owned()
    } else if bytes >= GIB {
        #[allow(clippy::cast_precision_loss)] // WHY: a tenth of a gigabyte is all that is written
        let gb = bytes as f64 / GIB as f64;
        format!("{gb:.1} GB")
    } else {
        format!("{} MB", (bytes + MIB / 2) >> 20)
    }
}

/// A countdown as `m:ss`, the way the board counts down to sleep. (Go: `Clock`.)
#[must_use]
pub fn clock(d: Duration) -> String {
    clock_nanos(gotime::nanos(d))
}

pub(crate) fn clock_nanos(d: i64) -> String {
    let d = gotime::round(d, SECOND).max(0);
    format!(
        "{}:{:02}",
        gotime::whole_minutes(d),
        gotime::whole_seconds(d) % 60
    )
}

/// How long a span is, in the coarsest unit that still says something: `12s`, `4m`, `3h`, `2d`. (Go: `Ago`.)
#[must_use]
pub fn ago(d: Duration) -> String {
    ago_nanos(gotime::nanos(d))
}

/// [`ago`] of a span that may be negative, when the clocks disagree: `-3s`.
pub(crate) fn ago_nanos(d: i64) -> String {
    if d < MINUTE {
        format!("{}s", gotime::whole_seconds(d))
    } else if d < HOUR {
        format!("{}m", gotime::whole_minutes(d))
    } else if d < 48 * HOUR {
        format!("{}h", gotime::whole_hours(d))
    } else {
        #[allow(clippy::cast_possible_truncation)]
        // WHY: Go's int(d.Hours()/24), truncation and all
        let days = (gotime::hours(d) / 24.0) as i64;
        format!("{days}d")
    }
}

/// The home directory as the Go surfaces find it: `$HOME` (`%USERPROFILE%` on Windows), when it is set and not empty.
#[must_use]
pub fn home() -> Option<String> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var(var).ok().filter(|h| !h.is_empty())
}

/// A path under `home` written with a `~`, as a shell would. (Go: `Tilde`, given [`home`].)
#[must_use]
pub fn tilde<'a>(dir: &'a str, home: Option<&str>) -> Cow<'a, str> {
    match home
        .filter(|h| !h.is_empty())
        .and_then(|h| dir.strip_prefix(h))
    {
        Some(rest) => Cow::Owned(format!("~{rest}")),
        None => Cow::Borrowed(dir),
    }
}

/// The `s` that makes a count read: "1 connection", "2 connections". (Go: `Plural`.)
#[must_use]
pub fn plural(n: u64) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// An integer with commas: `1,043`, `-12,000`. (Go: `Thousands`.)
#[must_use]
pub fn thousands(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if n < 0 {
        out.push('-');
    }
    let head = digits.len() % 3;
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (i + 3 - head).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A scale's label for a percentage. (Go: `Pct`.)
#[must_use]
pub fn pct(v: f64) -> String {
    format!("{}%", fixed(v, 0))
}

/// A scale's label for a count. (Go: `Count`.)
#[must_use]
pub fn count(v: f64) -> String {
    fixed(v, 0)
}

/// A scale's label for a size. (Go: `BytesLabel`.)
#[must_use]
pub fn bytes_label(v: f64) -> String {
    #[allow(clippy::cast_possible_truncation)] // WHY: Go's int64(v), saturating
    mb(v as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_groups_from_the_right() {
        for (n, want) in [
            (0, "0"),
            (999, "999"),
            (1000, "1,000"),
            (-1_234_567, "-1,234,567"),
            (i64::MIN, "-9,223,372,036,854,775,808"),
        ] {
            assert_eq!(thousands(n), want);
        }
    }
}
