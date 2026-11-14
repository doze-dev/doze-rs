//! Protobuf compatibility as Confluent's registry decides it (its `SchemaDiff`): a comparison of declarations, not a
//! proof about bytes on the wire. Messages are matched by name and fields by number. A field may be renamed, added or
//! removed; it may not change to a type that is not wire-compatible, nor to another message, nor (where the syntax
//! writes labels) between optional, required and repeated for numbers. Removing a message or changing the package
//! breaks readers; enums and services are not examined. Held to Confluent's verdicts and sentences for every pair of
//! the recorded corpus (`tests/data/compat/protobuf.json`, `protobuf-reasons.json`), as the Go edition's
//! `protocompat.go` was.

use std::collections::BTreeMap;

use super::link::{Card, Fld, Msg, Pool, Ty};
use crate::format::{CompatReason, Role};

/// Confluent's sentence by change type, the reader written as the new schema.
fn sentence(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "FIELD_KIND_CHANGED" => "The type of a field at path '{}' in the new schema does not match the old schema",
        "FIELD_MOVED_TO_EXISTING_ONEOF" => {
            "A field in the oneof at path '{}' in the new schema is outside an existing oneof in the old schema"
        }
        "FIELD_NAMED_TYPE_CHANGED" => {
            "The type of a MESSAGE field at path '{}' in the new schema does not match its type in the old schema"
        }
        "FIELD_NUMERIC_LABEL_CHANGED" => {
            "The label for a NUMERIC field at path '{}' in the new schema does not match its label in the old schema"
        }
        "FIELD_SCALAR_KIND_CHANGED" => {
            "The kind of a SCALAR field at path '{}' in the new schema does not match its kind in the old schema"
        }
        "MESSAGE_REMOVED" => "The new schema is missing a MESSAGE type at path '{}' in the old schema",
        "MULTIPLE_FIELDS_MOVED_TO_ONEOF" => {
            "Multiple fields in the oneof at path '{}' in the new schema are outside a oneof in the old schema"
        }
        "ONEOF_FIELD_REMOVED" => "The new schema is missing a oneof field at path '{}' in the old schema",
        "PACKAGE_CHANGED" => "The package at '{}' in the new schema does not match the package in the old schema",
        "REQUIRED_FIELD_ADDED" => "A required field  at path '{}' in the new schema is missing in the old schema",
        "REQUIRED_FIELD_REMOVED" => "The new schema is missing a required field at path: '{}' in the old schema",
        _ => return None,
    })
}

/// One side of the comparison: the pool and which of its files is the schema.
#[derive(Clone, Copy)]
struct Side<'a> {
    pool: &'a Pool,
    file: usize,
}

impl<'a> Side<'a> {
    fn proto3_of(self, m: &Msg) -> bool {
        self.pool.files.get(m.file).is_some_and(|f| f.proto3)
    }

    /// Every message of the file, nested ones too, by name relative to the package; map entries left out.
    fn messages(self) -> BTreeMap<String, &'a Msg> {
        let mut out = BTreeMap::new();
        let Some(f) = self.pool.files.get(self.file) else {
            return out;
        };
        let prefix = format!("{}.", f.package);
        let mut todo: Vec<usize> = f.msgs.clone();
        while let Some(i) = todo.pop() {
            let Some(m) = self.pool.msg(i).filter(|m| !m.map_entry) else {
                continue;
            };
            let name = if f.package.is_empty() { &m.full } else { m.full.strip_prefix(&prefix).unwrap_or(&m.full) };
            out.insert(name.to_owned(), m);
            todo.extend(&m.nested);
        }
        out
    }

    fn entry(self, f: &Fld) -> Option<&'a Msg> {
        match f.ty {
            Ty::Message(m) if f.card == Card::Repeated => self.pool.msg(m).filter(|m| m.map_entry),
            _ => None,
        }
    }

    /// The message a field names, without the package when it is of this file.
    fn type_name(self, f: &Fld) -> String {
        let (Ty::Message(m) | Ty::Group(m)) = f.ty else {
            return String::new();
        };
        let Some(m) = self.pool.msg(m) else {
            return String::new();
        };
        if m.file == self.file
            && let Some(p) = self.pool.files.get(self.file).filter(|f| !f.package.is_empty())
        {
            return m.full.strip_prefix(&format!("{}.", p.package)).unwrap_or(&m.full).to_owned();
        }
        m.full.clone()
    }
}

