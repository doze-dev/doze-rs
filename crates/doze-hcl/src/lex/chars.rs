//! Character classes and token lengths for the scanner: Ragel's UTF-8 reading, identifiers,
//! numbers and literal runs.

/// The length of the UTF-8 sequence at the start of `b` as Ragel's `AnyUTF8` reads it (a lead byte
/// and the right number of continuation bytes, without Go's validity checks), or `None` for a
/// byte that starts no sequence (`BrokenUTF8`).
pub(crate) fn utf8_len(b: &[u8]) -> Option<usize> {
    let lead = *b.first()?;
    let n = match lead {
        0x00..=0x7f => return Some(1),
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        _ => return None,
    };
    let cont = b.get(1..n)?;
    cont.iter().all(|c| (0x80..=0xbf).contains(c)).then_some(n)
}

pub(super) fn char_at(b: &[u8]) -> Option<(char, usize)> {
    let n = utf8_len(b)?;
    let c = std::str::from_utf8(b.get(..n)?).ok()?.chars().next()?;
    Some((c, n))
}

pub(super) fn is_id_start(c: char) -> bool {
    c == '_' || unicode_ident::is_xid_start(c)
}

/// Go's tables predate Unicode 15.1, which made the zero-width joiners identifier characters.
fn is_id_continue(c: char) -> bool {
    c == '-' || unicode_ident::is_xid_continue(c) && c != '\u{200c}' && c != '\u{200d}'
}

pub(super) fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Go's `NumberLit`: a digit, then digits, dots and exponents (`e`, a sign, a digit), not ending
/// in a dot.
pub(super) fn number_len(b: &[u8]) -> usize {
    let (mut i, mut last) = (1, 1);
    loop {
        match (b.get(i), b.get(i + 1), b.get(i + 2)) {
            (Some(c), _, _) if c.is_ascii_digit() => {
                i += 1;
                last = i;
            }
            (Some(b'.'), _, _) => i += 1,
            (Some(b'e' | b'E'), Some(d), _) if d.is_ascii_digit() => {
                i += 2;
                last = i;
            }
            (Some(b'e' | b'E'), Some(b'+' | b'-'), Some(d)) if d.is_ascii_digit() => {
                i += 3;
                last = i;
            }
            _ => return last,
        }
    }
}

pub(super) fn ident_len(b: &[u8]) -> usize {
    let mut i = 0;
    while let Some((c, n)) = b.get(i..).and_then(char_at) {
        let ok = if i == 0 {
            is_id_start(c)
        } else {
            is_id_continue(c)
        };
        if !ok {
            break;
        }
        i += n;
    }
    i
}

/// `(\\ char | char - [$%"\\])+` with no newlines: the run of quoted literal text here.
pub(super) fn quoted_lit_len(b: &[u8]) -> usize {
    let mut i = 0;
    loop {
        match b.get(i) {
            None | Some(b'$' | b'%' | b'"' | b'\r' | b'\n') => return i,
            Some(b'\\') => match b.get(i + 1..).and_then(utf8_len) {
                Some(n) if !matches!(b.get(i + 1), Some(b'\r' | b'\n')) => i += 1 + n,
                _ => return i,
            },
            Some(_) => match b.get(i..).and_then(utf8_len) {
                Some(n) => i += n,
                None => return i,
            },
        }
    }
}

/// `(char - [$%])*` with no newlines: heredoc literal text up to the end of its line.
pub(super) fn heredoc_lit_len(b: &[u8]) -> usize {
    let mut i = 0;
    loop {
        match b.get(i) {
            None | Some(b'$' | b'%' | b'\r' | b'\n') => return i,
            Some(_) => match b.get(i..).and_then(utf8_len) {
                Some(n) => i += n,
                None => return i,
            },
        }
    }
}
