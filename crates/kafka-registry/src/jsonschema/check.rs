//! Whether a document is a valid schema of its draft: the draft's meta-schema, checked keyword by keyword, and every
//! `$ref` the schema uses resolved. This is what Confluent's library refuses a schema for (and the Go edition's,
//! santhosh-tekuri/jsonschema): a keyword of the wrong shape, a type that is not one, a pattern that is not a regular
//! expression, a reference to nothing. Keywords a draft does not have are annotations, as the drafts say.

use std::collections::HashMap;

use super::doc::{Docs, Draft, field, members};
use super::number::Dec;
use crate::json::Json;

const TYPES: [&str; 7] = [
    "array", "boolean", "integer", "null", "number", "object", "string",
];

/// Checks every document from its root.
pub(crate) fn check(docs: &Docs) -> Result<(), String> {
    let mut c = Checker {
        docs,
        seen: HashMap::new(),
    };
    for i in 0..docs.0.len() {
        if let Some(d) = docs.doc(i) {
            // The schema read is used whole; a referenced one only where a reference reaches it.
            c.schema(
                Ctx {
                    doc: i,
                    reached: i == 0,
                    depth: 0,
                },
                &d.json,
                "#",
            )?;
        }
    }
    Ok(())
}

struct Checker<'a> {
    docs: &'a Docs,
    /// (document, value) checked, and whether it was reached (its references followed).
    seen: HashMap<(usize, *const Json), bool>,
}

/// How a keyword's value is read.
#[derive(Clone, Copy)]
enum Shape {
    Any,
    Str,
    Bool,
    Num,
    Count,
    Positive,
    Array,
    Pattern,
    Types,
    Required,
    Enum,
    Schema,
    SchemaOrBool,
    Schemas,
    SchemaOrSchemas,
    SchemaMap,
    PatternMap,
    Dependencies,
    StringArrays,
    BoolMap,
}

/// The shape of a keyword in a draft, or `None` when the draft does not have it.
fn shape(draft: Draft, key: &str) -> Option<Shape> {
    use Shape::*;
    let s = match key {
        "$schema" | "title" | "description" | "format" | "$ref" => Str,
        "uniqueItems" => Bool,
        "default" => Any,
        "multipleOf" => Positive,
        "maximum" | "minimum" => Num,
        "maxLength" | "minLength" | "maxItems" | "minItems" | "maxProperties" | "minProperties" => {
            Count
        }
        "pattern" => Pattern,
        "type" => Types,
        "required" => Required,
        "enum" => Enum,
        "allOf" | "anyOf" | "oneOf" => Schemas,
        "properties" | "definitions" => SchemaMap,
        "patternProperties" => PatternMap,
        "dependencies" => Dependencies,
        _ => return drafted(draft, key),
    };
    Some(s)
}

