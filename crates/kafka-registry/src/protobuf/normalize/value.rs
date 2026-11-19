//! Option values as Wire reads them, and the strings, names and message literals in them.

use super::*;

/// An option's value as Wire reads it (`OptionElement` and the values of its maps and lists).
#[derive(Debug, Clone, PartialEq)]
pub(super) enum V {
    Str(String),
    Bool(String),
    Enum(String),
    Num(String),
    Map(Vec<(String, V)>),
    List(Vec<V>),
    Opt(Box<Named>),
}

/// An option: its name (in parentheses for an extension) and value.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Named {
    pub(super) name: String,
    pub(super) paren: bool,
    pub(super) val: V,
}

/// The characters a string literal stands for, as Wire's `readQuotedString` reads them: an escape of a number is the
/// character of that code, an escape it does not know is the character itself.
pub(super) fn wire_unquote(lit: &str) -> String {
    let mut out = String::new();
    let mut chars = lit.chars().peekable();
    while let Some(q) = chars.next() {
        if q != '"' && q != '\'' {
            continue;
        }
        while let Some(c) = chars.next() {
            if c == q {
                break;
            }
            if c != '\\' {
                out.push(c);
                continue;
            }
            let Some(e) = chars.next() else { break };
            let numeric = |chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
                           radix: u32,
                           len: usize,
                           first: Option<u32>| {
                let mut v = first;
                for _ in 0..len - usize::from(first.is_some()) {
                    match chars
                        .peek()
                        .and_then(|c| c.to_digit(16))
                        .filter(|&d| d < radix)
                    {
                        Some(d) => {
                            v = Some(v.map_or(d, |v| v * radix + d));
                            chars.next();
                        }
                        None => break,
                    }
                }
                v.and_then(char::from_u32).unwrap_or('\0')
            };
            out.push(match e {
                'a' => '\u{7}',
                'b' => '\u{8}',
                'f' => '\u{c}',
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                'v' => '\u{b}',
                'x' | 'X' => numeric(&mut chars, 16, 2, None),
                '0'..='7' => numeric(&mut chars, 8, 3, e.to_digit(8)),
                other => other,
            });
        }
    }
    out
}

/// Confluent's `escapeChars`.
pub(super) fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\u{7}' => out.push_str("\\a"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{b}' => out.push_str("\\v"),
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '"' => out.push_str("\\\""),
            c => out.push(c),
        }
    }
    out
}

/// A Java `String` order (UTF-16 code units).
pub(super) fn java_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// Reads a message literal (`{ a: 1 b { c: "x" } d: [1, 2] }`) as Wire's `OptionReader.readMap` does.
pub(super) struct Literal<'a> {
    src: &'a str,
    toks: Vec<crate::protobuf::lex::Token>,
    i: usize,
}

