//! String quoting: Go's `strconv.Quote` (cty hashes set elements with it) and JSON strings.

use std::fmt::Write;

/// Go's `strconv.Quote`: printable runes as they are, the usual backslash escapes, `\xNN`, `\uNNNN`
/// or `\UNNNNNNNN` for the rest.
pub fn go_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{7}' => out.push_str("\\a"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{b}' => out.push_str("\\v"),
            c if is_go_printable(c) => out.push(c),
            c if u32::from(c) < 0x80 => {
                let _ = write!(out, "\\x{:02x}", u32::from(c));
            }
            c if u32::from(c) < 0x10000 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => {
                let _ = write!(out, "\\U{:08x}", u32::from(c));
            }
        }
    }
    out.push('"');
    out
}

/// Go's `unicode.IsPrint`, closely: controls, separators other than the space, format characters,
/// surrogates' neighbours and non-characters are not printable. Rust's standard library has no
/// general-category tables; the ranges below cover what occurs in text.
pub(crate) fn is_go_printable(c: char) -> bool {
    if c == ' ' {
        return true;
    }
    if c.is_control() || c.is_whitespace() {
        return false;
    }
    let u = u32::from(c);
    !matches!(u,
        0x00AD | 0x0600..=0x0605 | 0x061C | 0x06DD | 0x070F | 0x180E | 0x200B..=0x200F | 0x202A..=0x202E
        | 0x2060..=0x2064 | 0x2066..=0x206F | 0xFEFF | 0xFFF9..=0xFFFB | 0xFFFE | 0xFFFF
        | 0xE000..=0xF8FF | 0xF0000..=0x10FFFF | 0xE0001 | 0xE0020..=0xE007F)
}

/// A JSON string literal.
pub(crate) fn json_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}
