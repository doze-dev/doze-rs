//! A stored schema as the `FileDescriptorProto` Confluent answers for `?format=serialized` (base64): the form
//! confluent-kafka's Python, Go and JavaScript deserializers ask for. It is not protoc's descriptor but the one Confluent
//! builds from its own parse (recorded by `tools/registry-diff`): the file is named `default`; type names are as written
//! (no leading dot, nothing resolved); a message or enum field has its `type_name` and no `type`; a proto3 field without a
//! label has no label, one in a oneof is `LABEL_OPTIONAL`; a oneof's fields come before the other fields; a map is a
//! repeated field of a nested `<field>Entry` message added after the declared nested messages; a proto3 `optional` field
//! has a synthetic oneof `_<field>` after the declared ones; groups are left out; ranges end one past their last number;
//! an rpc's two streaming flags are always written; `syntax` is written when the text has it (recorded:
//! `protobuf-normalize/proto2`, whose normalized text has none).

use super::ast::{self, Decl, EnumItem, File, Item, Label, MAX_FIELD, MAX_WORD, Opt, TypeRef, Val};
use super::link::SCALARS;

mod options;

use options::{
    ENUM_OPTIONS, FIELD_OPTIONS, FILE_OPTIONS, MESSAGE_OPTIONS, METHOD_OPTIONS, Out,
    SERVICE_OPTIONS, VALUE_OPTIONS, options, string_value,
};

fn scalar(name: &str) -> Option<i64> {
    SCALARS
        .iter()
        .position(|s| !s.is_empty() && *s == name)
        .and_then(|p| i64::try_from(p).ok())
}

/// A field as Confluent writes it.
struct FieldIn<'a> {
    name: &'a str,
    number: i64,
    label: Option<Label>,
    /// The type as written, or a map's entry message.
    ty: &'a str,
    opts: &'a [Opt],
    oneof: Option<i64>,
    proto3_optional: bool,
    extendee: Option<&'a str>,
}

fn field(src: &str, f: &FieldIn<'_>, proto3: bool) -> Out {
    let mut m = Out::default();
    m.str(1, f.name);
    if let Some(e) = f.extendee {
        m.str(2, e);
    }
    m.int(3, f.number);
    let label = match f.label {
        Some(Label::Optional) => Some(1),
        Some(Label::Required) => Some(2),
        Some(Label::Repeated) => Some(3),
        None if f.oneof.is_some() || !proto3 => Some(1),
        None => None,
    };
    if let Some(l) = label {
        m.int(4, l);
    }
    match scalar(f.ty) {
        Some(t) => m.int(5, t),
        None => m.str(6, f.ty),
    }
    let mut default = None;
    let mut json = None;
    let mut rest = Vec::new();
    for o in f.opts {
        match o.simple() {
            Some("default") => default = Some(o),
            Some("json_name") => json = Some(string_value(src, o)),
            _ => rest.push(o),
        }
    }
    if let Some(d) = default {
        let text = match &d.val {
            Val::Str => string_value(src, d),
            Val::Ident(v) | Val::Num(v, _) => v.clone(),
            Val::Aggregate => String::new(),
        };
        m.str(7, &text);
    }
    if let Some(o) = options(src, &rest, FIELD_OPTIONS) {
        m.msg(8, &o);
    }
    if let Some(i) = f.oneof {
        m.int(9, i);
    }
    if let Some(j) = json {
        m.str(10, &j);
    }
    if f.proto3_optional {
        m.bool(17, true);
    }
    m
}

/// A reserved or extension range's end, one past its last number.
fn end_of(hi: i64) -> i64 {
    if hi == MAX_WORD {
        MAX_FIELD + 1
    } else {
        hi.saturating_add(1)
    }
}

fn range(lo: i64, hi: i64) -> Out {
    let mut r = Out::default();
    r.int(1, lo);
    r.int(2, hi);
    r
}

fn enumeration(src: &str, e: &ast::Enum) -> Out {
    let mut m = Out::default();
    m.str(1, &e.name);
    let mut opts = Vec::new();
    let mut reserved = Vec::new();
    for item in &e.body {
        match item {
            EnumItem::Value(v) => {
                let mut val = Out::default();
                val.str(1, &v.name);
                val.int(2, v.number);
                let os: Vec<&Opt> = v.opts.iter().collect();
                if let Some(o) = options(src, &os, VALUE_OPTIONS) {
                    val.msg(3, &o);
                }
                m.msg(2, &val);
            }
            EnumItem::Option(o) => opts.push(o),
            EnumItem::Reserved(r) => reserved.push(r),
        }
    }
    if let Some(o) = options(src, &opts, ENUM_OPTIONS) {
        m.msg(3, &o);
    }
    // An enum's reserved range is inclusive at both ends in the descriptor.
    for r in &reserved {
        for &(lo, hi) in &r.ranges {
            let hi = if hi == MAX_WORD {
                i64::from(i32::MAX)
            } else {
                hi
            };
            m.msg(4, &range(lo, hi));
        }
    }
    for r in &reserved {
        for n in &r.names {
            m.str(5, n);
        }
    }
    m
}