/// The wire-compatible family of a field's type.
fn family(f: &Fld) -> &'static str {
    match f.ty {
        Ty::Scalar(3 | 4 | 5 | 8 | 13) | Ty::Enum(_) => "varint",
        Ty::Scalar(17 | 18) => "zigzag",
        Ty::Scalar(7 | 15) => "fixed32",
        Ty::Scalar(6 | 16) => "fixed64",
        Ty::Scalar(9 | 12) => "bytes",
        Ty::Scalar(2) => "float",
        Ty::Scalar(1) => "double",
        _ => "message",
    }
}

fn length_delimited(f: &Fld) -> bool {
    matches!(f.ty, Ty::Scalar(9 | 12) | Ty::Message(_) | Ty::Group(_))
}

/// A field's label where the syntax writes one; "" for a plain proto3 field or a oneof's.
fn label(f: &Fld, proto3: bool) -> &'static str {
    match f.card {
        Card::Repeated => "repeated",
        Card::Required => "required",
        _ if f.oneof.is_some() => "",
        _ if !proto3 || f.explicit => "optional",
        _ => "",
    }
}

/// Where Confluent says a field is: its message, its oneof if it is in one, its number.
fn field_path(name: &str, m: &Msg, f: &Fld) -> String {
    match f.oneof.and_then(|o| m.oneofs.get(o)) {
        Some(o) => format!("#/{name}/{o}/{}", f.number),
        None => format!("#/{name}/{}", f.number),
    }
}

struct Diff<'a> {
    old: Side<'a>,
    new: Side<'a>,
    changes: Vec<(&'static str, String, String)>,
}

