//! An Avro schema read from its JSON, and the checks Confluent's parser (Avro's Java `Schema.Parser`) makes of it: names
//! are valid and defined once, unions hold no union and no type twice, records have fields and fields have names and
//! types, enums have symbols. Defaults are not checked against their types, as Confluent does not, with the one exception
//! it cannot avoid: an empty string as the default of a number.
//!
//! Named types live in an arena and are referred to by index, so a recursive type is a cycle of indexes, not of pointers.

use std::collections::HashMap;

use crate::json::Json;

/// A type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Ty {
    Null,
    Boolean,
    Int,
    Long,
    Float,
    Double,
    Bytes,
    String,
    Array(Box<Ty>),
    Map(Box<Ty>),
    Union(Vec<Ty>),
    /// A record, enum or fixed: an index into [`Schema::named`].
    Named(usize),
}

/// A record, enum or fixed.
#[derive(Debug, Clone)]
pub(crate) struct Named {
    /// The name with its namespace.
    pub(crate) full: String,
    /// The name without it.
    pub(crate) name: String,
    /// The aliases, each with the namespace it is in.
    pub(crate) aliases: Vec<String>,
    pub(crate) kind: NamedKind,
}

#[derive(Debug, Clone)]
pub(crate) enum NamedKind {
    Record(Vec<Field>),
    Enum {
        symbols: Vec<String>,
        has_default: bool,
    },
    Fixed(u64),
}

#[derive(Debug, Clone)]
pub(crate) struct Field {
    pub(crate) name: String,
    pub(crate) aliases: Vec<String>,
    pub(crate) ty: Ty,
    pub(crate) has_default: bool,
}

/// A parsed schema: its root type and every named type it and its references define.
#[derive(Debug, Clone, Default)]
pub(crate) struct Schema {
    pub(crate) root: Option<Ty>,
    pub(crate) named: Vec<Named>,
}

impl Schema {
    pub(crate) fn named(&self, i: usize) -> Option<&Named> {
        self.named.get(i)
    }
}

const PRIMITIVES: [&str; 8] = [
    "null", "boolean", "int", "long", "float", "double", "bytes", "string",
];

pub(crate) fn primitive(name: &str) -> Option<Ty> {
    Some(match name {
        "null" => Ty::Null,
        "boolean" => Ty::Boolean,
        "int" => Ty::Int,
        "long" => Ty::Long,
        "float" => Ty::Float,
        "double" => Ty::Double,
        "bytes" => Ty::Bytes,
        "string" => Ty::String,
        _ => return None,
    })
}

/// A name or a symbol, as Avro's Java parser checks one: a letter or `_`, then letters, digits and `_`.
fn valid_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
}

/// The full name of `name` in namespace `space`: a dotted name is already full.
pub(crate) fn full_name(name: &str, space: Option<&str>) -> String {
    match space {
        Some(s) if !s.is_empty() && !name.contains('.') => format!("{s}.{name}"),
        _ => name.to_owned(),
    }
}

/// Reads schemas into one arena: the references first, then the schema that uses them.
#[derive(Debug, Default)]
pub(crate) struct Parser {
    schema: Schema,
    names: HashMap<String, usize>,
}

type Res<T> = Result<T, String>;

impl Parser {
    /// Reads one schema; its named types join the ones read before.
    pub(crate) fn read(&mut self, j: &Json) -> Res<Ty> {
        self.ty(j, None)
    }

    pub(crate) fn finish(mut self, root: Ty) -> Schema {
        self.schema.root = Some(root);
        self.schema
    }

    fn lookup(&self, name: &str, space: Option<&str>) -> Res<Ty> {
        if let Some(t) = primitive(name) {
            return Ok(t);
        }
        let found = self
            .names
            .get(&full_name(name, space))
            .or_else(|| self.names.get(name));
        found
            .map(|&i| Ty::Named(i))
            .ok_or_else(|| format!("Undefined name: \"{name}\""))
    }

    fn ty(&mut self, j: &Json, space: Option<&str>) -> Res<Ty> {
        match j {
            Json::Str(name) => self.lookup(name, space),
            Json::Array(items) => self.union(items, space),
            Json::Object(_) => self.object(j, space),
            Json::Raw(r) => Err(format!("No type: {r}")),
        }
    }

    fn union(&mut self, items: &[Json], space: Option<&str>) -> Res<Ty> {
        let mut out = Vec::with_capacity(items.len());
        let mut seen: Vec<String> = Vec::new();
        for it in items {
            let t = self.ty(it, space)?;
            let key = match &t {
                Ty::Union(_) => return Err("Nested union".into()),
                Ty::Named(i) => self
                    .schema
                    .named(*i)
                    .map(|n| n.full.clone())
                    .unwrap_or_default(),
                other => type_word(other).to_owned(),
            };
            if seen.contains(&key) {
                return Err(format!("Duplicate in union:{key}"));
            }
            seen.push(key);
            out.push(t);
        }
        Ok(Ty::Union(out))
    }

    fn object(&mut self, j: &Json, space: Option<&str>) -> Res<Ty> {
        let t = j.get("type").ok_or("No type")?;
        let Json::Str(word) = t else {
            return self.ty(t, space);
        };
        match word.as_str() {
            "record" | "error" | "enum" | "fixed" => self.define(j, word, space),
            "array" => {
                let items = j.get("items").ok_or("Array has no items type")?;
                Ok(Ty::Array(Box::new(self.ty(items, space)?)))
            }
            "map" => {
                let values = j.get("values").ok_or("Map has no values type")?;
                Ok(Ty::Map(Box::new(self.ty(values, space)?)))
            }
            other => self.lookup(other, space),
        }
    }

