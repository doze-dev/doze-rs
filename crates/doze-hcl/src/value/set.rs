//! cty's sets: which elements are the same, and the order they iterate in.
//!
//! cty keeps a set as hash buckets (the hash is a serialization of the value, below) and finds
//! duplicates only within a bucket, with `Equals`. It iterates in a sorted order: nulls and
//! unknowns last, strings, numbers and bools by value, anything else by its serialization.

use super::Value;
use super::quote::go_quote;
use crate::types::Type;
use std::cmp::Ordering;

pub(super) fn normalize(ety: &Type, elems: Vec<Value>) -> Vec<Value> {
    let mut kept: Vec<(Vec<u8>, Value)> = Vec::with_capacity(elems.len());
    for v in elems {
        let h = hash_bytes(&v);
        let dup = kept
            .iter()
            .any(|(kh, k)| *kh == h && v.equals(k) == Value::Bool(true));
        if !dup {
            kept.push((h, v));
        }
    }
    kept.sort_by(|(ha, a), (hb, b)| order(ety, (ha, a), (hb, b)));
    kept.into_iter().map(|(_, v)| v).collect()
}

/// cty's `setRules.Less`, as an ordering (ties keep insertion order: the sort is stable).
fn order(ety: &Type, (ha, a): (&[u8], &Value), (hb, b): (&[u8], &Value)) -> Ordering {
    if a.raw_equals(b) {
        return Ordering::Equal;
    }
    let rank = |v: &Value| match v {
        Value::Null(_) => 2,
        Value::Unknown(_) => 1,
        _ => 0,
    };
    match rank(a).cmp(&rank(b)) {
        Ordering::Equal if rank(a) != 0 => return Ordering::Equal,
        Ordering::Equal => {}
        other => return other,
    }
    match (ety, a, b) {
        (Type::String, Value::String(x), Value::String(y)) => x.cmp(y),
        (Type::Bool, Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (Type::Number, Value::Number(x), Value::Number(y)) => x.cmp_value(y),
        _ => ha.cmp(hb),
    }
}

/// cty's `makeSetHashBytes`.
pub(crate) fn hash_bytes(v: &Value) -> Vec<u8> {
    let mut out = String::new();
    append(v, &mut out);
    out.into_bytes()
}

fn append(v: &Value, out: &mut String) {
    match v {
        Value::Unknown(_) => out.push('?'),
        Value::Null(_) => out.push('~'),
        Value::Number(n) => out.push_str(&n.text_g10()),
        Value::Bool(b) => out.push(if *b { 'T' } else { 'F' }),
        Value::String(s) => out.push_str(&go_quote(s)),
        Value::Map(_, m) => {
            out.push('{');
            for (k, e) in m {
                out.push_str(&go_quote(k));
                out.push(':');
                append(e, out);
                out.push(';');
            }
            out.push('}');
        }
        Value::List(_, es) | Value::Set(_, es) => {
            out.push('[');
            for e in es {
                append(e, out);
                out.push(';');
            }
            out.push(']');
        }
        Value::Object(m) => {
            out.push('<');
            for e in m.values() {
                append(e, out);
                out.push(';');
            }
            out.push('>');
        }
        Value::Tuple(es) => {
            out.push('<');
            for e in es {
                append(e, out);
                out.push(';');
            }
            out.push('>');
        }
    }
}
