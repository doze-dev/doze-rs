//! Times, ages and bytes as the topic pages write them: how long ago, a clock time with milliseconds, a time typed into a
//! filter, a hex dump. Pure.

const SEC: i64 = 1000;
const MIN: i64 = 60 * SEC;
const HOUR: i64 = 60 * MIN;
const DAY: i64 = 24 * HOUR;

/// How long ago, from a difference in milliseconds: `0.2 s ago`, `4 s ago`, `1 m ago`, `6 h ago`, `3 d ago`. A time ahead of
/// the clock (the two clocks differ a little) reads as `0 s ago`.
#[must_use]
pub fn ago(diff_ms: i64) -> String {
    let d = diff_ms.max(0);
    if d < SEC {
        format!("{}.{} s ago", d / SEC, d % SEC / 100)
    } else if d < MIN {
        format!("{} s ago", d / SEC)
    } else if d < HOUR {
        format!("{} m ago", d / MIN)
    } else if d < DAY {
        format!("{} h ago", d / HOUR)
    } else {
        format!("{} d ago", d / DAY)
    }
}

/// Days since the epoch of a civil date (Howard Hinnant's algorithm, public domain), or `None` for a date that is not one.
fn days_from_civil(year: i64, month: i64, day: i64) -> Option<i64> {
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let longest = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => 28 + i64::from(leap),
        _ => return None,
    };
    if !(1..=longest).contains(&day) {
        return None;
    }
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

fn number(s: &str, width: std::ops::RangeInclusive<usize>) -> Option<i64> {
    (width.contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit()))
        .then(|| s.parse().ok())
        .flatten()
}

/// A time typed into a filter, in milliseconds since the epoch: a whole number of milliseconds, or a UTC civil time
/// `YYYY-MM-DD`, `YYYY-MM-DD HH:MM` or `YYYY-MM-DD HH:MM:SS` (`T` for the space, a trailing `Z` allowed). Years 1970 to 9999.
#[must_use]
pub fn parse_time(text: &str) -> Option<i64> {
    let t = text.trim();
    if let Some(ms) = number(t, 1..=15) {
        return Some(ms);
    }
    let t = t.strip_suffix('Z').unwrap_or(t);
    let (date, time) = match t.split_once([' ', 'T']) {
        Some((d, rest)) => (d, Some(rest.trim())),
        None => (t, None),
    };
    let mut d = date.split('-');
    let (y, mo, da) = (
        number(d.next()?, 4..=4)?,
        number(d.next()?, 2..=2)?,
        number(d.next()?, 2..=2)?,
    );
    if d.next().is_some() || y < 1970 {
        return None;
    }
    let (mut h, mut mi, mut s) = (0, 0, 0);
    if let Some(time) = time {
        let mut p = time.split(':');
        h = number(p.next()?, 2..=2)?;
        mi = number(p.next()?, 2..=2)?;
        if let Some(sec) = p.next() {
            s = number(sec, 2..=2)?;
        }
        if p.next().is_some() || h > 23 || mi > 59 || s > 59 {
            return None;
        }
    }
    let days = days_from_civil(y, mo, da)?;
    Some(((days * 24 + h) * 60 + mi) * 60 * SEC + s * SEC)
}

/// The hex of the first `n` bytes, `0a 09 6f`, and whether there were more.
#[must_use]
pub fn hex_prefix(bytes: &[u8], n: usize) -> (String, bool) {
    let shown: Vec<String> = bytes.iter().take(n).map(|b| format!("{b:02x}")).collect();
    (shown.join(" "), bytes.len() > n)
}

/// A hex dump, eight bytes to a line: the offset, the bytes in hex, and the bytes as printable ASCII with a dot for the
/// rest.
#[must_use]
pub fn hex_dump(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 5);
    for (i, line) in bytes.chunks(8).enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let hex: Vec<String> = line.iter().map(|b| format!("{b:02x}")).collect();
        let ascii: String = line
            .iter()
            .map(|b| {
                if (0x20..0x7f).contains(b) {
                    char::from(*b)
                } else {
                    '.'
                }
            })
            .collect();
        out.push_str(&format!("{:04x}  {:<23}  {ascii}", i * 8, hex.join(" ")));
    }
    out
}

#[cfg(test)]
mod tests;
