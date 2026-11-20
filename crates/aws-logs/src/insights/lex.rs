//! The tokens of a Logs Insights query, read on demand: whether `/` begins a regular expression or divides depends on where the parser
//! is, so the parser asks for a token at a position and says which it expects.

use super::QueryError;

/// One token and where it ends.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Tok {
    /// A name: a command, a keyword, a function or a field (`@timestamp`, `a.b.c`, or anything between backticks).
    Ident(String),
    /// A name written between backticks: always a field, never a keyword.
    Quoted(String),
    Str(String),
    Num(f64),
    /// A length of time (`5m`, `1h`, `30s`): the number and the unit.
    Period(f64, String),
    /// `|`, `,`, `(`, `)`, `[`, `]` and the operators.
    Sym(&'static str),
    End,
}

const SYMBOLS: [&str; 19] = [
    "!=", "<=", ">=", "=~", "|", ",", "(", ")", "[", "]", "=", "<", ">", "+", "-", "*", "/", "%",
    "^",
];

/// The units a period may be written in (memory: Insights' `bin()` documentation lists these).
const UNITS: [&str; 15] = [
    "ms", "msec", "s", "sec", "m", "min", "h", "hr", "d", "w", "mo", "mon", "q", "y", "yr",
];

/// The query text and a position in it.
#[derive(Debug)]
pub(crate) struct Lexer<'a> {
    pub(crate) src: &'a str,
    pub(crate) pos: usize,
}

impl<'a> Lexer<'a> {
    pub(crate) const fn new(src: &'a str) -> Self {
        Self { src, pos: 0 }
    }

    fn rest(&self) -> &'a str {
        self.src.get(self.pos..).unwrap_or_default()
    }

    /// Moves past white space and `#` comments.
    pub(crate) fn skip(&mut self) {
        loop {
            let rest = self.rest();
            let trimmed = rest.trim_start();
            self.pos += rest.len() - trimmed.len();
            if trimmed.starts_with('#') {
                let line = trimmed.find('\n').unwrap_or(trimmed.len());
                self.pos += line;
            } else {
                return;
            }
        }
    }

    /// The next character after white space, without moving.
    pub(crate) fn peek_char(&mut self) -> Option<char> {
        self.skip();
        self.rest().chars().next()
    }

    /// The next token and the position after it, without moving.
    pub(crate) fn peek(&mut self) -> Result<(Tok, usize), QueryError> {
        self.skip();
        let start = self.pos;
        let rest = self.rest();
        let Some(c) = rest.chars().next() else {
            return Ok((Tok::End, start));
        };
        if c == '\'' || c == '"' {
            let (text, len) = quoted(rest, c).ok_or_else(|| self.error("unterminated string"))?;
            return Ok((Tok::Str(text), start + len));
        }
        if c == '`' {
            let end = rest
                .get(1..)
                .and_then(|r| r.find('`'))
                .ok_or_else(|| self.error("unterminated field name"))?;
            let name = rest.get(1..=end).unwrap_or_default().to_owned();
            return Ok((Tok::Quoted(name), start + end + 2));
        }
        if c.is_ascii_digit()
            || (c == '.' && rest.chars().nth(1).is_some_and(|d| d.is_ascii_digit()))
        {
            return Ok(number(rest, start));
        }
        if is_name(c) {
            let len = rest.find(|ch: char| !is_name(ch)).unwrap_or(rest.len());
            let word = rest.get(..len).unwrap_or_default().to_owned();
            return Ok((Tok::Ident(word), start + len));
        }
        SYMBOLS
            .iter()
            .find(|s| rest.starts_with(**s))
            .map(|s| (Tok::Sym(s), start + s.len()))
            .ok_or_else(|| self.error(&format!("unexpected symbol '{c}'")))
    }

    /// Takes the next token.
    pub(crate) fn next(&mut self) -> Result<Tok, QueryError> {
        let (tok, end) = self.peek()?;
        self.pos = end;
        Ok(tok)
    }

    /// Takes `sym` if it is next.
    pub(crate) fn eat(&mut self, sym: &str) -> Result<bool, QueryError> {
        match self.peek()? {
            (Tok::Sym(s), end) if s == sym => {
                self.pos = end;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Takes the keyword `word` (any case) if it is next.
    pub(crate) fn eat_word(&mut self, word: &str) -> Result<bool, QueryError> {
        match self.peek()? {
            (Tok::Ident(w), end) if w.eq_ignore_ascii_case(word) => {
                self.pos = end;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Requires `sym` next.
    pub(crate) fn expect(&mut self, sym: &str) -> Result<(), QueryError> {
        if self.eat(sym)? {
            return Ok(());
        }
        Err(self.error(&format!("expected '{sym}'")))
    }

    /// A regular expression between slashes at the position (the parser has seen the `/`): its text, unescaped `\/`.
    pub(crate) fn regex(&mut self) -> Result<String, QueryError> {
        self.skip();
        let rest = self.rest();
        let body = rest.get(1..).unwrap_or_default();
        let mut out = String::new();
        let mut chars = body.char_indices();
        while let Some((i, c)) = chars.next() {
            match c {
                '\\' => match chars.next() {
                    Some((_, '/')) => out.push('/'),
                    Some((_, other)) => {
                        out.push('\\');
                        out.push(other);
                    }
                    None => break,
                },
                '/' => {
                    self.pos += i + 2;
                    return Ok(out);
                }
                _ => out.push(c),
            }
        }
        Err(self.error("unterminated regular expression"))
    }

    /// A refusal at the current position.
    pub(crate) fn error(&self, what: &str) -> QueryError {
        QueryError::at(self.pos, what)
    }
}

/// A character that may be part of a name: field names hold `@`, `.` and `$` (`@timestamp`, `a.b`, `$.x` is not one, but
/// `@ptr` and `a.0.b` are).
fn is_name(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '@' | '.' | '$')
}

/// A number, or a period when a unit follows it at once (`5m`).
fn number(rest: &str, start: usize) -> (Tok, usize) {
    let len = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(rest.len());
    let digits = rest.get(..len).unwrap_or_default();
    let value: f64 = digits.parse().unwrap_or(0.0);
    let after = rest.get(len..).unwrap_or_default();
    let word = after
        .find(|c: char| !c.is_ascii_alphanumeric())
        .unwrap_or(after.len());
    let unit = after.get(..word).unwrap_or_default();
    if !unit.is_empty() && UNITS.contains(&unit.to_ascii_lowercase().as_str()) {
        return (
            Tok::Period(value, unit.to_ascii_lowercase()),
            start + len + word,
        );
    }
    (Tok::Num(value), start + len)
}

/// A string between `quote`s with backslash escapes, and the bytes it took.
fn quoted(rest: &str, quote: char) -> Option<(String, usize)> {
    let mut out = String::new();
    let mut chars = rest.char_indices().skip(1);
    while let Some((i, c)) = chars.next() {
        if c == '\\' {
            let (_, e) = chars.next()?;
            match e {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                other if other == quote || other == '\\' => out.push(other),
                other => {
                    out.push('\\');
                    out.push(other);
                }
            }
        } else if c == quote {
            return Some((out, i + c.len_utf8()));
        } else {
            out.push(c);
        }
    }
    None
}
