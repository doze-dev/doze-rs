//! Event filtering for event source mappings: the content-filter pattern language of EventBridge, which Lambda uses to decide which
//! records of a source reach the function. A pattern is a JSON object; each key must match the same key of the record. An array of
//! conditions matches if any one does; an object nests; `$or` joins alternative patterns. Conditions: an exact value, `prefix`,
//! `suffix`, `equals-ignore-case`, `wildcard`, `anything-but`, `numeric` and `exists`.

use serde_json::Value;

/// Whether `pattern` (JSON text) is a pattern at all: an object whose leaves are arrays of conditions.
pub(crate) fn valid(pattern: &str) -> bool {
    serde_json::from_str::<Value>(pattern).is_ok_and(|p| p.is_object() && node_ok(&p))
}

fn node_ok(p: &Value) -> bool {
    match p {
        Value::Object(o) => o.iter().all(|(k, v)| match (k.as_str(), v) {
            ("$or", Value::Array(alts)) => alts.iter().all(node_ok),
            ("$or", _) => false,
            (_, Value::Object(_)) => node_ok(v),
            (_, Value::Array(conds)) => conds.iter().all(cond_ok),
            _ => false,
        }),
        _ => false,
    }
}

fn cond_ok(c: &Value) -> bool {
    match c {
        Value::Object(o) => o.len() == 1 && o.keys().all(|k| matches!(k.as_str(), "prefix" | "suffix" | "equals-ignore-case" | "wildcard" | "anything-but" | "numeric" | "exists" | "cidr")),
        _ => true,
    }
}

/// Whether `event` matches `pattern`.
pub(crate) fn matches(pattern: &Value, event: &Value) -> bool {
    let Value::Object(p) = pattern else { return false };
    p.iter().all(|(k, v)| {
        if k == "$or" {
            return v.as_array().is_some_and(|alts| alts.iter().any(|a| matches(a, event)));
        }
        let field = event.get(k);
        match v {
            Value::Object(_) => match field {
                Some(Value::Array(items)) => items.iter().any(|i| matches(v, i)),
                Some(f @ Value::Object(_)) => matches(v, f),
                _ => false,
            },
            Value::Array(conds) => leaf(conds, field),
            _ => false,
        }
    })
}

fn leaf(conds: &[Value], field: Option<&Value>) -> bool {
    let absent = |c: &Value| c.get("exists").and_then(Value::as_bool) == Some(false);
    let present = |c: &Value| c.get("exists").and_then(Value::as_bool) == Some(true);
    let Some(f) = field else { return conds.iter().any(absent) };
    let values: Vec<&Value> = match f {
        Value::Array(items) => items.iter().collect(),
        one => vec![one],
    };
    conds.iter().any(|c| present(c) || (!absent(c) && values.iter().any(|v| one(c, v))))
}

fn one(cond: &Value, v: &Value) -> bool {
    let Some(o) = cond.as_object() else { return scalar_eq(cond, v) };
    let Some((op, arg)) = o.iter().next() else { return false };
    let s = v.as_str();
    match (op.as_str(), arg) {
        ("prefix", Value::String(p)) => s.is_some_and(|s| s.starts_with(p.as_str())),
        ("suffix", Value::String(p)) => s.is_some_and(|s| s.ends_with(p.as_str())),
        ("equals-ignore-case", Value::String(p)) => s.is_some_and(|s| s.eq_ignore_ascii_case(p)),
        ("wildcard", Value::String(p)) => s.is_some_and(|s| wild(p, s)),
        ("anything-but", Value::Array(xs)) => !xs.iter().any(|x| scalar_eq(x, v)),
        ("anything-but", Value::Object(inner)) => !inner.iter().next().is_some_and(|(k, a)| one(&serde_json::json!({ k.as_str(): a }), v)),
        ("anything-but", x) => !scalar_eq(x, v),
        ("numeric", Value::Array(ops)) => v.as_f64().is_some_and(|n| numeric(ops, n)),
        _ => false,
    }
}

fn scalar_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        _ => a == b,
    }
}

/// `["=", 5]`, `[">", 0, "<=", 10]`.
fn numeric(ops: &[Value], n: f64) -> bool {
    ops.chunks(2).all(|pair| match pair {
        [Value::String(op), x] => x.as_f64().is_some_and(|x| match op.as_str() {
            "=" => (n - x).abs() < f64::EPSILON,
            ">" => n > x,
            ">=" => n >= x,
            "<" => n < x,
            "<=" => n <= x,
            _ => false,
        }),
        _ => false,
    })
}

/// A string against a pattern whose `*` stands for any run of characters.
fn wild(pattern: &str, s: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut rest = s;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            let Some(r) = rest.strip_prefix(part) else { return false };
            rest = r;
        } else if i == parts.len() - 1 {
            return rest.ends_with(part);
        } else {
            let Some(at) = rest.find(part) else { return false };
            rest = rest.get(at + part.len()..).unwrap_or_default();
        }
    }
    rest.is_empty()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn exact_values_nesting_and_the_condition_operators() {
        let event = json!({"body": {"kind": "order", "n": 7, "tags": ["a", "b"]}, "eventName": "INSERT"});
        let yes = |p: Value| matches(&p, &event);
        assert!(yes(json!({"eventName": ["INSERT", "MODIFY"]})));
        assert!(!yes(json!({"eventName": ["REMOVE"]})));
        assert!(yes(json!({"body": {"kind": [{"prefix": "ord"}]}})));
        assert!(yes(json!({"body": {"n": [{"numeric": [">", 5, "<=", 7]}]}})));
        assert!(yes(json!({"body": {"tags": ["b"]}})));
        assert!(yes(json!({"body": {"missing": [{"exists": false}]}})));
        assert!(yes(json!({"body": {"kind": [{"anything-but": ["x"]}]}})));
        assert!(yes(json!({"$or": [{"eventName": ["REMOVE"]}, {"body": {"n": [7]}}]})));
        assert!(!yes(json!({"body": {"kind": [{"suffix": "x"}]}})));
        assert!(yes(json!({"eventName": [{"wildcard": "I*RT"}]})));
    }

    #[test]
    fn a_pattern_must_be_an_object_of_condition_lists() {
        assert!(valid(r#"{"a":{"b":["x",{"prefix":"y"}]}}"#));
        assert!(!valid(r#"{"a":"x"}"#));
        assert!(!valid("[1]"));
        assert!(!valid("{"));
    }
}
