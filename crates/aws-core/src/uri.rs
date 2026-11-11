//! Percent-encoding the way SigV4 and the REST protocols need it (RFC 3986
//! unreserved set, upper-case hex), without the `percent-encoding` crate.
use crate::hex;
use std::borrow::Cow;

const fn unreserved(b: u8) -> bool {
    matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~')
}

const fn upper_hex(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        _ => (b'A' + (n - 10)) as char,
    }
}

/// Append the SigV4 URI-encoding of `bytes` to `out`. `/` is kept when `keep_slash`.
pub fn encode_into(out: &mut String, bytes: &[u8], keep_slash: bool) {
    for &b in bytes {
        if unreserved(b) || (keep_slash && b == b'/') {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(upper_hex(b >> 4));
            out.push(upper_hex(b & 15));
        }
    }
}

/// [`encode_into`] returning a new string.
#[must_use]
pub fn encode(bytes: &[u8], keep_slash: bool) -> String {
    let mut s = String::with_capacity(bytes.len());
    encode_into(&mut s, bytes, keep_slash);
    s
}

/// Percent-decode (`+` stays `+`: this is path semantics, not form semantics).
/// Malformed escapes pass through literally.
#[must_use]
pub fn decode(s: &str) -> Cow<'_, [u8]> {
    if !s.bytes().any(|b| b == b'%') {
        return Cow::Borrowed(s.as_bytes());
    }
    let mut out = Vec::with_capacity(s.len());
    let mut it = s.bytes().peekable();
    while let Some(b) = it.next() {
        if b == b'%' {
            let mut look = it.clone();
            if let (Some(h), Some(l)) = (
                look.next().and_then(hex::digit),
                look.next().and_then(hex::digit),
            ) {
                out.push(h << 4 | l);
                it = look;
                continue;
            }
        }
        out.push(b);
    }
    Cow::Owned(out)
}

/// Percent-decode to a string; `None` when the result is not UTF-8.
#[must_use]
pub fn decode_str(s: &str) -> Option<Cow<'_, str>> {
    match decode(s) {
        Cow::Borrowed(b) => std::str::from_utf8(b).ok().map(Cow::Borrowed),
        Cow::Owned(v) => String::from_utf8(v).ok().map(Cow::Owned),
    }
}

/// RFC 3986 dot-segment removal as SigV4 clients do for non-S3 services:
/// drops empty and `.` segments, resolves `..`, keeps a trailing slash.
#[must_use]
pub fn normalize_path(path: &str) -> String {
    let mut stack: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                stack.pop();
            }
            s => stack.push(s),
        }
    }
    let mut out = String::with_capacity(path.len());
    for seg in &stack {
        out.push('/');
        out.push_str(seg);
    }
    if out.is_empty() || (path.ends_with('/') && !out.ends_with('/')) {
        out.push('/');
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn encode_decode() {
        assert_eq!(encode(b"a b/c~d!", true), "a%20b/c~d%21");
        assert_eq!(encode(b"a/b", false), "a%2Fb");
        assert_eq!(decode_str("a%20b+c%2").unwrap(), "a b+c%2");
        assert_eq!(&*decode("%e2%82%ac"), "€".as_bytes());
    }

    #[test]
    fn dot_segments() {
        assert_eq!(normalize_path("/a/./b/../c//d/"), "/a/c/d/");
        assert_eq!(normalize_path(""), "/");
        assert_eq!(normalize_path("/"), "/");
    }
}
