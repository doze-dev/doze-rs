//! The values a query works with, how they compare, and how they are written in a result (every result value is text).

use std::cmp::Ordering;

/// A value: a field of an event, a literal, or what an expression makes.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Val {
    Null,
    Str(String),
    Num(f64),
    Bool(bool),
    /// A point in time, in milliseconds since the epoch (`@timestamp`, `bin()`).
    Time(i64),
}

impl Val {
    pub(crate) const fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// The value as a number, if it is one or is text that reads as one.
    pub(crate) fn num(&self) -> Option<f64> {
        match self {
            Self::Num(n) => Some(*n),
            #[allow(clippy::cast_precision_loss)]
            Self::Time(t) => Some(*t as f64),
            Self::Str(s) => s.trim().parse::<f64>().ok().filter(|n| n.is_finite()),
            Self::Bool(_) | Self::Null => None,
        }
    }

    /// The value as a time: a time, or a number of milliseconds (held within some hundred thousand years of the epoch, so the
    /// calendar arithmetic cannot overflow).
    pub(crate) fn time(&self) -> Option<i64> {
        const BOUND: i64 = 10_000_000_000_000_000;
        let t = match self {
            Self::Time(t) => Some(*t),
            #[allow(clippy::cast_possible_truncation)]
            Self::Num(n) if n.is_finite() => Some(*n as i64),
            Self::Str(s) => parse_time(s),
            _ => None,
        };
        t.map(|t| t.clamp(-BOUND, BOUND))
    }

    pub(crate) fn truthy(&self) -> bool {
        match self {
            Self::Bool(b) => *b,
            Self::Null => false,
            Self::Num(n) => *n != 0.0,
            Self::Str(s) => !s.is_empty() && s != "false",
            Self::Time(_) => true,
        }
    }

    /// The value as a result shows it.
    pub(crate) fn text(&self) -> String {
        match self {
            Self::Null => String::new(),
            Self::Str(s) => s.clone(),
            Self::Num(n) => number(*n),
            Self::Bool(b) => b.to_string(),
            Self::Time(t) => time_text(*t),
        }
    }
}

/// A number as Insights writes it: whole numbers without a point. (memory)
pub(crate) fn number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        #[allow(clippy::cast_possible_truncation)]
        return format!("{}", n as i64);
    }
    format!("{n}")
}

/// `2026-11-18 09:30:00.000`, the way Insights shows `@timestamp`. (memory)
pub(crate) fn time_text(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    let rem = ms.rem_euclid(86_400_000);
    let (y, mo, d) = aws_core::time::civil_from_days(days);
    format!(
        "{y:04}-{mo:02}-{d:02} {:02}:{:02}:{:02}.{:03}",
        rem / 3_600_000,
        rem / 60_000 % 60,
        rem / 1000 % 60,
        rem % 1000
    )
}

/// A time written as Insights writes one, or as RFC 3339; a number of milliseconds is read as such.
fn parse_time(s: &str) -> Option<i64> {
    if let Ok(n) = s.parse::<i64>() {
        return Some(n);
    }
    let (secs, frac) = match s.split_once('.') {
        Some((a, b)) => (format!("{a}Z"), b.trim_end_matches('Z')),
        None => (format!("{}Z", s.trim_end_matches('Z')), ""),
    };
    let whole = aws_core::time::parse_rfc3339(&secs)?;
    let ms = frac
        .get(..frac.len().min(3))
        .and_then(|f| format!("{f:0<3}").parse::<i64>().ok())
        .unwrap_or(0);
    Some(whole * 1000 + ms)
}

/// How two values order: as numbers when both are, as times when either is one, otherwise as text. Null sorts first.
pub(crate) fn compare(a: &Val, b: &Val) -> Ordering {
    match (a, b) {
        (Val::Null, Val::Null) => Ordering::Equal,
        (Val::Null, _) => Ordering::Less,
        (_, Val::Null) => Ordering::Greater,
        (Val::Time(_), _) | (_, Val::Time(_)) => match (a.time(), b.time()) {
            (Some(x), Some(y)) => x.cmp(&y),
            _ => a.text().cmp(&b.text()),
        },
        _ => match (a.num(), b.num()) {
            (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
            _ => a.text().cmp(&b.text()),
        },
    }
}

/// Whether two values are equal for `=`: as numbers when both read as numbers, otherwise as text.
pub(crate) fn equal(a: &Val, b: &Val) -> bool {
    if a.is_null() || b.is_null() {
        return false;
    }
    compare(a, b) == Ordering::Equal
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_written_and_compared() {
        assert_eq!(number(5.0), "5");
        assert_eq!(number(2.5), "2.5");
        assert_eq!(time_text(1_700_000_000_123), "2023-11-14 22:13:20.123");
        assert_eq!(
            Val::Str("2023-11-14 22:13:20.123".into()).time(),
            Some(1_700_000_000_123)
        );
        assert!(equal(&Val::Str("200".into()), &Val::Num(200.0)));
        assert_eq!(
            compare(&Val::Str("10".into()), &Val::Str("9".into())),
            Ordering::Greater
        );
        assert!(!equal(&Val::Null, &Val::Null));
    }
}