    /// A named type: its name is taken before its fields are read, so a record can name itself.
    fn define(&mut self, j: &Json, word: &str, enclosing: Option<&str>) -> Res<Ty> {
        let name = j
            .get("name")
            .and_then(Json::as_str)
            .ok_or("No name in schema")?;
        let (space, simple) = split_name(j, name, enclosing);
        for part in name
            .split('.')
            .chain(space.iter().flat_map(|s| s.split('.')))
        {
            if !valid_name(part) && !(part.is_empty() && space.as_deref() == Some("")) {
                return Err(format!("Illegal character in: {name}"));
            }
        }
        if PRIMITIVES.contains(&simple.as_str()) {
            return Err(format!(
                "Schemas may not be named after primitives: {simple}"
            ));
        }
        let full = full_name(&simple, space.as_deref());
        if self.names.contains_key(&full) {
            return Err(format!("Can't redefine: {full}"));
        }
        let aliases = aliases(j)?
            .iter()
            .map(|a| full_name(a, space.as_deref()))
            .collect();
        let index = self.schema.named.len();
        self.schema.named.push(Named {
            full: full.clone(),
            name: simple,
            aliases,
            kind: NamedKind::Fixed(0),
        });
        self.names.insert(full, index);
        let kind = match word {
            "enum" => enum_kind(j)?,
            "fixed" => {
                let size = j.get("size").and_then(|s| match s {
                    Json::Raw(r) => r.parse::<u32>().ok(),
                    _ => None,
                });
                NamedKind::Fixed(u64::from(size.ok_or("Invalid or no size")?))
            }
            _ => NamedKind::Record(self.fields(j, space.as_deref())?),
        };
        if let Some(n) = self.schema.named.get_mut(index) {
            n.kind = kind;
        }
        Ok(Ty::Named(index))
    }

    fn fields(&mut self, j: &Json, space: Option<&str>) -> Res<Vec<Field>> {
        let Some(Json::Array(items)) = j.get("fields") else {
            return Err("Record has no fields".into());
        };
        let mut out: Vec<Field> = Vec::with_capacity(items.len());
        for f in items {
            if !matches!(f, Json::Object(_)) {
                return Err("Not a field".into());
            }
            let name = f
                .get("name")
                .and_then(Json::as_str)
                .ok_or("No field name")?;
            if !valid_name(name) {
                return Err(format!("Illegal character in: {name}"));
            }
            let tj = f
                .get("type")
                .ok_or_else(|| format!("No field type: {name}"))?;
            let ty = self.ty(tj, space)?;
            let default = f.get("default");
            if let (Some(Json::Str(d)), Json::Str(t)) = (default, tj)
                && d.is_empty()
                && matches!(t.as_str(), "int" | "long" | "float" | "double")
            {
                return Err("empty String".into());
            }
            if let Some(order) = f.get("order") {
                let ok = order.as_str().is_some_and(|o| {
                    matches!(
                        o.to_ascii_uppercase().as_str(),
                        "ASCENDING" | "DESCENDING" | "IGNORE"
                    )
                });
                if !ok {
                    return Err("Illegal order".into());
                }
            }
            if out.iter().any(|o| o.name == name) {
                return Err(format!("Duplicate field {name}"));
            }
            out.push(Field {
                name: name.to_owned(),
                aliases: aliases(f)?,
                ty,
                has_default: default.is_some(),
            });
        }
        Ok(out)
    }
}

fn enum_kind(j: &Json) -> Res<NamedKind> {
    let Some(Json::Array(items)) = j.get("symbols") else {
        return Err("Enum has no symbols".into());
    };
    let mut symbols: Vec<String> = Vec::with_capacity(items.len());
    for s in items {
        let s = s.as_str().ok_or("Enum symbols must be strings")?;
        if !valid_name(s) {
            return Err(format!("Illegal character in: {s}"));
        }
        if symbols.iter().any(|x| x == s) {
            return Err(format!("Duplicate enum symbol: {s}"));
        }
        symbols.push(s.to_owned());
    }
    let has_default = match j.get("default") {
        None => false,
        Some(Json::Str(d)) if symbols.contains(d) => true,
        Some(d) => {
            return Err(format!(
                "The Enum Default: {} is not in the enum symbol set",
                d.to_text()
            ));
        }
    };
    Ok(NamedKind::Enum {
        symbols,
        has_default,
    })
}

fn aliases(j: &Json) -> Res<Vec<String>> {
    match j.get("aliases") {
        None => Ok(Vec::new()),
        Some(Json::Array(items)) => items
            .iter()
            .map(|a| {
                a.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| "alias not a string".to_owned())
            })
            .collect(),
        Some(_) => Err("aliases not an array".into()),
    }
}

/// The namespace (`None`: none given anywhere) and the simple name of a named type's declaration.
pub(crate) fn split_name(
    j: &Json,
    name: &str,
    enclosing: Option<&str>,
) -> (Option<String>, String) {
    if let Some((space, simple)) = name.rsplit_once('.') {
        return (Some(space.to_owned()), simple.to_owned());
    }
    if let Some(space) = j.get("namespace").and_then(Json::as_str) {
        return (Some(space.to_owned()), name.to_owned());
    }
    (enclosing.map(str::to_owned), name.to_owned())
}

/// Avro's lower-case word for an unnamed type.
pub(crate) fn type_word(t: &Ty) -> &'static str {
    match t {
        Ty::Null => "null",
        Ty::Boolean => "boolean",
        Ty::Int => "int",
        Ty::Long => "long",
        Ty::Float => "float",
        Ty::Double => "double",
        Ty::Bytes => "bytes",
        Ty::String => "string",
        Ty::Array(_) => "array",
        Ty::Map(_) => "map",
        Ty::Union(_) => "union",
        Ty::Named(_) => "named",
    }
}
