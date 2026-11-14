//! A schema in the form Confluent's compatibility compares it: each subschema classed as one kind (string, number,
//! object, a combination, ...) the way Confluent's JSON Schema library models it, with references followed. Nodes live
//! in an arena and are shared where a reference reaches one target from many places, so a diamond of references costs
//! its size; a reference back into a schema being read compares as "anything" at the point of recursion.

use std::collections::HashMap;

use super::doc::{Docs, field, members};
use super::number::Dec;
use crate::json::Json;

/// A node's place in its schema's arena.
pub(crate) type Id = u32;

/// The node that accepts anything (`true`, `{}`), at the same place in every arena.
pub(crate) const ANY: Id = 0;
/// The node that accepts nothing (`false`).
pub(crate) const NOTHING: Id = 1;

/// How deeply subschemas (references followed) may nest: the Go edition's bound for comparing.
const MAX_NESTING: usize = 64;
/// How long a chain of nodes may be, counting the targets of references shared from elsewhere; past it a comparison
/// would recurse too deeply.
const MAX_HEIGHT: u32 = 160;
/// How many nodes a schema may make (references to overlapping parts of a document each make their own).
const MAX_NODES: usize = 200_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Crit {
    All,
    Any,
    One,
}

impl Crit {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::All => "allOf",
            Self::Any => "anyOf",
            Self::One => "oneOf",
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct Str {
    pub(crate) min_len: Option<Dec>,
    pub(crate) max_len: Option<Dec>,
    pub(crate) pattern: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct Num {
    pub(crate) integer: bool,
    pub(crate) min: Option<Dec>,
    pub(crate) max: Option<Dec>,
    pub(crate) ex_min: Option<Dec>,
    pub(crate) ex_max: Option<Dec>,
    pub(crate) multiple_of: Option<Dec>,
}

#[derive(Debug, Default)]
pub(crate) struct Arr {
    /// Every item, when `items` is one schema.
    pub(crate) items: Option<Id>,
    /// Positional items, when `items` is an array.
    pub(crate) tuple: Option<Vec<Id>>,
    /// `None`: anything allowed past the tuple; [`NOTHING`]: nothing.
    pub(crate) additional: Option<Id>,
    pub(crate) min_items: Option<Dec>,
    pub(crate) max_items: Option<Dec>,
    pub(crate) unique: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Obj {
    pub(crate) props: Vec<(String, Id)>,
    pub(crate) required: Vec<String>,
    /// `None`: open (anything); [`NOTHING`]: closed; else a schema.
    pub(crate) additional: Option<Id>,
    pub(crate) min_props: Option<Dec>,
    pub(crate) max_props: Option<Dec>,
    /// Array dependencies, each sorted.
    pub(crate) dependencies: Vec<(String, Vec<String>)>,
}

impl Obj {
    pub(crate) fn prop(&self, name: &str) -> Option<Id> {
        self.props
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, id)| *id)
    }

    pub(crate) fn requires(&self, name: &str) -> bool {
        self.required.iter().any(|r| r == name)
    }
}

#[derive(Debug)]
pub(crate) enum Kind {
    Any,
    Nothing,
    Str(Str),
    Num(Num),
    Bool,
    Null,
    Arr(Arr),
    Obj(Obj),
    /// Each value's canonical JSON, sorted.
    Enum(Vec<String>),
    Const(String),
    Combined(Crit, Vec<Id>),
    Not(Id),
    /// if, then, else.
    Cond([Option<Id>; 3]),
}

#[derive(Debug)]
pub(crate) struct Node {
    pub(crate) kind: Kind,
    pub(crate) has_default: bool,
    height: u32,
}

/// A schema's nodes and its root.
#[derive(Debug)]
pub(crate) struct Arena {
    nodes: Vec<Node>,
    root: Id,
}

impl Arena {
    pub(crate) fn node(&self, id: Id) -> &Node {
        static FALLBACK: Node = Node {
            kind: Kind::Any,
            has_default: false,
            height: 0,
        };
        self.nodes.get(id as usize).unwrap_or(&FALLBACK)
    }