impl Literal<'_> {
    pub(super) fn text(&self, i: usize) -> &str {
        self.toks
            .get(i)
            .and_then(|t| self.src.get(t.start as usize..t.end as usize))
            .unwrap_or_default()
    }

    pub(super) fn is(&self, c: u8) -> bool {
        self.toks
            .get(self.i)
            .is_some_and(|t| t.kind == Tok::Punct(c))
    }

    pub(super) fn eat(&mut self, c: u8) -> bool {
        let yes = self.is(c);
        if yes {
            self.i += 1;
        }
        yes
    }

    /// A dotted word (`a.b.c`), as Wire's `readWord` takes dots into a word.
    pub(super) fn word(&mut self) -> String {
        let mut w = String::new();
        while let Some(Tok::Ident | Tok::Int | Tok::Float) = self.toks.get(self.i).map(|t| t.kind) {
            w.push_str(self.text(self.i));
            self.i += 1;
            if self.is(b'.')
                && matches!(self.toks.get(self.i + 1).map(|t| t.kind), Some(Tok::Ident))
            {
                w.push('.');
                self.i += 1;
            } else {
                break;
            }
        }
        w
    }

    pub(super) fn map(&mut self, depth: usize) -> Vec<(String, V)> {
        let mut out: Vec<(String, V)> = Vec::new();
        self.eat(b'{');
        while self.i < self.toks.len() && !self.eat(b'}') {
            let key = if self.eat(b'[') {
                let mut k = String::new();
                while self.i < self.toks.len() && !self.eat(b']') {
                    k.push_str(self.text(self.i));
                    self.i += 1;
                }
                format!("[{k}]")
            } else {
                self.word()
            };
            if key.is_empty() {
                self.i += 1;
                continue;
            }
            self.eat(b':');
            let v = self.value(depth + 1);
            match out.iter_mut().find(|(k, _)| *k == key) {
                None => out.push((key, v)),
                Some((_, prev)) => {
                    let mut list = match std::mem::replace(prev, V::List(Vec::new())) {
                        V::List(l) => l,
                        other => vec![other],
                    };
                    match v {
                        V::List(l) => list.extend(l),
                        v => list.push(v),
                    }
                    *prev = V::List(list);
                }
            }
            if !self.eat(b',') {
                self.eat(b';');
            }
        }
        out
    }

    pub(super) fn value(&mut self, depth: usize) -> V {
        if depth > 100 {
            return V::Map(Vec::new());
        }
        let Some(t) = self.toks.get(self.i).copied() else {
            return V::Enum(String::new());
        };
        match t.kind {
            Tok::Punct(b'{') => V::Map(self.map(depth)),
            Tok::Punct(b'[') => {
                self.i += 1;
                let mut items = Vec::new();
                while self.i < self.toks.len() && !self.eat(b']') {
                    items.push(self.value(depth + 1));
                    self.eat(b',');
                }
                V::List(items)
            }
            Tok::Str => {
                let mut s = String::new();
                while self.toks.get(self.i).is_some_and(|t| t.kind == Tok::Str) {
                    s.push_str(&wire_unquote(self.text(self.i)));
                    self.i += 1;
                }
                V::Str(s)
            }
            Tok::Punct(b'-') => {
                self.i += 1;
                V::Num(format!("-{}", self.word()))
            }
            Tok::Int | Tok::Float => V::Num(self.word()),
            _ => {
                let w = self.word();
                if w.is_empty() {
                    self.i += 1;
                }
                match w.as_str() {
                    "true" | "false" => V::Bool(w),
                    _ => V::Enum(w),
                }
            }
        }
    }
}

/// An option of the schema as Wire reads it: the first part of its name, then its value, a dotted name nesting it.
pub(super) fn named(src: &str, o: &Opt) -> Named {
    let span = |(a, b): (u32, u32)| src.get(a as usize..b as usize).unwrap_or_default();
    let mut val = match &o.val {
        Val::Str => V::Str(wire_unquote(span(o.val_span))),
        Val::Ident(w) if w == "true" || w == "false" => V::Bool(w.clone()),
        Val::Ident(w) => V::Enum(w.clone()),
        Val::Num(n, _) => V::Num(n.clone()),
        Val::Aggregate => {
            let text = span(o.val_span);
            let toks = lex(text).unwrap_or_default();
            V::Map(
                Literal {
                    src: text,
                    toks,
                    i: 0,
                }
                .map(0),
            )
        }
    };
    let (first, rest) = o
        .name
        .split_first()
        .map_or((None, &[][..]), |(f, r)| (Some(f), r));
    for (n, paren) in rest.iter().rev() {
        val = V::Opt(Box::new(Named {
            name: n.clone(),
            paren: *paren,
            val,
        }));
    }
    let (name, paren) = first.cloned().unwrap_or_default();
    Named { name, paren, val }
}

/// Confluent's `transform`: a dotted option made a message literal.
pub(super) fn transform(o: Named) -> Named {
    pub(super) fn map_of(inner: Named) -> V {
        let v = match inner.val {
            V::Opt(n) => map_of(*n),
            v => v,
        };
        V::Map(vec![(inner.name, v)])
    }
    match o.val {
        V::Opt(inner) => Named {
            val: map_of(*inner),
            ..o
        },
        _ => o,
    }
}
