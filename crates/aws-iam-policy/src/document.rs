//! The shape of a policy document, and reading one from JSON.

use serde_json::Value;

/// Allow or Deny.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    Allow,
    Deny,
}

/// A list of patterns, or (`not`) everything but them: `Action`/`NotAction`, `Resource`/`NotResource`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Matcher {
    pub not: bool,
    pub patterns: Vec<String>,
}

/// What kind of principal an entry names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrincipalKind {
    /// `*`: anyone.
    Anyone,
    Aws,
    Service,
    Federated,
    CanonicalUser,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Principal {
    pub kind: PrincipalKind,
    pub id: String,
}

/// `Principal` or (`not`) `NotPrincipal`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Principals {
    pub not: bool,
    pub entries: Vec<Principal>,
}

/// One condition: `operator` is as written (`StringEqualsIgnoreCase`, `ForAnyValue:StringLike`, `NumericLessThanIfExists`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Condition {
    pub operator: String,
    pub key: String,
    pub values: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Statement {
    pub sid: Option<String>,
    pub effect: Effect,
    pub principal: Option<Principals>,
    pub actions: Option<Matcher>,
    pub resources: Option<Matcher>,
    pub conditions: Vec<Condition>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    pub version: Option<String>,
    pub id: Option<String>,
    pub statements: Vec<Statement>,
}

/// The most levels of JSON a document may nest: a policy is three or four deep.
const MAX_DEPTH: usize = 12;

fn depth(v: &Value, at: usize) -> usize {
    match v {
        Value::Array(items) => items.iter().map(|x| depth(x, at + 1)).max().unwrap_or(at),
        Value::Object(map) => map.values().map(|x| depth(x, at + 1)).max().unwrap_or(at),
        _ => at,
    }
}

/// Reads `text` as a policy document, or says what is wrong in a word: `None` for text that is not JSON, otherwise the document or
/// the first structural fault.
// fuzz:parser fuzz_policy_document
#[must_use]
pub fn parse(text: &str) -> Option<Result<Document, String>> {
    let value: Value = serde_json::from_str(text).ok()?;
    if depth(&value, 0) > MAX_DEPTH {
        return Some(Err("the policy is nested too deeply".into()));
    }
    Some(document(&value))
}

fn document(v: &Value) -> Result<Document, String> {
    let obj = v.as_object().ok_or("a policy document is a JSON object")?;
    let statements = match obj.get("Statement") {
        Some(Value::Array(items)) => items.iter().map(statement).collect::<Result<Vec<_>, _>>()?,
        Some(one @ Value::Object(_)) => vec![statement(one)?],
        _ => return Err("a policy needs a Statement".into()),
    };
    Ok(Document {
        version: obj
            .get("Version")
            .and_then(Value::as_str)
            .map(str::to_owned),
        id: obj.get("Id").and_then(Value::as_str).map(str::to_owned),
        statements,
    })
}

fn strings(v: &Value) -> Result<Vec<String>, String> {
    match v {
        Value::String(s) => Ok(vec![s.clone()]),
        Value::Array(items) => items
            .iter()
            .map(|x| {
                x.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| "a list holds strings".to_owned())
            })
            .collect(),
        _ => Err("expected a string or a list of strings".into()),
    }
}

fn matcher(
    obj: &serde_json::Map<String, Value>,
    name: &str,
    not_name: &str,
) -> Result<Option<Matcher>, String> {
    match (obj.get(name), obj.get(not_name)) {
        (Some(_), Some(_)) => Err(format!("{name} and {not_name} are both present")),
        (Some(v), None) => Ok(Some(Matcher {
            not: false,
            patterns: strings(v)?,
        })),
        (None, Some(v)) => Ok(Some(Matcher {
            not: true,
            patterns: strings(v)?,
        })),
        (None, None) => Ok(None),
    }
}

fn principals(v: &Value, not: bool) -> Result<Principals, String> {
    let mut entries = Vec::new();
    match v {
        Value::String(s) if s == "*" => entries.push(Principal {
            kind: PrincipalKind::Anyone,
            id: "*".into(),
        }),
        Value::Object(map) => {
            for (kind, ids) in map {
                let kind = match kind.as_str() {
                    "AWS" => PrincipalKind::Aws,
                    "Service" => PrincipalKind::Service,
                    "Federated" => PrincipalKind::Federated,
                    "CanonicalUser" => PrincipalKind::CanonicalUser,
                    other => return Err(format!("unknown principal type {other}")),
                };
                entries.extend(strings(ids)?.into_iter().map(|id| Principal { kind, id }));
            }
        }
        _ => return Err("a Principal is * or an object".into()),
    }
    Ok(Principals { not, entries })
}

fn conditions(v: &Value) -> Result<Vec<Condition>, String> {
    let ops = v.as_object().ok_or("Condition is an object")?;
    let mut out = Vec::new();
    for (operator, keys) in ops {
        let keys = keys
            .as_object()
            .ok_or("each condition operator holds an object")?;
        for (key, values) in keys {
            let values = match values {
                Value::Bool(b) => vec![b.to_string()],
                Value::Number(n) => vec![n.to_string()],
                other => strings(other)?,
            };
            out.push(Condition {
                operator: operator.clone(),
                key: key.clone(),
                values,
            });
        }
    }
    Ok(out)
}

fn statement(v: &Value) -> Result<Statement, String> {
    let obj = v.as_object().ok_or("a Statement is an object")?;
    let effect = match obj.get("Effect").and_then(Value::as_str) {
        Some("Allow") => Effect::Allow,
        Some("Deny") => Effect::Deny,
        _ => return Err("a statement needs an Effect of Allow or Deny".into()),
    };
    let principal = match (obj.get("Principal"), obj.get("NotPrincipal")) {
        (Some(_), Some(_)) => return Err("Principal and NotPrincipal are both present".into()),
        (Some(p), None) => Some(principals(p, false)?),
        (None, Some(p)) => Some(principals(p, true)?),
        (None, None) => None,
    };
    Ok(Statement {
        sid: obj.get("Sid").and_then(Value::as_str).map(str::to_owned),
        effect,
        principal,
        actions: matcher(obj, "Action", "NotAction")?,
        resources: matcher(obj, "Resource", "NotResource")?,
        conditions: obj
            .get("Condition")
            .map(conditions)
            .transpose()?
            .unwrap_or_default(),
    })
}
