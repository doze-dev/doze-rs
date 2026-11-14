//! The `patchOperations` of the v1 `Update*` operations: `{op, path, value, from}` applied to the record the operation changes. Each
//! record names the paths it takes and what each holds; the path names the member in the record (`/accessLogSettings/format` is
//! `accessLogSettings.format`), a map takes its key as the path's last segment (`/variables/color`), a list its element
//! (`/binaryMediaTypes/image~1png`), with `~1` for `/` and `~0` for `~` as in a JSON Pointer. A path the record does not take is refused.

use aws_wire::ApiError;
use serde_json::{Map, Value};

use crate::errors::bad;

/// One patch operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Patch {
    pub(crate) op: String,
    pub(crate) path: String,
    pub(crate) value: Option<String>,
    pub(crate) from: Option<String>,
}

/// What a path holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Str,
    Bool,
    Int,
    Num,
    /// A list of strings; the element is the last segment of the path.
    List,
    /// A map of strings; the key is the last segment.
    Map,
    /// A map of booleans (`requestParameters` of a method).
    MapBool,
}

/// A path a record takes.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Field {
    pub(crate) path: &'static str,
    pub(crate) kind: Kind,
}

/// A field, for the tables of each record.
pub(crate) const fn f(path: &'static str, kind: Kind) -> Field {
    Field { path, kind }
}

/// The patch operations of a request.
pub(crate) fn of(v: Option<&Value>) -> Vec<Patch> {
    let text = |p: &Value, k: &str| p.get(k).and_then(Value::as_str).map(str::to_owned);
    v.and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|p| Patch {
                    op: text(p, "op").unwrap_or_default(),
                    path: text(p, "path").unwrap_or_default(),
                    value: text(p, "value"),
                    from: text(p, "from"),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A path segment with its escapes undone.
pub(crate) fn unescape(seg: &str) -> String {
    seg.replace("~1", "/").replace("~0", "~")
}

/// The refusal for a path the record does not take `(memory)`.
pub(crate) fn invalid_path(p: &Patch, fields: &[Field]) -> ApiError {
    let paths: Vec<&str> = fields.iter().map(|f| f.path).collect();
    bad(format!(
        "Invalid patch path '{}' specified for op '{}'. Must be one of: [{}]",
        p.path,
        p.op,
        paths.join(", ")
    ))
}

/// Applies every patch to `doc`, in order. Nothing is applied when one fails: the caller works on a copy.
pub(crate) fn apply(doc: &mut Value, patches: &[Patch], fields: &[Field]) -> Result<(), ApiError> {
    for p in patches {
        apply_one(doc, p, fields)?;
    }
    Ok(())
}

fn find<'f>(fields: &'f [Field], path: &str) -> Option<(&'f Field, Option<String>)> {
    fields.iter().find_map(|f| {
        if path == f.path {
            return Some((f, None));
        }
        let rest = path.strip_prefix(f.path)?.strip_prefix('/')?;
        matches!(f.kind, Kind::List | Kind::Map | Kind::MapBool).then(|| (f, Some(unescape(rest))))
    })
}

fn apply_one(doc: &mut Value, p: &Patch, fields: &[Field]) -> Result<(), ApiError> {
    if p.op == "copy" {
        let from = p.from.as_deref().unwrap_or_default();
        let (src, _) = find(fields, from).ok_or_else(|| invalid_path(p, fields))?;
        let value = at(doc, src.path).cloned();
        let (dst, _) = find(fields, &p.path).ok_or_else(|| invalid_path(p, fields))?;
        let (parent, last) = container(doc, dst.path)?;
        match value {
            Some(v) => parent.insert(last, v),
            None => parent.remove(&last),
        };
        return Ok(());
    }
    if !matches!(p.op.as_str(), "add" | "remove" | "replace") {
        return Err(bad(format!(
            "Invalid patch operation specified. Must be one of: [add, remove, replace, copy] (got {})",
            p.op
        )));
    }
    let (field, sub) = find(fields, &p.path).ok_or_else(|| invalid_path(p, fields))?;
    let (parent, last) = container(doc, field.path)?;
    match (field.kind, sub) {
        (Kind::List, Some(item)) => list_op(parent, &last, &p.op, item, p.value.as_deref()),
        (Kind::Map | Kind::MapBool, Some(key)) => {
            let m = parent
                .entry(last)
                .or_insert_with(|| Value::Object(Map::new()));
            if !m.is_object() {
                *m = Value::Object(Map::new());
            }
            if let Some(m) = m.as_object_mut() {
                if p.op == "remove" {
                    m.remove(&key);
                } else {
                    let kind = if field.kind == Kind::MapBool {
                        Kind::Bool
                    } else {
                        Kind::Str
                    };
                    m.insert(key, coerce(kind, p)?);
                }
            }
            Ok(())
        }
        (kind, _) => {
            if p.op == "remove" {
                parent.remove(&last);
            } else {
                parent.insert(last, coerce(kind, p)?);
            }
            Ok(())
        }
    }
}

fn list_op(
    parent: &mut Map<String, Value>,
    key: &str,
    op: &str,
    item: String,
    value: Option<&str>,
) -> Result<(), ApiError> {
    let list = parent
        .entry(key.to_owned())
        .or_insert_with(|| Value::Array(Vec::new()));
    if !list.is_array() {
        *list = Value::Array(Vec::new());
    }
    let Some(list) = list.as_array_mut() else {
        return Ok(());
    };
    let pos = list.iter().position(|v| v.as_str() == Some(item.as_str()));
    match (op, pos) {
        ("add", None) => list.push(Value::String(item)),
        ("remove", Some(i)) => {
            list.remove(i);
        }
        ("replace", Some(i)) => {
            if let (Some(slot), Some(v)) = (list.get_mut(i), value) {
                *slot = Value::String(v.to_owned());
            }
        }
        ("replace" | "remove", None) => {
            return Err(bad(format!("Invalid patch path: {item} is not in {key}")));
        }
        _ => {}
    }
    Ok(())
}

/// The value a patch sets, as the kind of its field.
fn coerce(kind: Kind, p: &Patch) -> Result<Value, ApiError> {
    let v = p.value.as_deref().unwrap_or_default();
    let fail = || bad(format!("Invalid value '{v}' for path {}", p.path));
    Ok(match kind {
        Kind::Str | Kind::List | Kind::Map => Value::String(v.to_owned()),
        Kind::Bool | Kind::MapBool => match v {
            "true" | "TRUE" | "True" => Value::Bool(true),
            "false" | "FALSE" | "False" => Value::Bool(false),
            _ => return Err(fail()),
        },
        Kind::Int => Value::from(v.parse::<i64>().map_err(|_| fail())?),
        Kind::Num => serde_json::Number::from_f64(v.parse::<f64>().map_err(|_| fail())?)
            .map(Value::Number)
            .ok_or_else(fail)?,
    })
}

/// The value at a field's path, if set.
fn at<'a>(doc: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('/')
        .filter(|s| !s.is_empty())
        .try_fold(doc, |v, seg| v.get(seg))
}

