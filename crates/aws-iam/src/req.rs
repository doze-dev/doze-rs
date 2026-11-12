//! A request read as JSON (the gateway turned a Query form into JSON before it was checked against the model, so the body is JSON
//! whichever way the request came), and the small helpers every handler uses.

use aws_wire::ApiError;
use aws_wire::service::Call;
use serde_json::{Map, Value, json};

/// A decoded request.
#[derive(Debug)]
pub(crate) struct Req<'a> {
    pub(crate) call: &'a Call,
    pub(crate) v: Value,
}

impl<'a> Req<'a> {
    pub(crate) fn new(call: &'a Call) -> Self {
        let v = serde_json::from_str(&call.body).unwrap_or(Value::Null);
        Self {
            call,
            v: if v.is_object() {
                v
            } else {
                Value::Object(Map::new())
            },
        }
    }

    /// A string member, or `""`.
    pub(crate) fn s(&self, name: &str) -> &str {
        self.opt(name).unwrap_or_default()
    }

    pub(crate) fn opt(&self, name: &str) -> Option<&str> {
        self.v.get(name).and_then(Value::as_str)
    }

    pub(crate) fn int(&self, name: &str) -> Option<i64> {
        self.v.get(name).and_then(Value::as_i64)
    }

    pub(crate) fn flag(&self, name: &str) -> Option<bool> {
        self.v.get(name).and_then(Value::as_bool)
    }

    pub(crate) fn strings(&self, name: &str) -> Vec<String> {
        self.v
            .get(name)
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The `Tags` member as pairs.
    pub(crate) fn tags(&self) -> Vec<(String, String)> {
        tag_pairs(self.v.get("Tags"))
    }

    pub(crate) fn account(&self) -> &str {
        &self.call.caller.account_id
    }
}

pub(crate) fn tag_pairs(v: Option<&Value>) -> Vec<(String, String)> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|t| {
                    let get = |k: &str| {
                        t.get(k)
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned()
                    };
                    (get("Key"), get("Value"))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Tags as the list IAM writes.
pub(crate) fn tag_list<'a>(tags: impl Iterator<Item = (&'a String, &'a String)>) -> Value {
    Value::Array(tags.map(|(k, v)| json!({"Key": k, "Value": v})).collect())
}

/// IAM's paging: `MaxItems` (default 100) and `Marker` (here the offset), answered as `IsTruncated` and `Marker`.
pub(crate) fn page(r: &Req<'_>, items: Vec<Value>, member: &str) -> Result<Value, ApiError> {
    let limit = r
        .int("MaxItems")
        .map_or(100, |n| usize::try_from(n).unwrap_or(100))
        .max(1);
    let start = match r.opt("Marker") {
        Some(m) => m
            .parse::<usize>()
            .map_err(|_| ApiError::modeled("InvalidInputException", "Invalid Marker."))?,
        None => 0,
    };
    let more = items.len() > start + limit;
    let shown: Vec<Value> = items.into_iter().skip(start).take(limit).collect();
    let mut out = Map::new();
    out.insert(member.to_owned(), Value::Array(shown));
    out.insert("IsTruncated".to_owned(), Value::Bool(more));
    if more {
        out.insert(
            "Marker".to_owned(),
            Value::String((start + limit).to_string()),
        );
    }
    Ok(Value::Object(out))
}

/// The seconds since the epoch, as a timestamp member.
#[allow(clippy::cast_precision_loss)]
pub(crate) fn secs(ms: i64) -> Value {
    json!(ms as f64 / 1000.0)
}

/// A policy document as IAM writes one in an answer: URL-encoded.
pub(crate) fn encoded(document: &str) -> String {
    aws_core::uri::encode(document.as_bytes(), false)
}
