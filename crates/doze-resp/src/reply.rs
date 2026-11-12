//! RESP2 replies: what a server sends back, parsed from a buffer that may hold only part of one. The parser is bounded
//! (bulk size, element count, nesting) so a hostile or broken peer cannot make it allocate or recurse without limit.

use std::fmt;

/// One reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// `+OK`
    Simple(String),
    /// `-ERR …`
    Error(String),
    /// `:42`
    Integer(i64),
    /// `$3\r\nfoo`, or `$-1` (none).
    Bulk(Option<Vec<u8>>),
    /// `*2\r\n…`, or `*-1` (none).
    Array(Option<Vec<Reply>>),
}

impl Reply {
    /// The reply as text when it is a simple string or a bulk string.
    pub fn text(&self) -> Option<String> {
        match self {
            Reply::Simple(s) => Some(s.clone()),
            Reply::Bulk(Some(b)) => Some(String::from_utf8_lossy(b).into_owned()),
            _ => None,
        }
    }
}

/// A reply that cannot be parsed, however many more bytes arrive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    Malformed(&'static str),
    TooLarge,
    TooDeep,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Malformed(what) => write!(f, "malformed reply: {what}"),
            ParseError::TooLarge => f.write_str("reply larger than the client accepts"),
            ParseError::TooDeep => f.write_str("reply nested deeper than the client accepts"),
        }
    }
}

impl std::error::Error for ParseError {}

/// A server's largest bulk string (valkey's own `proto-max-bulk-len` default).
const MAX_BULK: usize = 512 << 20;
const MAX_ITEMS: usize = 1 << 20;
const MAX_DEPTH: usize = 32;

