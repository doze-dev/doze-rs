//! A `.proto` schema normalized as Confluent normalizes it (`?normalize=true`, or the `normalize` setting): its
//! `ProtobufSchemaUtils.toNormalizedString`, written again here from Confluent's source (cp-schema-registry 8.3.2, the
//! same as 7.7.1 but where said) and held to its answers by tools/registry-diff (`protobuf-normalize`):
//!
//! - no `syntax` line for proto2; imports and public imports sorted, each once;
//! - every type name qualified with a leading dot (`.shop.v1.Money`), found as Confluent finds it: the main file's
//!   package and the enclosing names tried innermost first, among every type of the file, its references and the
//!   well-known files; a map's value type too, and a field of a map-entry message (a descriptor's) is its `map<,>`;
//! - options sorted by name, a custom option named by its extension's full name, a dotted one (`(a).b = 1`) made a
//!   message literal, a non-repeated custom option given twice merged into one (8.3.2; 7.7.1 makes a repeated key a
//!   list); a message literal's keys sorted and a one-item list written as the item; numbers as Java's `DecimalFormat`
//!   writes the number commons-lang's `createNumber` reads (`0x10` is `16`, `2.50` is `2.5`, three decimals at most);
//! - fields, a oneof's fields, groups and an extend's fields by number; oneofs by their first field, empty ones gone;
//!   enum values by number, then name; reserved and extension ranges one to a statement, by start (names after
//!   numbers, by name), a range of one number written as the number; the extends of one message type made one;
//! - messages, enums, services and rpcs stay in their order: a client numbers messages by their place.
//!
//! What it writes is the stored text itself, as Confluent stores the normalized string.

use std::collections::HashMap;

use super::ast::{
    Decl, Enum, EnumItem, Extend, Field, File, Group, Item, Label, MAX_FIELD, Message, Oneof, Opt,
    Reserved, Service, TypeRef, Val,
};
use super::lex::{Tok, lex};
use super::link::SCALARS;

/// A message's field, as Confluent's merging of options looks it up.
#[derive(Debug, Clone)]
struct FieldInfo {
    name: String,
    repeated: bool,
    ty: String,
}

/// What Confluent knows of a type by its full name: whether it is a map entry (its key and value types), and its
/// fields when it is a message.
#[derive(Debug, Default)]
struct TypeInfo {
    map: Option<(String, String)>,
    fields: Option<Vec<FieldInfo>>,
}

/// Every type and extension a schema sees, by full name (no leading dot), and the main file's package.
#[derive(Debug, Default)]
struct Names {
    package: Vec<String>,
    types: HashMap<String, TypeInfo>,
    exts: HashMap<String, FieldInfo>,
}

fn field_ty(t: &TypeRef) -> String {
    match t {
        TypeRef::Named(n, _) => n.clone(),
        TypeRef::Map(k, _, v, _) => format!("map<{k}, {v}>"),
    }
}

fn info(f: &Field) -> FieldInfo {
    FieldInfo {
        name: f.name.clone(),
        repeated: f.label == Some(Label::Repeated),
        ty: field_ty(&f.ty),
    }
}

fn join(path: &[String], name: &str) -> String {
    let mut p = path.join(".");
    if !p.is_empty() {
        p.push('.');
    }
    p.push_str(name);
    p
}

impl Names {
    /// Collects a file's types and extensions under its package, as Confluent's `Context.collectTypeInfo`.
    fn collect(&mut self, file: &File) {
        let package: Vec<String> = file
            .decls
            .iter()
            .find_map(|d| match d {
                Decl::Package(p) => Some(p.split('.').map(str::to_owned).collect()),
                _ => None,
            })
            .unwrap_or_default();
        let mut path = package.clone();
        for d in &file.decls {
            match d {
                Decl::Message(m) => self.message(&mut path, m),
                Decl::Enum(e) => {
                    self.types.insert(join(&path, &e.name), TypeInfo::default());
                }
                Decl::Extend(e) => self.extend(&path, e),
                _ => {}
            }
        }
        self.package = package;
    }

    fn extend(&mut self, path: &[String], e: &Extend) {
        for i in &e.body {
            if let Item::Field(f) = i {
                self.exts.insert(join(path, &f.name), info(f));
            }
        }
    }

    fn message(&mut self, path: &mut Vec<String>, m: &Message) {
        let full = join(path, &m.name);
        path.push(m.name.clone());
        let mut fields = Vec::new();
        let mut map_entry = false;
        for i in &m.body {
            match i {
                Item::Field(f) => fields.push(info(f)),
                Item::Oneof(o) => {
                    for x in &o.body {
                        if let Item::Field(f) = x {
                            fields.push(info(f));
                        }
                    }
                }
                Item::Option(o) => {
                    map_entry |= o.simple() == Some("map_entry")
                        && matches!(&o.val, Val::Ident(v) if v == "true");
                }
                Item::Message(n) => self.message(path, n),
                Item::Enum(e) => {
                    self.types.insert(join(path, &e.name), TypeInfo::default());
                }
                Item::Extend(e) => self.extend(path, e),
                _ => {}
            }
        }
        path.pop();
        let pick = |n: &str| fields.iter().find(|f| f.name == n).map(|f| f.ty.clone());
        let map = if map_entry {
            Some((
                pick("key").unwrap_or_default(),
                pick("value").unwrap_or_default(),
            ))
        } else {
            None
        };
        self.types.insert(
            full,
            TypeInfo {
                map,
                fields: Some(fields),
            },
        );
    }

