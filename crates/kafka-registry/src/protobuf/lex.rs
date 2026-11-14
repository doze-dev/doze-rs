//! The tokens of the `.proto` language: identifiers, numbers, strings and punctuation, with comments and white space
//! dropped. A token is a span of the source; its text is read from the source when it is needed.

use super::Error;

/// What a token is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Tok {
    Ident,
    Int,
    Float,
    Str,
    Punct(u8),
}

/// A token: its kind and where it is in the source (byte offsets).
#[derive(Debug, Clone, Copy)]
pub(super) struct Token {
    pub(super) kind: Tok,
    pub(super) start: u32,
    pub(super) end: u32,
}

fn word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

fn off(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

/// The tokens of `src`.
///
/// # Errors
/// A character that is not part of the language, an unterminated string or comment, a malformed number.
pub(super) fn lex(src: &str) -> Result<Vec<Token>, Error> {
    if src.len() >= u32::MAX as usize {
        return Err(Error::syntax(0, "schema too large"));
    }
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        let start = i;
        let kind = match c {
            b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c => {
                i += 1;
                continue;
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while b.get(i).is_some_and(|&c| c != b'\n') {
                    i += 1;
                }
                continue;
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                let end = src
                    .get(i + 2..)
                    .and_then(|r| r.find("*/"))
                    .ok_or_else(|| Error::syntax(off(i), "unterminated block comment"))?;
                i += end + 4;
                continue;
            }
            b'"' | b'\'' => {
                i += 1;
                loop {
                    match b.get(i) {
                        None | Some(b'\n' | 0) => {
                            return Err(Error::syntax(off(start), "unterminated string"));
                        }
                        Some(b'\\') => i += 2,
                        Some(&q) if q == c => break,
                        Some(_) => i += 1,
                    }
                }
                i += 1;
                Tok::Str
            }
            b'0'..=b'9' => number(b, &mut i)?,
            b'.' if b.get(i + 1).is_some_and(u8::is_ascii_digit) => number(b, &mut i)?,
            c if c.is_ascii_alphabetic() || c == b'_' => {
                while b.get(i).copied().is_some_and(word) {
                    i += 1;
                }
                Tok::Ident
            }
            b'=' | b';' | b'{' | b'}' | b'[' | b']' | b'(' | b')' | b'<' | b'>' | b',' | b'.' | b':'
            | b'-' | b'+' | b'/' => {
                i += 1;
                Tok::Punct(c)
            }
            _ => return Err(Error::syntax(off(i), "unexpected character")),
        };
        out.push(Token {
            kind,
            start: off(start),
            end: off(i),
        });
    }
    Ok(out)
}

/// A number from `b[*i]`: decimal, octal or hex integers, and floats with a point or an exponent.
fn number(b: &[u8], i: &mut usize) -> Result<Tok, Error> {
    let start = *i;
    let digits = |i: &mut usize, hex: bool| {
        while b
            .get(*i)
            .is_some_and(|c| c.is_ascii_digit() || (hex && c.is_ascii_hexdigit()))
        {
            *i += 1;
        }
    };
    let mut kind = Tok::Int;
    if b.get(*i) == Some(&b'0') && matches!(b.get(*i + 1), Some(b'x' | b'X')) {
        *i += 2;
        digits(i, true);
    } else {
        digits(i, false);
        if b.get(*i) == Some(&b'.') {
            kind = Tok::Float;
            *i += 1;
            digits(i, false);
        }
        if matches!(b.get(*i), Some(b'e' | b'E')) {
            kind = Tok::Float;
            *i += 1;
            if matches!(b.get(*i), Some(b'+' | b'-')) {
                *i += 1;
            }
            let at = *i;
            digits(i, false);
            if at == *i {
                return Err(Error::syntax(off(start), "invalid number"));
            }
        }
    }
    if b.get(*i).copied().is_some_and(word) {
        return Err(Error::syntax(off(start), "invalid number"));
    }
    Ok(kind)
}

/// The bytes a string literal stands for (its quotes and escapes removed).
///
/// # Errors
/// An escape the language does not have.
pub(super) fn unquote(lit: &str, at: u32) -> Result<Vec<u8>, Error> {
    let inner = lit.get(1..lit.len().saturating_sub(1)).unwrap_or_default();
    let b = inner.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    let bad = || Error::syntax(at, "invalid escape in string");
    while let Some(&c) = b.get(i) {
        i += 1;
        if c != b'\\' {
            out.push(c);
            continue;
        }
        let e = *b.get(i).ok_or_else(bad)?;
        i += 1;
        let simple = match e {
            b'a' => Some(7),
            b'b' => Some(8),
            b'f' => Some(12),
            b'n' => Some(b'\n'),
            b'r' => Some(b'\r'),
            b't' => Some(b'\t'),
            b'v' => Some(11),
            b'\\' | b'\'' | b'"' | b'?' => Some(e),
            _ => None,
        };
        if let Some(s) = simple {
            out.push(s);
            continue;
        }
        let (radix, max, skip) = match e {
            b'x' | b'X' => (16, 2, 0),
            b'0'..=b'7' => (8, 3, 1),
            b'u' => (16, 4, 0),
            b'U' => (16, 8, 0),
            _ => return Err(bad()),
        };
        i -= skip;
        let mut v: u32 = 0;
        let mut n = 0;
        while n < max
            && let Some(d) = b.get(i).and_then(|&c| char::from(c).to_digit(radix))
        {
            v = v * radix + d;
            i += 1;
            n += 1;
        }
        if n == 0 || (matches!(e, b'u' | b'U') && n != max) {
            return Err(bad());
        }
        if matches!(e, b'u' | b'U') {
            let ch = char::from_u32(v).ok_or_else(bad)?;
            out.extend_from_slice(ch.encode_utf8(&mut [0; 4]).as_bytes());
        } else {
            out.push(u8::try_from(v).map_err(|_| bad())?);
        }
    }
    Ok(out)
}