/// The object holding a field's member (made where missing), and the member's name.
fn container<'a>(
    doc: &'a mut Value,
    path: &str,
) -> Result<(&'a mut Map<String, Value>, String), ApiError> {
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let (last, parents) = segs.split_last().ok_or_else(|| bad("Invalid patch path"))?;
    let mut cur = doc;
    for seg in parents {
        cur = member(cur, seg).ok_or_else(|| bad("Invalid patch path"))?;
    }
    let obj = cur
        .as_object_mut()
        .ok_or_else(|| bad("Invalid patch path"))?;
    Ok((obj, (*last).to_owned()))
}

/// The object member `key` of `v`, made an object if it was not one.
fn member<'a>(v: &'a mut Value, key: &str) -> Option<&'a mut Value> {
    let entry = v
        .as_object_mut()?
        .entry(key.to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    Some(entry)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const FIELDS: &[Field] = &[
        f("/name", Kind::Str),
        f("/enabled", Kind::Bool),
        f("/variables", Kind::Map),
        f("/binaryMediaTypes", Kind::List),
        f("/accessLogSettings/format", Kind::Str),
        f("/throttle/rateLimit", Kind::Num),
    ];

    fn p(op: &str, path: &str, value: Option<&str>) -> Patch {
        Patch {
            op: op.into(),
            path: path.into(),
            value: value.map(Into::into),
            from: None,
        }
    }

    #[test]
    fn scalars_maps_lists_and_nested_members_are_patched() {
        let mut doc = json!({"name": "a", "variables": {"x": "1"}});
        apply(
            &mut doc,
            &[
                p("replace", "/name", Some("b")),
                p("replace", "/enabled", Some("true")),
                p("add", "/variables/y", Some("2")),
                p("remove", "/variables/x", None),
                p("add", "/binaryMediaTypes/image~1png", None),
                p(
                    "replace",
                    "/accessLogSettings/format",
                    Some("$context.requestId"),
                ),
                p("replace", "/throttle/rateLimit", Some("2.5")),
            ],
            FIELDS,
        )
        .unwrap();
        assert_eq!(
            doc,
            json!({"name": "b", "enabled": true, "variables": {"y": "2"}, "binaryMediaTypes": ["image/png"],
                "accessLogSettings": {"format": "$context.requestId"}, "throttle": {"rateLimit": 2.5}})
        );
    }

    #[test]
    fn an_unknown_path_or_a_bad_value_is_refused() {
        let mut doc = json!({});
        let e = apply(&mut doc, &[p("replace", "/nope", Some("x"))], FIELDS).unwrap_err();
        assert_eq!(e.code, "BadRequestException");
        assert!(apply(&mut doc, &[p("replace", "/enabled", Some("maybe"))], FIELDS).is_err());
        assert!(apply(&mut doc, &[p("move", "/name", Some("x"))], FIELDS).is_err());
    }
}
