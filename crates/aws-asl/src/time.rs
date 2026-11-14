//! ISO-8601 timestamps as the States Language uses them (`Timestamp` waits, `Timestamp...` comparisons, the context object).

/// Milliseconds since the epoch of `YYYY-MM-DDTHH:MM:SS[.fraction](Z|±HH:MM)`.
#[must_use]
pub fn parse(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 20
        || b.get(4) != Some(&b'-')
        || b.get(7) != Some(&b'-')
        || !matches!(b.get(10), Some(b'T' | b't'))
    {
        return None;
    }
    if b.get(13) != Some(&b':') || b.get(16) != Some(&b':') {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<i64> {
        let t = s.get(r)?;
        if t.bytes().all(|c| c.is_ascii_digit()) {
            t.parse().ok()
        } else {
            None
        }
    };
    let (y, mo, d, h, mi, se) = (
        num(0..4)?,
        num(5..7)?,
        num(8..10)?,
        num(11..13)?,
        num(14..16)?,
        num(17..19)?,
    );
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    let mut rest = s.get(19..)?;
    let mut ms = 0;
    if let Some(frac) = rest.strip_prefix('.') {
        let digits: String = frac.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            return None;
        }
        let padded = format!("{digits:0<3}");
        ms = padded.get(..3)?.parse::<i64>().ok()?;
        rest = frac.get(digits.len()..)?;
    }
    let offset = zone(rest)?;
    let days = aws_core::time::days_from_civil(y, mo, d);
    Some((((days * 24 + h) * 60 + mi) * 60 + se - offset) * 1000 + ms)
}

/// The offset of `Z` or `±HH:MM`, in seconds.
fn zone(z: &str) -> Option<i64> {
    if z == "Z" || z == "z" {
        return Some(0);
    }
    let sign = match z.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let (h, m) = z.get(1..)?.split_once(':')?;
    if h.len() != 2 || m.len() != 2 {
        return None;
    }
    Some(sign * (h.parse::<i64>().ok()? * 3600 + m.parse::<i64>().ok()? * 60))
}

/// `2026-11-13T10:00:00.123Z`, as the context object writes times.
#[must_use]
pub fn format(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let millis = ms.rem_euclid(1000);
    let (y, mo, d) = aws_core::time::civil_from_days(secs.div_euclid(86_400));
    let day = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{mo:02}-{d:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        day / 3600,
        day % 3600 / 60,
        day % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        assert_eq!(parse("1970-01-01T00:00:01Z"), Some(1000));
        assert_eq!(parse("1970-01-01T01:00:00+01:00"), Some(0));
        assert_eq!(parse("1970-01-01T00:00:00.5Z"), Some(500));
        assert_eq!(
            parse("2016-03-14T01:59:00Z").map(format).as_deref(),
            Some("2016-03-14T01:59:00.000Z")
        );
        assert!(parse("2016-03-14").is_none());
        assert!(parse("2016-03-14T01:59:00").is_none());
    }
}