/// A command as RESP2 sends it: an array of bulk strings.
pub fn encode(args: &[&str]) -> Vec<u8> {
    let mut out = format!("*{}\r\n", args.len()).into_bytes();
    for a in args {
        out.extend_from_slice(format!("${}\r\n", a.len()).as_bytes());
        out.extend_from_slice(a.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    out
}

/// The text up to the first CRLF, and the bytes used including it; `None` while the line is incomplete.
fn line(buf: &[u8]) -> Option<(&[u8], usize)> {
    let end = buf.windows(2).position(|w| w == b"\r\n")?;
    Some((buf.get(..end)?, end + 2))
}

fn number(b: &[u8]) -> Result<i64, ParseError> {
    std::str::from_utf8(b)
        .ok()
        .and_then(|s| s.parse().ok())
        .ok_or(ParseError::Malformed(
            "a length or integer that is not a number",
        ))
}

/// A non-negative length within `max`, or `None` for RESP's -1 (a null).
fn length(head: &[u8], max: usize) -> Result<Option<usize>, ParseError> {
    match number(head)? {
        -1 => Ok(None),
        n if n < 0 => Err(ParseError::Malformed("a negative length")),
        n => {
            let n = usize::try_from(n).map_err(|_| ParseError::TooLarge)?;
            if n > max {
                return Err(ParseError::TooLarge);
            }
            Ok(Some(n))
        }
    }
}

/// Parses one reply from the start of `buf`: the reply and the bytes it used, or `None` if `buf` holds only part of
/// it so far.
// fuzz:parser fuzz_reply
pub fn parse(buf: &[u8]) -> Result<Option<(Reply, usize)>, ParseError> {
    parse_at(buf, 0)
}

fn parse_at(buf: &[u8], depth: usize) -> Result<Option<(Reply, usize)>, ParseError> {
    if depth > MAX_DEPTH {
        return Err(ParseError::TooDeep);
    }
    let Some((kind, rest)) = buf.split_first() else {
        return Ok(None);
    };
    let Some((head, used)) = line(rest) else {
        return Ok(None);
    };
    let at = 1 + used;
    let text = || String::from_utf8_lossy(head).into_owned();
    let done = |r: Reply, n: usize| Ok(Some((r, n)));
    match kind {
        b'+' => done(Reply::Simple(text()), at),
        b'-' => done(Reply::Error(text()), at),
        b':' => done(Reply::Integer(number(head)?), at),
        b'$' => match length(head, MAX_BULK)? {
            None => done(Reply::Bulk(None), at),
            Some(n) => bulk(buf, at, n),
        },
        b'*' => match length(head, MAX_ITEMS)? {
            None => done(Reply::Array(None), at),
            Some(n) => array(buf, at, n, depth),
        },
        _ => Err(ParseError::Malformed("an unknown reply type")),
    }
}

fn bulk(buf: &[u8], at: usize, n: usize) -> Result<Option<(Reply, usize)>, ParseError> {
    let end = at.checked_add(n).ok_or(ParseError::TooLarge)?;
    let (Some(body), Some(crlf)) = (buf.get(at..end), buf.get(end..end + 2)) else {
        return Ok(None);
    };
    if crlf != b"\r\n" {
        return Err(ParseError::Malformed("a bulk string not ended by CRLF"));
    }
    Ok(Some((Reply::Bulk(Some(body.to_vec())), end + 2)))
}

fn array(
    buf: &[u8],
    mut at: usize,
    n: usize,
    depth: usize,
) -> Result<Option<(Reply, usize)>, ParseError> {
    let mut items = Vec::with_capacity(n.min(64));
    for _ in 0..n {
        let rest = buf.get(at..).unwrap_or_default();
        let Some((item, used)) = parse_at(rest, depth + 1)? else {
            return Ok(None);
        };
        items.push(item);
        at += used;
    }
    Ok(Some((Reply::Array(Some(items)), at)))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;

    fn one(b: &[u8]) -> Reply {
        let (r, used) = parse(b).unwrap().unwrap();
        assert_eq!(used, b.len(), "{r:?} used {used} of {}", b.len());
        r
    }

    #[test]
    fn each_kind_of_reply() {
        assert_eq!(one(b"+PONG\r\n"), Reply::Simple("PONG".into()));
        assert_eq!(
            one(b"-NOAUTH Authentication required.\r\n"),
            Reply::Error("NOAUTH Authentication required.".into())
        );
        assert_eq!(one(b":-7\r\n"), Reply::Integer(-7));
        assert_eq!(
            one(b"$3\r\na\r\n\r\n"),
            Reply::Bulk(Some(b"a\r\n".to_vec()))
        );
        assert_eq!(one(b"$-1\r\n"), Reply::Bulk(None));
        assert_eq!(one(b"*-1\r\n"), Reply::Array(None));
        assert_eq!(
            one(b"*2\r\n$1\r\nx\r\n*1\r\n:1\r\n"),
            Reply::Array(Some(vec![
                Reply::Bulk(Some(b"x".to_vec())),
                Reply::Array(Some(vec![Reply::Integer(1)]))
            ]))
        );
    }

    #[test]
    fn a_partial_reply_waits_for_more_at_every_cut() {
        let whole = b"*2\r\n$5\r\nhello\r\n+OK\r\n";
        for cut in 0..whole.len() {
            assert_eq!(parse(&whole[..cut]).unwrap(), None, "cut at {cut}");
        }
        assert_eq!(parse(whole).unwrap().unwrap().1, whole.len());
    }

    #[test]
    fn bad_and_oversized_replies_are_refused_not_waited_for() {
        assert!(parse(b"?x\r\n").is_err());
        assert!(parse(b":x\r\n").is_err());
        assert!(parse(b"$-2\r\n").is_err());
        assert!(parse(b"$3\r\nabcde").is_err(), "no CRLF after the body");
        assert_eq!(parse(b"$9999999999\r\n"), Err(ParseError::TooLarge));
        assert_eq!(parse(b"*9999999999\r\n"), Err(ParseError::TooLarge));
        let deep = "*1\r\n".repeat(MAX_DEPTH + 2);
        assert_eq!(parse(deep.as_bytes()), Err(ParseError::TooDeep));
    }

    #[test]
    fn a_command_encodes_as_an_array_of_bulk_strings() {
        assert_eq!(
            encode(&["AUTH", "p w"]),
            b"*2\r\n$4\r\nAUTH\r\n$3\r\np w\r\n".to_vec()
        );
        assert_eq!(
            one(&encode(&["SET", "k", ""])),
            Reply::Array(Some(vec![
                Reply::Bulk(Some(b"SET".to_vec())),
                Reply::Bulk(Some(b"k".to_vec())),
                Reply::Bulk(Some(vec![]))
            ]))
        );
    }
}