/// The keywords whose shape, or presence, depends on the draft: the first drafts and the last that have each.
const DRAFTED: &[(&str, Draft, Draft, Shape)] = &[
    ("id", Draft::D4, Draft::D4, Shape::Str),
    ("$id", Draft::D6, Draft::D2020, Shape::Str),
    ("$comment", Draft::D7, Draft::D2020, Shape::Str),
    ("contentMediaType", Draft::D7, Draft::D2020, Shape::Str),
    ("contentEncoding", Draft::D7, Draft::D2020, Shape::Str),
    ("$anchor", Draft::D2019, Draft::D2020, Shape::Str),
    ("$recursiveRef", Draft::D2019, Draft::D2020, Shape::Str),
    ("$dynamicRef", Draft::D2020, Draft::D2020, Shape::Str),
    ("$dynamicAnchor", Draft::D2020, Draft::D2020, Shape::Str),
    ("$recursiveAnchor", Draft::D2019, Draft::D2019, Shape::Bool),
    ("readOnly", Draft::D7, Draft::D2020, Shape::Bool),
    ("writeOnly", Draft::D7, Draft::D2020, Shape::Bool),
    ("deprecated", Draft::D2019, Draft::D2020, Shape::Bool),
    ("examples", Draft::D6, Draft::D2020, Shape::Array),
    ("const", Draft::D6, Draft::D2020, Shape::Any),
    ("exclusiveMaximum", Draft::D4, Draft::D4, Shape::Bool),
    ("exclusiveMinimum", Draft::D4, Draft::D4, Shape::Bool),
    ("exclusiveMaximum", Draft::D6, Draft::D2020, Shape::Num),
    ("exclusiveMinimum", Draft::D6, Draft::D2020, Shape::Num),
    ("maxContains", Draft::D2019, Draft::D2020, Shape::Count),
    ("minContains", Draft::D2019, Draft::D2020, Shape::Count),
    ("additionalItems", Draft::D4, Draft::D4, Shape::SchemaOrBool),
    (
        "additionalProperties",
        Draft::D4,
        Draft::D4,
        Shape::SchemaOrBool,
    ),
    ("additionalItems", Draft::D6, Draft::D2019, Shape::Schema),
    (
        "additionalProperties",
        Draft::D6,
        Draft::D2020,
        Shape::Schema,
    ),
    ("not", Draft::D4, Draft::D2020, Shape::Schema),
    ("contains", Draft::D6, Draft::D2020, Shape::Schema),
    ("propertyNames", Draft::D6, Draft::D2020, Shape::Schema),
    ("if", Draft::D7, Draft::D2020, Shape::Schema),
    ("then", Draft::D7, Draft::D2020, Shape::Schema),
    ("else", Draft::D7, Draft::D2020, Shape::Schema),
    (
        "unevaluatedItems",
        Draft::D2019,
        Draft::D2020,
        Shape::Schema,
    ),
    (
        "unevaluatedProperties",
        Draft::D2019,
        Draft::D2020,
        Shape::Schema,
    ),
    ("contentSchema", Draft::D2019, Draft::D2020, Shape::Schema),
    ("items", Draft::D2020, Draft::D2020, Shape::Schema),
    ("items", Draft::D4, Draft::D2019, Shape::SchemaOrSchemas),
    ("prefixItems", Draft::D2020, Draft::D2020, Shape::Schemas),
    ("$defs", Draft::D2019, Draft::D2020, Shape::SchemaMap),
    (
        "dependentSchemas",
        Draft::D2019,
        Draft::D2020,
        Shape::SchemaMap,
    ),
    (
        "dependentRequired",
        Draft::D2019,
        Draft::D2020,
        Shape::StringArrays,
    ),
    ("$vocabulary", Draft::D2019, Draft::D2020, Shape::BoolMap),
];

fn drafted(draft: Draft, key: &str) -> Option<Shape> {
    DRAFTED
        .iter()
        .find(|(k, from, to, _)| *k == key && (*from..=*to).contains(&draft))
        .map(|(.., s)| *s)
}

/// Keywords whose subschemas are used to validate (a reference in them is followed); `definitions` and `$defs` only
/// hold schemas for references to name.
fn applies(key: &str) -> bool {
    !matches!(key, "definitions" | "$defs")
}

/// Where a value is checked: its document, whether a reference reaching it is followed, how deep.
#[derive(Clone, Copy)]
struct Ctx {
    doc: usize,
    reached: bool,
    depth: usize,
}

impl Ctx {
    fn below(self, reached: bool) -> Self {
        Self {
            reached,
            depth: self.depth + 1,
            ..self
        }
    }
}