fn message(src: &str, msg: &ast::Message, proto3: bool) -> Out {
    let mut p = Parts {
        src,
        proto3,
        ..Parts::default()
    };
    // A oneof's fields first, as Confluent lists them.
    for item in &msg.body {
        if let Item::Oneof(o) = item {
            p.oneof(o);
        }
    }
    for item in &msg.body {
        p.item(item);
    }
    p.finish(&msg.name)
}

fn plain<'a>(
    name: &'a str,
    number: i64,
    label: Option<Label>,
    ty: &'a str,
    opts: &'a [Opt],
) -> FieldIn<'a> {
    FieldIn {
        name,
        number,
        label,
        ty,
        opts,
        oneof: None,
        proto3_optional: false,
        extendee: None,
    }
}

/// A message's parts, gathered in Confluent's order and written in field-number order.
#[derive(Default)]
struct Parts<'a> {
    src: &'a str,
    proto3: bool,
    fields: Vec<Out>,
    nested: Vec<Out>,
    entries: Vec<Out>,
    enums: Vec<Out>,
    extensions: Vec<Out>,
    ext_ranges: Vec<Out>,
    opts: Vec<&'a Opt>,
    oneofs: Vec<String>,
    synthetic: Vec<String>,
    reserved: Vec<Out>,
    reserved_names: Vec<&'a str>,
}

impl<'a> Parts<'a> {
    fn oneof(&mut self, o: &ast::Oneof) {
        let index = i64::try_from(self.oneofs.len()).unwrap_or(0);
        self.oneofs.push(o.name.clone());
        for f in &o.body {
            if let Item::Field(f) = f
                && let TypeRef::Named(ty, _) = &f.ty
            {
                let fi = FieldIn {
                    oneof: Some(index),
                    ..plain(&f.name, f.number, None, ty, &f.opts)
                };
                self.fields.push(field(self.src, &fi, self.proto3));
            }
        }
    }

    fn item(&mut self, item: &'a Item) {
        let (src, proto3) = (self.src, self.proto3);
        match item {
            Item::Field(f) => match &f.ty {
                TypeRef::Named(ty, _) => {
                    let p3opt = proto3 && f.label == Some(Label::Optional);
                    // A proto3 `optional` field is in a synthetic oneof, after the declared ones.
                    let oneof = p3opt.then(|| {
                        i64::try_from(self.oneofs.len() + self.synthetic.len()).unwrap_or(0)
                    });
                    if p3opt {
                        self.synthetic.push(format!("_{}", f.name));
                    }
                    let fi = FieldIn {
                        oneof,
                        proto3_optional: p3opt,
                        ..plain(&f.name, f.number, f.label, ty, &f.opts)
                    };
                    self.fields.push(field(src, &fi, proto3));
                }
                TypeRef::Map(k, _, v, _) => self.map(f, k, v),
            },
            Item::Message(m) => self.nested.push(message(src, m, proto3)),
            Item::Enum(e) => self.enums.push(enumeration(src, e)),
            Item::Extend(x) => self.extensions.extend(extend(src, x, proto3)),
            Item::Option(o) => self.opts.push(o),
            Item::Reserved(r) => {
                for &(lo, hi) in &r.ranges {
                    self.reserved.push(range(lo, end_of(hi)));
                }
                self.reserved_names
                    .extend(r.names.iter().map(String::as_str));
            }
            Item::Extensions(r, _) => {
                for &(lo, hi) in &r.ranges {
                    self.ext_ranges.push(range(lo, end_of(hi)));
                }
            }
            Item::Oneof(_) | Item::Group(_) => {}
        }
    }

    /// A map field: a repeated field of its `<field>Entry` message, added after the declared nested messages.
    fn map(&mut self, f: &ast::Field, k: &str, v: &str) {
        let entry = format!("{}Entry", f.name);
        let fi = plain(&f.name, f.number, Some(Label::Repeated), &entry, &f.opts);
        self.fields.push(field(self.src, &fi, self.proto3));
        let mut e = Out::default();
        e.str(1, &entry);
        for (name, num, ty) in [("key", 1, k), ("value", 2, v)] {
            e.msg(2, &field(self.src, &plain(name, num, None, ty, &[]), true));
        }
        let mut eo = Out::default();
        eo.bool(7, true);
        e.msg(7, &eo);
        self.entries.push(e);
    }

