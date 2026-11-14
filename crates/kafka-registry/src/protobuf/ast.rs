//! The `.proto` text as written: declarations in their order, each with the spans of the source the stored text
//! repeats (types, numbers, option names and values, reserved and extension ranges), and with what the linker needs
//! (names, numbers, labels) already read out.

use super::Error;
use super::lex::{Tok, Token, lex, unquote};

/// A span of the source, in bytes.
pub(super) type Span = (u32, u32);

/// The most deeply messages, groups and option values may nest. Deeper text is refused, so a hostile schema cannot
/// exhaust the stack.
const MAX_DEPTH: u32 = 100;

/// The largest field number.
pub(super) const MAX_FIELD: i64 = 536_870_911;

/// Stands for `max` in a range until the range is known to be a field's or an enum value's.
pub(super) const MAX_WORD: i64 = i64::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Label {
    Optional,
    Required,
    Repeated,
}

impl Label {
    pub(super) fn word(self) -> &'static str {
        match self {
            Self::Optional => "optional",
            Self::Required => "required",
            Self::Repeated => "repeated",
        }
    }
}

#[derive(Debug)]
pub(super) struct File {
    /// The `syntax` string's source and value, if the file has one.
    pub(super) syntax: Option<(Span, String)>,
    pub(super) decls: Vec<Decl>,
}

