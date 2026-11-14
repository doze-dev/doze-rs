//! The text an Avro schema is stored as: the way Avro's own library writes a schema (`Schema.toString()`), because that is
//! what Confluent stores. The same schema sent with its keys in another order, or with a redundant namespace, is one
//! schema with one id. The order is fixed (type, name, namespace, doc, then the type's own attributes, then any others in
//! the order they came, or sorted with `?normalize=true`); a namespace is written only where it differs from the
//! enclosing one; a reference to a named type is written as short as the enclosing namespace allows; and an unnamed type
//! with nothing but Avro's own attributes is written as its bare name (`{"type":"string","doc":"d"}` is `"string"`).
//!
//! This is not Avro's Parsing Canonical Form, which drops docs, defaults and aliases: Confluent keeps them, and so a
//! schema read back is the schema registered.

use std::collections::HashSet;

use super::schema::split_name;
use crate::json::{Json, write_str};

const PRIMITIVES: [&str; 8] = [
    "null", "boolean", "int", "long", "float", "double", "bytes", "string",
];

/// The attributes Avro gives a meaning to. On an unnamed type the ones that are not its own are dropped, as Avro's parser
/// drops them; any other attribute is kept as a property.
const RESERVED: [&str; 10] = [
    "type",
    "doc",
    "fields",
    "items",
    "name",
    "namespace",
    "size",
    "symbols",
    "values",
    "aliases",
];

/// Writes `schema` as it is stored. `known` are the full names that referenced schemas define.
///
/// # Errors
/// The JSON is not the shape of an Avro schema (the parser, which runs first, says so in more words).
pub(crate) fn stored_text(
    schema: &Json,
    known: &[String],
    normalize: bool,
) -> Result<String, String> {
    let mut w = Writer {
        out: String::new(),
        defined: known.iter().cloned().collect(),
        sort_all: normalize,
    };
    w.schema(schema, None)?;
    Ok(w.out)
}

/// The full names a schema defines, nested ones included.
pub(crate) fn defined_names(schema: &Json) -> Vec<String> {
    let mut out = Vec::new();
    walk_defs(schema, None, &mut |full, _| out.push(full.to_owned()));
    out
}

