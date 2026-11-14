//! Character classes, escapes and repetition counts of Go's regular expressions (`parseClass`,
//! `parseEscape`, `parsePerlClassEscape`, `parseUnicodeClass`, `parseNamedClass`, `parseRepeat`),
//! written in the `regex` crate's syntax with every literal as a `\x{…}` escape, so nothing in a
//! class means more than in Go.

use super::regex_names::unicode_class;
use super::regex_syntax::{Parser, err};

/// Go's Perl classes, which are ASCII-only: the crate's class body and whether it is negated.
fn perl_class(c: char) -> Option<(&'static str, bool)> {
    match c {
        'd' => Some(("0-9", false)),
        'D' => Some(("0-9", true)),
        's' => Some(("\\t\\n\\x{C}\\r ", false)),
        'S' => Some(("\\t\\n\\x{C}\\r ", true)),
        'w' => Some(("0-9A-Za-z_", false)),
        'W' => Some(("0-9A-Za-z_", true)),
        _ => None,
    }
}

const POSIX: [&str; 14] = [
    "alnum", "alpha", "ascii", "blank", "cntrl", "digit", "graph", "lower", "print", "punct",
    "space", "upper", "word", "xdigit",
];

/// `\d`-style or `\p`-style class at the parser (on its backslash), in the crate's syntax.
pub(super) fn escape_class(p: &mut Parser<'_>) -> Result<Option<String>, String> {
    if let Some(u) = unicode(p)? {
        return Ok(Some(u));
    }
    let Some((body, neg)) = p.rest().chars().nth(1).and_then(perl_class) else {
        return Ok(None);
    };
    p.advance(2);
    Ok(Some(format!("[{}{body}]", if neg { "^" } else { "" })))
}

/// `\pL`, `\p{Name}`, `\PL`, `\p{^Name}`: `None` when the text is no such class.
fn unicode(p: &mut Parser<'_>) -> Result<Option<String>, String> {
    let rest = p.rest();
    let mut sign = match rest.get(..2) {
        Some("\\p") => true,
        Some("\\P") => false,
        _ => return Ok(None),
    };
    let after = rest.get(2..).unwrap_or("");
    let Some(first) = after.chars().next() else {
        return Ok(None);
    };
    let (seq_len, mut name) = if first == '{' {
        let Some(end) = rest.find('}') else {
            return Err(err("invalid character class range", rest));
        };
        (end + 1, rest.get(3..end).unwrap_or(""))
    } else {
        (
            2 + first.len_utf8(),
            after.get(..first.len_utf8()).unwrap_or(""),
        )
    };
    let seq = rest.get(..seq_len).unwrap_or("").to_owned();
    if let Some(n) = name.strip_prefix('^') {
        sign = !sign;
        name = n;
    }
    let Some(rust) = unicode_class(name) else {
        return Err(err("invalid character class range", &seq));
    };
    p.advance(seq_len);
    if rust == "Cs" {
        // Surrogates: Go knows the class, no text holds one; the crate refuses the name.
        let all = "\\x{0}-\\x{10FFFF}";
        return Ok(Some(if sign {
            format!("[^{all}]")
        } else {
            format!("[{all}]")
        }));
    }
    Ok(Some(format!(
        "\\{}{{{rust}}}",
        if sign { 'p' } else { 'P' }
    )))
}

/// Go's `parseEscape` at the parser's backslash: the character it stands for.
pub(super) fn escape_char(p: &mut Parser<'_>) -> Result<u32, String> {
    let rest = p.rest().to_owned();
    let rest = rest.as_str();
    let mut chars = rest.chars().skip(1);
    let Some(c) = chars.next() else {
        return Err(err("trailing backslash at end of expression", ""));
    };
    let bad = |len: usize| err("invalid escape sequence", rest.get(..len).unwrap_or(rest));
    let simple = match c {
        'a' => Some(7),
        'f' => Some(12),
        'n' => Some(10),
        'r' => Some(13),
        't' => Some(9),
        'v' => Some(11),
        c if c.is_ascii() && !c.is_ascii_alphanumeric() => Some(u32::from(c)),
        _ => None,
    };
    if let Some(v) = simple {
        p.advance(1 + c.len_utf8());
        return Ok(v);
    }
    let b = rest.as_bytes();
    let octal = |i: usize| b.get(i).filter(|d| (b'0'..=b'7').contains(d)).copied();
    if c == '0' || (('1'..='7').contains(&c) && octal(2).is_some()) {
        let mut v = 0;
        let mut i = 1;
        while i < 4
            && let Some(d) = octal(i)
        {
            v = v * 8 + u32::from(d - b'0');
            i += 1;
        }
        p.advance(i);
        return Ok(v);
    }
    if c == 'x' {
        return hex(p).ok_or_else(|| bad(rest.len().min(4)));
    }
    Err(bad(1 + c.len_utf8()))
}

/// `\xHH` or `\x{H…}` at the parser.
fn hex(p: &mut Parser<'_>) -> Option<u32> {
    let rest = p.rest();
    let body = rest.get(2..)?;
    if let Some(inner) = body.strip_prefix('{') {
        let end = inner.find('}')?;
        let digits = inner.get(..end)?;
        if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let v = digits
            .trim_start_matches('0')
            .chars()
            .try_fold(0u32, |v, c| {
                let v = v.checked_mul(16)? + c.to_digit(16)?;
                (v <= 0x10FFFF).then_some(v)
            })?;
        p.advance(2 + 1 + end + 1);
        return Some(v);
    }
    let digits = body.get(..2)?;
    let v = u32::from_str_radix(digits, 16).ok()?;
    if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    p.advance(4);
    Some(v)
}