impl Checker<'_> {
    fn schema(&mut self, c: Ctx, v: &Json, at: &str) -> Result<(), String> {
        let Ctx {
            doc,
            reached,
            depth,
        } = c;
        if depth > crate::json::MAX_SCHEMA_DEPTH * 2 {
            return Err(format!("{at}: nested or referenced too deeply"));
        }
        let key = (doc, std::ptr::from_ref(v));
        match self.seen.get(&key) {
            Some(true) => return Ok(()),
            Some(false) if !reached => return Ok(()),
            _ => {}
        }
        self.seen.insert(key, reached);
        let draft = self.docs.doc(doc).map_or(Draft::D7, |d| d.draft);
        match v {
            Json::Raw(r) if (r == "true" || r == "false") && draft >= Draft::D6 => Ok(()),
            Json::Object(_) => self.object(c, v, at),
            _ => Err(format!(
                "{at}: a schema must be an object{}",
                if draft >= Draft::D6 {
                    " or a boolean"
                } else {
                    ""
                }
            )),
        }
    }

    fn object(&mut self, c: Ctx, v: &Json, at: &str) -> Result<(), String> {
        let Ctx { doc, reached, .. } = c;
        let draft = self.docs.doc(doc).map_or(Draft::D7, |d| d.draft);
        let reference = field(v, "$ref").and_then(Json::as_str);
        if draft == Draft::D4 {
            for (key, needs) in [
                ("exclusiveMaximum", "maximum"),
                ("exclusiveMinimum", "minimum"),
            ] {
                if field(v, key).is_some() && field(v, needs).is_none() {
                    return Err(format!("{at}: {key} needs {needs}"));
                }
            }
        }
        // Before 2019-09 a reference stands for the whole schema: what is beside it is checked, never used.
        let beside = reached && (reference.is_none() || draft >= Draft::D2019);
        for (key, value) in members(v) {
            let Some(shape) = shape(draft, key) else {
                continue;
            };
            let here = format!("{at}/{}", key.replace('~', "~0").replace('/', "~1"));
            self.keyword(
                Ctx {
                    reached: beside && applies(key),
                    ..c
                },
                shape,
                value,
                &here,
            )?;
        }
        if let (Some(r), true) = (reference, reached) {
            let (to, target) = self
                .docs
                .resolve(doc, r)
                .map_err(|e| format!("{at}: {e}"))?;
            self.schema(
                Ctx {
                    doc: to,
                    ..c.below(true)
                },
                target,
                r,
            )?;
        }
        Ok(())
    }

    fn keyword(&mut self, c: Ctx, shape: Shape, v: &Json, at: &str) -> Result<(), String> {
        let doc = c.doc;
        let bad = |what: &str| Err(format!("{at}: {what}"));
        let sub = |me: &mut Self, x: &Json, k: &str| {
            me.schema(c.below(c.reached), x, &format!("{at}/{k}"))
        };
        match (shape, v) {
            (Shape::Any, _) | (Shape::Str, Json::Str(_)) => Ok(()),
            (Shape::Bool, Json::Raw(r)) if r == "true" || r == "false" => Ok(()),
            (Shape::Num | Shape::Count | Shape::Positive, Json::Raw(r)) => {
                number(shape, r).map_or_else(bad, Ok)
            }
            (Shape::Array, Json::Array(_)) => Ok(()),
            (Shape::Pattern, Json::Str(p)) => regex(p).map_or_else(|w| bad(&w), Ok),
            (Shape::Types, _) => types(v).map_or_else(|w| bad(&w), Ok),
            (Shape::Required, Json::Array(a)) => {
                strings(a, doc_draft(self, doc) == Draft::D4).map_or_else(|w| bad(&w), Ok)
            }
            (Shape::Enum, Json::Array(a)) => {
                enumeration(a, doc_draft(self, doc)).map_or_else(|w| bad(&w), Ok)
            }
            (Shape::Schema, _) => self.schema(c.below(c.reached), v, at),
            (Shape::SchemaOrBool, Json::Raw(r)) if r == "true" || r == "false" => Ok(()),
            (Shape::SchemaOrBool, _) => self.schema(c.below(c.reached), v, at),
            (Shape::SchemaOrSchemas, Json::Array(_)) | (Shape::Schemas, Json::Array(_)) => {
                let Json::Array(items) = v else { return Ok(()) };
                if items.is_empty() {
                    return bad("must not be empty");
                }
                items
                    .iter()
                    .enumerate()
                    .try_for_each(|(i, x)| sub(self, x, &i.to_string()))
            }
            (Shape::SchemaOrSchemas, _) => self.schema(c.below(c.reached), v, at),
            (Shape::SchemaMap | Shape::PatternMap, Json::Object(_)) => {
                members(v).into_iter().try_for_each(|(k, x)| {
                    if matches!(shape, Shape::PatternMap) {
                        regex(k).map_err(|w| format!("{at}: {w}"))?;
                    }
                    sub(self, x, k)
                })
            }
            (Shape::Dependencies, Json::Object(_)) => {
                members(v).into_iter().try_for_each(|(k, x)| match x {
                    Json::Array(a) => strings(a, doc_draft(self, doc) == Draft::D4)
                        .map_err(|w| format!("{at}/{k}: {w}")),
                    _ => sub(self, x, k),
                })
            }
            (Shape::StringArrays, Json::Object(_)) => {
                members(v).into_iter().try_for_each(|(k, x)| match x {
                    Json::Array(a) => strings(a, false).map_err(|w| format!("{at}/{k}: {w}")),
                    _ => Err(format!("{at}/{k}: must be an array of strings")),
                })
            }
            (Shape::BoolMap, Json::Object(_)) => Ok(()),
            _ => bad(expected(shape)),
        }
    }
}

fn doc_draft(c: &Checker<'_>, doc: usize) -> Draft {
    c.docs.doc(doc).map_or(Draft::D7, |d| d.draft)
}

fn expected(shape: Shape) -> &'static str {
    match shape {
        Shape::Str | Shape::Pattern => "must be a string",
        Shape::Bool => "must be a boolean",
        Shape::Array | Shape::Required | Shape::Enum | Shape::Schemas => "must be an array",
        Shape::Num | Shape::Count | Shape::Positive => "must be a number",
        _ => "must be an object",
    }
}

