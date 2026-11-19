//! A contract read from a request's JSON as Jackson binds it to Confluent's classes: a scalar where a string is wanted is
//! its text, a key it does not know is read past, and each refusal in Jackson's words (400; recorded by
//! tools/registry-diff, `data-contracts/errors`).

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::{Metadata, Rule, RuleKind, RuleMode, RuleSet};
use crate::error::RegistryError;

fn bad(message: String) -> RegistryError {
    RegistryError::http(400, message)
}

/// Jackson's refusal of a value that is not the object `class` is read from.
fn not_object(v: &Value, class: &str) -> RegistryError {
    bad(match v {
        Value::Array(_) => format!(
            "Cannot deserialize value of type `{class}` from Array value (token `JsonToken.START_ARRAY`)"
        ),
        Value::String(s) => format!(
            "Cannot construct instance of `{class}` (although at least one Creator exists): no String-argument constructor/factory method to deserialize from String value ('{s}')"
        ),
        Value::Number(n) => format!(
            "Cannot construct instance of `{class}` (although at least one Creator exists): no int/Int-argument constructor/factory method to deserialize from Number value ({n})"
        ),
        Value::Bool(b) => format!(
            "Cannot construct instance of `{class}` (although at least one Creator exists): no boolean/Boolean-argument constructor/factory method to deserialize from boolean value ({b})"
        ),
        Value::Object(_) | Value::Null => format!(
            "Cannot deserialize value of type `{class}` from Object value (token `JsonToken.START_OBJECT`)"
        ),
    })
}

/// A string as Jackson coerces one: a number or boolean is its text, `null` is none.
fn text(v: &Value) -> Result<Option<String>, RegistryError> {
    match v {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(s.clone())),
        Value::Number(n) => Ok(Some(n.to_string())),
        Value::Bool(b) => Ok(Some(b.to_string())),
        Value::Object(_) => Err(bad(
            "Cannot deserialize value of type `java.lang.String` from Object value (token `JsonToken.START_OBJECT`)"
                .into(),
        )),
        Value::Array(_) => Err(bad(
            "Cannot deserialize value of type `java.lang.String` from Array value (token `JsonToken.START_ARRAY`)"
                .into(),
        )),
    }
}

fn strings(v: &Value, class: &str) -> Result<BTreeSet<String>, RegistryError> {
    match v {
        Value::Null => Ok(BTreeSet::new()),
        Value::Array(items) => Ok(items
            .iter()
            .map(text)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect()),
        other => Err(not_object(other, class)),
    }
}

fn string_map(v: &Value) -> Result<BTreeMap<String, String>, RegistryError> {
    match v {
        Value::Null => Ok(BTreeMap::new()),
        Value::Object(m) => {
            let mut out = BTreeMap::new();
            for (k, v) in m {
                if let Some(t) = text(v)? {
                    out.insert(k.clone(), t);
                }
            }
            Ok(out)
        }
        other => Err(not_object(other, "LinkedHashMap")),
    }
}

/// `metadata`: `None` for `null`.
///
/// # Errors
/// Jackson's 400 for what is not one.
// fuzz:parser registry_api
pub fn metadata(v: &Value) -> Result<Option<Metadata>, RegistryError> {
    let m = match v {
        Value::Null => return Ok(None),
        Value::Object(m) => m,
        other => return Err(not_object(other, "Metadata")),
    };
    let mut out = Metadata::default();
    if let Some(t) = m.get("tags") {
        match t {
            Value::Null => {}
            Value::Object(tags) => {
                for (k, v) in tags {
                    out.tags.insert(k.clone(), strings(v, "HashSet")?);
                }
            }
            other => return Err(not_object(other, "LinkedHashMap")),
        }
    }
    if let Some(p) = m.get("properties") {
        out.properties = string_map(p)?;
    }
    if let Some(s) = m.get("sensitive") {
        out.sensitive = strings(s, "HashSet")?;
    }
    Ok(Some(out))
}

fn rule(v: &Value) -> Result<Rule, RegistryError> {
    let m = match v {
        Value::Object(m) => m,
        other => return Err(not_object(other, "Rule")),
    };
    let field = |k: &str| m.get(k).map_or(Ok(None), text);
    let kind = match field("kind")? {
        None => RuleKind::Transform,
        Some(k) => RuleKind::parse(&k).ok_or_else(|| {
            bad(format!(
                "Cannot deserialize value of type `RuleKind` from String \"{k}\": not one of the values accepted for Enum class: [CONDITION, TRANSFORM]"
            ))
        })?,
    };
    let mode = match field("mode")? {
        None => RuleMode::WriteRead,
        Some(k) => RuleMode::parse(&k).ok_or_else(|| {
            bad(format!(
                "Cannot deserialize value of type `RuleMode` from String \"{k}\": not one of the values accepted for Enum class: [DOWNGRADE, READ, UPGRADE, UPDOWN, WRITE, WRITEREAD]"
            ))
        })?,
    };
    let disabled = match m.get("disabled") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) if s == "true" || s == "false" => s == "true",
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(_) => {
            return Err(bad(
                "Cannot deserialize value of type `boolean` from Object value (token `JsonToken.START_OBJECT`)"
                    .into(),
            ));
        }
    };
    Ok(Rule {
        name: field("name")?,
        doc: field("doc")?,
        kind,
        mode,
        rule_type: field("type")?,
        tags: m
            .get("tags")
            .map_or(Ok(BTreeSet::new()), |t| strings(t, "HashSet"))?,
        params: m.get("params").map_or(Ok(BTreeMap::new()), string_map)?,
        expr: field("expr")?,
        on_success: field("onSuccess")?,
        on_failure: field("onFailure")?,
        disabled,
    })
}

fn rules(v: Option<&Value>) -> Result<Vec<Rule>, RegistryError> {
    match v {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => items.iter().map(rule).collect(),
        // Jackson's own name for a List<Rule> (sic).
        Some(other) => Err(not_object(other, "Rule>")),
    }
}

/// `ruleSet`: `None` for `null`.
///
/// # Errors
/// Jackson's 400 for what is not one.
// fuzz:parser registry_api
pub fn rule_set(v: &Value) -> Result<Option<RuleSet>, RegistryError> {
    let m = match v {
        Value::Null => return Ok(None),
        Value::Object(m) => m,
        other => return Err(not_object(other, "RuleSet")),
    };
    Ok(Some(RuleSet {
        migration: rules(m.get("migrationRules"))?,
        domain: rules(m.get("domainRules"))?,
        encoding: rules(m.get("encodingRules"))?,
    }))
}

/// A stored setting's metadata, read back (it was checked when it was set).
#[must_use]
pub fn stored_metadata(text: Option<&str>) -> Option<Metadata> {
    let v: Value = serde_json::from_str(text?).ok()?;
    metadata(&v).ok().flatten()
}

/// A stored setting's rule set, read back.
#[must_use]
pub fn stored_rule_set(text: Option<&str>) -> Option<RuleSet> {
    let v: Value = serde_json::from_str(text?).ok()?;
    rule_set(&v).ok().flatten()
}