    /// A name as Confluent's `Context.resolveFull` finds it from `scope` (the names entered so far): with a leading
    /// dot, as it is; otherwise prefixed by the package and the scope, innermost first, then on its own.
    fn resolve(&self, name: &str, scope: &[String], has: impl Fn(&str) -> bool) -> Option<String> {
        if let Some(n) = name.strip_prefix('.') {
            return has(n).then(|| n.to_owned());
        }
        let mut prefix: Vec<&str> = self.package.iter().map(String::as_str).collect();
        prefix.extend(scope.iter().map(String::as_str));
        while !prefix.is_empty() {
            let n = format!("{}.{name}", prefix.join("."));
            if has(&n) {
                return Some(n);
            }
            prefix.pop();
        }
        has(name).then(|| name.to_owned())
    }

    fn resolve_type(&self, name: &str, scope: &[String]) -> Option<String> {
        self.resolve(name, scope, |n| self.types.contains_key(n))
    }

    fn resolve_ext(&self, name: &str, scope: &[String]) -> Option<String> {
        self.resolve(name, scope, |n| self.exts.contains_key(n))
    }
}

mod members;
mod number;
mod options;
mod print;
mod value;

use number::java_format_number;
use value::{Named, V, escape, java_cmp, named, transform, wire_unquote};

/// Confluent's `appendIndented`: each line of `value` (Java's `split("\n")`, trailing empty lines dropped) indented by
/// two spaces.
fn indented(out: &mut String, value: &str) {
    let mut lines: Vec<&str> = value.split('\n').collect();
    while lines.last() == Some(&"") {
        lines.pop();
    }
    if lines.is_empty() && !value.contains('\n') {
        lines.push(value);
    }
    for l in lines {
        out.push_str("  ");
        out.push_str(l);
        out.push('\n');
    }
}

/// The printer: the names a schema sees and the names entered (messages and enums) around what is printed.
struct Printer<'a> {
    src: &'a str,
    names: &'a Names,
    scope: Vec<String>,
}

type Res<T> = Result<T, String>;

fn unresolved(name: &str) -> String {
    format!("Could not resolve type: {name}")
}

/// A field number or range bound as Confluent writes a range: `max` from the largest field number.
fn bound(n: i64) -> String {
    if n >= MAX_FIELD {
        "max".into()
    } else {
        n.to_string()
    }
}

/// Reserved ranges and names, one to a statement, by start (names last, by name).
fn reserved_lines(rs: &[&Reserved], word: &str, opts: &[String]) -> Vec<String> {
    let mut items: Vec<(i64, String, String)> = Vec::new();
    for (r, o) in rs
        .iter()
        .zip(opts.iter().chain(std::iter::repeat(&String::new())))
    {
        for &(a, b) in &r.ranges {
            let text = if a == b {
                a.to_string()
            } else {
                format!("{a} to {}", bound(b))
            };
            let key = if a == b {
                a.to_string()
            } else {
                format!("{a}..{b}")
            };
            items.push((a, key, format!("{word} {text}{o};\n")));
        }
        for n in &r.names {
            items.push((i64::from(i32::MAX), n.clone(), format!("{word} \"{n}\";\n")));
        }
    }
    if word == "extensions" {
        items.sort_by_key(|x| x.0);
    } else {
        items.sort_by(|x, y| x.0.cmp(&y.0).then_with(|| java_cmp(&x.1, &y.1)));
    }
    items.into_iter().map(|x| x.2).collect()
}

fn is_scalar(t: &str) -> bool {
    SCALARS.contains(&t) && !t.is_empty()
}

/// Confluent's `toKind` of a field's type, for its `default`.
fn kind_of(simple: &str, default: &str) -> V {
    match simple {
        "bool" => V::Bool(default.to_owned()),
        "string" => V::Str(default.to_owned()),
        "bytes" | "double" | "float" | "fixed32" | "fixed64" | "int32" | "int64" | "sfixed32"
        | "sfixed64" | "sint32" | "sint64" | "uint32" | "uint64" => V::Num(default.to_owned()),
        _ => V::Enum(default.to_owned()),
    }
}

/// The normalized text of `file` (read from `src`), with the files `deps` (its references, and the well-known files)
/// giving the names it may use.
///
/// # Errors
/// A type name Confluent's resolution does not find: `Could not resolve type: <name>`.
pub(super) fn normalize(src: &str, file: &File, deps: &[File]) -> Result<String, String> {
    let mut names = Names::default();
    for d in deps {
        names.collect(d);
    }
    names.collect(file);
    let mut p = Printer {
        src,
        names: &names,
        scope: Vec::new(),
    };
    p.file(file)
}

#[cfg(test)]
mod tests;