fn number(shape: Shape, raw: &str) -> Result<(), &'static str> {
    let n = Dec::parse(raw).ok_or("must be a number")?;
    match shape {
        Shape::Count if !n.is_integer() || n.is_negative() => Err("must be a non-negative integer"),
        Shape::Positive if n.is_negative() || n.is_zero() => Err("must be greater than 0"),
        _ => Ok(()),
    }
}

fn types(v: &Json) -> Result<(), String> {
    fn one(t: &Json) -> Result<&str, String> {
        match t {
            Json::Str(s) if TYPES.contains(&s.as_str()) => Ok(s.as_str()),
            Json::Str(s) => Err(format!("\"{s}\" is not a type")),
            _ => Err("a type must be a string".to_owned()),
        }
    }
    match v {
        Json::Array(items) => {
            if items.is_empty() {
                return Err("must not be empty".to_owned());
            }
            let mut seen = Vec::new();
            for t in items {
                let t = one(t)?;
                if seen.contains(&t) {
                    return Err(format!("\"{t}\" is named twice"));
                }
                seen.push(t);
            }
            Ok(())
        }
        _ => one(v).map(|_| ()),
    }
}

/// An array of distinct strings (`required`, a dependency); before draft 6 it may not be empty.
fn strings(items: &[Json], non_empty: bool) -> Result<(), String> {
    if non_empty && items.is_empty() {
        return Err("must not be empty".to_owned());
    }
    let mut seen: Vec<&str> = Vec::new();
    for x in items {
        let s = x.as_str().ok_or("must be an array of strings")?;
        if seen.contains(&s) {
            return Err(format!("\"{s}\" is named twice"));
        }
        seen.push(s);
    }
    Ok(())
}

/// `enum`: distinct values, and at least one until 2019-09.
fn enumeration(items: &[Json], draft: Draft) -> Result<(), String> {
    if draft >= Draft::D2019 {
        return Ok(());
    }
    if items.is_empty() {
        return Err("must not be empty".to_owned());
    }
    let keys: Vec<String> = items.iter().map(value_key).collect();
    for (i, k) in keys.iter().enumerate() {
        if keys.iter().skip(i + 1).any(|o| o == k) {
            return Err("values must be distinct".to_owned());
        }
    }
    Ok(())
}

/// A value as JSON Schema compares values: object keys in order, numbers by value.
fn value_key(v: &Json) -> String {
    match v {
        Json::Raw(r) => Dec::parse(r).map_or_else(|| r.clone(), |d| format!("{d:?}")),
        Json::Str(s) => format!("{s:?}"),
        Json::Array(a) => format!(
            "[{}]",
            a.iter().map(value_key).collect::<Vec<_>>().join(",")
        ),
        Json::Object(_) => {
            let mut m: Vec<(&str, String)> = members(v)
                .into_iter()
                .map(|(k, x)| (k, value_key(x)))
                .collect();
            m.sort();
            format!(
                "{{{}}}",
                m.iter()
                    .map(|(k, x)| format!("{k:?}:{x}"))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
    }
}

/// Whether `p` is a regular expression, judged by what makes one malformed in Java's `Pattern` (Confluent's): an
/// unclosed group or class, a `)` with no group, a trailing backslash, a quantifier with nothing to repeat. A pattern
/// these pass and Java would still refuse is taken (it is compared as text, never run here).
pub(crate) fn regex(p: &str) -> Result<(), String> {
    let bad = |what: &str| Err(format!("\"{p}\" is not a regular expression: {what}"));
    let (mut groups, mut class, mut atom) = (0_usize, 0_usize, false);
    let mut chars = p.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if chars.next().is_none() {
                    return bad("a trailing backslash");
                }
                atom = true;
            }
            '[' => {
                class += 1;
                if chars.peek() == Some(&'^') {
                    chars.next();
                }
                if class == 1 && chars.peek() == Some(&']') {
                    chars.next();
                }
            }
            ']' if class > 0 => {
                class -= 1;
                atom = true;
            }
            _ if class > 0 => {}
            '(' => {
                groups += 1;
                atom = false;
                if chars.peek() == Some(&'?') {
                    chars.next();
                }
            }
            ')' => {
                if groups == 0 {
                    return bad("an unmatched ')'");
                }
                groups -= 1;
                atom = true;
            }
            '|' => atom = false,
            '*' | '+' | '?' if !atom => return bad("a quantifier with nothing to repeat"),
            '*' | '+' | '?' => {
                if matches!(chars.peek(), Some('?' | '+')) {
                    chars.next();
                }
            }
            '^' | '$' => atom = false,
            _ => atom = true,
        }
    }
    if class > 0 {
        return bad("an unclosed '['");
    }
    if groups > 0 {
        return bad("an unclosed '('");
    }
    Ok(())
}
