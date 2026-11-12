//! The request half of awsQuery: a form body of dotted keys, read into the JSON the model's decoder takes. Going through JSON means
//! every check the model makes of a JSON request (required members, lengths, ranges, enums, patterns) is made of a Query one too.
//!
//! The keys follow the member names (an `@xmlName` wins): a structure member is `Parent.Member`; a list is `Name.member.N` (the
//! element's `xmlName` instead of `member` when it has one) or, flattened, `Name.N`; a map is `Name.entry.N.key` and
//! `Name.entry.N.value`, or flattened `Name.N.key` and `Name.N.value`.

use std::collections::BTreeMap;

use aws_core::uri;
use aws_schema::tables::{
    K_BOOL, K_FLOAT, K_INT, K_LIST, K_MAP, K_STRUCT, K_UNION, MemberRef, Tables,
};
use serde_json::{Map, Number, Value};

/// The most dotted-key depth read: deeper than any model nests.
const MAX_DEPTH: usize = 24;

/// The pairs of a form body (`a=1&b.c=2`), percent-decoded. A key given twice keeps its last value.
// fuzz:parser fuzz_form
#[must_use]
pub fn parse_form(body: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for pair in body.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let (k, v) = (decode(k), decode(v));
        out.insert(k, v);
    }
    out
}

fn decode(s: &str) -> String {
    let plus = s.replace('+', " ");
    let decoded = uri::decode_str(&plus).map(std::borrow::Cow::into_owned);
    decoded.unwrap_or(plus)
}

/// The JSON for the structure `shape` out of `form`.
#[must_use]
pub fn to_json(tables: &Tables, shape: u32, form: &BTreeMap<String, String>) -> Value {
    Value::Object(object(tables, shape, form, "", 0))
}

fn object(
    t: &Tables,
    shape: u32,
    form: &BTreeMap<String, String>,
    prefix: &str,
    depth: usize,
) -> Map<String, Value> {
    let mut out = Map::new();
    let Some(sh) = t.shape(shape) else {
        return out;
    };
    if depth > MAX_DEPTH {
        return out;
    }
    for i in sh.first..sh.first + sh.count {
        let Some(m) = t.member(i) else { continue };
        let key = format!("{prefix}{}", m.xml.unwrap_or(m.name));
        if let Some(v) = value(t, &m, form, &key, depth + 1) {
            out.insert(m.wire.to_owned(), v);
        }
    }
    out
}

fn has_children(form: &BTreeMap<String, String>, key: &str) -> bool {
    let from = format!("{key}.");
    form.range(from.clone()..)
        .next()
        .is_some_and(|(k, _)| k.starts_with(&from))
}

fn value(
    t: &Tables,
    m: &MemberRef<'_>,
    form: &BTreeMap<String, String>,
    key: &str,
    depth: usize,
) -> Option<Value> {
    match m.kind {
        K_STRUCT | K_UNION => has_children(form, key)
            .then(|| Value::Object(object(t, m.target, form, &format!("{key}."), depth))),
        K_LIST => list(t, m, form, key, depth),
        K_MAP => map(t, m, form, key, depth),
        kind => form.get(key).map(|text| scalar(kind, text)),
    }
}

fn scalar(kind: u8, text: &str) -> Value {
    match kind {
        K_BOOL => match text {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            _ => Value::String(text.to_owned()),
        },
        K_INT => text.parse::<i64>().map_or_else(
            |_| Value::String(text.to_owned()),
            |n| Value::Number(n.into()),
        ),
        K_FLOAT => text
            .parse::<f64>()
            .ok()
            .and_then(Number::from_f64)
            .map_or_else(|| Value::String(text.to_owned()), Value::Number),
        _ => Value::String(text.to_owned()),
    }
}

/// The indices `N` under `base` (`base` ends in a dot): the numbers that start each key, sorted.
fn indices(form: &BTreeMap<String, String>, base: &str) -> Vec<u32> {
    let mut found: Vec<u32> = form
        .range(base.to_owned()..)
        .take_while(|(k, _)| k.starts_with(base))
        .filter_map(|(k, _)| k.get(base.len()..)?.split('.').next()?.parse().ok())
        .collect();
    found.sort_unstable();
    found.dedup();
    found
}

fn list(
    t: &Tables,
    m: &MemberRef<'_>,
    form: &BTreeMap<String, String>,
    key: &str,
    depth: usize,
) -> Option<Value> {
    let element = t.member(m.target)?;
    let base = if m.flags & 2 != 0 {
        format!("{key}.")
    } else {
        format!("{key}.{}.", element.xml.unwrap_or("member"))
    };
    let items: Vec<Value> = indices(form, &base)
        .into_iter()
        .filter_map(|n| value(t, &element, form, &format!("{base}{n}"), depth + 1))
        .collect();
    // `Name=` with nothing after it is an empty list.
    (!items.is_empty() || form.get(key).is_some_and(String::is_empty))
        .then_some(Value::Array(items))
}

fn map(
    t: &Tables,
    m: &MemberRef<'_>,
    form: &BTreeMap<String, String>,
    key: &str,
    depth: usize,
) -> Option<Value> {
    let (k, v) = (t.member(m.target)?, t.member(m.target + 1)?);
    let base = if m.flags & 2 != 0 {
        format!("{key}.")
    } else {
        format!("{key}.entry.")
    };
    let mut out = Map::new();
    for n in indices(form, &base) {
        let name = form.get(&format!("{base}{n}.{}", k.xml.unwrap_or("key")))?;
        if let Some(item) = value(
            t,
            &v,
            form,
            &format!("{base}{n}.{}", v.xml.unwrap_or("value")),
            depth + 1,
        ) {
            out.insert(name.clone(), item);
        }
    }
    (!out.is_empty()).then_some(Value::Object(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_form_is_percent_decoded_and_plus_is_a_space() {
        let f = parse_form("Action=CreateRole&Description=a+b%2Fc&RoleName=x&Empty=");
        assert_eq!(f.get("Description").map(String::as_str), Some("a b/c"));
        assert_eq!(f.get("Empty").map(String::as_str), Some(""));
        assert!(parse_form("").is_empty());
        assert_eq!(parse_form("a&b=1").len(), 2);
    }

    #[test]
    fn indices_are_sorted_numerically_and_each_once() {
        let f = parse_form("L.member.10=a&L.member.2=b&L.member.2.x=c&L.member.1=d");
        assert_eq!(indices(&f, "L.member."), [1, 2, 10]);
    }
}