    pub(crate) fn root(&self) -> Id {
        self.root
    }
}

/// Builds the comparison form of document 0.
pub(crate) fn load(docs: &Docs) -> Result<Arena, String> {
    let mut l = Loader {
        docs,
        nodes: vec![leaf(Kind::Any), leaf(Kind::Nothing)],
        memo: HashMap::new(),
        depth: 0,
    };
    let root = docs.doc(0).map(|d| &d.json).ok_or("no schema")?;
    let root = l.node(0, root)?;
    Ok(Arena {
        nodes: l.nodes,
        root,
    })
}

fn leaf(kind: Kind) -> Node {
    Node {
        kind,
        has_default: false,
        height: 1,
    }
}

struct Loader<'a> {
    docs: &'a Docs,
    nodes: Vec<Node>,
    /// A reference's target (document, value) and the node made of it.
    memo: HashMap<(usize, *const Json), Id>,
    depth: usize,
}

fn too_deep() -> String {
    "schema is nested or recursive too deeply to compare".to_owned()
}

impl Loader<'_> {
    fn push(&mut self, kind: Kind) -> Result<Id, String> {
        self.push_node(kind, false)
    }

    fn push_node(&mut self, kind: Kind, has_default: bool) -> Result<Id, String> {
        if self.nodes.len() >= MAX_NODES {
            return Err(format!(
                "schema has more than {MAX_NODES} subschemas to compare"
            ));
        }
        let height = 1 + children(&kind)
            .iter()
            .map(|c| self.height(*c))
            .max()
            .unwrap_or(0);
        if height > MAX_HEIGHT {
            return Err(too_deep());
        }
        self.nodes.push(Node {
            kind,
            has_default,
            height,
        });
        Id::try_from(self.nodes.len() - 1).map_err(|_| too_deep())
    }

    fn height(&self, id: Id) -> u32 {
        self.nodes.get(id as usize).map_or(0, |n| n.height)
    }

    fn node(&mut self, doc: usize, v: &Json) -> Result<Id, String> {
        self.depth += 1;
        let out = if self.depth > MAX_NESTING {
            Err(too_deep())
        } else {
            match v {
                Json::Raw(r) if r == "true" => Ok(ANY),
                Json::Raw(r) if r == "false" => Ok(NOTHING),
                Json::Object(_) => self.object(doc, v),
                _ => Err("a schema must be an object or a boolean".to_owned()),
            }
        };
        self.depth -= 1;
        out
    }

    fn object(&mut self, doc: usize, m: &Json) -> Result<Id, String> {
        if let Some(Json::Str(r)) = field(m, "$ref") {
            let (to, target) = self.docs.resolve(doc, r)?;
            let key = (to, std::ptr::from_ref(target));
            if let Some(id) = self.memo.get(&key) {
                return Ok(*id);
            }
            self.memo.insert(key, ANY);
            let id = self.node(to, target)?;
            self.memo.insert(key, id);
            return Ok(id);
        }
        // A schema with a combinator beside its own keywords is the product of all of them.
        let mut parts = Vec::new();
        if let Some(own) = self.typed(doc, m)? {
            parts.push(own);
        }
        for (key, crit) in [
            ("allOf", Crit::All),
            ("anyOf", Crit::Any),
            ("oneOf", Crit::One),
        ] {
            if let Some(Json::Array(items)) = field(m, key) {
                let subs = items
                    .iter()
                    .map(|s| self.node(doc, s))
                    .collect::<Result<_, _>>()?;
                parts.push(self.push(Kind::Combined(crit, subs))?);
            }
        }
        if let Some(not) = field(m, "not") {
            let sub = self.node(doc, not)?;
            parts.push(self.push(Kind::Not(sub))?);
        }
        if field(m, "if").is_some() {
            let mut cond = [None; 3];
            for (slot, key) in cond.iter_mut().zip(["if", "then", "else"]) {
                if let Some(s) = field(m, key) {
                    *slot = Some(self.node(doc, s)?);
                }
            }
            parts.push(self.push(Kind::Cond(cond))?);
        }
        let has_default = field(m, "default").is_some();
        match parts.as_slice() {
            [] if has_default => self.push_node(Kind::Any, true),
            [] => Ok(ANY),
            [one] if has_default => {
                // The one part was made here and is the last node.
                if let Some(n) = self.nodes.last_mut() {
                    n.has_default = true;
                }
                Ok(*one)
            }
            [one] => Ok(*one),
            _ => self.push_node(Kind::Combined(Crit::All, parts), has_default),
        }
    }
}