/// Calls `f` with the full name and the declaration of every named type `n` defines.
pub(crate) fn walk_defs(n: &Json, enclosing: Option<&str>, f: &mut dyn FnMut(&str, &Json)) {
    match n {
        Json::Array(items) => items.iter().for_each(|it| walk_defs(it, enclosing, f)),
        Json::Object(_) => {
            let Some(t) = n.get("type") else { return };
            let Json::Str(word) = t else {
                walk_defs(t, enclosing, f);
                return;
            };
            match word.as_str() {
                "record" | "error" | "enum" | "fixed" => {
                    let name = n.get("name").and_then(Json::as_str).unwrap_or_default();
                    let (space, simple) = split_name(n, name, enclosing);
                    f(&super::schema::full_name(&simple, space.as_deref()), n);
                    if let Some(Json::Array(fields)) = n.get("fields") {
                        for fld in fields {
                            if let Some(ft) = fld.get("type") {
                                walk_defs(ft, space.as_deref(), f);
                            }
                        }
                    }
                }
                "array" => {
                    if let Some(it) = n.get("items") {
                        walk_defs(it, enclosing, f);
                    }
                }
                "map" => {
                    if let Some(v) = n.get("values") {
                        walk_defs(v, enclosing, f);
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
}

struct Writer {
    out: String,
    /// The full names of the named types seen so far.
    defined: HashSet<String>,
    /// `?normalize=true`: the other attributes in alphabetical order.
    sort_all: bool,
}

impl Writer {
    fn key(&mut self, first: &mut bool, key: &str) {
        if !*first {
            self.out.push(',');
        }
        *first = false;
        write_str(&mut self.out, key);
        self.out.push(':');
    }

    /// Writes the attributes of `n` that are not in `reserved`, in the order they came (sorted when normalizing).
    fn props(&mut self, first: &mut bool, n: &Json, reserved: &[&str]) {
        let Json::Object(m) = n else { return };
        let mut keep: Vec<&(String, Json)> = m
            .iter()
            .filter(|(k, _)| !reserved.contains(&k.as_str()))
            .collect();
        if self.sort_all {
            keep.sort_by(|a, b| a.0.cmp(&b.0));
        }
        for (k, v) in keep {
            self.key(first, k);
            v.write(&mut self.out);
        }
    }

    fn schema(&mut self, n: &Json, enclosing: Option<&str>) -> Result<(), String> {
        match n {
            Json::Str(s) => {
                self.reference(s, enclosing);
                Ok(())
            }
            Json::Array(items) => {
                self.out.push('[');
                for (i, it) in items.iter().enumerate() {
                    if i > 0 {
                        self.out.push(',');
                    }
                    self.schema(it, enclosing)?;
                }
                self.out.push(']');
                Ok(())
            }
            Json::Object(_) => self.object(n, enclosing),
            Json::Raw(_) => Err("not an Avro schema".into()),
        }
    }

    fn reference(&mut self, name: &str, enclosing: Option<&str>) {
        if PRIMITIVES.contains(&name) {
            write_str(&mut self.out, name);
            return;
        }
        let (space, simple) = match name.rsplit_once('.') {
            Some((s, simple)) => (Some(s.to_owned()), simple),
            None => match enclosing {
                Some(e) if self.defined.contains(&format!("{e}.{name}")) => {
                    (Some(e.to_owned()), name)
                }
                Some(e) if !e.is_empty() && !self.defined.contains(name) => {
                    (Some(e.to_owned()), name)
                }
                _ => (None, name),
            },
        };
        match space {
            Some(s) if !s.is_empty() && Some(s.as_str()) != enclosing => {
                write_str(&mut self.out, &format!("{s}.{simple}"));
            }
            _ => write_str(&mut self.out, simple),
        }
    }

    fn object(&mut self, n: &Json, enclosing: Option<&str>) -> Result<(), String> {
        let t = n.get("type").ok_or("schema has no type")?;
        let Json::Str(word) = t else {
            // {"type": {...}} or {"type": [...]}: the inner schema is the schema.
            return self.schema(t, enclosing);
        };
        let word = word.as_str();
        let unnamed_bare = !matches!(
            word,
            "record" | "error" | "enum" | "fixed" | "array" | "map"
        ) && matches!(n, Json::Object(m) if m.iter().all(|(k, _)| RESERVED.contains(&k.as_str())));
        if unnamed_bare {
            self.reference(word, enclosing);
            return Ok(());
        }
        let mut first = true;
        self.out.push('{');
        match word {
            "record" | "error" | "enum" | "fixed" => self.named(n, word, enclosing, &mut first)?,
            "array" | "map" => {
                let inner = if word == "array" { "items" } else { "values" };
                self.key(&mut first, "type");
                write_str(&mut self.out, word);
                self.key(&mut first, inner);
                if let Some(it) = n.get(inner) {
                    self.schema(it, enclosing)?;
                }
                self.props(&mut first, n, &RESERVED);
            }
            _ => {
                // A primitive with attributes ({"type":"long","logicalType":"timestamp-millis"}).
                self.key(&mut first, "type");
                write_str(&mut self.out, word);
                self.props(&mut first, n, &RESERVED);
            }
        }
        self.out.push('}');
        Ok(())
    }

    fn named(
        &mut self,
        n: &Json,
        word: &str,
        enclosing: Option<&str>,
        first: &mut bool,
    ) -> Result<(), String> {
        let name = n.get("name").and_then(Json::as_str).unwrap_or_default();
        let (space, simple) = split_name(n, name, enclosing);
        self.defined
            .insert(super::schema::full_name(&simple, space.as_deref()));
        self.key(first, "type");
        write_str(&mut self.out, word);
        self.key(first, "name");
        write_str(&mut self.out, &simple);
        match (&space, enclosing) {
            (Some(s), e) if Some(s.as_str()) != e => {
                self.key(first, "namespace");
                write_str(&mut self.out, s);
            }
            (None, Some(_)) => {
                self.key(first, "namespace");
                write_str(&mut self.out, "");
            }
            _ => {}
        }
        if let Some(d) = n.get("doc") {
            self.key(first, "doc");
            d.write(&mut self.out);
        }
        match word {
            "enum" => {
                self.attr(first, n, "symbols");
                self.attr(first, n, "default");
                self.props(
                    first,
                    n,
                    &[
                        "type",
                        "name",
                        "namespace",
                        "doc",
                        "symbols",
                        "default",
                        "aliases",
                    ],
                );
            }
            "fixed" => {
                self.attr(first, n, "size");
                self.props(
                    first,
                    n,
                    &["type", "name", "namespace", "doc", "size", "aliases"],
                );
            }
            _ => {
                self.key(first, "fields");
                self.out.push('[');
                if let Some(Json::Array(fields)) = n.get("fields") {
                    for (i, fld) in fields.iter().enumerate() {
                        if i > 0 {
                            self.out.push(',');
                        }
                        self.field(fld, space.as_deref())?;
                    }
                }
                self.out.push(']');
                self.props(
                    first,
                    n,
                    &["type", "name", "namespace", "doc", "fields", "aliases"],
                );
            }
        }
        if let Some(a) = n.get("aliases") {
            self.key(first, "aliases");
            a.write(&mut self.out);
        }
        Ok(())
    }

    /// Writes `key` when `n` has it (an enum's symbols are written, empty, even when it has none).
    fn attr(&mut self, first: &mut bool, n: &Json, key: &str) {
        match n.get(key) {
            Some(v) => {
                self.key(first, key);
                v.write(&mut self.out);
            }
            None if key == "symbols" || key == "size" => self.key(first, key),
            None => {}
        }
    }

    /// One field of a record: name, type, doc, default, order, aliases, then any other attributes.
    fn field(&mut self, f: &Json, enclosing: Option<&str>) -> Result<(), String> {
        if !matches!(f, Json::Object(_)) {
            return Err("a field must be an object".into());
        }
        let mut first = true;
        self.out.push('{');
        if let Some(nm) = f.get("name") {
            self.key(&mut first, "name");
            nm.write(&mut self.out);
        }
        if let Some(ty) = f.get("type") {
            self.key(&mut first, "type");
            self.schema(ty, enclosing)?;
        }
        for key in ["doc", "default", "order", "aliases"] {
            if let Some(v) = f.get(key) {
                self.key(&mut first, key);
                v.write(&mut self.out);
            }
        }
        self.props(
            &mut first,
            f,
            &["name", "type", "doc", "default", "order", "aliases"],
        );
        self.out.push('}');
        Ok(())
    }
}
