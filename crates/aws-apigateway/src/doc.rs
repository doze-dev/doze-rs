//! What API Gateway keeps: each API is one JSON document (the API as `Get*` shows it, with its resources, methods, stages and the rest
//! under it), stored as JSON text. One representation for the store, the answers and the execute-api plane, so a member AWS adds
//! needs no field here. Members whose name begins with `_` are ours (a deployment's snapshot) and never leave in an answer.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

/// A JSON document, kept as its text.
#[derive(Debug, Default)]
pub(crate) struct Doc(pub(crate) Value);

impl Serialize for Doc {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for Doc {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        serde_json::from_str(&text)
            .map(Doc)
            .map_err(D::Error::custom)
    }
}

/// The object member `key` of `v`, made (as `{}`) if it is not there; `None` only when `v` is not an object.
pub(crate) fn child<'a>(v: &'a mut Value, key: &str) -> Option<&'a mut Map<String, Value>> {
    let o = v.as_object_mut()?;
    let entry = o
        .entry(key.to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    entry.as_object_mut()
}

/// The object member `key` of `v`, if there is one.
pub(crate) fn obj<'a>(v: &'a Value, key: &str) -> Option<&'a Map<String, Value>> {
    v.get(key).and_then(Value::as_object)
}

/// The string member `key` of `v`.
pub(crate) fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// Sets `key` of object `v` (nothing for a value that is not an object).
pub(crate) fn put(v: &mut Value, key: &str, value: Value) {
    if let Some(o) = v.as_object_mut() {
        o.insert(key.to_owned(), value);
    }
}

/// Copies the members of `from` named in `keys` that are set into `to`.
pub(crate) fn copy(to: &mut Map<String, Value>, from: &Value, keys: &[&str]) {
    for k in keys {
        if let Some(v) = from.get(*k).filter(|v| !v.is_null()) {
            // WHY: the request is read once and the record outlives it.
            to.insert((*k).to_owned(), v.clone());
        }
    }
}

/// `v` without our own members (`_snapshot`): what an answer may show.
pub(crate) fn public(v: &Value) -> Value {
    match v {
        Value::Object(o) => Value::Object(
            o.iter()
                .filter(|(k, _)| !k.starts_with('_'))
                // WHY: an answer is built from the record, which stays.
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// The values of a map member as a list, our own members left out, in the order `order` gives.
pub(crate) fn items(map: Option<&Map<String, Value>>) -> Vec<Value> {
    map.map(|m| m.values().map(public).collect())
        .unwrap_or_default()
}

/// A JSON value as text (a string without its quotes, null as nothing).
pub(crate) fn plain(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}
