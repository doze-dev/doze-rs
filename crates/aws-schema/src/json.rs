//! Decode and validate in one pass: JSON text in, a tree in the arena out, every constraint of the schema checked on the way.
//! Nothing is parsed into an intermediate value and nothing is copied except an escaped string and a decoded blob.

use crate::arena::{Arena, NIL, Node, Val};
use crate::patterns::Patterns;
mod compound;
mod scalars;
mod strings;

use crate::tables::{
    K_BLOB, K_BOOL, K_EVENT, K_FLOAT, K_INT, K_LIST, K_MAP, K_STR, K_STREAM, K_STRUCT, K_TS,
    K_UNION, MemberRef, Tables,
};

/// Deeper than any real request; a hostile body cannot recurse further (PLAN.md rule 15).
const MAX_DEPTH: u32 = 100;

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Violation {
    Syntax {
        at: usize,
    },
    TooDeep,
    Type {
        member: u32,
        expected: &'static str,
    },
    Missing {
        shape: u32,
        member: u32,
    },
    /// `value` is the offending value as text, when it is a scalar (a list or map has none to show).
    Length {
        member: u32,
        value: Option<String>,
    },
    Range {
        member: u32,
        value: Option<String>,
    },
    Pattern {
        member: u32,
        value: Option<String>,
    },
    Enum {
        member: u32,
        value: Option<String>,
    },
    Union {
        shape: u32,
    },
    Table,
}

type R<X> = Result<X, Violation>;

struct D<'t> {
    t: &'t Tables,
    pats: &'t Patterns,
    src: &'t str,
    pos: usize,
    ar: &'t mut Arena,
}

// fuzz:parser fuzz_json
/// Decodes `body` as the structure `shape`; returns the root node.
pub fn decode(t: &Tables, pats: &Patterns, shape: u32, body: &str, ar: &mut Arena) -> R<u32> {
    ar.reserve_for(body.len());
    let mut d = D {
        t,
        pats,
        src: body,
        pos: 0,
        ar,
    };
    d.ws();
    let v = d.object(shape, 0)?;
    d.ws();
    if d.pos != d.src.len() {
        return Err(Violation::Syntax { at: d.pos });
    }
    Ok(d.ar.push(Node {
        next: NIL,
        member: NIL,
        val: v,
    }))
}

impl D<'_> {
    fn bytes(&self) -> &[u8] {
        self.src.as_bytes()
    }
    fn peek(&self) -> Option<u8> {
        self.bytes().get(self.pos).copied()
    }
    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }
    fn expect(&mut self, c: u8) -> R<()> {
        self.ws();
        if self.peek() == Some(c) {
            self.pos += 1;
            Ok(())
        } else {
            Err(Violation::Syntax { at: self.pos })
        }
    }
    fn lit(&mut self, word: &str) -> R<()> {
        if self
            .src
            .get(self.pos..)
            .is_some_and(|r| r.starts_with(word))
        {
            self.pos += word.len();
            Ok(())
        } else {
            Err(Violation::Syntax { at: self.pos })
        }
    }

    // ---- skipping ----

    fn skip(&mut self, depth: u32) -> R<()> {
        if depth > MAX_DEPTH {
            return Err(Violation::TooDeep);
        }
        self.ws();
        match self.peek() {
            Some(b'"') => self.string().map(|_| ()),
            Some(b'{') => self.skip_seq(b'{', b'}', depth),
            Some(b'[') => self.skip_seq(b'[', b']', depth),
            Some(b't') => self.lit("true"),
            Some(b'f') => self.lit("false"),
            Some(b'n') => self.lit("null"),
            Some(_) => self.number().map(|_| ()),
            None => Err(Violation::Syntax { at: self.pos }),
        }
    }

    fn skip_seq(&mut self, open: u8, close: u8, depth: u32) -> R<()> {
        self.expect(open)?;
        self.ws();
        if self.peek() == Some(close) {
            self.pos += 1;
            return Ok(());
        }
        loop {
            if open == b'{' {
                self.string()?;
                self.expect(b':')?;
            }
            self.skip(depth + 1)?;
            self.ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(c) if c == close => {
                    self.pos += 1;
                    return Ok(());
                }
                _ => return Err(Violation::Syntax { at: self.pos }),
            }
        }
    }

    // ---- numbers ----

    fn number(&mut self) -> R<&str> {
        let start = self.pos;
        while matches!(
            self.peek(),
            Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
        ) {
            self.pos += 1;
        }
        if self.pos == start {
            return Err(Violation::Syntax { at: start });
        }
        self.src
            .get(start..self.pos)
            .ok_or(Violation::Syntax { at: start })
    }

    // ---- values by schema ----

    fn value(&mut self, m: &MemberRef<'_>, mi: u32, depth: u32) -> R<Val> {
        if depth > MAX_DEPTH {
            return Err(Violation::TooDeep);
        }
        self.ws();
        if self.peek().is_none() {
            return Err(Violation::Syntax { at: self.pos });
        }
        if self.peek() == Some(b'n') {
            self.lit("null")?;
            return Ok(Val::Null);
        }
        let t = self.t;
        let cons = if m.cons == 0 { None } else { t.cons(m.cons) };
        match m.kind {
            K_BOOL => match self.peek() {
                Some(b't') => self.lit("true").map(|()| Val::Bool(true)),
                Some(b'f') => self.lit("false").map(|()| Val::Bool(false)),
                _ => Err(Violation::Type {
                    member: mi,
                    expected: "boolean",
                }),
            },
            K_INT => self.int(mi, cons.as_ref()),
            K_FLOAT | K_TS => self.float(mi, m.kind == K_TS, cons.as_ref()),
            K_STR => self.str_value(m, mi, cons.as_ref()),
            K_BLOB => self.blob(mi, cons.as_ref()),
            K_LIST => self.list(m, mi, cons.as_ref(), depth),
            K_MAP => self.map(m, mi, cons.as_ref(), depth),
            K_STRUCT | K_UNION => self.object(m.target, depth + 1),
            // A streaming member (payload blob, event stream) is not a JSON key: ignore it as an unknown key, like the typed design.
            K_STREAM | K_EVENT => self.skip(depth + 1).map(|()| Val::Null),
            _ => Err(Violation::Table),
        }
    }
}
