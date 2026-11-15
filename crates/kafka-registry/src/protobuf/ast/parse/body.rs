//! Message bodies (fields, maps, groups, oneofs, reserved and extension ranges), enums, extends and services.

use super::super::super::Error;
use super::super::super::lex::Tok;
use super::super::{
    Enum, EnumItem, EnumValue, Extend, Field, Group, Item, Label, MAX_WORD, Message, Oneof, Opt,
    Reserved, Rpc, RpcType, Service, TypeRef,
};
use super::Parser;

impl Parser<'_> {
    pub(super) fn message(&mut self) -> Result<Message, Error> {
        self.i += 1;
        let at = self.at();
        let name = self.ident()?;
        let body = self.message_body()?;
        Ok(Message { name, at, body })
    }

    pub(super) fn message_body(&mut self) -> Result<Vec<Item>, Error> {
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

    pub(super) fn message_item(&mut self) -> Result<Item, Error> {
        let decl = |p: &Self| p.peek(1).is_some_and(|t| t.kind == Tok::Ident) && p.is(2, b'{');
        let word = self
            .peek(0)
            .filter(|t| t.kind == Tok::Ident)
            .map(|t| self.text(t).to_owned());
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
    pub(super) fn reserved(&mut self, names: bool) -> Result<(Reserved, Vec<Opt>), Error> {
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
            return Err(Error::syntax(
                at,
                "reserved names and numbers must be separate statements",
            ));
        }
        let opts = if names {
            Vec::new()
        } else {
            self.compact_options()?
        };
        r.span = (at, self.end());
        self.end_statement()?;
        Ok((r, opts))
    }

    pub(super) fn label(&mut self) -> Option<Label> {
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
    pub(super) fn field(&mut self, labels: bool) -> Result<Item, Error> {
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
                return Err(Error::syntax(
                    at,
                    "map fields cannot have labels or be in a oneof",
                ));
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

    pub(super) fn group(&mut self, label: Option<Label>, at: u32) -> Result<Item, Error> {
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

    pub(super) fn oneof(&mut self) -> Result<Oneof, Error> {
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

    pub(super) fn enumeration(&mut self) -> Result<Enum, Error> {
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

    pub(super) fn extend(&mut self) -> Result<Extend, Error> {
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

    pub(super) fn service(&mut self) -> Result<Service, Error> {
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

    pub(super) fn rpc_type(&mut self) -> Result<RpcType, Error> {
        self.expect(b'(')?;
        // `stream .a.B` streams; `stream.B` (no space) is a type named so.
        let glued = self
            .peek(0)
            .zip(self.peek(1))
            .is_some_and(|(a, b)| a.end == b.start && b.kind == Tok::Punct(b'.'));
        let stream = self.is_word(0, "stream") && !self.is(1, b')') && !glued;
        self.i += usize::from(stream);
        let (name, span) = self.dotted(true)?;
        self.expect(b')')?;
        Ok((stream, name, span))
    }

    pub(super) fn rpc(&mut self) -> Result<Rpc, Error> {
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
