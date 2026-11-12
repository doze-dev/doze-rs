//! Numbers, sizes, ages and clock times as the pages write them. Pure functions: the same input, the same words.
//!
//! Times are UTC and say so: the process has no portable, safe way to ask for the local zone, and a console that
//! guessed it would be wrong twice a year.

const MIN: i64 = 60;
const HOUR: i64 = 3600;
const DAY: i64 = 86_400;

/// How long, in the two largest units that matter: `41 s`, `2 min`, `3 h 5 min`, `3 d 4 h`. Negative is zero.
#[must_use]
pub fn span(secs: i64) -> String {
    let s = secs.max(0);
    let pair = |big: i64, big_unit: &str, rest: i64, small_unit: &str| {
        if rest == 0 {
            format!("{big} {big_unit}")
        } else {
            format!("{big} {big_unit} {rest} {small_unit}")
        }
    };
    if s < MIN {
        format!("{s} s")
    } else if s < HOUR {
        format!("{} min", s / MIN)
    } else if s < DAY {
        pair(s / HOUR, "h", s % HOUR / MIN, "min")
    } else {
        pair(s / DAY, "d", s % DAY / HOUR, "h")
    }
}

/// [`span`] of milliseconds.
#[must_use]
pub fn span_ms(ms: i64) -> String {
    span(ms / 1000)
}

/// A whole number with thousands separators: `16,104`.
#[must_use]
pub fn number(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if n < 0 {
        out.push('-');
    }
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A size in bytes, in binary units, one decimal above a kilobyte: `512 B`, `4.5 KiB`, `4.7 MiB`.
#[must_use]
pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 4] = ["KiB", "MiB", "GiB", "TiB"];
    if n < 1024 {
        return format!("{n} B");
    }
    // The largest unit that leaves at least one of it; tenths are rounded down, so no size reads `1024.0` of the
    // unit below.
    let mut unit = 0;
    while unit + 1 < UNITS.len() && n >> (10 * (unit + 2)) > 0 {
        unit += 1;
    }
    let shift = 10 * (unit + 1);
    let tenths = ((n & ((1 << shift) - 1)) * 10) >> shift;
    let name = UNITS.get(unit).copied().unwrap_or("TiB");
    format!("{}.{tenths} {name}", n >> shift)
}

/// Milliseconds as the rules of the probe write them: `2 ms`, `1.4 s`.
#[must_use]
pub fn millis(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else {
        format!("{}.{} s", ms / 1000, ms % 1000 / 100)
    }
}

/// `1 topic`, `3 topics`.
#[must_use]
pub fn count(n: usize, one: &str, many: &str) -> String {
    format!(
        "{} {}",
        number(i64::try_from(n).unwrap_or(i64::MAX)),
        if n == 1 { one } else { many }
    )
}

/// A civil date and time (UTC) from milliseconds since the epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Civil {
    pub year: i64,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

/// The days-to-civil algorithm of Howard Hinnant (public domain), for any `i64` of milliseconds that fits a year.
#[must_use]
pub fn civil(ms: i64) -> Civil {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(DAY);
    let of_day = secs.rem_euclid(DAY);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let narrow = |n: i64| u8::try_from(n).unwrap_or(0);
    Civil {
        year,
        month: narrow(month),
        day: narrow(day),
        hour: narrow(of_day / HOUR),
        minute: narrow(of_day % HOUR / MIN),
        second: narrow(of_day % MIN),
    }
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// `Nov 6 18:02 UTC`.
#[must_use]
pub fn date_time(ms: i64) -> String {
    let c = civil(ms);
    let month = MONTHS
        .get(usize::from(c.month).saturating_sub(1))
        .copied()
        .unwrap_or("?");
    format!("{month} {} {:02}:{:02} UTC", c.day, c.hour, c.minute)
}

/// `22:13:33`, UTC.
#[must_use]
pub fn clock(ms: i64) -> String {
    let c = civil(ms);
    format!("{:02}:{:02}:{:02}", c.hour, c.minute, c.second)
}

#[cfg(test)]
mod tests;
