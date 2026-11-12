//! Does a message pass a filter policy? Against its attributes (the default scope) or against its body, which must then be JSON.
//!
//! A value is a string, a number, a boolean or null. A key's conditions match if any of them matches any value the key has; a key
//! with no value matches only `exists: false`. Strings match string conditions only and numbers numeric ones, so `100` does not match
//! `"100"` (the scope of the unrecorded cases is AWS's documentation, not a recording).

use std::collections::BTreeMap;
use std::net::IpAddr;

use serde_json::Value;

use crate::filter::{Cmp, Cond, Node, Rules};

/// A message attribute as published.
#[derive(Clone, Debug)]
pub(crate) struct Attribute {
    pub(crate) data_type: String,
    pub(crate) string_value: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
enum Scalar {
    Str(String),
    Num(f64),
    Bool(bool),
    Null,
}

/// Whether the message attributes pass the policy.
pub(crate) fn passes_attributes(rules: &Rules, attrs: &BTreeMap<String, Attribute>) -> bool {
    holds(rules, &|key| attribute_field(attrs.get(key)), &|_, _| false)
}

/// Whether the JSON body passes the policy. A body that is not JSON passes nothing.
pub(crate) fn passes_body(rules: &Rules, body: &str) -> bool {
    match serde_json::from_str::<Value>(body) {
        Ok(Value::Object(m)) => body_holds(rules, &m),
        _ => false,
    }
}

/// A key's presence and values.
struct Field {
    present: bool,
    values: Vec<Scalar>,
}

fn attribute_field(attr: Option<&Attribute>) -> Field {
    let Some(a) = attr else {
        return Field {
            present: false,
            values: Vec::new(),
        };
    };
    let text = a.string_value.as_deref().unwrap_or_default();
    let values = if a.data_type.starts_with("String.Array") {
        match serde_json::from_str::<Value>(text) {
            Ok(Value::Array(items)) => items.iter().filter_map(scalar).collect(),
            _ => Vec::new(),
        }
    } else if a.data_type.starts_with("String") {
        vec![Scalar::Str(text.to_owned())]
    } else if a.data_type.starts_with("Number") {
        text.trim().parse().map(Scalar::Num).into_iter().collect()
    } else {
        Vec::new()
    };
    Field {
        present: true,
        values,
    }
}

fn scalar(v: &Value) -> Option<Scalar> {
    match v {
        Value::String(s) => Some(Scalar::Str(s.clone())),
        Value::Number(n) => n.as_f64().map(Scalar::Num),
        Value::Bool(b) => Some(Scalar::Bool(*b)),
        Value::Null => Some(Scalar::Null),
        _ => None,
    }
}

/// Every key matches, and one `$or` branch does. `nested` says whether a nested rule holds for a key (only a body has any).
fn holds(
    rules: &Rules,
    field: &dyn Fn(&str) -> Field,
    nested: &dyn Fn(&str, &Rules) -> bool,
) -> bool {
    let keys = rules.keys.iter().all(|(key, node)| match node {
        Node::Leaf(conds) => leaf(conds, &field(key)),
        Node::Nested(inner) => nested(key, inner),
    });
    keys && rules
        .or
        .as_ref()
        .is_none_or(|branches| branches.iter().any(|b| holds(b, field, nested)))
}

fn body_holds(rules: &Rules, object: &serde_json::Map<String, Value>) -> bool {
    holds(
        rules,
        &|key| body_field(object.get(key)),
        &|key, inner| match object.get(key) {
            Some(Value::Object(m)) => body_holds(inner, m),
            Some(Value::Array(items)) => items
                .iter()
                .any(|i| matches!(i, Value::Object(m) if body_holds(inner, m))),
            _ => false,
        },
    )
}

fn body_field(v: Option<&Value>) -> Field {
    match v {
        None => Field {
            present: false,
            values: Vec::new(),
        },
        Some(Value::Array(items)) => Field {
            present: true,
            values: items.iter().filter_map(scalar).collect(),
        },
        Some(other) => Field {
            present: true,
            values: scalar(other).into_iter().collect(),
        },
    }
}

fn leaf(conds: &[Cond], field: &Field) -> bool {
    conds.iter().any(|c| match c {
        Cond::Exists(want) => *want == field.present,
        other => field.values.iter().any(|v| one(other, v)),
    })
}

fn one(c: &Cond, v: &Scalar) -> bool {
    match (c, v) {
        (Cond::Str(a), Scalar::Str(b)) => a == b,
        (Cond::Num(a), Scalar::Num(b)) => (a - b).abs() < f64::EPSILON,
        (Cond::Bool(a), Scalar::Bool(b)) => a == b,
        (Cond::Null, Scalar::Null) => true,
        (Cond::Prefix(p), Scalar::Str(s)) => s.starts_with(p.as_str()),
        (Cond::Suffix(p), Scalar::Str(s)) => s.ends_with(p.as_str()),
        (Cond::EqualsIgnoreCase(a), Scalar::Str(s)) => a.to_lowercase() == s.to_lowercase(),
        (Cond::ButExact(list), Scalar::Str(_) | Scalar::Num(_)) => !list.iter().any(|x| one(x, v)),
        (Cond::ButPrefix(p), Scalar::Str(s)) => !s.starts_with(p.as_str()),
        (Cond::ButSuffix(p), Scalar::Str(s)) => !s.ends_with(p.as_str()),
        (Cond::Numeric(tests), Scalar::Num(n)) => {
            tests.iter().all(|(cmp, x)| compare(*cmp, *n, *x))
        }
        (Cond::Cidr(net, bits), Scalar::Str(s)) => s.parse().is_ok_and(|a| in_net(a, *net, *bits)),
        _ => false,
    }
}

fn compare(cmp: Cmp, a: f64, b: f64) -> bool {
    match cmp {
        Cmp::Eq => (a - b).abs() < f64::EPSILON,
        Cmp::Gt => a > b,
        Cmp::Ge => a >= b,
        Cmp::Lt => a < b,
        Cmp::Le => a <= b,
    }
}

/// Whether `addr` is in `net`/`bits` (same family, the first `bits` bits equal).
fn in_net(addr: IpAddr, net: IpAddr, bits: u8) -> bool {
    match (addr, net) {
        (IpAddr::V4(a), IpAddr::V4(n)) => same_prefix(&a.octets(), &n.octets(), bits),
        (IpAddr::V6(a), IpAddr::V6(n)) => same_prefix(&a.octets(), &n.octets(), bits),
        _ => false,
    }
}

fn same_prefix(a: &[u8], b: &[u8], bits: u8) -> bool {
    let whole = usize::from(bits / 8);
    let rest = bits % 8;
    let head = a.get(..whole) == b.get(..whole);
    let tail = match (a.get(whole), b.get(whole)) {
        (Some(x), Some(y)) if rest > 0 => (x ^ y) >> (8 - rest) == 0,
        _ => true,
    };
    head && tail
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::parse;

    fn attrs(pairs: &[(&str, &str, &str)]) -> BTreeMap<String, Attribute> {
        pairs
            .iter()
            .map(|(k, t, v)| {
                (
                    (*k).to_owned(),
                    Attribute {
                        data_type: (*t).to_owned(),
                        string_value: Some((*v).to_owned()),
                    },
                )
            })
            .collect()
    }

    fn attr_passes(policy: &str, a: &[(&str, &str, &str)]) -> bool {
        passes_attributes(&parse(policy, false).unwrap(), &attrs(a))
    }

    #[test]
    fn exact_values_match_by_type() {
        assert!(attr_passes(
            r#"{"kind":["order","refund"]}"#,
            &[("kind", "String", "refund")]
        ));
        assert!(!attr_passes(
            r#"{"kind":["order"]}"#,
            &[("kind", "String", "refund")]
        ));
        assert!(!attr_passes(r#"{"kind":["order"]}"#, &[]));
        assert!(attr_passes(r#"{"n":[100]}"#, &[("n", "Number", "100.0")]));
        assert!(!attr_passes(r#"{"n":[100]}"#, &[("n", "String", "100")]));
        assert!(!attr_passes(r#"{"n":["100"]}"#, &[("n", "Number", "100")]));
    }

    #[test]
    fn every_key_must_match_and_or_branches_are_alternatives() {
        let both = r#"{"a":["1"],"b":["2"]}"#;
        assert!(attr_passes(
            both,
            &[("a", "String", "1"), ("b", "String", "2")]
        ));
        assert!(!attr_passes(both, &[("a", "String", "1")]));
        let or = r#"{"$or":[{"a":["1"]},{"b":["2"]}]}"#;
        assert!(attr_passes(or, &[("b", "String", "2")]));
        assert!(!attr_passes(or, &[("c", "String", "2")]));
    }

    #[test]
    fn the_operators_each_do_their_one_thing() {
        let s = |p: &str, v: &str| attr_passes(p, &[("k", "String", v)]);
        assert!(s(r#"{"k":[{"prefix":"ord"}]}"#, "order"));
        assert!(!s(r#"{"k":[{"prefix":"ord"}]}"#, "reorder"));
        assert!(s(r#"{"k":[{"suffix":"der"}]}"#, "order"));
        assert!(s(r#"{"k":[{"equals-ignore-case":"ORDER"}]}"#, "order"));
        assert!(s(r#"{"k":[{"anything-but":"refund"}]}"#, "order"));
        assert!(!s(
            r#"{"k":[{"anything-but":["order","refund"]}]}"#,
            "order"
        ));
        assert!(!s(r#"{"k":[{"anything-but":{"prefix":"or"}}]}"#, "order"));
        assert!(s(r#"{"k":[{"cidr":"10.0.0.0/24"}]}"#, "10.0.0.77"));
        assert!(!s(r#"{"k":[{"cidr":"10.0.0.0/24"}]}"#, "10.0.1.1"));
        assert!(s(r#"{"k":[{"cidr":"10.0.0.0/20"}]}"#, "10.0.15.1"));
        assert!(!s(r#"{"k":[{"cidr":"::1/128"}]}"#, "10.0.0.1"));
        let n = |p: &str, v: &str| attr_passes(p, &[("k", "Number", v)]);
        assert!(n(r#"{"k":[{"numeric":[">",1,"<=",5]}]}"#, "5"));
        assert!(!n(r#"{"k":[{"numeric":[">",1,"<=",5]}]}"#, "6"));
        assert!(n(r#"{"k":[{"numeric":["=",3]}]}"#, "3"));
    }

    #[test]
    fn exists_looks_at_presence_and_an_array_matches_any_element() {
        assert!(attr_passes(r#"{"k":[{"exists":false}]}"#, &[]));
        assert!(!attr_passes(
            r#"{"k":[{"exists":false}]}"#,
            &[("k", "String", "x")]
        ));
        assert!(attr_passes(
            r#"{"k":[{"exists":true}]}"#,
            &[("k", "Binary", "x")]
        ));
        assert!(attr_passes(
            r#"{"k":["b"]}"#,
            &[("k", "String.Array", r#"["a","b"]"#)]
        ));
        assert!(!attr_passes(
            r#"{"k":["c"]}"#,
            &[("k", "String.Array", r#"["a","b"]"#)]
        ));
    }

    fn body(policy: &str, text: &str) -> bool {
        passes_body(&parse(policy, true).unwrap(), text)
    }

    #[test]
    fn a_body_policy_reaches_nested_fields_and_array_elements() {
        let p = r#"{"order":{"kind":["rush"],"qty":[{"numeric":[">",1]}]}}"#;
        assert!(body(p, r#"{"order":{"kind":"rush","qty":3}}"#));
        assert!(!body(p, r#"{"order":{"kind":"rush","qty":1}}"#));
        assert!(body(
            p,
            r#"{"order":[{"kind":"slow"},{"kind":"rush","qty":9}]}"#
        ));
        assert!(body(r#"{"tags":["b"]}"#, r#"{"tags":["a","b"]}"#));
        assert!(body(
            r#"{"ok":[true],"gone":[null]}"#,
            r#"{"ok":true,"gone":null}"#
        ));
        assert!(!body(
            r#"{"order":{"kind":["rush"]}}"#,
            r#"{"order":"rush"}"#
        ));
        assert!(body(r#"{"x":[{"exists":false}]}"#, r#"{"y":1}"#));
    }

    #[test]
    fn a_body_that_is_not_a_json_object_passes_nothing() {
        assert!(!body(r#"{"a":[{"exists":false}]}"#, "plain text"));
        assert!(!body(r#"{"a":["1"]}"#, "[1]"));
    }
}
