//! Matching a pattern against an event.
//!
//! Where AWS's documentation is silent (memory): a field that is absent matches only `exists: false`, and so do its absent parents;
//! `anything-but` over an array matches when any element qualifies; numbers match numbers and text matches text, never each other;
//! a field that is an object matches no value condition except `exists: true`.

use std::net::IpAddr;

use serde_json::Value;

use super::{Cmp, Cond, Node, Object, Pattern, Text, wildcard};

/// Whether `event` (the whole event document) matches the pattern.
pub(crate) fn matches(pattern: &Pattern, event: &Value) -> bool {
    object(&pattern.0, Some(event))
}

/// Whether a field's value (or its absence) satisfies an object of the pattern.
fn object(o: &Object, value: Option<&Value>) -> bool {
    // An array of objects matches when any one of them does.
    if let Some(Value::Array(items)) = value {
        return items.iter().any(|item| object(o, Some(item)));
    }
    let map = match value {
        Some(Value::Object(map)) => Some(map),
        Some(_) => return false,
        None => None,
    };
    o.keys.iter().all(|(key, node)| {
        let field = map.and_then(|m| m.get(key));
        match node {
            Node::Leaf(conds) => leaf(conds, field),
            Node::Nested(inner) => object(inner, field),
        }
    }) && o
        .any_of
        .iter()
        .all(|group| group.iter().any(|alt| object(alt, value)))
}

fn leaf(conds: &[Cond], field: Option<&Value>) -> bool {
    let Some(field) = field else {
        return conds.iter().any(|c| matches!(c, Cond::Exists(false)));
    };
    let present = |c: &Cond| match c {
        Cond::Exists(want) => Some(*want),
        _ => None,
    };
    if let Some(want) = conds.iter().find_map(present)
        && want
    {
        return true;
    }
    match field {
        Value::Array(items) => items.iter().any(|v| scalar(conds, v)),
        other => scalar(conds, other),
    }
}

/// Whether any condition holds for one value that is not an array.
fn scalar(conds: &[Cond], v: &Value) -> bool {
    conds.iter().any(|c| one(c, v))
}

fn one(c: &Cond, v: &Value) -> bool {
    match (c, v) {
        (Cond::Null, Value::Null) => true,
        (Cond::Bool(a), Value::Bool(b)) => a == b,
        (Cond::Num(a), Value::Number(n)) => n.as_f64().is_some_and(|b| *a == b),
        (Cond::Text(t), Value::String(s)) => text(t, s),
        (Cond::AnythingBut(list), v) => {
            !matches!(v, Value::Object(_)) && !list.iter().any(|x| one(x, v))
        }
        (Cond::Numeric(pairs), Value::Number(n)) => n
            .as_f64()
            .is_some_and(|x| pairs.iter().all(|(cmp, bound)| compare(*cmp, x, *bound))),
        (Cond::Cidr(net, bits), Value::String(s)) => s
            .parse::<IpAddr>()
            .is_ok_and(|ip| in_network(ip, *net, *bits)),
        _ => false,
    }
}

fn compare(cmp: Cmp, x: f64, bound: f64) -> bool {
    match cmp {
        Cmp::Eq => (x - bound).abs() < f64::EPSILON * bound.abs().max(1.0),
        Cmp::Gt => x > bound,
        Cmp::Ge => x >= bound,
        Cmp::Lt => x < bound,
        Cmp::Le => x <= bound,
    }
}

fn text(t: &Text, s: &str) -> bool {
    match t {
        Text::Exact(x) => x == s,
        Text::Prefix(x) => s.starts_with(x.as_str()),
        Text::Suffix(x) => s.ends_with(x.as_str()),
        Text::PrefixIgnoreCase(x) => s.to_lowercase().starts_with(&x.to_lowercase()),
        Text::SuffixIgnoreCase(x) => s.to_lowercase().ends_with(&x.to_lowercase()),
        Text::EqualsIgnoreCase(x) => s.to_lowercase() == x.to_lowercase(),
        Text::Wildcard(p) => wildcard::matches(p, s),
    }
}

/// Whether `ip` is inside `net/bits`. An address of the other family is outside.
fn in_network(ip: IpAddr, net: IpAddr, bits: u8) -> bool {
    match (ip, net) {
        (IpAddr::V4(a), IpAddr::V4(b)) => prefix_equal(&a.octets(), &b.octets(), bits),
        (IpAddr::V6(a), IpAddr::V6(b)) => prefix_equal(&a.octets(), &b.octets(), bits),
        _ => false,
    }
}

fn prefix_equal(a: &[u8], b: &[u8], bits: u8) -> bool {
    let whole = usize::from(bits / 8);
    let rest = bits % 8;
    if a.get(..whole) != b.get(..whole) {
        return false;
    }
    if rest == 0 {
        return true;
    }
    let mask = 0xffu8 << (8 - rest);
    match (a.get(whole), b.get(whole)) {
        (Some(x), Some(y)) => x & mask == y & mask,
        _ => false,
    }
}
