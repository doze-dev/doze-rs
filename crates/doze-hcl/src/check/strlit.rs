//! Token-level diagnostics: characters HCL never uses (`checkInvalidTokens`) and escape sequences
//! in quoted literals (`ParseStringLiteralToken` over `scanStringLit`'s slices).

use super::{Ds, err};
use crate::diag::{Pos, Range};
use crate::lex::scan_utf8_len;
use crate::lex::{Kind, Token};

/// `checkInvalidTokens`, with Go's limits on how often each kind is told.
pub(crate) fn invalid_tokens(src: &[u8], toks: &[Token]) -> Ds {
    let mut ds = Ds::new();
    let (mut bitwise, mut backtick, mut apostrophe, mut semicolon, mut bad_utf8) = (0, 0, 0, 0, 0);
    for t in toks {
        let at = t.range;
        match t.kind {
            Kind::BitwiseAnd | Kind::BitwiseOr | Kind::BitwiseXor | Kind::BitwiseNot if bitwise < 4 => {
                let hint = match t.kind {
                    Kind::BitwiseAnd => " Did you mean boolean AND (\"&&\")?",
                    Kind::BitwiseOr => " Did you mean boolean OR (\"||\")?",
                    Kind::BitwiseNot => " Did you mean boolean NOT (\"!\")?",
                    _ => "",
                };
                ds.push(err("Unsupported operator", format!("Bitwise operators are not supported.{hint}"), at));
                bitwise += 1;
            }
            Kind::Backtick => {
                if backtick % 2 == 0 {
                    ds.push(err("Invalid character", "The \"`\" character is not valid. To create a multi-line string, use the \"heredoc\" syntax, like \"<<EOT\".", at));
                }
                if backtick <= 2 {
                    backtick += 1;
                }
            }
            Kind::Apostrophe => {
                if apostrophe % 2 == 0 {
                    ds.push(err("Invalid character", "Single quotes are not valid. Use double quotes (\") to enclose strings.", at));
                }
                if apostrophe <= 2 {
                    apostrophe += 1;
                }
            }
            Kind::Semicolon if semicolon < 1 => {
                ds.push(err("Invalid character", "The \";\" character is not valid. Use newlines to separate arguments and blocks, and commas to separate items in collection values.", at));
                semicolon += 1;
            }
            Kind::BadUtf8 if bad_utf8 < 1 => {
                ds.push(err("Invalid character encoding", "All input files must be UTF-8 encoded. Ensure that UTF-8 encoding is selected in your editor.", at));
                bad_utf8 += 1;
            }
            Kind::QuotedNewline => ds.push(err("Invalid multi-line string", "Quoted strings may not be split over multiple lines. To produce a multi-line string, either use the \\n escape to represent a newline character or use the \"heredoc\" multi-line template syntax.", at)),
            Kind::Invalid => {
                let curly = [&"\u{201c}", &"\u{201d}"].iter().any(|q| t.bytes(src) == q.as_bytes());
                ds.push(err("Invalid character", if curly {
                    "\"Curly quotes\" are not valid here. These can sometimes be inadvertently introduced when sharing code via documents or discussion forums. It might help to replace the character with a \"straight quote\"."
                } else {
                    "This character is not used within the language."
                }, at));
            }
            _ => {}
        }
    }
    ds
}

/// The diagnostics for the escapes in one literal token (only a quoted one can have any).
pub(crate) fn escapes(bytes: &[u8], at: Range, quoted: bool) -> Ds {
    let mut ds = Ds::new();
    if !quoted || !bytes.contains(&b'\\') {
        return ds;
    }
    let mut end = at.start;
    for (s, e) in slices(bytes) {
        let slice = bytes.get(s..e).unwrap_or_default();
        let start = end;
        advance(&mut end, slice);
        if slice.first() == Some(&b'\\')
            && let Some(detail) = bad_escape(slice)
        {
            ds.push(err("Invalid escape sequence", detail, Range { start, end }));
        }
    }
    ds
}

fn bad_escape(slice: &[u8]) -> Option<String> {
    let Some(sel) = slice.get(1) else {
        return Some("Backslash must be followed by an escape sequence selector character.".into());
    };
    let hex = |n: usize, word: &str| -> Option<String> {
        if slice.len() != n + 2 {
            return Some(format!(
                "The \\{} escape sequence must be followed by {word} hexadecimal digits.",
                char::from(*sel)
            ));
        }
        let text = std::str::from_utf8(slice.get(2..)?).ok()?;
        let v = u32::from_str_radix(text, 16).ok()?;
        char::from_u32(v)
            .is_none()
            .then(|| format!("Cannot encode character U+{v:04x} in UTF-8."))
    };
    match sel {
        b'n' | b'r' | b't' | b'"' | b'\\' => None,
        b'u' => hex(4, "four"),
        b'U' => hex(8, "eight"),
        _ => {
            let sym = String::from_utf8_lossy(slice.get(1..).unwrap_or_default());
            Some(format!(
                "The symbol {} is not a valid escape sequence selector.",
                crate::value::quote::go_quote(&sym)
            ))
        }
    }
}

/// `scanStringLit` in quoted mode: escapes, `$$`/`%%` sequences and line breaks are slices of their
/// own, and the literal text between them is one slice each.
fn slices(b: &[u8]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let (mut i, mut lit) = (0, 0);
    while let Some(&c) = b.get(i) {
        let at = |k: usize| b.get(i + k).copied();
        let n = match c {
            b'\\' => match at(1) {
                Some(u @ (b'u' | b'U')) => {
                    let max = if u == b'u' { 4 } else { 8 };
                    2 + b
                        .get(i + 2..)
                        .unwrap_or_default()
                        .iter()
                        .take(max)
                        .take_while(|h| h.is_ascii_hexdigit())
                        .count()
                }
                Some(_) => 1 + b.get(i + 1..).and_then(scan_utf8_len).unwrap_or(0),
                None => 1,
            },
            b'$' | b'%' if at(1) == Some(c) => 2 + usize::from(at(2) == Some(b'{')),
            b'$' | b'%' => 1,
            b'\r' => 1 + usize::from(at(1) == Some(b'\n')),
            b'\n' => 1,
            _ => 0,
        };
        if n == 0 {
            i += 1;
            continue;
        }
        if lit < i {
            out.push((lit, i));
        }
        out.push((i, i + n));
        i += n;
        lit = i;
    }
    if lit < b.len() {
        out.push((lit, b.len()));
    }
    out
}

/// `ParseStringLiteralToken`'s position walk: grapheme clusters, and a cluster that starts with a
/// line break starts a new line.
fn advance(p: &mut Pos, slice: &[u8]) {
    crate::funcs::graphemes::each(slice, |g| {
        p.byte += u32::try_from(g.len()).unwrap_or(u32::MAX);
        if g.starts_with(b"\r") || g.starts_with(b"\n") {
            p.line += 1;
            p.column = 1;
        } else {
            p.column += 1;
        }
    });
}
