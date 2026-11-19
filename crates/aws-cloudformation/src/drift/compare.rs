//! The comparison: each property the template sets (the model: "Only resource properties explicitly defined in the stack template
//! are checked for drift") against what the resource's service says now, as the model's `PropertyDifference`s: `NOT_EQUAL` for a
//! changed value, `REMOVE` for one that is gone, `ADD` for an element added to a list.
//!
//! Values are compared as CloudFormation's handlers take them: `"30"` and `30` are the same, a JSON document given as an object in
//! the template and kept as text by the service is read back as JSON. Lists of tags (and of anything keyed by `Key` or
//! `AttributeName`) are compared by key, not by position; the lists in [`SETS`] are compared as sets. A property the reader of the
//! type cannot read is not compared. A value with a dynamic reference (`{{resolve:...}}`) is not compared: what it resolved to is
//! never kept (memory: AWS does not check them either). Paths are written `/Property/Sub/0` (memory).

use serde_json::{Map, Value};

/// Lists whose order means nothing.
const SETS: &[&str] = &["/ManagedPolicyArns", "/Architectures"];

/// One property that differs.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Diff {
    pub(crate) path: String,
    pub(crate) expected: String,
    pub(crate) actual: String,
    /// `ADD`, `REMOVE` or `NOT_EQUAL`.
    pub(crate) kind: String,
}

/// What was compared: the expected and actual properties (the ones compared only), and how they differ.
#[derive(Debug, Default)]
pub(crate) struct Compared {
    pub(crate) expected: Map<String, Value>,
    pub(crate) actual: Map<String, Value>,
    pub(crate) diffs: Vec<Diff>,
}

/// A value as a property difference writes it: a string as it is, anything else as JSON.
fn shown(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn has_dynamic_reference(v: &Value) -> bool {
    match v {
        Value::String(s) => s.contains("{{resolve:"),
        Value::Array(a) => a.iter().any(has_dynamic_reference),
        Value::Object(o) => o.values().any(has_dynamic_reference),
        _ => false,
    }
}

/// Compares the properties the template sets with the ones read back. `actual` holds every property the reader knows (null for
/// one that is not set); one it does not hold is not compared.
pub(crate) fn compare(expected: &Map<String, Value>, actual: &Map<String, Value>) -> Compared {
    let mut out = Compared::default();
    for (k, e) in expected {
        let Some(a) = actual.get(k) else { continue };
        if has_dynamic_reference(e) {
            continue;
        }
        out.expected.insert(k.clone(), e.clone());
        if !a.is_null() {
            out.actual.insert(k.clone(), a.clone());
        }
        diff(&format!("/{k}"), e, a, &mut out.diffs);
    }
    out
}

fn push(out: &mut Vec<Diff>, path: &str, e: &Value, a: &Value, kind: &str) {
    out.push(Diff {
        path: path.to_owned(),
        expected: shown(e),
        actual: shown(a),
        kind: kind.to_owned(),
    });
}

/// A JSON document kept as text.
fn parsed(s: &str) -> Option<Value> {
    serde_json::from_str::<Value>(s)
        .ok()
        .filter(|v| v.is_object() || v.is_array())
}

fn diff(path: &str, e: &Value, a: &Value, out: &mut Vec<Diff>) {
    match (e, a) {
        (Value::Null, _) => {}
        (_, Value::Null) => push(out, path, e, a, "REMOVE"),
        (Value::Object(eo), Value::Object(ao)) => objects(path, eo, ao, out),
        (Value::Object(_) | Value::Array(_), Value::String(s)) => match parsed(s) {
            Some(p) => diff(path, e, &p, out),
            None => push(out, path, e, a, "NOT_EQUAL"),
        },
        (Value::String(s), Value::Object(_) | Value::Array(_)) => match parsed(s) {
            Some(p) => diff(path, &p, a, out),
            None => push(out, path, e, a, "NOT_EQUAL"),
        },
        (Value::Array(ea), Value::Array(aa)) => arrays(path, ea, aa, out),
        _ if shown(e) != shown(a) => push(out, path, e, a, "NOT_EQUAL"),
        _ => {}
    }
}

/// The members the template sets; a tags object also says which keys were added.
fn objects(path: &str, eo: &Map<String, Value>, ao: &Map<String, Value>, out: &mut Vec<Diff>) {
    for (k, ev) in eo {
        diff(
            &format!("{path}/{k}"),
            ev,
            ao.get(k).unwrap_or(&Value::Null),
            out,
        );
    }
    if path == "/Tags" {
        for (k, av) in ao.iter().filter(|(k, _)| !eo.contains_key(*k)) {
            push(out, &format!("{path}/{k}"), &Value::Null, av, "ADD");
        }
    }
}

/// The member a keyed list's elements are told apart by.
fn key_of(items: &[Value]) -> Option<&'static str> {
    ["Key", "AttributeName"]
        .into_iter()
        .find(|k| !items.is_empty() && items.iter().all(|i| i.get(*k).is_some()))
}

