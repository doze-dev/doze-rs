//! `AWS::LanguageExtensions`: `Fn::ForEach` loops expanded in `Resources`, `Outputs`, `Conditions` and anywhere a property map may hold
//! one; `DeletionPolicy` and `UpdateReplacePolicy` given by `Ref` or `Fn::If` resolved to their text. `Fn::Length`,
//! `Fn::ToJsonString` and the default value of `Fn::FindInMap` are evaluated where they stand, like every other function.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use aws_wire::ApiError;

use crate::errors::{format, template};
use crate::j::J;
use crate::tmpl::conditions;
use crate::tmpl::eval::{Nothing, Pseudo, Scope};

const FOR_EACH: &str = "Fn::ForEach::";
/// How many items all loops of a template may expand to.
const MAX_EXPANSION: usize = 10_000;

/// Applies the extensions to `t`.
///
/// # Errors
/// A loop or policy written in a way the extensions refuse.
pub(crate) fn apply(
    t: &mut Value,
    params: &BTreeMap<String, Value>,
    pseudo: &Pseudo,
) -> Result<(), ApiError> {
    let mut budget = MAX_EXPANSION;
    let snapshot = t.clone();
    let exports = BTreeMap::new();
    let none = BTreeMap::new();
    let scope = Scope {
        params,
        pseudo,
        conditions: &none,
        mappings: snapshot.get("Mappings"),
        known: &Nothing,
        exports: &exports,
    };
    if let Some(o) = t.as_object_mut() {
        for section in ["Conditions", "Resources", "Outputs"] {
            if let Some(v) = o.get_mut(section) {
                expand(v, &scope, &mut budget, 0)?;
            }
        }
    }
    policies(t, params, pseudo)
}

/// Expands every `Fn::ForEach::<name>` key in `v` and below.
fn expand(
    v: &mut Value,
    scope: &Scope<'_>,
    budget: &mut usize,
    depth: usize,
) -> Result<(), ApiError> {
    if depth > 16 {
        return Err(format("Fn::ForEach is nested too deeply")); // (memory)
    }
    match v {
        Value::Object(o) => {
            let loops: Vec<String> = o
                .keys()
                .filter(|k| k.starts_with(FOR_EACH))
                .cloned()
                .collect();
            for key in loops {
                let Some(body) = o.remove(&key) else { continue };
                let mut made = Map::new();
                one_loop(&key, &body, scope, budget, &mut made)?;
                for (k, mut x) in made {
                    expand(&mut x, scope, budget, depth + 1)?;
                    if o.insert(k.clone(), x).is_some() {
                        // (memory)
                        return Err(format(format!("Fn::ForEach produced the key {k} twice")));
                    }
                }
            }
            for x in o.values_mut() {
                expand(x, scope, budget, depth + 1)?;
            }
        }
        Value::Array(a) => {
            for x in a {
                expand(x, scope, budget, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn one_loop(
    key: &str,
    body: &Value,
    scope: &Scope<'_>,
    budget: &mut usize,
    out: &mut Map<String, Value>,
) -> Result<(), ApiError> {
    let Some([ident, collection, fragment]) = body.as_array().map(Vec::as_slice) else {
        // (memory)
        return Err(template(format!(
            "{key} requires a list of three: an identifier, a collection and a fragment"
        )));
    };
    let ident = ident.as_str().unwrap_or_default();
    let items = scope
        .list(collection, "Fn::ForEach")
        .map_err(|_| template(format!("{key}: the collection must be a list of strings")))?; // (memory)
    let Some(fragment) = fragment.as_object() else {
        return Err(template(format!("{key}: the fragment must be an object"))); // (memory)
    };
    for item in items {
        let text = match &item {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        *budget = budget
            .checked_sub(1)
            .ok_or_else(|| format("Fn::ForEach expands to too many items"))?; // (memory)
        for (k, v) in fragment {
            out.insert(replace(k, ident, &text), substitute(v, ident, &text));
        }
    }
    Ok(())
}

/// `${x}` and `&{x}` (the item with its non-alphanumeric characters dropped) replaced in a key or string.
fn replace(s: &str, ident: &str, item: &str) -> String {
    let alnum: String = item.chars().filter(char::is_ascii_alphanumeric).collect();
    s.replace(&format!("${{{ident}}}"), item)
        .replace(&format!("&{{{ident}}}"), &alnum)
}

/// The fragment for one item: the identifier replaced in keys, in `Fn::Sub` strings and as a `Ref`.
fn substitute(v: &Value, ident: &str, item: &str) -> Value {
    match v {
        Value::String(s) => Value::String(replace(s, ident, item)),
        Value::Array(a) => Value::Array(a.iter().map(|x| substitute(x, ident, item)).collect()),
        Value::Object(o) => {
            if o.len() == 1 && o.get("Ref").and_then(Value::as_str) == Some(ident) {
                return Value::String(item.to_owned());
            }
            Value::Object(
                o.iter()
                    .map(|(k, x)| (replace(k, ident, item), substitute(x, ident, item)))
                    .collect(),
            )
        }
        other => other.clone(),
    }
}

/// `DeletionPolicy` and `UpdateReplacePolicy` written as a function, resolved to text.
fn policies(
    t: &mut Value,
    params: &BTreeMap<String, Value>,
    pseudo: &Pseudo,
) -> Result<(), ApiError> {
    let conds = conditions::evaluate(t, params, pseudo)?;
    let snapshot = t.clone();
    let exports = BTreeMap::new();
    let scope = Scope {
        params,
        pseudo,
        conditions: &conds,
        mappings: snapshot.get("Mappings"),
        known: &Nothing,
        exports: &exports,
    };
    let Some(res) = t.get_mut("Resources").and_then(Value::as_object_mut) else {
        return Ok(());
    };
    for r in res.values_mut() {
        for key in ["DeletionPolicy", "UpdateReplacePolicy"] {
            if let Some(v) = r.get(key).filter(|v| v.is_object()).cloned() {
                let text = scope
                    .text(&v, key)
                    .map_err(|_| template(format!("{key} must resolve to a string")))?; // (memory)
                r.put(key, Value::String(text));
            }
        }
    }
    Ok(())
}
