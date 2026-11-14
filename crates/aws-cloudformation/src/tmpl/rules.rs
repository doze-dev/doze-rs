//! The `Rules` section: each rule whose `RuleCondition` holds must have every `Assertions` entry hold, or the call is refused with
//! the assertion's `AssertDescription`. The rule functions that read the account (`Fn::RefAll`, `Fn::ValueOf`, `Fn::ValueOfAll`) have
//! nothing local to read: an assertion that uses them is not evaluated, and the log says so.

use std::collections::BTreeMap;

use serde_json::Value;

use aws_wire::ApiError;

use super::eval::{Fault, Nothing, Pseudo, Scope, intrinsic};
use crate::errors::validation;

/// Checks the rules against the bound parameters.
///
/// # Errors
/// `ValidationError` with the failing assertion's description.
pub(crate) fn check(
    t: &Value,
    params: &BTreeMap<String, Value>,
    pseudo: &Pseudo,
) -> Result<(), ApiError> {
    let Some(rules) = t.get("Rules").and_then(Value::as_object) else {
        return Ok(());
    };
    let exports = BTreeMap::new();
    let conditions = BTreeMap::new();
    let scope = Scope {
        params,
        pseudo,
        conditions: &conditions,
        mappings: t.get("Mappings"),
        known: &Nothing,
        exports: &exports,
    };
    for (name, rule) in rules {
        if let Some(c) = rule.get("RuleCondition") {
            match truth(&scope, c) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(()) => {
                    tracing::warn!(
                        "rule {name}: its condition reads the account, which has nothing local to read; not evaluated"
                    );
                    continue;
                }
            }
        }
        for a in rule
            .get("Assertions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(assert) = a.get("Assert") else {
                continue;
            };
            match truth(&scope, assert) {
                Ok(true) => {}
                Ok(false) => {
                    let why = a
                        .get("AssertDescription")
                        .and_then(Value::as_str)
                        .map_or_else(
                            || format!("Template error: rule {name} failed"),
                            str::to_owned,
                        ); // (memory)
                    return Err(validation(why));
                }
                Err(()) => tracing::warn!(
                    "rule {name}: an assertion reads the account, which has nothing local to read; not evaluated"
                ),
            }
        }
    }
    Ok(())
}

/// A rule expression's truth; `Err` when it uses a function with nothing local to read.
fn truth(s: &Scope<'_>, v: &Value) -> Result<bool, ()> {
    let Some((f, arg)) = intrinsic(v) else {
        return match v {
            Value::Bool(b) => Ok(*b),
            Value::String(t) => Ok(t == "true"),
            _ => Ok(false),
        };
    };
    let args = || arg.as_array().cloned().unwrap_or_default();
    let val = |x: &Value| s.value(x, f).map_err(|_: Fault| ());
    let texts = |x: &Value| -> Result<Vec<String>, ()> {
        Ok(match val(x)? {
            Value::Array(a) => a.iter().map(text).collect(),
            other => vec![text(&other)],
        })
    };
    match f {
        "Fn::Equals" => match args().as_slice() {
            [a, b] => Ok(text(&val(a)?) == text(&val(b)?)),
            _ => Ok(false),
        },
        "Fn::Not" => match args().as_slice() {
            [a] => Ok(!truth(s, a)?),
            _ => Ok(false),
        },
        "Fn::And" => {
            for a in &args() {
                if !truth(s, a)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        "Fn::Or" => {
            for a in &args() {
                if truth(s, a)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        "Fn::Contains" => match args().as_slice() {
            [list, x] => Ok(texts(list)?.contains(&text(&val(x)?))),
            _ => Ok(false),
        },
        "Fn::EachMemberEquals" => match args().as_slice() {
            [list, x] => {
                let want = text(&val(x)?);
                Ok(texts(list)?.iter().all(|m| *m == want))
            }
            _ => Ok(false),
        },
        "Fn::EachMemberIn" => match args().as_slice() {
            [list, allowed] => {
                let allowed = texts(allowed)?;
                Ok(texts(list)?.iter().all(|m| allowed.contains(m)))
            }
            _ => Ok(false),
        },
        _ => Err(()),
    }
}

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
