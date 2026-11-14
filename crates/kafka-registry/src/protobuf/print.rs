//! The text Confluent stores for a `.proto` schema, so the same schema written two ways (comments, spacing, the order
//! of messages and enums) is one schema with one id, and a client reading it back sees Confluent's layout:
//!
//! - syntax and package; a blank line and the imports; a blank line and the file options; a blank line and the types
//!   (every message in order, then every enum); a blank line and the extends; a blank line and the services;
//! - in a message the parts that exist, as sections parted by a blank line: reserved, options, fields, oneofs, groups,
//!   nested types (messages, then enums), extends, extension ranges. A blank line in a nested message carries the
//!   indent of the message around it (Confluent indents a nested message's text line by line);
//! - comments dropped, two-space indent, a token's own text kept with its runs of white space made one space; a field
//!   with one option keeps it inline, with several one to a line (its own options, then `default`, then `json_name`).
//!
//! Learned from Confluent's answers (`tests/data/conformance/protobuf-*.json`), as the Go edition's `protoprint.go`.

use std::fmt::Write as _;

use super::ast::{
    Decl, Enum, EnumItem, Extend, Field, File, Group, Item, Message, Opt, Service, Span, TypeRef,
};

/// The stored text of `file`, read from `src`.
pub(super) fn print(src: &str, file: &File) -> String {
    let mut p = Printer {
        src,
        out: String::new(),
    };
    p.file(file);
    p.out
}

struct Printer<'a> {
    src: &'a str,
    out: String,
}

