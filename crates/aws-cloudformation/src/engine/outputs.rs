//! A stack's outputs and exports, worked out when its resources are done, and the exports its template imports.

use std::collections::BTreeSet;

use serde_json::Value;

use super::{Job, Working};
use crate::model::Output;
use crate::tmpl::eval::{Fault, intrinsic};

/// The outputs whose condition holds, with their exports.
///
/// # Errors
/// An output that does not resolve, or an export another stack already has (the reason the stack fails with).
pub(crate) fn evaluate(w: &Working, job: &Job) -> Result<Vec<Output>, String> {
    let Some(outs) = w.template.get("Outputs").and_then(Value::as_object) else {
        return Ok(Vec::new());
    };
    let scope = w.scope();
    let text = |f: Fault| match f {
        Fault::Error(m) => m,
        Fault::Pending(r) => format!("Template error: resource {r} has no value"), // (memory)
    };
    let mut out = Vec::new();
    for (key, o) in outs {
        if let Some(c) = o.get("Condition").and_then(Value::as_str)
            && !w.conditions.get(c).copied().unwrap_or(false)
        {
            continue;
        }
        let value = match scope.value(o.get("Value").unwrap_or(&Value::Null), "Outputs") {
            Ok(Value::String(s)) => s,
            Ok(Value::Array(a)) => a
                .iter()
                .map(|x| x.as_str().map_or_else(|| x.to_string(), str::to_owned))
                .collect::<Vec<_>>()
                .join(","),
            Ok(other) => other.to_string(),
            Err(f) => return Err(text(f)),
        };
        let export = match o.get("Export").and_then(|e| e.get("Name")) {
            Some(n) => Some(scope.text(n, "Export").map_err(text)?),
            None => None,
        };
        if let Some(name) = &export
            && let Some((_, owner)) = job.exports.get(name)
            && *owner != job.stack_id
        {
            // (memory)
            let owner_name = owner.split('/').nth(1).unwrap_or(owner);
            return Err(format!(
                "Export with name {name} is already exported by stack {owner_name}"
            ));
        }
        out.push(Output {
            key: key.clone(),
            value,
            description: o
                .get("Description")
                .and_then(Value::as_str)
                .map(str::to_owned),
            export,
        });
    }
    Ok(out)
}

/// The export names the template imports with `Fn::ImportValue`.
pub(crate) fn imports(w: &Working) -> Vec<String> {
    let mut found = BTreeSet::new();
    collect(&w.template, w, &mut found);
    found.into_iter().collect()
}

fn collect(v: &Value, w: &Working, out: &mut BTreeSet<String>) {
    if let Some(("Fn::ImportValue", arg)) = intrinsic(v) {
        if let Ok(name) = w.scope().text(arg, "Fn::ImportValue") {
            out.insert(name);
        }
        return;
    }
    match v {
        Value::Object(o) => o.values().for_each(|x| collect(x, w, out)),
        Value::Array(a) => a.iter().for_each(|x| collect(x, w, out)),
        _ => {}
    }
}
