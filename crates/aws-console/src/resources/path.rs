//! Reading answers and writing bodies: a dotted path into JSON, the name in a URL or ARN, and the placeholders of a body.

use serde_json::{Map, Value};

use super::Format;

/// The value at `path` (`Attributes.RedrivePolicy.deadLetterTargetArn`, `Table.KeySchema.0.AttributeName`): members by name,
/// list items by number. A text met on the way that holds a JSON object or list is read as one (SQS keeps its redrive policy as
/// a JSON text). The empty path is `value` itself. In a list, `@member` is the item whose `member` is the resource's name
/// (`$name` of `value`): the log group of that name among those a prefix found. `a|b` is the first of the two that has a value
/// (an alarm is a metric alarm or a composite one).
#[must_use]
pub fn get(value: &Value, path: &str) -> Option<Value> {
    if path.contains('|') {
        return path.split('|').find_map(|p| get(value, p));
    }
    let mut at = value.clone();
    for step in path.split('.').filter(|s| !s.is_empty()) {
        if let Value::String(text) = &at
            && let Ok(inner @ (Value::Object(_) | Value::Array(_))) = serde_json::from_str(text)
        {
            at = inner;
        }
        at = match at {
            Value::Object(mut m) => m.remove(step)?,
            Value::Array(a) if step.starts_with('@') => {
                let name = value.get("$name")?;
                let member = step.get(1..)?;
                a.into_iter().find(|i| i.get(member) == Some(name))?
            }
            Value::Array(mut a) => {
                let i: usize = step.parse().ok()?;
                (i < a.len()).then(|| a.swap_remove(i))?
            }
            _ => return None,
        };
    }
    Some(at)
}

/// `value` set at the dotted `at` of `input`, the objects on the way made (`Delete.Objects`).
pub fn put_at(input: &mut Map<String, Value>, at: &str, value: Value) {
    match at.split_once('.') {
        None => drop(input.insert(at.to_owned(), value)),
        Some((head, rest)) => {
            let inner = input
                .entry(head.to_owned())
                .or_insert_with(|| Value::Object(Map::new()));
            if let Value::Object(m) = inner {
                put_at(m, rest, value);
            }
        }
    }
}

/// The value at `path` as text: a text as it is, a number or a true/false written out, nothing for an absent or null value,
/// and a list or object as JSON.
#[must_use]
pub fn text(value: &Value, path: &str) -> Option<String> {
    match get(value, path)? {
        Value::Null => None,
        Value::String(s) => Some(s),
        other => Some(other.to_string()),
    }
}

/// The name in `value`: the last segment of a URL's path; the resource of an ARN (`arn:aws:sns:…:orders` is `orders`,
/// `arn:aws:lambda:…:function:resize` is `resize`, `arn:aws:dynamodb:…:table/orders` is `orders`); anything else as it is.
/// The same reading as the console's Live page (`pages/live/resource.ts`).
#[must_use]
pub fn tail(value: &str) -> String {
    if let Some(rest) = value
        .strip_prefix("http://")
        .or_else(|| value.strip_prefix("https://"))
    {
        let path = rest.split_once('/').map_or("", |(_, p)| p);
        let path = path.split(['?', '#']).next().unwrap_or_default();
        return path
            .split('/')
            .rfind(|s| !s.is_empty())
            .unwrap_or(value)
            .to_owned();
    }
    if value.starts_with("arn:") {
        let parts: Vec<&str> = value.split(':').collect();
        let Some(rest) = parts.get(5..) else {
            return value.to_owned();
        };
        if let Some(second) = rest.get(1) {
            return if second.is_empty() { value } else { second }.to_owned();
        }
        let one = rest.first().copied().unwrap_or_default();
        let slash: Vec<&str> = one.split('/').filter(|s| !s.is_empty()).collect();
        return match slash.as_slice() {
            [_, .., last] => (*last).to_owned(),
            _ if one.is_empty() => value.to_owned(),
            _ => one.to_owned(),
        };
    }
    value.to_owned()
}

/// What a placeholder can be filled with.
pub trait Vars {
    /// The text of `name` (`id`, `out:QueueUrl`, `form:Name`); `None` or empty when it has none.
    fn var(&self, name: &str) -> Option<String>;
}

/// `body` with its placeholders filled from `vars` (module text of [`crate::resources`]). A text that is a placeholder whole
/// takes the value's type (`{num:x}` a number, `{bool:x}` true or false); one inside a longer text is written into it.
#[must_use]
pub fn fill(body: &Value, vars: &dyn Vars) -> Value {
    filled(body, vars).0.unwrap_or(Value::Null)
}