/// The nodes a node is made of.
fn children(kind: &Kind) -> Vec<Id> {
    match kind {
        Kind::Arr(a) => a
            .items
            .iter()
            .chain(a.tuple.iter().flatten())
            .chain(a.additional.iter())
            .copied()
            .collect(),
        Kind::Obj(o) => o
            .props
            .iter()
            .map(|(_, id)| *id)
            .chain(o.additional)
            .collect(),
        Kind::Combined(_, subs) => subs.clone(),
        Kind::Not(id) => vec![*id],
        Kind::Cond(c) => c.iter().flatten().copied().collect(),
        _ => Vec::new(),
    }
}

/// A value's canonical JSON for an `enum` or `const`: object keys sorted, numbers as written.
fn canonical_value(v: &Json) -> String {
    match v {
        Json::Object(_) => {
            let mut m = members(v);
            m.sort_by(|a, b| a.0.cmp(b.0));
            let inner: Vec<String> = m
                .iter()
                .map(|(k, x)| {
                    let mut s = String::new();
                    crate::json::write_str(&mut s, k);
                    format!("{s}:{}", canonical_value(x))
                })
                .collect();
            format!("{{{}}}", inner.join(","))
        }
        Json::Array(a) => format!(
            "[{}]",
            a.iter().map(canonical_value).collect::<Vec<_>>().join(",")
        ),
        other => other.to_text(),
    }
}

fn has_any(m: &Json, keys: &[&str]) -> bool {
    keys.iter().any(|k| field(m, k).is_some())
}

fn number(v: Option<&Json>) -> Option<Dec> {
    match v {
        Some(Json::Raw(r)) => Dec::parse(r),
        _ => None,
    }
}

