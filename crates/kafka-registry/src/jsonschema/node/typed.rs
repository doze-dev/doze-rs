//! The keywords that make a node one kind or another: enum and const, type (or the keywords that imply one), and each
//! type's own.

use super::*;

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
    pub(super) fn typed(&mut self, doc: usize, m: &Json) -> Result<Option<Id>, String> {
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