#[derive(Debug)]
pub(super) enum Decl {
    Package(String, Span),
    /// The modifier (`public`, `weak` or none), the path, the path's source.
    Import(&'static str, String, Span),
    Option(Opt),
    Message(Message),
    Enum(Enum),
    Extend(Extend),
    Service(Service),
}

/// An option: `name = value`, a statement or one of a field's.
#[derive(Debug)]
pub(super) struct Opt {
    /// The name's parts; a part in parentheses names an extension.
    pub(super) name: Vec<(String, bool)>,
    pub(super) name_span: Span,
    pub(super) val: Val,
    pub(super) val_span: Span,
    pub(super) at: u32,
}

impl Opt {
    /// The name when it is one plain word (a built-in option).
    pub(super) fn simple(&self) -> Option<&str> {
        match self.name.as_slice() {
            [(n, false)] => Some(n),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub(super) enum Val {
    Ident(String),
    Str,
    /// A number as written (sign included); float or integer.
    Num(String, bool),
    /// A message literal in braces, not read further.
    Aggregate,
}

#[derive(Debug)]
pub(super) struct Message {
    pub(super) name: String,
    pub(super) at: u32,
    pub(super) body: Vec<Item>,
}

#[derive(Debug)]
pub(super) enum Item {
    Field(Field),
    Group(Group),
    Oneof(Oneof),
    Option(Opt),
    Message(Message),
    Enum(Enum),
    Extend(Extend),
    Reserved(Reserved),
    Extensions(Reserved, Vec<Opt>),
}

/// `reserved` (ranges or names) or `extensions` (ranges): the source and what it lists.
#[derive(Debug)]
pub(super) struct Reserved {
    pub(super) span: Span,
    pub(super) ranges: Vec<(i64, i64)>,
    pub(super) names: Vec<String>,
    pub(super) at: u32,
}

/// A field's type as written: a name (scalar or message or enum), or a map.
#[derive(Debug)]
pub(super) enum TypeRef {
    Named(String, Span),
    Map(String, Span, String, Span),
}

#[derive(Debug)]
pub(super) struct Field {
    pub(super) label: Option<Label>,
    pub(super) ty: TypeRef,
    pub(super) name: String,
    pub(super) number: i64,
    pub(super) tag: Span,
    pub(super) opts: Vec<Opt>,
    pub(super) at: u32,
}

#[derive(Debug)]
pub(super) struct Group {
    pub(super) label: Option<Label>,
    pub(super) name: String,
    pub(super) number: i64,
    pub(super) tag: Span,
    pub(super) opts: Vec<Opt>,
    pub(super) body: Vec<Item>,
    pub(super) at: u32,
}

#[derive(Debug)]
pub(super) struct Oneof {
    pub(super) name: String,
    pub(super) at: u32,
    /// Fields, groups and options.
    pub(super) body: Vec<Item>,
}

#[derive(Debug)]
pub(super) struct Enum {
    pub(super) name: String,
    pub(super) at: u32,
    pub(super) body: Vec<EnumItem>,
}

#[derive(Debug)]
pub(super) enum EnumItem {
    Option(Opt),
    Reserved(Reserved),
    Value(EnumValue),
}

#[derive(Debug)]
pub(super) struct EnumValue {
    pub(super) name: String,
    pub(super) number: i64,
    pub(super) num_span: Span,
    pub(super) opts: Vec<Opt>,
    pub(super) at: u32,
}

#[derive(Debug)]
pub(super) struct Extend {
    pub(super) extendee: String,
    pub(super) span: Span,
    pub(super) at: u32,
    /// Fields and groups.
    pub(super) body: Vec<Item>,
}

#[derive(Debug)]
pub(super) struct Service {
    pub(super) name: String,
    pub(super) at: u32,
    pub(super) opts: Vec<Opt>,
    pub(super) rpcs: Vec<Rpc>,
}

/// One end of an rpc: streamed or not, the message type and its source.
pub(super) type RpcType = (bool, String, Span);

#[derive(Debug)]
pub(super) struct Rpc {
    pub(super) name: String,
    pub(super) input: RpcType,
    pub(super) output: RpcType,
    pub(super) opts: Vec<Opt>,
    pub(super) at: u32,
}

/// Reads `.proto` text.
///
/// # Errors
/// The text is not in the language's grammar.
pub(super) fn parse(src: &str) -> Result<File, Error> {
    let mut p = Parser {
        src,
        toks: lex(src)?,
        i: 0,
        depth: 0,
    };
    p.file()
}

struct Parser<'a> {
    src: &'a str,
    toks: Vec<Token>,
    i: usize,
    depth: u32,
}

impl Parser<'_> {
    fn peek(&self, k: usize) -> Option<Token> {
        self.toks.get(self.i + k).copied()
    }

    fn text(&self, t: Token) -> &str {
        self.src.get(t.start as usize..t.end as usize).unwrap_or_default()
    }

    /// Where the next token starts (the end of the text when there is none).
    fn at(&self) -> u32 {
        self.peek(0).map_or_else(
            || u32::try_from(self.src.len()).unwrap_or(u32::MAX),
            |t| t.start,
        )
    }

    /// Where the last token read ends.
    fn end(&self) -> u32 {
        self.i
            .checked_sub(1)
            .and_then(|i| self.toks.get(i))
            .map_or(0, |t| t.end)
    }

    fn err<T>(&self, what: &str) -> Result<T, Error> {
        let found = self.peek(0).map_or("end of input", |t| self.text(t));
        Err(Error::syntax(
            self.at(),
            &format!("expected {what}, found \"{found}\""),
        ))
    }

    fn is(&self, k: usize, c: u8) -> bool {
        self.peek(k).is_some_and(|t| t.kind == Tok::Punct(c))
    }

    fn is_word(&self, k: usize, w: &str) -> bool {
        self.peek(k)
            .is_some_and(|t| t.kind == Tok::Ident && self.text(t) == w)
    }

    fn eat(&mut self, c: u8) -> bool {
        let yes = self.is(0, c);
        self.i += usize::from(yes);
        yes
    }

    fn expect(&mut self, c: u8) -> Result<(), Error> {
        if self.eat(c) {
            Ok(())
        } else {
            self.err(&format!("\"{}\"", char::from(c)))
        }
    }

    fn ident(&mut self) -> Result<String, Error> {
        match self.peek(0) {
            Some(t) if t.kind == Tok::Ident => {
                self.i += 1;
                Ok(self.text(t).to_owned())
            }
            _ => self.err("a name"),
        }
    }

    /// `a.b.c`, with a leading dot when `dot` allows it; the name and its source.
    fn dotted(&mut self, dot: bool) -> Result<(String, Span), Error> {
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
    fn string(&mut self) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        let mut any = false;
        while let Some(t) = self.peek(0).filter(|t| t.kind == Tok::Str) {
            out.extend(unquote(self.text(t), t.start)?);
            self.i += 1;
            any = true;
        }
        if any { Ok(out) } else { self.err("a string") }
    }

    fn utf8(&mut self) -> Result<String, Error> {
        let at = self.at();
        String::from_utf8(self.string()?).map_err(|_| Error::syntax(at, "string is not UTF-8"))
    }

    /// An integer, with a minus sign when `neg` allows it.
    fn int(&mut self, neg: bool) -> Result<(i64, Span), Error> {
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

    fn end_statement(&mut self) -> Result<(), Error> {
        self.expect(b';')
    }

    fn enter(&mut self) -> Result<(), Error> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(Error::syntax(self.at(), "nested too deeply"));
        }
        Ok(())
    }

