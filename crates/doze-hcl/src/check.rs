//! Go's syntax errors, found where Go finds them: the token checks of `LexConfig`, then a port of
//! hclsyntax's recursive-descent parser that builds no tree and keeps only what its diagnostics
//! need (ranges, the recovery flag, the newline stack). Every diagnostic comes out in Go's order,
//! with Go's range and summary, so a refused file reads the same as in Go.
//!
//! hcl-edit still builds the tree; it is asked only once this pass finds nothing wrong, so it
//! never sees broken nesting (its slow path on that is catalogue S6).

mod body;
mod cons;
mod expr;
mod forexpr;
mod strlit;
mod template;

use crate::diag::{Diagnostic, Diagnostics, Range};
use crate::lex::{Kind, Token};

pub(crate) type Ds = Vec<Diagnostic>;

/// Every syntax diagnostic Go gives for this config file, in Go's order. `file` names the source
/// in details that point elsewhere ("the if directive at x.hcl:1,6-16").
// fuzz:parser fuzz_check
pub fn check(src: &[u8], file: &str) -> Diagnostics {
    let toks = crate::lex::lex(src);
    let mut out = strlit::invalid_tokens(src, &toks);
    let mut p = Parser::new(src, &toks, file);
    out.extend(p.body(Kind::Eof));
    Diagnostics(out)
}

/// Go's `hcl.RangeBetween`: from the start of `a` to the end of `b`.
pub(crate) fn between(a: Range, b: Range) -> Range {
    Range {
        start: a.start,
        end: b.end,
    }
}

pub(crate) fn err(summary: &str, detail: impl Into<String>, at: Range) -> Diagnostic {
    Diagnostic::error(summary, detail, at)
}

/// Go's `Range.String()` with the file name, for details that point at another place.
pub fn place(file: &str, r: Range) -> String {
    if r.start.line == r.end.line {
        format!(
            "{file}:{},{}-{}",
            r.start.line, r.start.column, r.end.column
        )
    } else {
        format!(
            "{file}:{},{}-{},{}",
            r.start.line, r.start.column, r.end.line, r.end.column
        )
    }
}

/// The parser state Go's diagnostics depend on: the peeker (which skips comments, and newlines
/// unless the innermost context includes them) and the recovery flag, which once set silences
/// most later diagnostics.
pub(crate) struct Parser<'a> {
    src: &'a [u8],
    toks: &'a [Token],
    next: usize,
    newlines: Vec<bool>,
    recovery: bool,
    file: &'a str,
    /// Nesting depth, bounded so a hostile file cannot exhaust the stack.
    depth: usize,
}

/// Deeper than any real file; Go has no limit, and hcl-edit stops at 64 levels (catalogue S5).
const MAX_DEPTH: usize = 128;

impl<'a> Parser<'a> {
    fn new(src: &'a [u8], toks: &'a [Token], file: &'a str) -> Parser<'a> {
        Parser {
            src,
            toks,
            next: 0,
            newlines: vec![true],
            recovery: false,
            file,
            depth: 0,
        }
    }

    pub(crate) fn bytes(&self, t: &Token) -> &'a [u8] {
        t.bytes(self.src)
    }

    pub(crate) fn is_keyword(&self, t: &Token, kw: &str) -> bool {
        t.kind == Kind::Ident && self.bytes(t) == kw.as_bytes()
    }

    fn next_token(&self) -> (Token, usize) {
        let include = self.newlines.last().copied().unwrap_or(true);
        let mut i = self.next;
        while let Some(t) = self.toks.get(i) {
            match t.kind {
                Kind::Comment if include && self.bytes(t).ends_with(b"\n") => {
                    let nl = Token {
                        kind: Kind::Newline,
                        start: t.end - 1,
                        end: t.end,
                        range: t.range,
                    };
                    return (nl, i + 1);
                }
                Kind::Comment => {}
                Kind::Newline if !include => {}
                _ => return (*t, i + 1),
            }
            i += 1;
        }
        let last = self.toks.last().copied().unwrap_or(Token {
            kind: Kind::Eof,
            start: 0,
            end: 0,
            range: Range::default(),
        });
        (last, self.toks.len())
    }

    pub(crate) fn peek(&self) -> Token {
        self.next_token().0
    }