    fn finish(self, name: &str) -> Out {
        let mut m = Out::default();
        m.str(1, name);
        for f in &self.fields {
            m.msg(2, f);
        }
        for n in self.nested.iter().chain(&self.entries) {
            m.msg(3, n);
        }
        for e in &self.enums {
            m.msg(4, e);
        }
        for r in &self.ext_ranges {
            m.msg(5, r);
        }
        for x in &self.extensions {
            m.msg(6, x);
        }
        if let Some(o) = options(self.src, &self.opts, MESSAGE_OPTIONS) {
            m.msg(7, &o);
        }
        for n in self.oneofs.iter().chain(&self.synthetic) {
            let mut o = Out::default();
            o.str(1, n);
            m.msg(8, &o);
        }
        for r in &self.reserved {
            m.msg(9, r);
        }
        for n in self.reserved_names {
            m.str(10, n);
        }
        m
    }
}

fn extend(src: &str, x: &ast::Extend, proto3: bool) -> Vec<Out> {
    x.body
        .iter()
        .filter_map(|item| match item {
            Item::Field(f) => match &f.ty {
                TypeRef::Named(ty, _) => Some(field(
                    src,
                    &FieldIn {
                        name: &f.name,
                        number: f.number,
                        label: f.label,
                        ty,
                        opts: &f.opts,
                        oneof: None,
                        proto3_optional: false,
                        extendee: Some(&x.extendee),
                    },
                    proto3,
                )),
                TypeRef::Map(..) => None,
            },
            _ => None,
        })
        .collect()
}

fn service(src: &str, s: &ast::Service) -> Out {
    let mut sv = Out::default();
    sv.str(1, &s.name);
    for r in &s.rpcs {
        let mut me = Out::default();
        me.str(1, &r.name);
        me.str(2, &r.input.1);
        me.str(3, &r.output.1);
        let os: Vec<&Opt> = r.opts.iter().collect();
        if let Some(o) = options(src, &os, METHOD_OPTIONS) {
            me.msg(4, &o);
        }
        me.bool(5, r.input.0);
        me.bool(6, r.output.0);
        sv.msg(2, &me);
    }
    let os: Vec<&Opt> = s.opts.iter().collect();
    if let Some(o) = options(src, &os, SERVICE_OPTIONS) {
        sv.msg(3, &o);
    }
    sv
}

/// The descriptor of a stored schema's text, as Confluent serializes it.
pub(super) fn descriptor(src: &str, file: &File) -> Vec<u8> {
    let proto3 = file.syntax.as_ref().is_some_and(|(_, s)| s == "proto3");
    let mut m = Out::default();
    m.str(1, "default");
    let mut public = Vec::new();
    let mut weak = Vec::new();
    let mut deps = 0i64;
    let (mut msgs, mut enums, mut services, mut exts, mut opts) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for d in &file.decls {
        if let Decl::Package(p) = d {
            m.str(2, p);
        }
    }
    for d in &file.decls {
        match d {
            Decl::Import(kind, path, _) => {
                m.str(3, path);
                match *kind {
                    "public" => public.push(deps),
                    "weak" => weak.push(deps),
                    _ => {}
                }
                deps += 1;
            }
            Decl::Message(msg) => msgs.push(message(src, msg, proto3)),
            Decl::Enum(e) => enums.push(enumeration(src, e)),
            Decl::Service(s) => services.push(service(src, s)),
            Decl::Extend(x) => exts.extend(extend(src, x, proto3)),
            Decl::Option(o) => opts.push(o),
            Decl::Package(_) => {}
        }
    }
    for x in &msgs {
        m.msg(4, x);
    }
    for x in &enums {
        m.msg(5, x);
    }
    for x in &services {
        m.msg(6, x);
    }
    for x in &exts {
        m.msg(7, x);
    }
    if let Some(o) = options(src, &opts, FILE_OPTIONS) {
        m.msg(8, &o);
    }
    for i in public {
        m.int(10, i);
    }
    for i in weak {
        m.int(11, i);
    }
    // A file without a `syntax` line (a proto2 one as Confluent normalizes it) has none in its descriptor either.
    if file.syntax.is_some() {
        m.str(12, if proto3 { "proto3" } else { "proto2" });
    }
    m.0
}
