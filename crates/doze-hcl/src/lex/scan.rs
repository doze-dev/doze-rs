//! The scanner's three machines (`main`, `stringTemplate`, `heredocTemplate` in `scan_tokens.rl`)
//! and the calls between them: a quote or heredoc opener calls a template machine, `${`/`%{` calls
//! `main`, and the brace that closes the sequence returns.

use super::chars::{
    char_at, find, heredoc_lit_len, ident_len, is_id_start, number_len, quoted_lit_len, utf8_len,
};
use super::{Accum, Kind, Token};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Main,
    Quoted,
    Heredoc,
}

#[derive(Debug)]
struct Heredoc {
    marker: Vec<u8>,
    start_of_line: bool,
}

pub(super) struct Scanner<'s> {
    data: &'s [u8],
    p: usize,
    out: Accum,
    modes: Vec<Mode>,
    braces: isize,
    ret_braces: Vec<isize>,
    heredocs: Vec<Heredoc>,
}

impl<'s> Scanner<'s> {
    pub(super) fn new(data: &'s [u8], skip: usize) -> Scanner<'s> {
        Scanner {
            data,
            p: skip,
            out: Accum::new(skip),
            modes: vec![Mode::Main],
            braces: 0,
            ret_braces: Vec::new(),
            heredocs: Vec::new(),
        }
    }

    pub(super) fn run(mut self) -> Vec<Token> {
        while self.p < self.data.len() {
            let mode = self.modes.last().copied().unwrap_or(Mode::Main);
            let ok = match mode {
                Mode::Main => {
                    self.main();
                    true
                }
                Mode::Quoted => {
                    self.quoted();
                    true
                }
                Mode::Heredoc => self.heredoc(),
            };
            if !ok {
                // The machine cannot match here: Go emits the rest as one invalid token and stops.
                self.emit(Kind::Invalid, self.p, self.data.len());
                break;
            }
        }
        let end = self.data.len();
        self.emit(Kind::Eof, end, end);
        self.out.tokens
    }

    fn at(&self, i: usize) -> Option<u8> {
        self.data.get(self.p + i).copied()
    }

    fn rest(&self) -> &'s [u8] {
        self.data.get(self.p..).unwrap_or_default()
    }

    fn emit(&mut self, kind: Kind, start: usize, end: usize) {
        self.out.emit(self.data, kind, start, end);
    }

    /// Emits `len` bytes from the current position and moves past them.
    fn take(&mut self, kind: Kind, len: usize) {
        let start = self.p;
        self.p += len;
        self.emit(kind, start, self.p);
    }

    fn call(&mut self, mode: Mode) {
        self.modes.push(mode);
    }

    fn ret(&mut self) {
        self.modes.pop();
        if self.modes.is_empty() {
            self.modes.push(Mode::Main);
        }
    }

    /// `${` or `%{`, with an optional `~`: a template sequence that scans `main` until its brace.
    fn begin_sequence(&mut self, kind: Kind) {
        let len = if self.at(2) == Some(b'~') { 3 } else { 2 };
        self.take(kind, len);
        self.braces += 1;
        self.ret_braces.push(self.braces);
        if let Some(h) = self.heredocs.last_mut() {
            h.start_of_line = false;
        }
        self.call(Mode::Main);
    }

    fn close_brace(&mut self, len: usize) {
        if self.ret_braces.last() == Some(&self.braces) {
            self.take(Kind::TemplateSeqEnd, len);
            self.braces -= 1;
            self.ret_braces.pop();
            self.ret();
        } else {
            let kind = if len == 2 {
                Kind::TemplateSeqEnd
            } else {
                Kind::CBrace
            };
            self.take(kind, len);
            self.braces -= 1;
        }
    }