    pub(crate) fn read(&mut self) -> Token {
        let (t, i) = self.next_token();
        self.next = i;
        t
    }

    pub(crate) fn next_range(&self) -> Range {
        self.peek().range
    }

    pub(crate) fn prev_range(&self) -> Range {
        match self.next.checked_sub(1).and_then(|i| self.toks.get(i)) {
            Some(t) => t.range,
            None => self.next_range(),
        }
    }

    pub(crate) fn push_newlines(&mut self, include: bool) {
        self.newlines.push(include);
    }

    pub(crate) fn pop_newlines(&mut self) {
        self.newlines.pop();
    }

    pub(crate) fn set_recovery(&mut self) {
        self.recovery = true;
    }

    /// Go's `recover`: skips to the token after the `end` that closes the current bracket.
    pub(crate) fn recover(&mut self, end: Kind) -> Token {
        let start = opposite(end);
        self.recovery = true;
        let mut nest = 0usize;
        loop {
            let tok = self.read();
            let mut ty = tok.kind;
            if end == Kind::TemplateSeqEnd && ty == Kind::TemplateControl {
                ty = Kind::TemplateInterp;
            }
            if Some(ty) == start {
                nest += 1;
            } else if ty == end {
                if nest == 0 {
                    return tok;
                }
                nest -= 1;
            } else if ty == Kind::Eof {
                return tok;
            }
        }
    }

    /// Go's `recoverOver`: past the next bracketed group that `start` opens.
    pub(crate) fn recover_over(&mut self, start: Kind) {
        loop {
            let k = self.read().kind;
            if k == start || k == Kind::Eof {
                break;
            }
        }
        if let Some(end) = opposite(start) {
            self.recover(end);
        }
    }

    /// Go's `recoverAfterBodyItem`: to the newline that ends the current item.
    pub(crate) fn recover_after_body_item(&mut self) {
        self.recovery = true;
        let mut open: Vec<Kind> = Vec::new();
        loop {
            let tok = self.read();
            match tok.kind {
                Kind::Newline if open.is_empty() => return,
                Kind::Eof => return,
                k @ (Kind::OBrace
                | Kind::OBrack
                | Kind::OParen
                | Kind::OQuote
                | Kind::OHeredoc
                | Kind::TemplateInterp
                | Kind::TemplateControl) => open.push(k),
                k
                @ (Kind::CBrace | Kind::CBrack | Kind::CParen | Kind::CQuote | Kind::CHeredoc) => {
                    let opener = opposite(k);
                    while open.last().is_some_and(|o| Some(*o) != opener) {
                        open.pop();
                    }
                    open.pop();
                }
                Kind::TemplateSeqEnd => {
                    while open
                        .last()
                        .is_some_and(|o| *o != Kind::TemplateInterp && *o != Kind::TemplateControl)
                    {
                        open.pop();
                    }
                    open.pop();
                }
                _ => {}
            }
        }
    }

    /// Enters one nesting level; `false` (with a diagnostic) past the limit.
    pub(crate) fn enter(&mut self, ds: &mut Ds) -> bool {
        if self.depth >= MAX_DEPTH {
            if !self.recovery {
                ds.push(err(
                    "Nesting too deep",
                    "This expression nests more deeply than doze reads.",
                    self.next_range(),
                ));
            }
            self.recovery = true;
            return false;
        }
        self.depth += 1;
        true
    }

    pub(crate) fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }
}

/// Go's `oppositeBracket`.
pub(crate) fn opposite(k: Kind) -> Option<Kind> {
    Some(match k {
        Kind::OBrace => Kind::CBrace,
        Kind::OBrack => Kind::CBrack,
        Kind::OParen => Kind::CParen,
        Kind::OQuote => Kind::CQuote,
        Kind::OHeredoc => Kind::CHeredoc,
        Kind::CBrace => Kind::OBrace,
        Kind::CBrack => Kind::OBrack,
        Kind::CParen => Kind::OParen,
        Kind::CQuote => Kind::OQuote,
        Kind::CHeredoc => Kind::OHeredoc,
        Kind::TemplateControl | Kind::TemplateInterp => Kind::TemplateSeqEnd,
        Kind::TemplateSeqEnd => Kind::TemplateInterp,
        _ => return None,
    })
}