/// `[...]` at the parser: Go's `parseClass`.
pub(super) fn class(p: &mut Parser<'_>) -> Result<String, String> {
    let whole = p.rest().to_owned();
    p.advance(1);
    let mut out = String::from("[");
    if p.peek() == Some('^') {
        p.advance(1);
        out.push('^');
    }
    let mut first = true;
    loop {
        match p.peek() {
            None => return Err(err("missing closing ]", &whole)),
            Some(']') if !first => break,
            _ => {}
        }
        first = false;
        if let Some(posix) = posix(p)? {
            out.push_str(&posix);
            continue;
        }
        if let Some(u) = escape_class(p)? {
            out.push_str(&u);
            continue;
        }
        let start = p.rest().to_owned();
        let lo = class_char(p, &whole)?;
        let mut hi = lo;
        let rest = p.rest();
        if rest.starts_with('-') && rest.get(1..2).is_some_and(|c| c != "]") {
            p.advance(1);
            hi = class_char(p, &whole)?;
            if hi < lo {
                let used = start.len() - p.rest().len();
                return Err(err(
                    "invalid character class range",
                    start.get(..used).unwrap_or(""),
                ));
            }
        }
        out.push_str(&range(lo, hi));
    }
    p.advance(1);
    out.push(']');
    Ok(out)
}

/// `[:name:]` or `[:^name:]` at the parser (`None` when there is no `:]` to end it).
fn posix(p: &mut Parser<'_>) -> Result<Option<String>, String> {
    let rest = p.rest();
    let Some(inner) = rest.strip_prefix("[:") else {
        return Ok(None);
    };
    if rest.len() <= 2 {
        return Ok(None);
    }
    let Some(end) = inner.find(":]") else {
        return Ok(None);
    };
    let name = inner.get(..end).unwrap_or("");
    let bare = name.strip_prefix('^').unwrap_or(name);
    if !POSIX.contains(&bare) {
        return Err(err(
            "invalid character class range",
            rest.get(..end + 4).unwrap_or(""),
        ));
    }
    let text = format!("[:{name}:]");
    p.advance(end + 4);
    Ok(Some(text))
}

fn class_char(p: &mut Parser<'_>, whole: &str) -> Result<u32, String> {
    match p.peek() {
        None => Err(err("missing closing ]", whole)),
        Some('\\') => escape_char(p),
        Some(c) => {
            p.advance(c.len_utf8());
            Ok(u32::from(c))
        }
    }
}

/// A class range (or one character) in the crate's syntax; surrogates, which no text holds, are
/// left out.
fn range(lo: u32, hi: u32) -> String {
    let clamp = |v: u32, up: bool| match v {
        0xD800..=0xDFFF if up => 0xE000,
        0xD800..=0xDFFF => 0xD7FF,
        v => v,
    };
    let (lo, hi) = (clamp(lo, true), clamp(hi, false));
    if lo > hi {
        return String::new();
    }
    if lo == hi {
        return format!("\\x{{{lo:X}}}");
    }
    format!("\\x{{{lo:X}}}-\\x{{{hi:X}}}")
}

/// What Go's `parseRepeat` read from text starting with `{`.
pub(super) struct Braces {
    pub(super) min: Option<u32>,
    /// `None` for `{n}`; `Some(None)` for `{n,}`; `Some(Some(m))` for `{n,m}` (`m` too large to
    /// count is `u32::MAX`).
    max: Option<Option<u32>>,
    pub(super) single: bool,
    pub(super) len: usize,
}

impl Braces {
    /// (min, max with -1 for none), or `None` when Go refuses the counts.
    pub(super) fn counts(&self) -> Option<(u32, i64)> {
        let min = self.min.filter(|m| *m <= 1000)?;
        let max = match self.max {
            None => i64::from(min),
            Some(None) => -1,
            Some(Some(m)) if m <= 1000 && m >= min => i64::from(m),
            Some(Some(_)) => return None,
        };
        Some((min, max))
    }
}

pub(super) fn braces(s: &str) -> Option<Braces> {
    let b = s.as_bytes();
    let mut at = 1;
    let min = int(b, &mut at)?;
    let max = if b.get(at) == Some(&b',') {
        at += 1;
        if b.get(at) == Some(&b'}') {
            Some(None)
        } else {
            Some(Some(int(b, &mut at)?.unwrap_or(u32::MAX)))
        }
    } else {
        None
    };
    if b.get(at) != Some(&b'}') {
        return None;
    }
    Some(Braces {
        min,
        single: max.is_none(),
        max,
        len: at + 1,
    })
}

/// Digits without a leading zero: `Some(None)` when there are too many to count.
fn int(b: &[u8], at: &mut usize) -> Option<Option<u32>> {
    let start = *at;
    if !b.get(start).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    if b.get(start) == Some(&b'0') && b.get(start + 1).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let mut n: u64 = 0;
    let mut big = false;
    while let Some(d) = b.get(*at).filter(|d| d.is_ascii_digit()) {
        big |= n >= 100_000_000;
        n = n.saturating_mul(10).saturating_add(u64::from(d - b'0'));
        *at += 1;
    }
    Some(if big { None } else { u32::try_from(n).ok() })
}
