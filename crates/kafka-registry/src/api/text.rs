//! Reading the parts of a request the way Go's `net/http` (the oracle's) reads them: path segments unescaped one by
//! one, query values (the first of a name, `+` as a space), Go's booleans, and HTTP basic credentials.

/// `%XX` escapes undone; `None` for a malformed escape or bytes that are not UTF-8.
#[must_use]
pub fn unescape(s: &str, plus_is_space: bool) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        match c {
            b'%' => {
                let hex = b.get(i + 1..i + 3)?;
                let hi = char::from(*hex.first()?).to_digit(16)?;
                let lo = char::from(*hex.get(1)?).to_digit(16)?;
                out.push(u8::try_from(hi * 16 + lo).ok()?);
                i += 3;
            }
            b'+' if plus_is_space => {
                out.push(b' ');
                i += 1;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// The path's segments, each unescaped on its own (so `%2F` stays inside its segment); `None` for a malformed escape.
#[must_use]
pub fn segments(path: &str) -> Option<Vec<String>> {
    path.strip_prefix('/')?
        .split('/')
        .map(|s| unescape(s, false))
        .collect()
}

/// The first value of `name` in a raw query string.
#[must_use]
pub fn query(raw: &str, name: &str) -> Option<String> {
    raw.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        (unescape(k, true)? == name)
            .then(|| unescape(v, true))
            .flatten()
    })
}

/// A query flag as Go's `strconv.ParseBool` reads it; anything else is false.
#[must_use]
pub fn flag(raw: &str, name: &str) -> bool {
    matches!(
        query(raw, name).as_deref(),
        Some("1" | "t" | "T" | "true" | "TRUE" | "True")
    )
}

/// A number as Go's `strconv.Atoi` reads it.
#[must_use]
pub fn atoi(s: &str) -> Option<i64> {
    let digits = s.strip_prefix(['+', '-']).unwrap_or(s);
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

fn base64_value(c: u8) -> Option<u32> {
    Some(u32::from(match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => return None,
    }))
}

/// Standard base64 with padding, strictly.
fn base64(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    if !b.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(b.len() / 4 * 3);
    for (n, chunk) in b.chunks(4).enumerate() {
        let last = n + 1 == b.len() / 4;
        let pad = chunk.iter().rev().take_while(|&&c| c == b'=').count();
        if pad > 2 || (pad > 0 && !last) {
            return None;
        }
        let mut v = 0_u32;
        for (i, &c) in chunk.iter().enumerate() {
            let d = if i >= 4 - pad { 0 } else { base64_value(c)? };
            v = (v << 6) | d;
        }
        let bytes = v.to_be_bytes();
        out.extend(bytes.get(1..4 - pad)?);
    }
    Some(out)
}

/// The user and password of an `Authorization: Basic` header.
// fuzz:parser registry_api
#[must_use]
pub fn basic(header: &str) -> Option<(String, String)> {
    let (scheme, rest) = header.split_at_checked(6)?;
    if !scheme.eq_ignore_ascii_case("basic ") {
        return None;
    }
    let raw = String::from_utf8(base64(rest)?).ok()?;
    let (user, pass) = raw.split_once(':')?;
    Some((user.to_owned(), pass.to_owned()))
}

/// The host of a `Host` header without its port.
#[must_use]
pub fn host_name(host: &str) -> &str {
    if let Some(rest) = host.strip_prefix('[') {
        return rest.split_once(']').map_or(host, |(h, _)| h);
    }
    match host.rsplit_once(':') {
        Some((h, port)) if !h.contains(':') && port.bytes().all(|c| c.is_ascii_digit()) => h,
        _ => host,
    }
}

/// A host quoted for a message, without the characters that could break it, and not more than 200 of them.
#[must_use]
pub fn quoted(s: &str) -> String {
    let clean: String = s
        .chars()
        .filter(|c| !matches!(c, '"' | '\n' | '\r'))
        .take(200)
        .collect();
    format!("\"{clean}\"")
}

/// `s` cut to at most `max` bytes, at a character boundary.
#[must_use]
pub fn cut(s: &str, max: usize) -> &str {
    let mut end = s.len().min(max);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_are_unescaped_one_by_one() {
        assert_eq!(
            segments("/subjects/a%2Fb/versions"),
            Some(vec!["subjects".into(), "a/b".into(), "versions".into()])
        );
        assert_eq!(segments("/"), Some(vec![String::new()]));
        assert_eq!(segments("/x%zz"), None);
        assert_eq!(segments("/x%e9"), None);
    }

    #[test]
    fn queries_read_as_go_reads_them() {
        assert_eq!(
            query("a=1&deleted=true&deleted=false", "deleted").as_deref(),
            Some("true")
        );
        assert_eq!(query("p=a+b%21", "p").as_deref(), Some("a b!"));
        assert!(
            flag("verbose=T", "verbose") && !flag("verbose=yes", "verbose") && !flag("", "verbose")
        );
        assert_eq!(atoi("+5"), Some(5));
        assert_eq!(atoi("-3"), Some(-3));
        assert_eq!(atoi("5x"), None);
        assert_eq!(atoi(""), None);
    }

    #[test]
    fn basic_credentials_are_decoded_strictly() {
        assert_eq!(
            basic("Basic YWxpY2U6czNjcjp0"),
            Some(("alice".into(), "s3cr:t".into()))
        );
        assert_eq!(basic("basic YTpi"), Some(("a".into(), "b".into())));
        assert_eq!(basic("Basic YTo="), Some(("a".into(), String::new())));
        for bad in [
            "Bearer YTpi",
            "Basic YTpi=",
            "Basic Y=pi",
            "Basic",
            "Basic YWJj",
        ] {
            assert_eq!(basic(bad), None, "{bad}");
        }
    }

    #[test]
    fn hosts_lose_their_port_and_quotes() {
        assert_eq!(host_name("attacker.example:8081"), "attacker.example");
        assert_eq!(host_name("[::1]:8081"), "::1");
        assert_eq!(host_name("plain"), "plain");
        assert_eq!(quoted("a\"b\nc"), "\"abc\"");
        assert_eq!(cut("héllo", 2), "h");
    }
}
