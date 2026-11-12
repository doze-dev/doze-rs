//! Reading the dates a request carries.

/// An HTTP date (`Sun, 06 Nov 1994 08:49:37 GMT`), an RFC 3339 time or epoch seconds, as Unix seconds.
pub(crate) fn parse_http_date(s: &str) -> Option<i64> {
    let s = s.trim();
    if let Ok(n) = s.parse::<i64>() {
        return Some(n);
    }
    if let Some(t) = aws_core::time::parse_rfc3339(s) {
        return Some(t);
    }
    let rest = s.split_once(", ").map_or(s, |(_, r)| r);
    let mut it = rest.split_whitespace();
    let (day, month, year, time) = (it.next()?, it.next()?, it.next()?, it.next()?);
    let m = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .iter()
    .position(|x| *x == month)?
        + 1;
    aws_core::time::parse_rfc3339(&format!("{year}-{m:02}-{day:0>2}T{time}Z"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_date_forms_agree() {
        let want = Some(784_111_777);
        assert_eq!(parse_http_date("Sun, 06 Nov 1994 08:49:37 GMT"), want);
        assert_eq!(parse_http_date("1994-11-06T08:49:37Z"), want);
        assert_eq!(parse_http_date("784111777"), want);
        assert_eq!(parse_http_date("yesterday"), None);
    }
}
