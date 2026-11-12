//! A key, a value or a header's value as a person reads it: text, JSON or bytes, with its true size.
//!
//! The rules (the page says the same in its own words):
//! - Absent is `Null` (a null key, the value of a deletion); zero bytes is `Empty`.
//! - Valid UTF-8 is text; text that is a complete JSON object or array is labelled JSON. Anything else is `Binary`: shown
//!   as a hex dump, never as a guess at text.
//! - Text is shown in full up to 64 KiB and then cut, with its true size beside it. Hex is shown up to 16 KiB.
//! - Control characters other than tab, newline and carriage return, the line separators and the bidirectional controls
//!   are shown as `⟨U+XXXX⟩`, so a value cannot move text around on the page or hide in it.
//! - A table cell shows the first 47 characters on one line, or `binary · ` and the first 8 bytes in hex.

use crate::format::{hex_dump, hex_prefix};
use crate::model::{Kind, Shown};

/// How much of a text is shown.
pub const TEXT_CAP: usize = 64 << 10;
/// How much of binary is shown, as a hex dump.
pub const HEX_CAP: usize = 16 << 10;
/// How many characters a table cell shows.
pub const PREVIEW_CHARS: usize = 47;
/// The deepest JSON that is recognised.
const JSON_DEPTH: usize = 64;

/// Whether `c` is shown as its code point instead of itself.
fn hidden(c: char) -> bool {
    matches!(c,
        '\u{0}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' | '\u{7f}'..='\u{9f}'
        | '\u{61c}' | '\u{200e}' | '\u{200f}' | '\u{2028}' | '\u{2029}'
        | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}')
}

/// `text` with the characters that must not act on the page written out.
#[must_use]
pub fn clean(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if hidden(c) {
            out.push_str(&format!("⟨U+{:04X}⟩", u32::from(c)));
        } else {
            out.push(c);
        }
    }
    out
}

/// The longest prefix of `text` that is at most `cap` bytes and ends on a character.
fn prefix(text: &str, cap: usize) -> &str {
    let mut end = text.len().min(cap);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or_default()
}

/// One line of at most [`PREVIEW_CHARS`] characters, with `…` when there was more.
fn one_line(text: &str, more: bool) -> String {
    let mut out = String::new();
    for (count, c) in clean(text).chars().enumerate() {
        if count == PREVIEW_CHARS {
            return format!("{out}…");
        }
        out.push(if matches!(c, '\n' | '\r' | '\t') {
            ' '
        } else {
            c
        });
    }
    if more { format!("{out}…") } else { out }
}

/// `value`, which is the first bytes of something `len` bytes long (or all of it), as it is shown.
#[must_use]
pub fn shown(value: Option<&[u8]>, len: u64) -> Shown {
    let Some(bytes) = value else {
        return Shown {
            kind: Kind::Null,
            bytes: 0,
            text: String::new(),
            clipped: false,
            preview: String::new(),
        };
    };
    if len == 0 {
        return Shown {
            kind: Kind::Empty,
            bytes: 0,
            text: String::new(),
            clipped: false,
            preview: String::new(),
        };
    }
    let kept_all = u64::try_from(bytes.len()).is_ok_and(|n| n >= len);
    match text_of(bytes, kept_all) {
        Some(text) => as_text(text, len, kept_all),
        None => as_binary(bytes, len),
    }
}

/// The text the bytes are, if they are UTF-8. A prefix cut in the middle of a character is still text up to there.
fn text_of(bytes: &[u8], kept_all: bool) -> Option<&str> {
    match std::str::from_utf8(bytes) {
        Ok(t) => Some(t),
        Err(e) if e.error_len().is_none() && !kept_all => bytes
            .get(..e.valid_up_to())
            .and_then(|b| std::str::from_utf8(b).ok()),
        Err(_) => None,
    }
}

fn as_text(text: &str, len: u64, kept_all: bool) -> Shown {
    let shown = prefix(text, TEXT_CAP);
    let clipped = u64::try_from(shown.len()).map_or(true, |n| n < len);
    let json = kept_all && !clipped && looks_like_json(text);
    Shown {
        kind: if json { Kind::Json } else { Kind::Text },
        bytes: len,
        text: clean(shown),
        clipped,
        preview: {
            let head = prefix(text, PREVIEW_CHARS * 4);
            one_line(head, clipped || head.len() < text.len())
        },
    }
}

