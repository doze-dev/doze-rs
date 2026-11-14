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
            c.schema(i, &d.json, "#", i == 0, 0)?;
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
    use Draft::{D4, D6, D7, D2019, D2020};
    use Shape::*;
    let s = match key {
        "$schema" | "title" | "description" | "format" | "$ref" => Str,
        "id" if draft == D4 => Str,
        "$id" if draft >= D6 => Str,
        "$comment" | "contentMediaType" | "contentEncoding" if draft >= D7 => Str,
        "$anchor" | "$recursiveRef" if draft >= D2019 => Str,
        "$dynamicRef" | "$dynamicAnchor" if draft >= D2020 => Str,
        "$recursiveAnchor" if draft == D2019 => Bool,
        "readOnly" | "writeOnly" if draft >= D7 => Bool,
        "deprecated" if draft >= D2019 => Bool,
        "uniqueItems" => Bool,
        "examples" if draft >= D6 => Array,
        "default" => Any,
        "const" if draft >= D6 => Any,
        "multipleOf" => Positive,
        "maximum" | "minimum" => Num,
        "exclusiveMaximum" | "exclusiveMinimum" if draft == D4 => Bool,
        "exclusiveMaximum" | "exclusiveMinimum" => Num,
        "maxLength" | "minLength" | "maxItems" | "minItems" | "maxProperties" | "minProperties" => Count,
        "maxContains" | "minContains" if draft >= D2019 => Count,
        "pattern" => Pattern,
        "type" => Types,
        "required" => Required,
        "enum" => Enum,
        "additionalItems" | "additionalProperties" if draft == D4 => SchemaOrBool,
        "additionalItems" if draft >= D2020 => return None,
        "additionalItems" | "additionalProperties" | "not" => Schema,
        "contains" | "propertyNames" if draft >= D6 => Schema,
        "if" | "then" | "else" if draft >= D7 => Schema,
        "unevaluatedItems" | "unevaluatedProperties" | "contentSchema" if draft >= D2019 => Schema,
        "items" if draft >= D2020 => Schema,
        "items" => SchemaOrSchemas,
        "prefixItems" if draft >= D2020 => Schemas,
        "allOf" | "anyOf" | "oneOf" => Schemas,
        "properties" | "definitions" => SchemaMap,
        "$defs" | "dependentSchemas" if draft >= D2019 => SchemaMap,
        "patternProperties" => PatternMap,
        "dependencies" => Dependencies,
        "dependentRequired" if draft >= D2019 => StringArrays,
        "$vocabulary" if draft >= D2019 => BoolMap,
        _ => return None,
    };
    Some(s)
}

/// Keywords whose subschemas are used to validate (a reference in them is followed); `definitions` and `$defs` only
/// hold schemas for references to name.
fn applies(key: &str) -> bool {
    !matches!(key, "definitions" | "$defs")
}

impl Checker<'_> {
    fn schema(&mut self, doc: usize, v: &Json, at: &str, reached: bool, depth: usize) -> Result<(), String> {
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
            Json::Object(_) => self.object(doc, v, at, reached, depth),
            _ => Err(format!("{at}: a schema must be an object{}", if draft >= Draft::D6 { " or a boolean" } else { "" })),
        }
    }

    fn object(&mut self, doc: usize, v: &Json, at: &str, reached: bool, depth: usize) -> Result<(), String> {
        let draft = self.docs.doc(doc).map_or(Draft::D7, |d| d.draft);
        let reference = field(v, "$ref").and_then(Json::as_str);
        if draft == Draft::D4 {
            for (key, needs) in [("exclusiveMaximum", "maximum"), ("exclusiveMinimum", "minimum")] {
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
            self.keyword(doc, shape, value, &here, beside && applies(key), depth)?;
        }
        if let (Some(r), true) = (reference, reached) {
            let (to, target) = self.docs.resolve(doc, r).map_err(|e| format!("{at}: {e}"))?;
            self.schema(to, target, &format!("{r}"), true, depth + 1)?;
        }
        Ok(())
    }

    fn keyword(&mut self, doc: usize, shape: Shape, v: &Json, at: &str, reached: bool, depth: usize) -> Result<(), String> {
        let bad = |what: &str| Err(format!("{at}: {what}"));
        let sub = |c: &mut Self, x: &Json, k: &str| c.schema(doc, x, &format!("{at}/{k}"), reached, depth + 1);
        match (shape, v) {
            (Shape::Any, _) | (Shape::Str, Json::Str(_)) => Ok(()),
            (Shape::Bool, Json::Raw(r)) if r == "true" || r == "false" => Ok(()),
            (Shape::Num | Shape::Count | Shape::Positive, Json::Raw(r)) => number(shape, r).map_or_else(|w| bad(w), Ok),
            (Shape::Array, Json::Array(_)) => Ok(()),
            (Shape::Pattern, Json::Str(p)) => regex(p).map_or_else(|w| bad(&w), Ok),
            (Shape::Types, _) => types(v).map_or_else(|w| bad(&w), Ok),
            (Shape::Required, Json::Array(a)) => strings(a, doc_draft(self, doc) == Draft::D4).map_or_else(|w| bad(&w), Ok),
            (Shape::Enum, Json::Array(a)) => enumeration(a, doc_draft(self, doc)).map_or_else(|w| bad(&w), Ok),
            (Shape::Schema, _) => self.schema(doc, v, at, reached, depth + 1),
            (Shape::SchemaOrBool, Json::Raw(r)) if r == "true" || r == "false" => Ok(()),
            (Shape::SchemaOrBool, _) => self.schema(doc, v, at, reached, depth + 1),
            (Shape::SchemaOrSchemas, Json::Array(_)) | (Shape::Schemas, Json::Array(_)) => {
                let Json::Array(items) = v else { return Ok(()) };
                if items.is_empty() {
                    return bad("must not be empty");
                }
                items.iter().enumerate().try_for_each(|(i, x)| sub(self, x, &i.to_string()))
            }
            (Shape::SchemaOrSchemas, _) => self.schema(doc, v, at, reached, depth + 1),
            (Shape::SchemaMap | Shape::PatternMap, Json::Object(_)) => members(v).into_iter().try_for_each(|(k, x)| {
                if matches!(shape, Shape::PatternMap) {
                    regex(k).map_err(|w| format!("{at}: {w}"))?;
                }
                sub(self, x, k)
            }),
            (Shape::Dependencies, Json::Object(_)) => members(v).into_iter().try_for_each(|(k, x)| match x {
                Json::Array(a) => strings(a, doc_draft(self, doc) == Draft::D4).map_err(|w| format!("{at}/{k}: {w}")),
                _ => sub(self, x, k),
            }),
            (Shape::StringArrays, Json::Object(_)) => members(v).into_iter().try_for_each(|(k, x)| match x {
                Json::Array(a) => strings(a, false).map_err(|w| format!("{at}/{k}: {w}")),
                _ => Err(format!("{at}/{k}: must be an array of strings")),
            }),
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
        Json::Array(a) => format!("[{}]", a.iter().map(value_key).collect::<Vec<_>>().join(",")),
        Json::Object(_) => {
            let mut m: Vec<(&str, String)> = members(v).into_iter().map(|(k, x)| (k, value_key(x))).collect();
            m.sort();
            format!("{{{}}}", m.iter().map(|(k, x)| format!("{k:?}:{x}")).collect::<Vec<_>>().join(","))
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