    fn main(&mut self) {
        let Some(b) = self.at(0) else { return };
        let next = self.at(1);
        match (b, next) {
            (b' ' | b'\t', _) => {
                let n = self
                    .rest()
                    .iter()
                    .take_while(|c| **c == b' ' || **c == b'\t')
                    .count();
                self.p += n;
            }
            (b'0'..=b'9', _) => {
                let n = number_len(self.rest());
                self.take(Kind::NumberLit, n);
            }
            (b'#', _) | (b'/', Some(b'/')) => {
                let n = self
                    .rest()
                    .iter()
                    .position(|c| *c == b'\n')
                    .map_or(self.rest().len(), |i| i + 1);
                self.take(Kind::Comment, n);
            }
            (b'/', Some(b'*')) => match find(self.rest().get(2..).unwrap_or_default(), b"*/") {
                Some(i) => self.take(Kind::Comment, i + 4),
                None => self.take(Kind::Slash, 1),
            },
            (b'\r', Some(b'\n')) => self.take(Kind::Newline, 2),
            (b'=', Some(b'=')) => self.take(Kind::EqualOp, 2),
            (b'=', Some(b'>')) => self.take(Kind::FatArrow, 2),
            (b'!', Some(b'=')) => self.take(Kind::NotEqual, 2),
            (b'>', Some(b'=')) => self.take(Kind::GreaterThanEq, 2),
            (b'<', Some(b'=')) => self.take(Kind::LessThanEq, 2),
            (b'&', Some(b'&')) => self.take(Kind::And, 2),
            (b'|', Some(b'|')) => self.take(Kind::Or, 2),
            (b':', Some(b':')) => self.take(Kind::DoubleColon, 2),
            (b'.', Some(b'.')) if self.at(2) == Some(b'.') => self.take(Kind::Ellipsis, 3),
            (b'~', Some(b'}')) => self.close_brace(2),
            (b'{', _) => {
                self.take(Kind::OBrace, 1);
                self.braces += 1;
            }
            (b'}', _) => self.close_brace(1),
            (b'"', _) => {
                self.take(Kind::OQuote, 1);
                self.call(Mode::Quoted);
            }
            (b'<', Some(b'<')) if self.heredoc_opener().is_some() => self.begin_heredoc(),
            _ => self.main_other(b),
        }
    }

    fn main_other(&mut self, b: u8) {
        if let Some(kind) = Kind::of_byte(b) {
            return self.take(kind, 1);
        }
        match char_at(self.rest()) {
            Some((c, _)) if is_id_start(c) => {
                let n = ident_len(self.rest());
                self.take(Kind::Ident, n);
            }
            Some((_, n)) => self.take(Kind::Invalid, n),
            None => match utf8_len(self.rest()) {
                Some(n) => self.take(Kind::Invalid, n),
                None => self.take(Kind::BadUtf8, 1),
            },
        }
    }

    /// The length of `<<` `-`? Ident Newline at the current position, if that is what is here.
    fn heredoc_opener(&self) -> Option<usize> {
        let r = self.rest();
        let mut i = 2;
        if r.get(i) == Some(&b'-') {
            i += 1;
        }
        let (c, _) = char_at(r.get(i..)?)?;
        if !is_id_start(c) {
            return None;
        }
        i += ident_len(r.get(i..)?);
        match (r.get(i), r.get(i + 1)) {
            (Some(b'\n'), _) => Some(i + 1),
            (Some(b'\r'), Some(b'\n')) => Some(i + 2),
            _ => None,
        }
    }

    fn begin_heredoc(&mut self) {
        let Some(len) = self.heredoc_opener() else {
            return;
        };
        let intro = self.rest().get(..len).unwrap_or_default();
        let mut marker = intro.get(2..len - 1).unwrap_or_default();
        if let Some(m) = marker.strip_prefix(b"-") {
            marker = m;
        }
        if let Some(m) = marker.strip_suffix(b"\r") {
            marker = m;
        }
        self.heredocs.push(Heredoc {
            marker: marker.to_vec(),
            start_of_line: true,
        });
        self.take(Kind::OHeredoc, len);
        self.call(Mode::Heredoc);
    }