/// The value filled, `None` when it was a placeholder with no value (left out of its object); and whether an object lost a
/// member so (dropped from a list: a key schema element whose name was not given).
fn filled(value: &Value, vars: &dyn Vars) -> (Option<Value>, bool) {
    match value {
        Value::String(s) => (one(s, vars), false),
        Value::Array(items) => (
            Some(Value::Array(
                items
                    .iter()
                    .filter_map(|v| match filled(v, vars) {
                        (Some(v), false) => Some(v),
                        _ => None,
                    })
                    .collect(),
            )),
            false,
        ),
        Value::Object(members) => {
            let mut out = Map::new();
            let mut hollow = false;
            for (key, v) in members {
                match filled(v, vars).0 {
                    Some(v) => drop(out.insert(key.clone(), v)),
                    None => hollow = true,
                }
            }
            (Some(Value::Object(out)), hollow)
        }
        other => (Some(other.clone()), false),
    }
}

fn one(text: &str, vars: &dyn Vars) -> Option<Value> {
    if let Some(name) = text.strip_prefix('{').and_then(|t| t.strip_suffix('}'))
        && !name.contains(['{', '}'])
    {
        let (typed, key) = match name.split_once(':') {
            Some((t @ ("num" | "bool"), rest)) => (t, format!("form:{rest}")),
            _ => ("", name.to_owned()),
        };
        let got = vars.var(&key).filter(|v| !v.is_empty())?;
        return Some(match typed {
            "num" => got
                .trim()
                .parse::<i64>()
                .map_or_else(|_| Value::String(got.clone()), Value::from),
            "bool" => Value::Bool(got == "true"),
            _ => Value::String(got),
        });
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let (before, after) = rest.split_at(open);
        out.push_str(before);
        let Some(close) = after.find('}') else {
            out.push_str(after);
            return Some(Value::String(out));
        };
        let name = after.get(1..close).unwrap_or_default();
        out.push_str(&vars.var(name).unwrap_or_default());
        rest = after.get(close + 1..).unwrap_or_default();
    }
    out.push_str(rest);
    Some(Value::String(out))
}

/// The value at `path` written as `format` asks: `None` when there is none.
#[must_use]
pub fn shown(value: &Value, path: &str, format: Format) -> Option<String> {
    match format {
        Format::Flag(yes, no) => Some(
            if text(value, path).is_some_and(|t| t.eq_ignore_ascii_case("true")) {
                yes
            } else {
                no
            }
            .to_owned(),
        ),
        Format::Tail => text(value, path).map(|t| tail(&t)),
        Format::Pairs => pairs(&get(value, path)?),
        Format::Named(names) => text(value, path).map(|t| {
            names
                .iter()
                .find(|(from, _)| *from == t)
                .map_or(t, |(_, to)| (*to).to_owned())
        }),
        Format::Json => {
            let v = get(value, path)?;
            let doc = match &v {
                // IAM answers its documents URL-encoded, as on its wire.
                Value::String(s) => serde_json::from_str::<Value>(s)
                    .or_else(|_| serde_json::from_str(&crate::api::resources::decode(s)))
                    .unwrap_or(v.clone()),
                _ => v.clone(),
            };
            match doc {
                Value::Null => None,
                Value::String(s) if s.is_empty() => None,
                Value::Array(a) if a.is_empty() => None,
                Value::Object(m) if m.is_empty() => None,
                Value::String(s) => Some(s),
                other => Some(super::pretty::pretty(&other)),
            }
        }
        _ => text(value, path).filter(|t| !t.is_empty()),
    }
}

/// `[{"Name":"a","Value":"1"}]` (or `Key`/`Value`, or `{"a":"1"}`) as `a=1`, pairs joined by `, `; `None` when there are none.
fn pairs(value: &Value) -> Option<String> {
    let one = |k: &Value, v: &Value| {
        let v = match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        format!("{}={v}", k.as_str().unwrap_or_default())
    };
    let all: Vec<String> = match value {
        Value::Array(items) => items
            .iter()
            .filter_map(|i| {
                let k = i.get("Name").or_else(|| i.get("Key"))?;
                Some(one(k, i.get("Value").unwrap_or(&Value::Null)))
            })
            .collect(),
        Value::Object(m) => m
            .iter()
            .map(|(k, v)| one(&Value::String(k.clone()), v))
            .collect(),
        _ => return None,
    };
    (!all.is_empty()).then(|| all.join(", "))
}

#[cfg(test)]
mod tests;