impl Printer<'_> {
    /// The source of a span with its runs of white space made one space.
    fn raw(&self, (a, b): Span) -> String {
        let s = self.src.get(a as usize..b as usize).unwrap_or_default();
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn line(&mut self, indent: usize, text: &str) {
        for _ in 0..indent {
            self.out.push_str("  ");
        }
        self.out.push_str(text);
        self.out.push('\n');
    }

    /// The blank line between sections: two spaces in a nested message.
    fn blank(&mut self, indent: usize) {
        if indent > 0 {
            self.out.push_str("  ");
        }
        self.out.push('\n');
    }

    fn opt(&self, o: &Opt) -> String {
        format!("{} = {}", self.raw(o.name_span), self.raw(o.val_span))
    }

    fn file(&mut self, f: &File) {
        if let Some((span, _)) = f.syntax {
            let s = format!("syntax = {};", self.raw(span));
            self.line(0, &s);
        }
        let of = |pick: fn(&Decl) -> bool| f.decls.iter().filter(move |d| pick(d));
        for d in of(|d| matches!(d, Decl::Package(..))) {
            if let Decl::Package(_, span) = d {
                let s = format!("package {};", self.raw(*span));
                self.line(0, &s);
            }
        }
        let mut first = true;
        for d in of(|d| matches!(d, Decl::Import(..))) {
            if let Decl::Import(kind, _, span) = d {
                if std::mem::take(&mut first) {
                    self.out.push('\n');
                }
                let sp = if kind.is_empty() { "" } else { " " };
                let s = format!("import {kind}{sp}{};", self.raw(*span));
                self.line(0, &s);
            }
        }
        first = true;
        for d in of(|d| matches!(d, Decl::Option(_))) {
            if let Decl::Option(o) = d {
                if std::mem::take(&mut first) {
                    self.out.push('\n');
                }
                let s = format!("option {};", self.opt(o));
                self.line(0, &s);
            }
        }
        if f.decls
            .iter()
            .any(|d| matches!(d, Decl::Message(_) | Decl::Enum(_)))
        {
            self.out.push('\n');
        }
        for d in &f.decls {
            if let Decl::Message(m) = d {
                self.message(m, 0);
            }
        }
        for d in &f.decls {
            if let Decl::Enum(e) = d {
                self.enumeration(e, 0);
            }
        }
        self.file_tail(f);
    }

    fn file_tail(&mut self, f: &File) {
        if f.decls.iter().any(|d| matches!(d, Decl::Extend(_))) {
            self.out.push('\n');
        }
        for d in &f.decls {
            if let Decl::Extend(e) = d {
                self.extend(e, 0);
            }
        }
        if f.decls.iter().any(|d| matches!(d, Decl::Service(_))) {
            self.out.push('\n');
        }
        for d in &f.decls {
            if let Decl::Service(s) = d {
                self.service(s);
            }
        }
    }

    /// A field's options: one inline, several one to a line (its own, then `default`, then `json_name`).
    fn field_options(&self, opts: &[Opt], indent: usize) -> String {
        let rank = |o: &&Opt| match o.simple() {
            Some("default") => 1,
            Some("json_name") => 2,
            _ => 0,
        };
        let mut sorted: Vec<&Opt> = opts.iter().collect();
        sorted.sort_by_key(rank);
        let parts: Vec<String> = sorted.iter().map(|o| self.opt(o)).collect();
        match parts.as_slice() {
            [] => String::new(),
            [one] => format!(" [{one}]"),
            _ => {
                let pad = "  ".repeat(indent);
                format!(" [\n{pad}  {}\n{pad}]", parts.join(&format!(",\n{pad}  ")))
            }
        }
    }

    fn compact_options(&self, opts: &[Opt]) -> String {
        if opts.is_empty() {
            return String::new();
        }
        let parts: Vec<String> = opts.iter().map(|o| self.opt(o)).collect();
        format!(" [{}]", parts.join(", "))
    }

    fn field(&mut self, f: &Field, indent: usize) {
        let label = f.label.map_or(String::new(), |l| format!("{} ", l.word()));
        let ty = match &f.ty {
            TypeRef::Named(_, span) => self.raw(*span),
            TypeRef::Map(_, k, _, v) => format!("map<{}, {}>", self.raw(*k), self.raw(*v)),
        };
        let s = format!(
            "{label}{ty} {} = {}{};",
            f.name,
            self.raw(f.tag),
            self.field_options(&f.opts, indent)
        );
        self.line(indent, &s);
    }

    fn group(&mut self, g: &Group, indent: usize) {
        let label = g.label.map_or(String::new(), |l| format!("{} ", l.word()));
        let s = format!(
            "{label}group {} = {}{} {{",
            g.name,
            self.raw(g.tag),
            self.compact_options(&g.opts)
        );
        self.line(indent, &s);
        self.body(&g.body, indent + 1);
        self.line(indent, "}");
    }

    fn message(&mut self, m: &Message, indent: usize) {
        if m.body.is_empty() {
            // Confluent prints an empty message on one line (recorded from a descriptor).
            self.line(indent, &format!("message {} {{}}", m.name));
            return;
        }
        self.line(indent, &format!("message {} {{", m.name));
        self.body(&m.body, indent + 1);
        self.line(indent, "}");
    }

    fn body(&mut self, items: &[Item], indent: usize) {
        let mut first = true;
        // Each section: which items are in it.
        let sections: [fn(&Item) -> bool; 8] = [
            |i| matches!(i, Item::Reserved(_)),
            |i| matches!(i, Item::Option(_)),
            |i| matches!(i, Item::Field(_)),
            |i| matches!(i, Item::Oneof(_)),
            |i| matches!(i, Item::Group(_)),
            |i| matches!(i, Item::Message(_) | Item::Enum(_)),
            |i| matches!(i, Item::Extend(_)),
            |i| matches!(i, Item::Extensions(..)),
        ];
        for (n, pick) in sections.iter().enumerate() {
            if !items.iter().any(pick) {
                continue;
            }
            if !std::mem::take(&mut first) {
                self.blank(indent - 1);
            }
            if n == 5 {
                // Nested messages, then nested enums.
                for i in items {
                    if let Item::Message(m) = i {
                        self.message(m, indent);
                    }
                }
                for i in items {
                    if let Item::Enum(e) = i {
                        self.enumeration(e, indent);
                    }
                }
                continue;
            }
            for i in items.iter().filter(|i| pick(i)) {
                self.item(i, indent);
            }
        }
    }

    fn item(&mut self, i: &Item, indent: usize) {
        match i {
            Item::Reserved(r) | Item::Extensions(r, _) => {
                // The statement as written (its options included).
                let mut s = self.raw(r.span);
                s.push(';');
                self.line(indent, &s);
            }
            Item::Option(o) => {
                let s = format!("option {};", self.opt(o));
                self.line(indent, &s);
            }
            Item::Field(f) => self.field(f, indent),
            Item::Group(g) => self.group(g, indent),
            Item::Oneof(o) => {
                self.line(indent, &format!("oneof {} {{", o.name));
                for d in &o.body {
                    self.item(d, indent + 1);
                }
                self.line(indent, "}");
            }
            Item::Extend(e) => self.extend(e, indent),
            Item::Message(m) => self.message(m, indent),
            Item::Enum(e) => self.enumeration(e, indent),
        }
    }

    fn enumeration(&mut self, e: &Enum, indent: usize) {
        self.line(indent, &format!("enum {} {{", e.name));
        for d in &e.body {
            match d {
                EnumItem::Option(o) => {
                    let s = format!("option {};", self.opt(o));
                    self.line(indent + 1, &s);
                }
                EnumItem::Reserved(r) => {
                    let s = format!("{};", self.raw(r.span));
                    self.line(indent + 1, &s);
                }
                EnumItem::Value(_) => {}
            }
        }
        for d in &e.body {
            if let EnumItem::Value(v) = d {
                let s = format!(
                    "{} = {}{};",
                    v.name,
                    self.raw(v.num_span),
                    self.compact_options(&v.opts)
                );
                self.line(indent + 1, &s);
            }
        }
        self.line(indent, "}");
    }

    fn extend(&mut self, e: &Extend, indent: usize) {
        let s = format!("extend {} {{", self.raw(e.span));
        self.line(indent, &s);
        for i in &e.body {
            self.item(i, indent + 1);
        }
        self.line(indent, "}");
    }

    fn service(&mut self, s: &Service) {
        self.line(0, &format!("service {} {{", s.name));
        for o in &s.opts {
            let t = format!("option {};", self.opt(o));
            self.line(1, &t);
        }
        for r in &s.rpcs {
            let end = |(stream, _, span): &(bool, String, Span)| {
                format!(
                    "{}{}",
                    if *stream { "stream " } else { "" },
                    self.raw(*span)
                )
            };
            let mut t = String::new();
            let _ = write!(
                t,
                "rpc {} ({}) returns ({})",
                r.name,
                end(&r.input),
                end(&r.output)
            );
            if r.opts.is_empty() {
                t.push(';');
                self.line(1, &t);
                continue;
            }
            // Not recorded from Confluent; written as its printer (Square's wire) writes an rpc with options.
            t.push_str(" {");
            self.line(1, &t);
            for o in &r.opts {
                let t = format!("option {};", self.opt(o));
                self.line(2, &t);
            }
            self.line(1, "};");
        }
        self.line(0, "}");
    }
}
