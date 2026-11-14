//! Printing as Go does for `format`: `fmt` with `*big.Int` and `*big.Float` operands (their
//! `Format` methods), cty's own padding by grapheme clusters, and `encoding/json` strings.

use super::format::Verb;
use crate::number::Number;
use num_bigint::BigUint;
use std::fmt::Write;

/// `(*big.Int).Format` for `b d o x X`: sign, `#` prefix, precision as a minimum digit count, width.
pub(super) fn int(v: &Verb, neg: bool, mag: &BigUint) -> String {
    let radix = match v.mode {
        'b' => 2,
        'o' => 8,
        'x' | 'X' => 16,
        _ => 10,
    };
    let mut digits = mag.to_str_radix(radix);
    if v.mode == 'X' {
        digits.make_ascii_uppercase();
    }
    let sign = match () {
        _ if neg => "-",
        _ if v.plus => "+",
        _ if v.space => " ",
        _ => "",
    };
    let prefix = match (v.sharp, v.mode) {
        (true, 'b') => "0b",
        (true, 'o') => "0",
        (true, 'x') => "0x",
        (true, 'X') => "0X",
        _ => "",
    };
    let mut zeros = 0;
    if let Some(p) = v.prec {
        if digits.len() < p {
            zeros = p - digits.len();
        } else if digits == "0" && p == 0 {
            return String::new();
        }
    }
    let (mut left, mut right) = (0, 0);
    let len = sign.len() + prefix.len() + zeros + digits.len();
    if let Some(w) = v.width
        && len < w
    {
        let d = w - len;
        match () {
            _ if v.minus => right = d,
            _ if v.zero && v.prec.is_none() => zeros = d,
            _ => left = d,
        }
    }
    format!(
        "{}{sign}{prefix}{}{digits}{}",
        " ".repeat(left),
        "0".repeat(zeros),
        " ".repeat(right)
    )
}

/// `(*big.Float).Format` for `e E f g G`: precision 6 for `e` and `f` and shortest for `g` unless
/// given, the sign, then zero padding (which wins over `-`, and never pads an infinity), left or
/// right padding.
pub(super) fn float(v: &Verb, n: &Number) -> String {
    let prec = match (v.prec, v.mode) {
        (Some(p), _) => i64::try_from(p).unwrap_or(i64::MAX),
        (None, 'g' | 'G') => -1,
        (None, _) => 6,
    };
    let text = n.text_with(u8::try_from(v.mode).unwrap_or(b'g'), prec);
    let (sign, body) = match text.as_bytes().first() {
        Some(b'-') => ("-", text.get(1..).unwrap_or("")),
        Some(b'+') if v.space => (" ", text.get(1..).unwrap_or("")),
        Some(b'+') => ("+", text.get(1..).unwrap_or("")),
        _ if v.plus => ("+", text.as_str()),
        _ if v.space => (" ", text.as_str()),
        _ => ("", text.as_str()),
    };
    let padding = v
        .width
        .map_or(0, |w| w.saturating_sub(sign.len() + body.len()));
    if v.zero && !n.is_inf() {
        format!("{sign}{}{body}", "0".repeat(padding))
    } else if v.minus {
        format!("{sign}{body}{}", " ".repeat(padding))
    } else {
        format!("{}{sign}{body}", " ".repeat(padding))
    }
}

/// cty's `formatPadWidth`: pads to the width in grapheme clusters, with zeros if asked.
pub(super) fn pad(v: &Verb, s: &str) -> String {
    let Some(w) = v.width else {
        return s.to_owned();
    };
    let have = super::graphemes::count(s);
    if have >= w {
        return s.to_owned();
    }
    let pads = if v.zero { "0" } else { " " }.repeat(w - have);
    if v.minus {
        format!("{s}{pads}")
    } else {
        format!("{pads}{s}")
    }
}

/// A JSON string as Go's `encoding/json` writes it: HTML-safe (`<`, `>`, `&` escaped), U+2028 and
/// U+2029 escaped, `\b` and `\f` by name.
pub(super) fn json_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if u32::from(c) < 0x20 || matches!(c, '<' | '>' | '&' | '\u{2028}' | '\u{2029}') => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}
