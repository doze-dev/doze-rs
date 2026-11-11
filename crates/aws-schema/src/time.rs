//! Is this text a timestamp the protocols accept: epoch seconds (with a fraction), an RFC 3339 date-time, or an HTTP date?
//! Shape and calendar ranges only; the value itself is parsed by the service that needs it.

#[must_use]
pub fn valid(s: &str) -> bool {
    epoch(s) || rfc3339(s) || http_date(s)
}

fn epoch(s: &str) -> bool {
    let body = s.strip_prefix('-').unwrap_or(s);
    let (int, frac) = body.split_once('.').map_or((body, ""), |(a, b)| (a, b));
    !int.is_empty()
        && int.bytes().all(|c| c.is_ascii_digit())
        && frac.bytes().all(|c| c.is_ascii_digit())
}

fn num(s: &str, lo: u32, hi: u32) -> Option<u32> {
    if s.is_empty() || !s.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    s.parse().ok().filter(|n| (lo..=hi).contains(n))
}

fn rfc3339(s: &str) -> bool {
    let Some((date, rest)) = s.split_once(['T', 't']) else {
        return false;
    };
    let mut d = date.split('-');
    let (Some(y), Some(m), Some(day), None) = (d.next(), d.next(), d.next(), d.next()) else {
        return false;
    };
    if y.len() != 4
        || m.len() != 2
        || day.len() != 2
        || num(y, 0, 9999).is_none()
        || num(m, 1, 12).is_none()
        || num(day, 1, 31).is_none()
    {
        return false;
    }
    let (time, zone) = match rest.find(['Z', 'z', '+', '-']) {
        Some(i) => rest.split_at(i),
        None => return false,
    };
    let (hms, frac) = time.split_once('.').map_or((time, ""), |(a, b)| (a, b));
    let mut t = hms.split(':');
    let (Some(h), Some(mi), Some(sec), None) = (t.next(), t.next(), t.next(), t.next()) else {
        return false;
    };
    let zone_ok = matches!(zone, "Z" | "z")
        || (zone.len() == 6
            && zone.get(3..4) == Some(":")
            && num(zone.get(1..3).unwrap_or(""), 0, 23).is_some()
            && num(zone.get(4..).unwrap_or(""), 0, 59).is_some());
    h.len() == 2
        && mi.len() == 2
        && sec.len() == 2
        && num(h, 0, 23).is_some()
        && num(mi, 0, 59).is_some()
        && num(sec, 0, 60).is_some()
        && frac.bytes().all(|c| c.is_ascii_digit())
        && zone_ok
}

fn http_date(s: &str) -> bool {
    const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mut p = s.split(' ');
    let (Some(day), Some(dd), Some(mon), Some(year), Some(time), Some(gmt), None) = (
        p.next(),
        p.next(),
        p.next(),
        p.next(),
        p.next(),
        p.next(),
        p.next(),
    ) else {
        return false;
    };
    let mut t = time.split(':');
    let (Some(h), Some(m), Some(sec), None) = (t.next(), t.next(), t.next(), t.next()) else {
        return false;
    };
    DAYS.iter().any(|d| day.strip_suffix(',') == Some(d))
        && num(dd, 1, 31).is_some()
        && MONTHS.contains(&mon)
        && year.len() == 4
        && num(year, 0, 9999).is_some()
        && num(h, 0, 23).is_some()
        && num(m, 0, 59).is_some()
        && num(sec, 0, 60).is_some()
        && gmt == "GMT"
}