impl Loader<'_> {
    /// The keywords that belong to a type, or `None` when there are none.
    ///
    /// An `enum` or `const` beside a `type` is two schemas combined, as Confluent's library reads it (recorded: "enum
    /// with type", where a narrower enum is COMBINED_TYPE_SUBSCHEMAS_CHANGED at the property).
    fn typed(&mut self, doc: usize, m: &Json) -> Result<Option<Id>, String> {
        let values = if let Some(Json::Array(values)) = field(m, "enum") {
            let mut v: Vec<String> = values.iter().map(canonical_value).collect();
            v.sort();
            Some(Kind::Enum(v))
        } else {
            field(m, "const").map(|c| Kind::Const(canonical_value(c)))
        };
        let Some(values) = values else {
            return self.types(doc, m);
        };
        if field(m, "type").is_none() {
            return self.push(values).map(Some);
        }
        let typed = self.types(doc, m)?;
        let values = self.push(values)?;
        match typed {
            Some(t) => self
                .push(Kind::Combined(Crit::All, vec![t, values]))
                .map(Some),
            None => Ok(Some(values)),
        }
    }

    fn types(&mut self, doc: usize, m: &Json) -> Result<Option<Id>, String> {
        let mut types: Vec<&str> = match field(m, "type") {
            Some(Json::Str(t)) => vec![t.as_str()],
            Some(Json::Array(a)) => a.iter().filter_map(Json::as_str).collect(),
            _ => Vec::new(),
        };
        if types.is_empty() {
            // No "type": the keywords present say which kind it is.
            let inferred = [
                (
                    "object",
                    &[
                        "properties",
                        "required",
                        "additionalProperties",
                        "patternProperties",
                        "minProperties",
                        "maxProperties",
                        "dependencies",
                        "propertyNames",
                    ][..],
                ),
                (
                    "array",
                    &[
                        "items",
                        "additionalItems",
                        "minItems",
                        "maxItems",
                        "uniqueItems",
                        "contains",
                    ][..],
                ),
                ("string", &["minLength", "maxLength", "pattern"][..]),
                (
                    "number",
                    &[
                        "minimum",
                        "maximum",
                        "exclusiveMinimum",
                        "exclusiveMaximum",
                        "multipleOf",
                    ][..],
                ),
            ];
            match inferred.iter().find(|(_, keys)| has_any(m, keys)) {
                Some((t, _)) => types.push(t),
                None => return Ok(None),
            }
        }
        let mut nodes = Vec::new();
        for t in types {
            nodes.push(self.of_type(doc, t, m)?);
        }
        match nodes.as_slice() {
            [one] => Ok(Some(*one)),
            _ => self.push(Kind::Combined(Crit::Any, nodes)).map(Some),
        }
    }

    fn of_type(&mut self, doc: usize, t: &str, m: &Json) -> Result<Id, String> {
        let kind = match t {
            "string" => Kind::Str(Str {
                min_len: number(field(m, "minLength")),
                max_len: number(field(m, "maxLength")),
                pattern: field(m, "pattern")
                    .and_then(Json::as_str)
                    .map(str::to_owned),
            }),
            "integer" | "number" => Kind::Num(numeric(m, t == "integer")),
            "boolean" => Kind::Bool,
            "null" => Kind::Null,
            "array" => Kind::Arr(self.array(doc, m)?),
            "object" => Kind::Obj(self.obj(doc, m)?),
            _ => return Err(format!("unknown type \"{t}\"")),
        };
        self.push(kind)
    }

    fn array(&mut self, doc: usize, m: &Json) -> Result<Arr, String> {
        let mut a = Arr {
            min_items: number(field(m, "minItems")),
            max_items: number(field(m, "maxItems")),
            unique: matches!(field(m, "uniqueItems"), Some(Json::Raw(r)) if r == "true"),
            ..Arr::default()
        };
        match field(m, "items") {
            Some(Json::Array(items)) => {
                a.tuple = Some(
                    items
                        .iter()
                        .map(|s| self.node(doc, s))
                        .collect::<Result<_, _>>()?,
                );
            }
            Some(s) => a.items = Some(self.node(doc, s)?),
            None => {}
        }
        if let Some(s) = field(m, "additionalItems") {
            a.additional = Some(self.node(doc, s)?).filter(|id| !self.is_any(*id));
        }
        Ok(a)
    }

    fn is_any(&self, id: Id) -> bool {
        matches!(
            self.nodes.get(id as usize).map(|n| &n.kind),
            Some(Kind::Any)
        )
    }

    fn obj(&mut self, doc: usize, m: &Json) -> Result<Obj, String> {
        let mut o = Obj {
            min_props: number(field(m, "minProperties")),
            max_props: number(field(m, "maxProperties")),
            ..Obj::default()
        };
        if let Some(ps @ Json::Object(_)) = field(m, "properties") {
            for (name, s) in members(ps) {
                let id = self.node(doc, s)?;
                o.props.push((name.to_owned(), id));
            }
        }
        if let Some(Json::Array(rq)) = field(m, "required") {
            for r in rq.iter().filter_map(Json::as_str) {
                if !o.requires(r) {
                    o.required.push(r.to_owned());
                }
            }
        }
        if let Some(s) = field(m, "additionalProperties") {
            o.additional = Some(self.node(doc, s)?).filter(|id| !self.is_any(*id));
        }
        if let Some(deps @ Json::Object(_)) = field(m, "dependencies") {
            for (k, v) in members(deps) {
                if let Json::Array(arr) = v {
                    let mut names: Vec<String> = arr
                        .iter()
                        .filter_map(Json::as_str)
                        .map(str::to_owned)
                        .collect();
                    names.sort();
                    o.dependencies.push((k.to_owned(), names));
                }
            }
        }
        Ok(o)
    }
}

fn numeric(m: &Json, integer: bool) -> Num {
    let mut n = Num {
        integer,
        min: number(field(m, "minimum")),
        max: number(field(m, "maximum")),
        multiple_of: number(field(m, "multipleOf")),
        ..Num::default()
    };
    // Draft 4: a boolean exclusiveMinimum makes "minimum" exclusive.
    match field(m, "exclusiveMinimum") {
        Some(Json::Raw(r)) if r == "true" => n.ex_min = n.min.take(),
        v => n.ex_min = number(v),
    }
    match field(m, "exclusiveMaximum") {
        Some(Json::Raw(r)) if r == "true" => n.ex_max = n.max.take(),
        v => n.ex_max = number(v),
    }
    n
}