    /// `$` or `%` in a template: a sequence opener, an escaped one (`$${`), or a literal `$`.
    /// `None` when it is the last byte (nothing can be decided).
    fn dollar(&self, sigil: u8) -> Option<(Kind, usize)> {
        let opener = if sigil == b'$' {
            Kind::TemplateInterp
        } else {
            Kind::TemplateControl
        };
        match (self.at(1)?, self.at(2), self.at(3)) {
            (b'{', _, _) => Some((opener, 0)),
            (s, Some(b'{'), t) if s == sigil => {
                Some((Kind::QuotedLit, 3 + usize::from(t == Some(b'~'))))
            }
            _ => Some((Kind::QuotedLit, 1)),
        }
    }

    fn quoted(&mut self) {
        let Some(b) = self.at(0) else { return };
        match b {
            b'$' | b'%' => match self.dollar(b) {
                Some((kind, 0)) => self.begin_sequence(kind),
                Some((_, n)) => self.take(Kind::QuotedLit, n),
                None => self.take(Kind::Invalid, 1),
            },
            b'"' => {
                self.take(Kind::CQuote, 1);
                self.ret();
            }
            b'\r' | b'\n' => {
                let n = self
                    .rest()
                    .iter()
                    .take_while(|c| **c == b'\r' || **c == b'\n')
                    .count();
                self.take(Kind::QuotedNewline, n);
            }
            _ => match quoted_lit_len(self.rest()) {
                0 => match utf8_len(self.rest()) {
                    Some(n) => self.take(Kind::Invalid, n),
                    None => self.take(Kind::BadUtf8, 1),
                },
                n => self.take(Kind::QuotedLit, n),
            },
        }
    }

    /// One heredoc token. `false` when the machine has no match here (a lone `\r`, or a `$`/`%`
    /// at the very end).
    fn heredoc(&mut self) -> bool {
        let Some(b) = self.at(0) else { return true };
        if b == b'$' || b == b'%' {
            match self.dollar(b) {
                Some((kind, 0)) => self.begin_sequence(kind),
                Some((_, 1)) => {
                    // Ragel's held character: `$` and the byte after it are one token when a line
                    // break follows them, otherwise `$` stands alone.
                    let n = if matches!(self.at(2), Some(b'\n' | b'\r')) {
                        2
                    } else {
                        1
                    };
                    self.heredoc_lit(n, false);
                }
                Some((_, n)) => match (self.at(n), self.at(n + 1)) {
                    (Some(b'\n'), _) => self.heredoc_lit(n + 1, true),
                    (Some(b'\r'), Some(b'\n')) => self.heredoc_lit(n + 2, true),
                    _ => self.heredoc_lit(n, false),
                },
                None => return false,
            }
            return true;
        }
        let r = self.rest();
        let n = heredoc_lit_len(r);
        match (r.get(n), r.get(n + 1)) {
            (Some(b'\n'), _) => self.heredoc_lit(n + 1, true),
            (Some(b'\r'), Some(b'\n')) => self.heredoc_lit(n + 2, true),
            _ if n > 0 => self.heredoc_lit(n, false),
            (Some(b'\r'), _) => return false,
            (Some(_), _) => self.take(Kind::BadUtf8, 1),
            (None, _) => {}
        }
        true
    }

    /// A heredoc literal of `len` bytes; one that ends a line may be the closing marker.
    fn heredoc_lit(&mut self, len: usize, eol: bool) {
        let start = self.p;
        let Some(top) = self.heredocs.last_mut() else {
            return self.take(Kind::StringLit, len);
        };
        if !eol {
            top.start_of_line = false;
            return self.take(Kind::StringLit, len);
        }
        let line = self.data.get(start..start + len).unwrap_or_default();
        let is_marker = top.start_of_line
            && std::str::from_utf8(line).is_ok_and(|s| s.trim().as_bytes() == top.marker);
        if !is_marker {
            top.start_of_line = true;
            return self.take(Kind::StringLit, len);
        }
        let nl = if line.ends_with(b"\r\n") { 2 } else { 1 };
        self.emit(Kind::CHeredoc, start, start + len - nl);
        self.emit(Kind::Newline, start + len - nl, start + len);
        self.p = start + len;
        self.heredocs.pop();
        self.ret();
    }
}
