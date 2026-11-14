//! A JSON value checked against a JSON Schema: what the console does before it sends a value framed for the schema, and
//! what it says of a record it shows. The check walks the same nodes the compatibility rules compare ([`super::node`]),
//! references followed; a reference back into a schema being read is "anything" at the point of recursion, as there.
//!
//! Checked: `type`, `enum`, `const`, the string lengths, the number bounds and `multipleOf` (exact decimals), `items`
//! (one schema or positional) and `additionalItems`, `minItems`, `maxItems`, `uniqueItems`, `properties`, `required`,
//! `additionalProperties`, `minProperties`, `maxProperties`, array `dependencies`, `allOf`, `anyOf`, `oneOf`, `not`,
//! `if`/`then`/`else`. Not checked: `pattern` and `patternProperties` (no regular-expression engine is linked) and
//! `format` (an annotation, as most validators treat it).

use super::node::{ANY, Arena, Crit, Id, Kind, NOTHING, canonical_value};
use super::number::Dec;
use crate::codec::MAX_DEPTH;
use crate::json::Json;

type Res = Result<(), String>;

/// Checks `v` against the schema; the first thing that does not fit, with its place (`#/items/0/price`).
pub(crate) fn check(a: &Arena, v: &Json) -> Res {
    at(a, a.root(), v, "#", 0)
}

/// A count as a decimal, to compare with a limit.
fn count(n: usize) -> Dec {
    Dec::from_count(n)
}

fn shown(v: &Json) -> String {
    let t = v.to_text();
    if t.chars().count() > 40 {
        format!("{}…", t.chars().take(40).collect::<String>())
    } else {
        t
    }
}

fn type_of(v: &Json) -> &'static str {
    match v {
        Json::Object(_) => "an object",
        Json::Array(_) => "an array",
        Json::Str(_) => "a string",
        Json::Raw(r) if r == "null" => "null",
        Json::Raw(r) if r == "true" || r == "false" => "a boolean",
        Json::Raw(_) => "a number",
    }
}

fn bounds(n: &Dec, min: Option<&Dec>, max: Option<&Dec>, path: &str) -> Res {
    if min.is_some_and(|m| n < m) {
        return Err(format!("{path}: it has fewer properties than its minProperties"));
    }
    if max.is_some_and(|m| n > m) {
        return Err(format!("{path}: it has more properties than its maxProperties"));
    }
    Ok(())
}

