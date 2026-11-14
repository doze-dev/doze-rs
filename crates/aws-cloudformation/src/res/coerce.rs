//! Template values coerced to the types an operation's model expects: CloudFormation reads `"30"` where a model has an integer,
//! `30` where it has a string, and `"true"` where it has a boolean, as its resource handlers do. Members the model does not have
//! are left alone (the decoder skips them).

use aws_schema::tables::{
    K_BOOL, K_FLOAT, K_INT, K_LIST, K_MAP, K_STR, K_STRUCT, K_TS, K_UNION, MemberRef, Tables,
};
use serde_json::Value;

/// Coerces `body` to the input of operation `op`.
pub(crate) fn input(t: &Tables, op: &str, body: &mut Value) {
    let Some(shape) = t.find_op(op).and_then(|i| t.op(i)).and_then(|o| o.input) else {
        return;
    };
    structure(t, shape, body, 0);
}

fn structure(t: &Tables, shape: u32, v: &mut Value, depth: u32) {
    if depth > 32 {
        return;
    }
    let (Some(s), Value::Object(o)) = (t.shape(shape), v) else {
        return;
    };
    for i in s.first..s.first + s.count {
        let Some(m) = t.member(i) else { continue };
        if let Some(x) = o.get_mut(m.wire) {
            member(t, &m, x, depth + 1);
        }
    }
}

fn member(t: &Tables, m: &MemberRef<'_>, v: &mut Value, depth: u32) {
    match m.kind {
        K_STR => {
            *v = match std::mem::take(v) {
                Value::Number(n) => Value::String(n.to_string()),
                Value::Bool(b) => Value::String(b.to_string()),
                // A document given as JSON where the model wants its text (a policy, a definition).
                x @ (Value::Object(_) | Value::Array(_)) => Value::String(x.to_string()),
                x => x,
            };
        }
        K_INT | K_FLOAT | K_TS => {
            if let Value::String(s) = v
                && let Ok(n) = s.trim().parse::<f64>()
            {
                *v = if m.kind == K_INT && n.fract() == 0.0 {
                    Value::Number((n as i64).into())
                } else {
                    serde_json::Number::from_f64(n).map_or(Value::Null, Value::Number)
                };
            }
        }
        K_BOOL => {
            if let Value::String(s) = v {
                match s.to_ascii_lowercase().as_str() {
                    "true" => *v = Value::Bool(true),
                    "false" => *v = Value::Bool(false),
                    _ => {}
                }
            }
        }
        K_LIST => {
            if let (Some(el), Value::Array(a)) = (t.member(m.target), &mut *v) {
                for x in a {
                    member(t, &el, x, depth + 1);
                }
            }
        }
        K_MAP => {
            if let (Some(val), Value::Object(o)) = (t.member(m.target + 1), &mut *v) {
                for x in o.values_mut() {
                    member(t, &val, x, depth + 1);
                }
            }
        }
        K_STRUCT | K_UNION => structure(t, m.target, v, depth + 1),
        _ => {}
    }
}
