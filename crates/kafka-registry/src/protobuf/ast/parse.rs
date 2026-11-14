//! The parser: a recursive descent over the tokens, one method per declaration. Statements of the file and options are
//! here; message bodies, enums, extends and services in [`body`].

use super::super::Error;
use super::super::lex::{Tok, Token, lex, unquote};
use super::{Decl, File, Opt, Span, Val, int_value};

/// Message bodies, enums, extends and services.
mod body;

/// The most deeply messages, groups and option values may nest. Deeper text is refused, so a hostile schema cannot
/// exhaust the stack.
const MAX_DEPTH: u32 = 100;

/// Reads `.proto` text.
///
/// # Errors
/// The text is not in the language's grammar.
pub(in super::super) fn parse(src: &str) -> Result<File, Error> {
    let mut p = Parser {
        src,
        toks: lex(src)?,
        i: 0,
        depth: 0,
    };
    p.file()
}

pub(super) struct Parser<'a> {
    src: &'a str,
    toks: Vec<Token>,
    i: usize,
    depth: u32,
}

impl Parser<'_> {
    pub(super) fn peek(&self, k: usize) -> Option<Token> {
        self.toks.get(self.i + k).copied()
    }

    pub(super) fn text(&self, t: Token) -> &str {
        self.src
            .get(t.start as usize..t.end as usize)
            .unwrap_or_default()
    }

    /// Where the next token starts (the end of the text when there is none).
    pub(super) fn at(&self) -> u32 {
        self.peek(0).map_or_else(
            || u32::try_from(self.src.len()).unwrap_or(u32::MAX),
            |t| t.start,
        )
    }

    /// Where the last token read ends.
    pub(super) fn end(&self) -> u32 {
        self.i
            .checked_sub(1)
            .and_then(|i| self.toks.get(i))
            .map_or(0, |t| t.end)
    }

    pub(super) fn err<T>(&self, what: &str) -> Result<T, Error> {
        let found = self.peek(0).map_or("end of input", |t| self.text(t));
        Err(Error::syntax(
            self.at(),
            &format!("expected {what}, found \"{found}\""),
        ))
    }

    pub(super) fn is(&self, k: usize, c: u8) -> bool {
        self.peek(k).is_some_and(|t| t.kind == Tok::Punct(c))
    }

    pub(super) fn is_word(&self, k: usize, w: &str) -> bool {
        self.peek(k)
            .is_some_and(|t| t.kind == Tok::Ident && self.text(t) == w)
    }

    pub(super) fn eat(&mut self, c: u8) -> bool {
        let yes = self.is(0, c);
        self.i += usize::from(yes);
        yes
    }

    pub(super) fn expect(&mut self, c: u8) -> Result<(), Error> {
        if self.eat(c) {
            Ok(())
        } else {
            self.err(&format!("\"{}\"", char::from(c)))
        }
    }

    pub(super) fn ident(&mut self) -> Result<String, Error> {
        match self.peek(0) {
            Some(t) if t.kind == Tok::Ident => {
                self.i += 1;
                Ok(self.text(t).to_owned())
            }
            _ => self.err("a name"),
        }
    }

    /// `a.b.c`, with a leading dot when `dot` allows it; the name and its source.
    pub(super) fn dotted(&mut self, dot: bool) -> Result<(String, Span), Error> {
        let start = self.at();
        let mut name = String::new();
        if dot && self.eat(b'.') {
            name.push('.');
        }
        name.push_str(&self.ident()?);
        while self.is(0, b'.') {
            self.i += 1;
            name.push('.');
            name.push_str(&self.ident()?);
        }
        Ok((name, (start, self.end())))
    }

    /// One or more adjacent string literals, joined.
    pub(super) fn string(&mut self) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        let mut any = false;
        while let Some(t) = self.peek(0).filter(|t| t.kind == Tok::Str) {
            out.extend(unquote(self.text(t), t.start)?);
            self.i += 1;
            any = true;
        }
        if any { Ok(out) } else { self.err("a string") }
    }

    pub(super) fn utf8(&mut self) -> Result<String, Error> {
        let at = self.at();
        String::from_utf8(self.string()?).map_err(|_| Error::syntax(at, "string is not UTF-8"))
    }

    /// An integer, with a minus sign when `neg` allows it.
    pub(super) fn int(&mut self, neg: bool) -> Result<(i64, Span), Error> {
        let start = self.at();
        let minus = neg && self.eat(b'-');
        let Some(t) = self.peek(0).filter(|t| t.kind == Tok::Int) else {
            return self.err("an integer");
        };
        self.i += 1;
        let v = int_value(self.text(t))
            .and_then(|v| i64::try_from(v).ok())
            .ok_or_else(|| Error::syntax(t.start, "integer out of range"))?;
        Ok((if minus { -v } else { v }, (start, t.end)))
    }

    pub(super) fn end_statement(&mut self) -> Result<(), Error> {
        self.expect(b';')
    }

    pub(super) fn enter(&mut self) -> Result<(), Error> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(Error::syntax(self.at(), "nested too deeply"));
        }
        Ok(())
    }

    pub(super) fn file(&mut self) -> Result<File, Error> {
        let mut file = File {
            syntax: None,
            decls: Vec::new(),
        };
        if self.is_word(0, "edition") {
            return Err(Error::syntax(self.at(), "editions are not supported"));
        }
        if self.is_word(0, "syntax") {
            self.i += 1;
            self.expect(b'=')?;
            let start = self.at();
            let v = self.utf8()?;
            let span = (start, self.end());
            if v != "proto2" && v != "proto3" {
                return Err(Error::syntax(
                    start,
                    &format!("syntax value must be \"proto2\" or \"proto3\", not \"{v}\""),
                ));
            }
            file.syntax = Some((span, v));
            self.end_statement()?;
        }
        while self.peek(0).is_some() {
            if !self.eat(b';') {
                let d = self.file_decl(&file)?;
                file.decls.push(d);
            }
        }
        Ok(file)
    }

    pub(super) fn file_decl(&mut self, file: &File) -> Result<Decl, Error> {
        let at = self.at();
        let word = self
            .peek(0)
            .filter(|t| t.kind == Tok::Ident)
            .map(|t| self.text(t).to_owned());
        match word.as_deref() {
            Some("package") => {
                if file.decls.iter().any(|d| matches!(d, Decl::Package(..))) {
                    return Err(Error::syntax(at, "a file has at most one package"));
                }
                self.i += 1;
                let (name, span) = self.dotted(false)?;
                self.end_statement()?;
                Ok(Decl::Package(name, span))
            }
            Some("import") => {
                self.i += 1;
                let mut kind = "";
                if self.is_word(0, "public") || self.is_word(0, "weak") {
                    kind = if self.is_word(0, "public") {
                        "public"
                    } else {
                        "weak"
                    };
                    self.i += 1;
                }
                let start = self.at();
                let path = self.utf8()?;
                let span = (start, self.end());
                self.end_statement()?;
                Ok(Decl::Import(kind, path, span))
            }
            Some("option") => Ok(Decl::Option(self.option_statement()?)),
            Some("message") => Ok(Decl::Message(self.message()?)),
            Some("enum") => Ok(Decl::Enum(self.enumeration()?)),
            Some("extend") => Ok(Decl::Extend(self.extend()?)),
            Some("service") => Ok(Decl::Service(self.service()?)),
            Some("syntax" | "edition") => Err(Error::syntax(at, "syntax must come first")),
            _ => self.err("a declaration"),
        }
    }

    pub(super) fn option_statement(&mut self) -> Result<Opt, Error> {
        self.i += 1;
        let o = self.option()?;
        self.end_statement()?;
        Ok(o)
    }

    /// `name = value`.
    pub(super) fn option(&mut self) -> Result<Opt, Error> {
        let at = self.at();
        let mut name = Vec::new();
        loop {
            if self.eat(b'(') {
                let (n, _) = self.dotted(true)?;
                self.expect(b')')?;
                name.push((n, true));
            } else {
                name.push((self.ident()?, false));
            }
            if !self.eat(b'.') {
                break;
            }
        }
        let name_span = (at, self.end());
        self.expect(b'=')?;
        let start = self.at();
        let val = self.value()?;
        Ok(Opt {
            name,
            name_span,
            val,
            val_span: (start, self.end()),
            at,
        })
    }

    pub(super) fn value(&mut self) -> Result<Val, Error> {
        let Some(t) = self.peek(0) else {
            return self.err("a value");
        };
        match t.kind {
            Tok::Str => {
                self.string()?;
                Ok(Val::Str)
            }
            Tok::Ident => {
                self.i += 1;
                Ok(Val::Ident(self.text(t).to_owned()))
            }
            Tok::Int | Tok::Float => {
                self.i += 1;
                Ok(Val::Num(self.text(t).to_owned(), t.kind == Tok::Float))
            }
            Tok::Punct(c @ (b'-' | b'+')) => {
                self.i += 1;
                let n = self
                    .peek(0)
                    .filter(|n| n.kind != Tok::Str && n.kind != Tok::Punct(b'{'));
                let Some(n) =
                    n.filter(|n| n.kind != Tok::Ident || matches!(self.text(*n), "inf" | "nan"))
                else {
                    return self.err("a number");
                };
                self.i += 1;
                let sign = if c == b'-' { "-" } else { "" };
                Ok(Val::Num(
                    format!("{sign}{}", self.text(n)),
                    n.kind != Tok::Int,
                ))
            }
            Tok::Punct(b'{') => {
                self.skip_aggregate()?;
                Ok(Val::Aggregate)
            }
            Tok::Punct(_) => self.err("a value"),
        }
    }

    /// A message literal, `{ ... }`, skipped with its brackets balanced.
    pub(super) fn skip_aggregate(&mut self) -> Result<(), Error> {
        let mut stack = Vec::new();
        loop {
            let Some(t) = self.peek(0) else {
                return self.err("\"}\"");
            };
            self.i += 1;
            match t.kind {
                Tok::Punct(c @ (b'{' | b'[' | b'<')) => {
                    if stack.len() as u32 >= MAX_DEPTH {
                        return Err(Error::syntax(t.start, "nested too deeply"));
                    }
                    stack.push(match c {
                        b'{' => b'}',
                        b'[' => b']',
                        _ => b'>',
                    });
                }
                Tok::Punct(c @ (b'}' | b']' | b'>')) => {
                    if stack.pop() != Some(c) {
                        return Err(Error::syntax(t.start, "unbalanced message literal"));
                    }
                    if stack.is_empty() {
                        return Ok(());
                    }
                }
                _ => {}
            }
        }
    }

    /// `[a = 1, b = 2]` after a field, or nothing.
    pub(super) fn compact_options(&mut self) -> Result<Vec<Opt>, Error> {
        let mut out = Vec::new();
        if self.eat(b'[') {
            loop {
                out.push(self.option()?);
                if !self.eat(b',') {
                    break;
                }
            }
            self.expect(b']')?;
        }
        Ok(out)
    }
}
