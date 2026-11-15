//! The `Conditions` section: each condition worked out once the parameters are known, in the order their references to each other
//! allow (a condition may name one declared after it), and the check that every condition a template names is declared.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use aws_wire::ApiError;

use super::eval::{Fault, Nothing, Pseudo, Scope, intrinsic};
use crate::errors::{format, template};

/// The conditions a value names (`{"Condition": "X"}` inside a definition, and the first argument of `Fn::If`).
pub(crate) fn named(v: &Value, out: &mut BTreeSet<String>) {
    if let Some((name, arg)) = intrinsic(v) {
        match name {
            "Condition" => {
                if let Some(s) = arg.as_str() {
                    out.insert(s.to_owned());
                }
                return;
            }
            "Fn::If" => {
                if let Some(c) = arg
                    .as_array()
                    .and_then(|a| a.first())
                    .and_then(Value::as_str)
                {
                    out.insert(c.to_owned());
                }
            }
            _ => {}
        }
    }
    match v {
        Value::Object(o) => o.values().for_each(|x| named(x, out)),
        Value::Array(a) => a.iter().for_each(|x| named(x, out)),
        _ => {}
    }
}

/// Every condition a template names is declared.
///
/// # Errors
/// `ValidationError` naming the first that is not.
pub(crate) fn check_declared(t: &Value) -> Result<(), ApiError> {
    let declared: BTreeSet<&str> = t
        .get("Conditions")
        .and_then(Value::as_object)
        .map(|m| m.keys().map(String::as_str).collect())
        .unwrap_or_default();
    for section in ["Resources", "Outputs", "Conditions"] {
        let Some(items) = t.get(section).and_then(Value::as_object) else {
            continue;
        };
        let mut used = BTreeSet::new();
        for item in items.values() {
            named(item, &mut used);
            if section != "Conditions"
                && let Some(c) = item.get("Condition").and_then(Value::as_str)
            {
                used.insert(c.to_owned());
            }
        }
        if let Some(missing) = used.iter().find(|c| !declared.contains(c.as_str())) {
            // (memory)
            return Err(format(format!(
                "Unresolved condition dependencies [{missing}] in the {section} block of the template"
            )));
        }
    }
    Ok(())
}

/// Every declared condition's value.
///
/// # Errors
/// A circular definition, or a definition that cannot be worked out.
pub(crate) fn evaluate(
    t: &Value,
    params: &BTreeMap<String, Value>,
    pseudo: &Pseudo,
) -> Result<BTreeMap<String, bool>, ApiError> {
    let Some(defs) = t.get("Conditions").and_then(Value::as_object) else {
        return Ok(BTreeMap::new());
    };
    let mut done: BTreeMap<String, bool> = BTreeMap::new();
    let mut visiting: Vec<String> = Vec::new();
    let ctx = Defs {
        defs,
        t,
        params,
        pseudo,
    };
    for name in defs.keys() {
        visit(name, &ctx, &mut done, &mut visiting)?;
    }
    Ok(done)
}

/// What a condition is worked out against.
struct Defs<'a> {
    defs: &'a serde_json::Map<String, Value>,
    t: &'a Value,
    params: &'a BTreeMap<String, Value>,
    pseudo: &'a Pseudo,
}

fn visit(
    name: &str,
    ctx: &Defs<'_>,
    done: &mut BTreeMap<String, bool>,
    visiting: &mut Vec<String>,
) -> Result<(), ApiError> {
    let Defs {
        defs,
        t,
        params,
        pseudo,
    } = *ctx;
    if done.contains_key(name) {
        return Ok(());
    }
    if visiting.iter().any(|v| v == name) {
        // (memory)
        return Err(format(format!(
            "Circular condition dependency: {}",
            visiting.join(", ")
        )));
    }
    let Some(def) = defs.get(name) else {
        return Err(format(format!(
            "Unresolved condition dependencies [{name}] in the Conditions block of the template"
        )));
    };
    visiting.push(name.to_owned());
    let mut needs = BTreeSet::new();
    named(def, &mut needs);
    for n in &needs {
        visit(n, ctx, done, visiting)?;
    }
    visiting.pop();
    let exports = BTreeMap::new();
    let scope = Scope {
        params,
        pseudo,
        conditions: done,
        mappings: t.get("Mappings"),
        known: &Nothing,
        exports: &exports,
    };
    let value = match intrinsic(def) {
        Some((f, arg)) => scope.boolean(f, arg),
        None => match def {
            Value::Bool(b) => Ok(*b),
            _ => Err(Fault::Error(format!(
                "Template error: condition {name} is not a boolean expression"
            ))),
        },
    };
    let value = value.map_err(|f| match f {
        Fault::Error(m) => template_error(&m),
        // (memory)
        Fault::Pending(r) => template(format!(
            "Cannot reference resources in the Conditions block of the template: {r}"
        )),
    })?;
    done.insert(name.to_owned(), value);
    Ok(())
}

/// A fault's message as a `ValidationError` (messages already carry their `Template ... error:` prefix).
pub(crate) fn template_error(m: &str) -> ApiError {
    crate::errors::validation(m)
}
