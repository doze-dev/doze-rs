//! Go's HCL scanner (`hclsyntax.LexConfig`): the same tokens, at the same positions.
//!
//! hcl-edit parses into a tree for evaluation and editing; this token stream is what Go's parser
//! and formatter see, so it is what syntax errors are found on (`check`) and what `hclwrite`'s
//! formatting is reproduced from. It is a hand-written port of the Ragel machine in
//! `scan_tokens.rl`, longest match first, checked against Go's own tokens (`oracle-dump lex`).

mod chars;
mod ident_tables;
mod scan;

pub(crate) use chars::utf8_len as scan_utf8_len;

use crate::diag::{Pos, Range};

/// Go's `TokenType`, named as Go names them without the `Token` prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    OBrace,
    CBrace,
    OBrack,
    CBrack,
    OParen,
    CParen,
    OQuote,
    CQuote,
    OHeredoc,
    CHeredoc,
    Star,
    Slash,
    Plus,
    Minus,
    Percent,
    Equal,
    EqualOp,
    NotEqual,
    LessThan,
    LessThanEq,
    GreaterThan,
    GreaterThanEq,
    And,
    Or,
    Bang,
    Dot,
    Comma,
    DoubleColon,
    Ellipsis,
    FatArrow,
    Question,
    Colon,
    TemplateInterp,
    TemplateControl,
    TemplateSeqEnd,
    QuotedLit,
    StringLit,
    NumberLit,
    Ident,
    Comment,
    Newline,
    Eof,
    BitwiseAnd,
    BitwiseOr,
    BitwiseNot,
    BitwiseXor,
    Apostrophe,
    Backtick,
    Semicolon,
    Invalid,
    BadUtf8,
    QuotedNewline,
}

impl Kind {
    /// Go's name for the token type (`TokenIdent`, …), as `oracle-dump lex` writes it.
    pub fn go_name(self) -> String {
        let name = match self {
            Kind::Eof => "EOF",
            Kind::BadUtf8 => "BadUTF8",
            other => return format!("Token{other:?}"),
        };
        format!("Token{name}")
    }

    /// The single-character tokens Go's scanner emits as themselves (`selfToken`).
    fn of_byte(b: u8) -> Option<Kind> {
        Some(match b {
            b'[' => Kind::OBrack,
            b']' => Kind::CBrack,
            b'(' => Kind::OParen,
            b')' => Kind::CParen,
            b'.' => Kind::Dot,
            b',' => Kind::Comma,
            b'*' => Kind::Star,
            b'/' => Kind::Slash,
            b'%' => Kind::Percent,
            b'+' => Kind::Plus,
            b'-' => Kind::Minus,
            b'=' => Kind::Equal,
            b'<' => Kind::LessThan,
            b'>' => Kind::GreaterThan,
            b'!' => Kind::Bang,
            b'?' => Kind::Question,
            b':' => Kind::Colon,
            b'\n' => Kind::Newline,
            b'&' => Kind::BitwiseAnd,
            b'|' => Kind::BitwiseOr,
            b'~' => Kind::BitwiseNot,
            b'^' => Kind::BitwiseXor,
            b';' => Kind::Semicolon,
            b'`' => Kind::Backtick,
            b'\'' => Kind::Apostrophe,
            _ => return None,
        })
    }
}

/// One token: its kind, its bytes in the source and its range as Go computes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub kind: Kind,
    pub start: usize,
    pub end: usize,
    pub range: Range,
}

impl Token {
    pub fn bytes<'s>(&self, src: &'s [u8]) -> &'s [u8] {
        src.get(self.start..self.end).unwrap_or_default()
    }
}

/// The tokens of a config file, ending with `Eof`. A leading UTF-8 byte order mark is skipped
/// (its bytes still count in byte offsets, not in columns), as Go does.
// fuzz:parser fuzz_check
pub fn lex(src: &[u8]) -> Vec<Token> {
    let skip = if src.starts_with(&[0xef, 0xbb, 0xbf]) {
        3
    } else {
        0
    };
    scan::Scanner::new(src, skip).run()
}

/// Go's `tokenAccum`: positions advance token by token. Between tokens only spaces and tabs are
/// skipped, so the gap is counted in bytes; inside a token, columns count grapheme clusters and a
/// `\n` or `\r\n` cluster starts a new line.
#[derive(Debug)]
pub(crate) struct Accum {
    pos: Pos,
    tokens: Vec<Token>,
}

impl Accum {
    fn new(start: usize) -> Accum {
        let n = |v: usize| u32::try_from(v).unwrap_or(u32::MAX);
        Accum {
            pos: Pos {
                line: 1,
                column: 1,
                byte: n(start),
            },
            tokens: Vec::new(),
        }
    }

    fn emit(&mut self, src: &[u8], kind: Kind, start: usize, end: usize) {
        let n = |v: usize| u32::try_from(v).unwrap_or(u32::MAX);
        let mut s = self.pos;
        s.column = s.column.saturating_add(n(start).saturating_sub(s.byte));
        s.byte = n(start);
        let mut e = s;
        e.byte = n(end);
        let bytes = src.get(start..end).unwrap_or_default();
        advance(&mut e, bytes);
        self.pos = e;
        self.tokens.push(Token {
            kind,
            start,
            end,
            range: Range { start: s, end: e },
        });
    }
}

/// Moves `p` over `bytes` by grapheme clusters (each invalid byte is a cluster of its own).
pub(crate) fn advance(p: &mut Pos, bytes: &[u8]) {
    crate::funcs::graphemes::each(bytes, |g| {
        if g == b"\n" || g == b"\r\n" {
            p.line += 1;
            p.column = 1;
        } else {
            p.column += 1;
        }
    });
}