    fn file(&mut self) -> Result<File, Error> {
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

    fn file_decl(&mut self, file: &File) -> Result<Decl, Error> {
        let at = self.at();
        let word = self.peek(0).filter(|t| t.kind == Tok::Ident).map(|t| self.text(t).to_owned());
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
                    kind = if self.is_word(0, "public") { "public" } else { "weak" };
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

    fn option_statement(&mut self) -> Result<Opt, Error> {
        self.i += 1;
        let o = self.option()?;
        self.end_statement()?;
        Ok(o)
    }

    /// `name = value`.
    fn option(&mut self) -> Result<Opt, Error> {
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

    fn value(&mut self) -> Result<Val, Error> {
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
                let n = self.peek(0).filter(|n| n.kind != Tok::Str && n.kind != Tok::Punct(b'{'));
                let Some(n) = n.filter(|n| n.kind != Tok::Ident || matches!(self.text(*n), "inf" | "nan")) else {
                    return self.err("a number");
                };
                self.i += 1;
                let sign = if c == b'-' { "-" } else { "" };
                Ok(Val::Num(format!("{sign}{}", self.text(n)), n.kind != Tok::Int))
            }
            Tok::Punct(b'{') => {
                self.skip_aggregate()?;
                Ok(Val::Aggregate)
            }
            Tok::Punct(_) => self.err("a value"),
        }
    }

    /// A message literal, `{ ... }`, skipped with its brackets balanced.
    fn skip_aggregate(&mut self) -> Result<(), Error> {
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
    fn compact_options(&mut self) -> Result<Vec<Opt>, Error> {
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

    fn message(&mut self) -> Result<Message, Error> {
        self.i += 1;
        let at = self.at();
        let name = self.ident()?;
        let body = self.message_body()?;
        Ok(Message { name, at, body })
    }

    fn message_body(&mut self) -> Result<Vec<Item>, Error> {
        self.enter()?;
        self.expect(b'{')?;
        let mut body = Vec::new();
        while !self.eat(b'}') {
            if self.peek(0).is_none() {
                return self.err("\"}\"");
            }
            if !self.eat(b';') {
                body.push(self.message_item()?);
            }
        }
        self.depth -= 1;
        Ok(body)
    }

    fn message_item(&mut self) -> Result<Item, Error> {
        let decl = |p: &Self| p.peek(1).is_some_and(|t| t.kind == Tok::Ident) && p.is(2, b'{');
        let word = self.peek(0).filter(|t| t.kind == Tok::Ident).map(|t| self.text(t).to_owned());
        Ok(match word.as_deref() {
            Some("message") if decl(self) => Item::Message(self.message()?),
            Some("enum") if decl(self) => Item::Enum(self.enumeration()?),
            Some("oneof") if decl(self) => Item::Oneof(self.oneof()?),
            Some("extend") if !self.is(2, b'=') => Item::Extend(self.extend()?),
            Some("option") => Item::Option(self.option_statement()?),
            Some("reserved") if !self.is(2, b'=') => Item::Reserved(self.reserved(true)?.0),
            Some("extensions") if !self.is(2, b'=') => {
                let r = self.reserved(false)?;
                Item::Extensions(r.0, r.1)
            }
            _ => self.field(true)?,
        })
    }

    /// `reserved ...;` or `extensions ...;` (the latter with options).
    fn reserved(&mut self, names: bool) -> Result<(Reserved, Vec<Opt>), Error> {
        let at = self.at();
        self.i += 1;
        let mut r = Reserved {
            span: (at, at),
            ranges: Vec::new(),
            names: Vec::new(),
            at,
        };
        loop {
            if names && self.peek(0).is_some_and(|t| t.kind == Tok::Str) {
                r.names.push(self.utf8()?);
            } else {
                let (from, _) = self.int(true)?;
                let mut to = from;
                if self.is_word(0, "to") {
                    self.i += 1;
                    to = if self.is_word(0, "max") {
                        self.i += 1;
                        MAX_WORD
                    } else {
                        self.int(true)?.0
                    };
                }
                r.ranges.push((from, to));
            }
            if !self.eat(b',') {
                break;
            }
        }
        if !r.names.is_empty() && !r.ranges.is_empty() {
            return Err(Error::syntax(at, "reserved names and numbers must be separate statements"));
        }
        let opts = if names { Vec::new() } else { self.compact_options()? };
        r.span = (at, self.end());
        self.end_statement()?;
        Ok((r, opts))
    }

    fn label(&mut self) -> Option<Label> {
        let l = match self.peek(0).map(|t| self.text(t)) {
            Some("optional") => Label::Optional,
            Some("required") => Label::Required,
            Some("repeated") => Label::Repeated,
            _ => return None,
        };
        // `optional = 1`, a field named after the label, is not a label.
        if self.is(1, b'=') {
            return None;
        }
        self.i += 1;
        Some(l)
    }

    /// A field, a map field or a group; `labels` is false in a oneof.
    fn field(&mut self, labels: bool) -> Result<Item, Error> {
        let at = self.at();
        let label = self.label();
        if label.is_some() && !labels {
            return Err(Error::syntax(at, "fields in a oneof must not have labels"));
        }
        if self.is_word(0, "group") && self.peek(1).is_some_and(|t| t.kind == Tok::Ident) {
            return self.group(label, at);
        }
        let ty = if self.is_word(0, "map") && self.is(1, b'<') {
            if label.is_some() || !labels {
                return Err(Error::syntax(at, "map fields cannot have labels or be in a oneof"));
            }
            self.i += 2;
            let (k, ks) = self.dotted(true)?;
            self.expect(b',')?;
            let (v, vs) = self.dotted(true)?;
            self.expect(b'>')?;
            TypeRef::Map(k, ks, v, vs)
        } else {
            let (n, s) = self.dotted(true)?;
            TypeRef::Named(n, s)
        };
        let name = self.ident()?;
        self.expect(b'=')?;
        let (number, tag) = self.int(false)?;
        let opts = self.compact_options()?;
        self.end_statement()?;
        Ok(Item::Field(Field {
            label,
            ty,
            name,
            number,
            tag,
            opts,
            at,
        }))
    }

    fn group(&mut self, label: Option<Label>, at: u32) -> Result<Item, Error> {
        self.i += 1;
        let name = self.ident()?;
        self.expect(b'=')?;
        let (number, tag) = self.int(false)?;
        let opts = self.compact_options()?;
        let body = self.message_body()?;
        Ok(Item::Group(Group {
            label,
            name,
            number,
            tag,
            opts,
            body,
            at,
        }))
    }

    fn oneof(&mut self) -> Result<Oneof, Error> {
        self.i += 1;
        let at = self.at();
        let name = self.ident()?;
        self.expect(b'{')?;
        let mut body = Vec::new();
        while !self.eat(b'}') {
            if self.peek(0).is_none() {
                return self.err("\"}\"");
            }
            if self.eat(b';') {
                continue;
            }
            body.push(if self.is_word(0, "option") && !self.is(1, b'=') {
                Item::Option(self.option_statement()?)
            } else {
                self.field(false)?
            });
        }
        Ok(Oneof { name, at, body })
    }

    fn enumeration(&mut self) -> Result<Enum, Error> {
        self.i += 1;
        let at = self.at();
        let name = self.ident()?;
        self.expect(b'{')?;
        let mut body = Vec::new();
        while !self.eat(b'}') {
            if self.peek(0).is_none() {
                return self.err("\"}\"");
            }
            if self.eat(b';') {
                continue;
            }
            body.push(if self.is_word(0, "option") && !self.is(1, b'=') {
                EnumItem::Option(self.option_statement()?)
            } else if self.is_word(0, "reserved") && !self.is(1, b'=') {
                EnumItem::Reserved(self.reserved(true)?.0)
            } else {
                let at = self.at();
                let name = self.ident()?;
                self.expect(b'=')?;
                let (number, num_span) = self.int(true)?;
                let opts = self.compact_options()?;
                self.end_statement()?;
                EnumItem::Value(EnumValue {
                    name,
                    number,
                    num_span,
                    opts,
                    at,
                })
            });
        }
        Ok(Enum { name, at, body })
    }

    fn extend(&mut self) -> Result<Extend, Error> {
        self.i += 1;
        let at = self.at();
        let (extendee, span) = self.dotted(true)?;
        self.enter()?;
        self.expect(b'{')?;
        let mut body = Vec::new();
        while !self.eat(b'}') {
            if self.peek(0).is_none() {
                return self.err("\"}\"");
            }
            if !self.eat(b';') {
                body.push(self.field(true)?);
            }
        }
        self.depth -= 1;
        Ok(Extend {
            extendee,
            span,
            at,
            body,
        })
    }

    fn service(&mut self) -> Result<Service, Error> {
        self.i += 1;
        let at = self.at();
        let name = self.ident()?;
        self.expect(b'{')?;
        let mut s = Service {
            name,
            at,
            opts: Vec::new(),
            rpcs: Vec::new(),
        };
        while !self.eat(b'}') {
            if self.eat(b';') {
                continue;
            }
            if self.is_word(0, "option") {
                s.opts.push(self.option_statement()?);
            } else if self.is_word(0, "rpc") {
                s.rpcs.push(self.rpc()?);
            } else {
                return self.err("\"rpc\" or \"option\"");
            }
        }
        Ok(s)
    }

    fn rpc_type(&mut self) -> Result<RpcType, Error> {
        self.expect(b'(')?;
        // `stream .a.B` streams; `stream.B` (no space) is a type named so.
        let glued = self.peek(0).zip(self.peek(1)).is_some_and(|(a, b)| a.end == b.start && b.kind == Tok::Punct(b'.'));
        let stream = self.is_word(0, "stream") && !self.is(1, b')') && !glued;
        self.i += usize::from(stream);
        let (name, span) = self.dotted(true)?;
        self.expect(b')')?;
        Ok((stream, name, span))
    }

    fn rpc(&mut self) -> Result<Rpc, Error> {
        self.i += 1;
        let at = self.at();
        let name = self.ident()?;
        let input = self.rpc_type()?;
        if !self.is_word(0, "returns") {
            return self.err("\"returns\"");
        }
        self.i += 1;
        let output = self.rpc_type()?;
        let mut opts = Vec::new();
        if self.eat(b'{') {
            while !self.eat(b'}') {
                if self.eat(b';') {
                    continue;
                }
                if !self.is_word(0, "option") {
                    return self.err("\"option\"");
                }
                opts.push(self.option_statement()?);
            }
            self.eat(b';');
        } else {
            self.end_statement()?;
        }
        Ok(Rpc {
            name,
            input,
            output,
            opts,
            at,
        })
    }
}

/// An integer literal's value: decimal, `0x` hex or `0` octal.
pub(super) fn int_value(t: &str) -> Option<u64> {
    if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        u64::from_str_radix(h, 16).ok()
    } else if t.len() > 1 && t.starts_with('0') {
        u64::from_str_radix(t.get(1..)?, 8).ok()
    } else {
        t.parse().ok()
    }
}
