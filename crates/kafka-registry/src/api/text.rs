//! Reading the parts of a request the way Confluent's web server and framework (Jetty and Jersey) read them: path
//! segments unescaped one by one, query values (the first of a name, `+` as a space), Java's booleans and integers, and
//! HTTP basic credentials.

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

/// The path's segments, each unescaped on its own (so `%2F` stays inside its segment). Empty segments are dropped, as
/// Confluent's web server merges `//` and ignores a trailing `/` (`/subjects/` is `/subjects`; `/` has none). `None` for
/// a path that server refuses with its own 400 page: a malformed escape, bytes that are not UTF-8, or an escaped NUL.
#[must_use]
pub fn segments(path: &str) -> Option<Vec<String>> {
    path.strip_prefix('/')?
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|s| unescape(s, false).filter(|u| !u.contains('\0')))
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

/// Every value of `name` in a raw query string, in order.
#[must_use]
pub fn query_all(raw: &str, name: &str) -> Vec<String> {
    raw.split('&')
        .filter_map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            (unescape(k, true)? == name)
                .then(|| unescape(v, true))
                .flatten()
        })
        .collect()
}

/// A query flag as Confluent's framework reads one (Java's `Boolean.valueOf` of the first value): `true` in any case;
/// anything else, `1` and `t` too, is false.
#[must_use]
pub fn flag(raw: &str, name: &str) -> bool {
    query(raw, name).is_some_and(|v| v.eq_ignore_ascii_case("true"))
}

/// A whole-number query parameter as Confluent's framework reads it: absent or empty is `Some(None)`; an optional `-`
/// then digits that fit 32 bits is the number; anything else (`x`, `+1`, `1.0`, too large) is `None`, which Confluent
/// answers with 404.
#[must_use]
pub fn int_param(raw: &str, name: &str) -> Option<Option<i32>> {
    match query(raw, name).as_deref() {
        None | Some("") => Some(None),
        Some(v) => {
            let digits = v.strip_prefix('-').unwrap_or(v);
            if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
                return None;
            }
            v.parse().ok().map(Some)
        }
    }
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

/// The user and password of an `Authorization: Basic` header.
// fuzz:parser registry_api
#[must_use]
pub fn basic(header: &str) -> Option<(String, String)> {
    let (scheme, rest) = header.split_at_checked(6)?;
    if !scheme.eq_ignore_ascii_case("basic ") {
        return None;
    }
    // Padded base64; bits past the data are not checked (as Go's decoder does not).
    let raw =
        String::from_utf8(kafka_auth::crypto::base64::decode_lax(rest.as_bytes()).ok()?).ok()?;
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
        assert_eq!(segments("/"), Some(vec![]));
        assert_eq!(
            segments("//subjects//x/"),
            Some(vec!["subjects".into(), "x".into()])
        );
        assert_eq!(segments("/x%zz"), None);
        assert_eq!(segments("/x%e9"), None);
        assert_eq!(segments("/subjects/%00x"), None);
    }

    #[test]
    fn queries_read_as_javas_framework_reads_them() {
        assert_eq!(
            query("a=1&deleted=true&deleted=false", "deleted").as_deref(),
            Some("true")
        );
        assert_eq!(query("p=a+b%21", "p").as_deref(), Some("a b!"));
        // Recorded from cp-schema-registry 7.7.1 (tools/registry-diff): TRUE and True are true, 1 and t are not, and the
        // first value of a name is the one read.
        assert!(flag("verbose=TRUE", "verbose") && flag("verbose=True", "verbose"));
        assert!(!flag("verbose=T", "verbose") && !flag("verbose=1", "verbose"));
        assert!(!flag("verbose=yes", "verbose") && !flag("", "verbose"));
        assert!(!flag("deleted=false&deleted=true", "deleted"));
        assert_eq!(int_param("offset=-1", "offset"), Some(Some(-1)));
        assert_eq!(int_param("offset=", "offset"), Some(None));
        assert_eq!(int_param("a=1", "offset"), Some(None));
        for bad in ["x", "+1", "1.0", "99999999999", "-"] {
            assert_eq!(int_param(&format!("offset={bad}"), "offset"), None, "{bad}");
        }
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
        assert_eq!(
            basic("Basic YTp="),
            Some(("a".into(), String::new())),
            "bits past the data are not checked"
        );
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
