//! `dateTimeConverter`'s patterns: the Java `DateTimeFormatter` letters AWS's documentation says the patterns are written in
//! (memory: the user guide's page, not the model). The subset read and written here: `y` `M` (number, `MMM`, `MMMM`) `d` `H` `h` `m`
//! `s` `S` (fraction) `a` `E` `Z` `X` (one to three), quoted literals and any other character as itself. English names (the model's
//! default locale is `locale.ROOT`).

use aws_cron::{Local, Zone};

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const DAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];
/// The longest pattern read.
const MAX_PATTERN: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Part {
    Lit(String),
    /// A letter and how many times it is written.
    Field(char, usize),
}

/// A parsed pattern.
#[derive(Clone, Debug)]
pub(crate) struct Pattern(Vec<Part>);

/// Reads a pattern; `None` for one with a letter this does not know or an unclosed quote.
// fuzz:parser fuzz_transformer
pub(crate) fn pattern(text: &str) -> Option<Pattern> {
    if text.len() > MAX_PATTERN {
        return None;
    }
    let mut parts = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\'' {
            let mut lit = String::new();
            loop {
                match chars.next()? {
                    '\'' if chars.peek() == Some(&'\'') => {
                        chars.next();
                        lit.push('\'');
                    }
                    '\'' => break,
                    other => lit.push(other),
                }
            }
            if lit.is_empty() {
                lit.push('\'');
            }
            parts.push(Part::Lit(lit));
        } else if c.is_ascii_alphabetic() {
            if !"yMdHhmsSaEZX".contains(c) {
                return None;
            }
            let mut n = 1;
            while chars.peek() == Some(&c) {
                chars.next();
                n += 1;
            }
            if (c == 'X' && n > 3) || n > 9 {
                return None;
            }
            parts.push(Part::Field(c, n));
        } else {
            parts.push(Part::Lit(c.to_string()));
        }
    }
    Some(Pattern(parts))
}

/// What a reading holds.
#[derive(Debug, Default)]
struct Read {
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
    ms: i64,
    pm: Option<bool>,
    offset_s: Option<i64>,
}

/// The instant (ms since the epoch) `text` stands for under `p`, its wall clock read in `zone` when it carries no offset.
pub(crate) fn parse(p: &Pattern, text: &str, zone: &Zone) -> Option<i64> {
    let mut r = Read {
        year: 1970,
        month: 1,
        day: 1,
        ..Read::default()
    };
    let mut rest = text;
    for part in &p.0 {
        rest = match part {
            Part::Lit(l) => rest.strip_prefix(l.as_str())?,
            Part::Field(c, n) => field(*c, *n, rest, &mut r)?,
        };
    }
    if !rest.is_empty() || !(1..=12).contains(&r.month) || !(1..=31).contains(&r.day) {
        return None;
    }
    if let Some(pm) = r.pm {
        r.hour = r.hour % 12 + if pm { 12 } else { 0 };
    }
    if r.hour > 23 || r.minute > 59 || r.second > 60 {
        return None;
    }
    let days = aws_core::time::days_from_civil(r.year, r.month, r.day);
    let wall = ((days * 24 + r.hour) * 60 + r.minute) * 60_000 + r.second * 1000 + r.ms;
    Some(match r.offset_s {
        Some(off) => wall - off * 1000,
        None => match zone.resolve(wall) {
            Local::Unique(t) | Local::Gap(t) | Local::Fold(t, _) => t,
        },
    })
}

/// Reads one field off the front of `s`.
fn field<'s>(c: char, n: usize, s: &'s str, r: &mut Read) -> Option<&'s str> {
    let (value, rest) = match c {
        'M' if n >= 3 => name(s, &MONTHS, n == 3)?,
        'E' => {
            let (_, rest) = name(s, &DAYS, n <= 3)?;
            return Some(rest);
        }
        'a' => {
            let upper = s.get(..2)?.to_ascii_uppercase();
            r.pm = Some(match upper.as_str() {
                "AM" => false,
                "PM" => true,
                _ => return None,
            });
            return s.get(2..);
        }
        'Z' | 'X' => return offset(s, r),
        'S' => {
            let (digits, rest) = digits(s, n, n)?;
            let scaled = format!("{digits:0<3}");
            r.ms = scaled.get(..3)?.parse().ok()?;
            return Some(rest);
        }
        'y' if n == 2 => {
            let (d, rest) = digits(s, 2, 2)?;
            (2000 + d.parse::<i64>().ok()?, rest)
        }
        'y' => {
            let (d, rest) = digits(s, n.max(1), 4)?;
            (d.parse().ok()?, rest)
        }
        _ => {
            let (d, rest) = digits(s, n, n.max(2))?;
            (d.parse().ok()?, rest)
        }
    };
    match c {
        'y' => r.year = value,
        'M' => r.month = value,
        'd' => r.day = value,
        'H' | 'h' => r.hour = value,
        'm' => r.minute = value,
        's' => r.second = value,
        _ => {}
    }
    Some(rest)
}