#[expect(clippy::too_many_lines, reason = "one arm a kind of node, read side by side")]
fn at(a: &Arena, id: Id, v: &Json, path: &str, depth: usize) -> Res {
    if depth > MAX_DEPTH {
        return Err(format!("{path}: the value nests more than {MAX_DEPTH} levels deep"));
    }
    let next = depth + 1;
    match &a.node(id).kind {
        Kind::Any => Ok(()),
        Kind::Nothing => Err(format!("{path}: no value is allowed here")),
        Kind::Null => match v {
            Json::Raw(r) if r == "null" => Ok(()),
            _ => Err(format!("{path}: expected null, not {}", type_of(v))),
        },
        Kind::Bool => match v {
            Json::Raw(r) if r == "true" || r == "false" => Ok(()),
            _ => Err(format!("{path}: expected a boolean, not {}", type_of(v))),
        },
        Kind::Str(s) => {
            let Json::Str(t) = v else {
                return Err(format!("{path}: expected a string, not {}", type_of(v)));
            };
            let n = count(t.chars().count());
            if s.min_len.as_ref().is_some_and(|m| n < *m) {
                return Err(format!("{path}: {} is shorter than its minLength", shown(v)));
            }
            if s.max_len.as_ref().is_some_and(|m| n > *m) {
                return Err(format!("{path}: {} is longer than its maxLength", shown(v)));
            }
            Ok(())
        }
        Kind::Num(n) => {
            let d = match v {
                Json::Raw(r) => Dec::parse(r),
                _ => None,
            };
            let Some(d) = d else {
                return Err(format!("{path}: expected a number, not {}", type_of(v)));
            };
            if n.integer && !d.is_integer() {
                return Err(format!("{path}: expected an integer, not {}", shown(v)));
            }
            let t = shown(v);
            if n.min.as_ref().is_some_and(|m| d < *m) || n.ex_min.as_ref().is_some_and(|m| d <= *m) {
                return Err(format!("{path}: {t} is under its minimum"));
            }
            if n.max.as_ref().is_some_and(|m| d > *m) || n.ex_max.as_ref().is_some_and(|m| d >= *m) {
                return Err(format!("{path}: {t} is over its maximum"));
            }
            if n.multiple_of.as_ref().is_some_and(|m| !d.is_multiple_of(m)) {
                return Err(format!("{path}: {t} is not a multiple of its multipleOf"));
            }
            Ok(())
        }
        Kind::Arr(arr) => {
            let Json::Array(items) = v else {
                return Err(format!("{path}: expected an array, not {}", type_of(v)));
            };
            let n = count(items.len());
            if arr.min_items.as_ref().is_some_and(|m| n < *m) {
                return Err(format!("{path}: {} items is fewer than its minItems", items.len()));
            }
            if arr.max_items.as_ref().is_some_and(|m| n > *m) {
                return Err(format!("{path}: {} items is more than its maxItems", items.len()));
            }
            for (i, x) in items.iter().enumerate() {
                let item = match &arr.tuple {
                    Some(t) => t.get(i).copied().or(arr.additional),
                    None => arr.items,
                };
                if let Some(item) = item {
                    at(a, item, x, &format!("{path}/{i}"), next)?;
                }
            }
            if arr.unique {
                let mut seen: Vec<String> = items.iter().map(canonical_value).collect();
                seen.sort();
                if seen.windows(2).any(|w| w.first() == w.get(1)) {
                    return Err(format!("{path}: its items are not unique"));
                }
            }
            Ok(())
        }
        Kind::Obj(o) => {
            let Json::Object(m) = v else {
                return Err(format!("{path}: expected an object, not {}", type_of(v)));
            };
            for r in &o.required {
                if v.get(r).is_none() {
                    return Err(format!("{path}: required property {r:?} is missing"));
                }
            }
            for (k, x) in m {
                let here = format!("{path}/{k}");
                match (o.prop(k), o.additional) {
                    (Some(p), _) => at(a, p, x, &here, next)?,
                    (None, Some(NOTHING)) => {
                        return Err(format!("{path}: property {k:?} is not allowed (additionalProperties is false)"));
                    }
                    (None, Some(extra)) => at(a, extra, x, &here, next)?,
                    (None, None) => {}
                }
            }
            bounds(&count(m.len()), o.min_props.as_ref(), o.max_props.as_ref(), path)?;
            for (k, needs) in &o.dependencies {
                if v.get(k).is_some()
                    && let Some(n) = needs.iter().find(|n| v.get(n).is_none())
                {
                    return Err(format!("{path}: {k:?} is present, so {n:?} must be too"));
                }
            }
            Ok(())
        }
        Kind::Enum(values) => {
            let c = canonical_value(v);
            if values.contains(&c) {
                Ok(())
            } else {
                Err(format!("{path}: {} is not one of its enum values", shown(v)))
            }
        }
        Kind::Const(c) => {
            if canonical_value(v) == *c {
                Ok(())
            } else {
                Err(format!("{path}: {} is not its const value {c}", shown(v)))
            }
        }
        Kind::Combined(crit, ids) => combined(a, *crit, ids, v, path, next),
        Kind::Not(n) => {
            if *n != ANY && at(a, *n, v, path, next).is_err() {
                Ok(())
            } else {
                Err(format!("{path}: the value matches the schema under \"not\""))
            }
        }
        Kind::Cond([cond, then, other]) => {
            let Some(cond) = cond else { return Ok(()) };
            let branch = if at(a, *cond, v, path, next).is_ok() { then } else { other };
            branch.map_or(Ok(()), |b| at(a, b, v, path, next))
        }
    }
}

fn combined(a: &Arena, crit: Crit, ids: &[Id], v: &Json, path: &str, next: usize) -> Res {
    match crit {
        Crit::All => ids.iter().try_for_each(|id| at(a, *id, v, path, next)),
        Crit::Any => {
            let mut first = None;
            for id in ids {
                match at(a, *id, v, path, next) {
                    Ok(()) => return Ok(()),
                    Err(e) => {
                        first.get_or_insert(e);
                    }
                }
            }
            Err(first.unwrap_or_else(|| format!("{path}: no schema of its anyOf fits")))
        }
        Crit::One => {
            let fits = ids.iter().filter(|id| at(a, **id, v, path, next).is_ok()).count();
            match fits {
                1 => Ok(()),
                0 => Err(format!("{path}: no schema of its oneOf fits")),
                n => Err(format!("{path}: {n} schemas of its oneOf fit, and exactly one must")),
            }
        }
    }
}

/// A value of the schema's shape: an object's properties (the required ones, or all when none is), each at its zero; an
/// enum's first value; a combination's first schema.
pub(crate) fn sample(a: &Arena, id: Id, depth: usize) -> Json {
    let raw = |s: &str| Json::Raw(s.to_owned());
    if depth > 6 {
        return raw("null");
    }
    match &a.node(id).kind {
        Kind::Any | Kind::Nothing | Kind::Null | Kind::Not(_) | Kind::Cond(_) => raw("null"),
        Kind::Str(_) => Json::Str(String::new()),
        Kind::Num(_) => raw("0"),
        Kind::Bool => raw("false"),
        Kind::Arr(_) => Json::Array(Vec::new()),
        Kind::Obj(o) => Json::Object(
            o.props
                .iter()
                .filter(|(n, _)| o.required.is_empty() || o.requires(n))
                .map(|(n, p)| (n.clone(), sample(a, *p, depth + 1)))
                .collect(),
        ),
        Kind::Enum(values) => values.first().and_then(|v| Json::parse(v).ok()).unwrap_or_else(|| raw("null")),
        Kind::Const(c) => Json::parse(c).unwrap_or_else(|_| raw("null")),
        Kind::Combined(_, ids) => ids.first().map_or_else(|| raw("null"), |i| sample(a, *i, depth + 1)),
    }
}