fn arrays(path: &str, ea: &[Value], aa: &[Value], out: &mut Vec<Diff>) {
    if let Some(k) = key_of(ea).or_else(|| key_of(aa)) {
        return keyed(path, k, ea, aa, out);
    }
    if SETS.contains(&path) {
        let texts: Vec<String> = aa.iter().map(shown).collect();
        let wanted: Vec<String> = ea.iter().map(shown).collect();
        for (i, e) in ea.iter().enumerate() {
            if !texts.contains(&shown(e)) {
                push(out, &format!("{path}/{i}"), e, &Value::Null, "REMOVE");
            }
        }
        for (i, a) in aa.iter().enumerate() {
            if !wanted.contains(&shown(a)) {
                push(out, &format!("{path}/{i}"), &Value::Null, a, "ADD");
            }
        }
        return;
    }
    for i in 0..ea.len().max(aa.len()) {
        let p = format!("{path}/{i}");
        match (ea.get(i), aa.get(i)) {
            (Some(e), Some(a)) => diff(&p, e, a, out),
            (Some(e), None) => push(out, &p, e, &Value::Null, "REMOVE"),
            (None, Some(a)) => push(out, &p, &Value::Null, a, "ADD"),
            (None, None) => {}
        }
    }
}

fn keyed(path: &str, k: &str, ea: &[Value], aa: &[Value], out: &mut Vec<Diff>) {
    let find = |items: &[Value], want: &Value| -> Option<Value> {
        items.iter().find(|x| x.get(k) == Some(want)).cloned()
    };
    for (i, e) in ea.iter().enumerate() {
        let p = format!("{path}/{i}");
        match e.get(k).and_then(|id| find(aa, id)) {
            Some(a) => diff(&p, e, &a, out),
            None => push(out, &p, e, &Value::Null, "REMOVE"),
        }
    }
    let mut next = ea.len();
    for a in aa {
        if a.get(k).and_then(|id| find(ea, id)).is_none() {
            push(out, &format!("{path}/{next}"), &Value::Null, a, "ADD");
            next += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use serde_json::json;

    use super::*;

    fn run(e: Value, a: Value) -> Vec<(String, String, String, String)> {
        compare(e.as_object().unwrap(), a.as_object().unwrap())
            .diffs
            .into_iter()
            .map(|d| (d.path, d.kind, d.expected, d.actual))
            .collect()
    }

    #[test]
    fn scalars_compare_as_text() {
        assert!(
            run(
                json!({"VisibilityTimeout": "30"}),
                json!({"VisibilityTimeout": 30})
            )
            .is_empty()
        );
        assert_eq!(
            run(
                json!({"VisibilityTimeout": 30}),
                json!({"VisibilityTimeout": "60"})
            ),
            vec![(
                "/VisibilityTimeout".into(),
                "NOT_EQUAL".into(),
                "30".into(),
                "60".into()
            )]
        );
    }

    #[test]
    fn unread_properties_and_dynamic_references_are_not_compared() {
        assert!(run(json!({"Code": {"ZipFile": "x"}}), json!({})).is_empty());
        assert!(
            run(
                json!({"Value": "{{resolve:ssm:/p}}"}),
                json!({"Value": "v"})
            )
            .is_empty()
        );
    }

    #[test]
    fn a_property_gone_is_removed() {
        assert_eq!(
            run(
                json!({"KmsMasterKeyId": "alias/k"}),
                json!({"KmsMasterKeyId": null})
            ),
            vec![(
                "/KmsMasterKeyId".into(),
                "REMOVE".into(),
                "alias/k".into(),
                "null".into()
            )]
        );
    }

    #[test]
    fn json_documents_compare_as_json() {
        let e = json!({"RedrivePolicy": {"deadLetterTargetArn": "a", "maxReceiveCount": 3}});
        let a =
            json!({"RedrivePolicy": "{\"maxReceiveCount\":\"3\",\"deadLetterTargetArn\":\"a\"}"});
        assert!(run(e, a).is_empty());
    }

    #[test]
    fn tags_compare_by_key() {
        let e = json!({"Tags": [{"Key": "a", "Value": "1"}, {"Key": "b", "Value": "2"}]});
        let a = json!({"Tags": [{"Key": "c", "Value": "3"}, {"Key": "a", "Value": "9"}]});
        assert_eq!(
            run(e, a),
            vec![
                (
                    "/Tags/0/Value".into(),
                    "NOT_EQUAL".into(),
                    "1".into(),
                    "9".into()
                ),
                (
                    "/Tags/1".into(),
                    "REMOVE".into(),
                    r#"{"Key":"b","Value":"2"}"#.into(),
                    "null".into()
                ),
                (
                    "/Tags/2".into(),
                    "ADD".into(),
                    "null".into(),
                    r#"{"Key":"c","Value":"3"}"#.into()
                ),
            ]
        );
        let map = run(
            json!({"Tags": {"a": "1"}}),
            json!({"Tags": {"a": "1", "z": "2"}}),
        );
        assert_eq!(
            map,
            vec![("/Tags/z".into(), "ADD".into(), "null".into(), "2".into())]
        );
    }

    #[test]
    fn sets_ignore_order() {
        let e = json!({"ManagedPolicyArns": ["x", "y"]});
        assert!(run(e.clone(), json!({"ManagedPolicyArns": ["y", "x"]})).is_empty());
        assert_eq!(run(e, json!({"ManagedPolicyArns": ["y"]})).len(), 1);
    }
}