/// At least `least` and at most `most` ASCII digits.
fn digits(s: &str, least: usize, most: usize) -> Option<(&str, &str)> {
    let len = s.bytes().take(most).take_while(u8::is_ascii_digit).count();
    (len >= least.max(1)).then(|| {
        (
            s.get(..len).unwrap_or_default(),
            s.get(len..).unwrap_or_default(),
        )
    })
}

/// A month or day name (the full name, or its first three letters when `short`), any case: its 1-based number.
fn name<'s>(s: &'s str, names: &[&str], short: bool) -> Option<(i64, &'s str)> {
    for (i, full) in names.iter().enumerate() {
        let want = if short { full.get(..3)? } else { full };
        if s.get(..want.len())
            .is_some_and(|h| h.eq_ignore_ascii_case(want))
        {
            return Some((i64::try_from(i).ok()? + 1, s.get(want.len()..)?));
        }
    }
    None
}

/// `Z`, `+HH`, `+HHMM` or `+HH:MM`.
fn offset<'s>(s: &'s str, r: &mut Read) -> Option<&'s str> {
    if let Some(rest) = s.strip_prefix('Z') {
        r.offset_s = Some(0);
        return Some(rest);
    }
    let sign = match s.chars().next()? {
        '+' => 1,
        '-' => -1,
        _ => return None,
    };
    let (h, rest) = digits(s.get(1..)?, 2, 2)?;
    let rest = rest.strip_prefix(':').unwrap_or(rest);
    let (m, rest) = digits(rest, 2, 2).unwrap_or(("00", rest));
    r.offset_s = Some(sign * (h.parse::<i64>().ok()? * 3600 + m.parse::<i64>().ok()? * 60));
    Some(rest)
}

/// `ms` written under `p`, on the wall clock of `zone`.
pub(crate) fn format(p: &Pattern, ms: i64, zone: &Zone) -> String {
    let off = i64::from(zone.offset_at(ms));
    let wall = ms + off * 1000;
    let days = wall.div_euclid(86_400_000);
    let rem = wall.rem_euclid(86_400_000);
    let (y, mo, d) = aws_core::time::civil_from_days(days);
    let (h, mi, s, milli) = (
        rem / 3_600_000,
        rem / 60_000 % 60,
        rem / 1000 % 60,
        rem % 1000,
    );
    let mut out = String::new();
    for part in &p.0 {
        match part {
            Part::Lit(l) => out.push_str(l),
            Part::Field(c, n) => {
                let n = *n;
                let pad = |v: i64| format!("{v:0n$}");
                let text = match c {
                    'y' if n == 2 => format!("{:02}", y.rem_euclid(100)),
                    'y' => pad(y),
                    'M' if n >= 3 => month(mo, n == 3),
                    'M' => pad(mo),
                    'd' => pad(d),
                    'H' => pad(h),
                    'h' => pad(if h % 12 == 0 { 12 } else { h % 12 }),
                    'm' => pad(mi),
                    's' => pad(s),
                    'S' => fraction(milli, n),
                    'a' => (if h < 12 { "AM" } else { "PM" }).to_owned(),
                    'E' => weekday(days, n <= 3),
                    _ => zone_text(*c, n, off),
                };
                out.push_str(&text);
            }
        }
    }
    out
}

fn month(m: i64, short: bool) -> String {
    let full = usize::try_from(m - 1)
        .ok()
        .and_then(|i| MONTHS.get(i))
        .copied()
        .unwrap_or("");
    if short {
        full.get(..3).unwrap_or(full).to_owned()
    } else {
        full.to_owned()
    }
}

fn weekday(days: i64, short: bool) -> String {
    // 1970-01-01 was a Thursday (index 3 from Monday).
    let full = usize::try_from((days + 3).rem_euclid(7))
        .ok()
        .and_then(|i| DAYS.get(i))
        .copied()
        .unwrap_or("");
    if short {
        full.get(..3).unwrap_or(full).to_owned()
    } else {
        full.to_owned()
    }
}

fn fraction(milli: i64, n: usize) -> String {
    let three = format!("{milli:03}");
    if n <= 3 {
        three.get(..n).unwrap_or_default().to_owned()
    } else {
        format!("{three:0<n$}")
    }
}

fn zone_text(c: char, n: usize, off: i64) -> String {
    if c == 'X' && off == 0 {
        return "Z".into();
    }
    let sign = if off < 0 { '-' } else { '+' };
    let (h, m) = (off.abs() / 3600, off.abs() / 60 % 60);
    match (c, n) {
        ('X', 1) => format!("{sign}{h:02}"),
        ('X', 3) => format!("{sign}{h:02}:{m:02}"),
        _ => format!("{sign}{h:02}{m:02}"),
    }
}