fn as_binary(bytes: &[u8], len: u64) -> Shown {
    let (hex, more) = hex_prefix(bytes, 8);
    let cut = bytes.get(..bytes.len().min(HEX_CAP)).unwrap_or_default();
    Shown {
        kind: Kind::Binary,
        bytes: len,
        text: hex_dump(cut),
        clipped: u64::try_from(cut.len()).map_or(true, |n| n < len),
        preview: format!("binary · {hex}{}", if more || len > 8 { " …" } else { "" }),
    }
}

/// Whether `text` is one complete JSON object or array (RFC 8259), nested at most 64 deep.
#[must_use]
pub fn looks_like_json(text: &str) -> bool {
    let b = text.trim().as_bytes();
    if !matches!(b.first(), Some(b'{' | b'[')) {
        return false;
    }
    let mut at = 0;
    value(b, &mut at, 0).is_some() && {
        skip_ws(b, &mut at);
        at == b.len()
    }
}

fn skip_ws(b: &[u8], at: &mut usize) {
    while matches!(b.get(*at), Some(b' ' | b'\t' | b'\n' | b'\r')) {
        *at += 1;
    }
}

fn value(b: &[u8], at: &mut usize, depth: usize) -> Option<()> {
    if depth > JSON_DEPTH {
        return None;
    }
    skip_ws(b, at);
    match b.get(*at)? {
        b'{' => container(b, at, depth, b'}', true),
        b'[' => container(b, at, depth, b']', false),
        b'"' => string(b, at),
        b't' => word(b, at, b"true"),
        b'f' => word(b, at, b"false"),
        b'n' => word(b, at, b"null"),
        b'-' | b'0'..=b'9' => number(b, at),
        _ => None,
    }
}

fn word(b: &[u8], at: &mut usize, w: &[u8]) -> Option<()> {
    (b.get(*at..*at + w.len())? == w).then(|| *at += w.len())
}

fn container(b: &[u8], at: &mut usize, depth: usize, close: u8, object: bool) -> Option<()> {
    *at += 1;
    skip_ws(b, at);
    if b.get(*at) == Some(&close) {
        *at += 1;
        return Some(());
    }
    loop {
        skip_ws(b, at);
        if object {
            string(b, at)?;
            skip_ws(b, at);
            (b.get(*at)? == &b':').then(|| *at += 1)?;
        }
        value(b, at, depth + 1)?;
        skip_ws(b, at);
        match b.get(*at)? {
            b',' => *at += 1,
            c if *c == close => {
                *at += 1;
                return Some(());
            }
            _ => return None,
        }
    }
}

fn string(b: &[u8], at: &mut usize) -> Option<()> {
    (b.get(*at)? == &b'"').then(|| *at += 1)?;
    loop {
        match *b.get(*at)? {
            b'"' => {
                *at += 1;
                return Some(());
            }
            b'\\' => match b.get(*at + 1)? {
                b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => *at += 2,
                b'u' => {
                    let hex = b.get(*at + 2..*at + 6)?;
                    hex.iter().all(u8::is_ascii_hexdigit).then_some(())?;
                    *at += 6;
                }
                _ => return None,
            },
            0..=0x1f => return None,
            _ => *at += 1,
        }
    }
}

fn number(b: &[u8], at: &mut usize) -> Option<()> {
    let digits = |at: &mut usize| {
        let from = *at;
        while b.get(*at).is_some_and(u8::is_ascii_digit) {
            *at += 1;
        }
        *at > from
    };
    if b.get(*at) == Some(&b'-') {
        *at += 1;
    }
    if b.get(*at) == Some(&b'0') {
        *at += 1;
    } else if !digits(at) {
        return None;
    }
    if b.get(*at) == Some(&b'.') {
        *at += 1;
        digits(at).then_some(())?;
    }
    if matches!(b.get(*at), Some(b'e' | b'E')) {
        *at += 1;
        if matches!(b.get(*at), Some(b'+' | b'-')) {
            *at += 1;
        }
        digits(at).then_some(())?;
    }
    Some(())
}

#[cfg(test)]
mod tests;