impl Diff<'_> {
    fn add(&mut self, kind: &'static str, path: String, text: &str) {
        if !matches!(kind, "MESSAGE_ADDED" | "FIELD_ADDED" | "FIELD_REMOVED" | "ONEOF_FIELD_ADDED") {
            self.changes.push((kind, path, text.to_owned()));
        }
    }

    fn files(&mut self) {
        let package = |s: Side<'_>| s.pool.files.get(s.file).map(|f| f.package.clone());
        if package(self.old) != package(self.new) {
            self.add("PACKAGE_CHANGED", "#".into(), "");
        }
        // By name, so a message's own fields come before what is missing among the messages nested in it.
        let (om, nm) = (self.old.messages(), self.new.messages());
        for (name, o) in &om {
            match nm.get(name) {
                Some(n) => self.messages(name, o, n),
                None => self.add("MESSAGE_REMOVED", format!("#/{name}"), ""),
            }
        }
    }

    fn messages(&mut self, name: &str, o: &Msg, n: &Msg) {
        fn by_number(m: &Msg) -> BTreeMap<i32, &Fld> {
            m.fields.iter().map(|f| (f.number, f)).collect()
        }
        let (of, nf) = (by_number(o), by_number(n));
        for (num, f) in &of {
            let mut path = field_path(name, o, f);
            let Some(g) = nf.get(num) else {
                let kind = if f.card == Card::Required { "REQUIRED_FIELD_REMOVED" } else { "FIELD_REMOVED" };
                self.add(kind, path, "");
                continue;
            };
            if f.oneof.is_none() {
                path = field_path(name, n, g); // in a oneof on the new side only
            }
            self.fields(&path, o, f, n, g);
        }
        for (num, g) in &nf {
            if !of.contains_key(num) {
                let kind = if g.card == Card::Required { "REQUIRED_FIELD_ADDED" } else { "FIELD_ADDED" };
                self.add(kind, field_path(name, n, g), "");
            }
        }
        self.oneofs(name, o, n, &of);
    }

    /// A field that was ordinary and joins a oneof: allowed into a new oneof one at a time, never into one that
    /// existed; and a oneof in both loses a member only by breaking readers. A oneof that lost a member is reported
    /// for that alone, not also for a field moved into it (recorded: "random 045" against "random 161").
    fn oneofs(&mut self, name: &str, o: &Msg, n: &Msg, of: &BTreeMap<i32, &Fld>) {
        let lost = |i: usize, one: &String| {
            o.oneofs.iter().position(|x| x == one).is_some_and(|j| {
                o.fields
                    .iter()
                    .filter(|f| f.oneof == Some(j))
                    .any(|f| !n.fields.iter().any(|g| g.oneof == Some(i) && g.number == f.number))
            })
        };
        for (i, one) in n.oneofs.iter().enumerate() {
            let moved = n
                .fields
                .iter()
                .filter(|f| f.oneof == Some(i) && of.get(&f.number).is_some_and(|g| g.oneof.is_none()))
                .count();
            if moved > 0 && o.oneofs.contains(one) {
                if !lost(i, one) {
                    self.add("FIELD_MOVED_TO_EXISTING_ONEOF", format!("#/{name}/{one}"), "");
                }
            } else if moved > 1 {
                self.add("MULTIPLE_FIELDS_MOVED_TO_ONEOF", format!("#/{name}/{one}"), "");
            }
        }
        for (i, one) in n.oneofs.iter().enumerate() {
            let Some(j) = o.oneofs.iter().position(|x| x == one) else {
                continue;
            };
            for f in o.fields.iter().filter(|f| f.oneof == Some(j)) {
                if !n.fields.iter().any(|g| g.oneof == Some(i) && g.number == f.number) {
                    self.add("ONEOF_FIELD_REMOVED", format!("#/{name}/{one}/{}", f.number), "");
                }
            }
        }
    }

    fn fields(&mut self, path: &str, om: &Msg, o: &Fld, nm: &Msg, n: &Fld) {
        let before = self.changes.len();
        // The kind first: a field whose kind changed is reported for that alone.
        match (self.old.entry(o), self.new.entry(n)) {
            (Some(oe), Some(ne)) => {
                // A map's key and value are reported at the map field.
                for (a, b) in oe.fields.iter().zip(&ne.fields) {
                    self.kinds(path, a, b);
                }
            }
            (None, None) => self.kinds(path, o, n),
            _ => self.add("FIELD_KIND_CHANGED", path.to_owned(), ""),
        }
        if self.changes.len() > before {
            return;
        }
        let (ol, nl) = (label(o, self.old.proto3_of(om)), label(n, self.new.proto3_of(nm)));
        let both_delimited = ol != "required" && nl != "required" && length_delimited(o) && length_delimited(n);
        if !ol.is_empty() && !nl.is_empty() && ol != nl && !both_delimited {
            if length_delimited(o) {
                self.add(
                    "FIELD_LABEL_CHANGED",
                    path.to_owned(),
                    "The label of a field in the new schema does not match its label in the old schema",
                );
            } else {
                self.add("FIELD_NUMERIC_LABEL_CHANGED", path.to_owned(), "");
            }
        }
    }

    fn kinds(&mut self, path: &str, o: &Fld, n: &Fld) {
        let (og, ng) = (family(o), family(n));
        if og == "message" || ng == "message" {
            if og != ng {
                self.add("FIELD_KIND_CHANGED", path.to_owned(), "");
            } else if self.old.type_name(o) != self.new.type_name(n) {
                self.add("FIELD_NAMED_TYPE_CHANGED", path.to_owned(), "");
            }
        } else if og != ng {
            self.add("FIELD_SCALAR_KIND_CHANGED", path.to_owned(), "");
        }
    }
}

/// Why a reader of `reader` cannot read what a writer of `writer` wrote. The sentences name the reader the new
/// schema; when it is the old one (a FORWARD check) they name the two the other way round, as Confluent does.
pub(super) fn can_read(
    reader: (&Pool, usize),
    writer: (&Pool, usize),
    reader_is: Role,
    writer_is: Role,
) -> Vec<CompatReason> {
    let mut d = Diff {
        old: Side { pool: writer.0, file: writer.1 },
        new: Side { pool: reader.0, file: reader.1 },
        changes: Vec::new(),
    };
    d.files();
    d.changes
        .into_iter()
        .map(|(kind, path, text)| {
            // A nested message's name is a path: #/O/I, not #/O.I.
            let path = path.replace('.', "/");
            let text = match sentence(kind) {
                Some(s) => s.replace("{}", if path == "#" { "#/" } else { &path }),
                None => text,
            };
            let description = text
                .replace("new schema", "\u{1}")
                .replace("old schema", "\u{2}")
                .replace('\u{1}', &format!("{} schema", reader_is.word()))
                .replace('\u{2}', &format!("{} schema", writer_is.word()));
            CompatReason {
                kind: kind.to_owned(),
                text: format!("{{errorType:\"{kind}\", description:\"{description}\"}}"),
                path,
                description,
                info: String::new(),
            }
        })
        .collect()
}
