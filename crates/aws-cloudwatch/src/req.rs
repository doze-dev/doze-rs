//! A request read as JSON (the gateway has already checked it against the model), and the small helpers every handler uses.

use aws_wire::ApiError;
use aws_wire::service::Call;
use serde_json::{Map, Value};

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

    pub(crate) fn opt(&self, name: &str) -> Option<&str> {
        self.v.get(name).and_then(Value::as_str)
    }

    pub(crate) fn s(&self, name: &str) -> &str {
        self.opt(name).unwrap_or_default()
    }

    pub(crate) fn int(&self, name: &str) -> Option<i64> {
        self.v.get(name).and_then(Value::as_i64)
    }

    pub(crate) fn num(&self, name: &str) -> Option<f64> {
        self.v.get(name).and_then(Value::as_f64)
    }

    pub(crate) fn flag(&self, name: &str) -> Option<bool> {
        self.v.get(name).and_then(Value::as_bool)
    }

    pub(crate) fn strings(&self, name: &str) -> Vec<String> {
        strings(self.v.get(name))
    }

    /// A timestamp member (seconds since the epoch, possibly fractional) as milliseconds.
    pub(crate) fn time(&self, name: &str) -> Option<i64> {
        self.num(name).map(secs_to_ms)
    }

    /// The `Dimensions` member, as (name, value) pairs sorted by name.
    pub(crate) fn dimensions(&self) -> Vec<(String, String)> {
        dimensions(self.v.get("Dimensions"))
    }

    /// The `Tags` member (a list of `Key`/`Value`) as pairs.
    pub(crate) fn tags(&self) -> Vec<(String, String)> {
        tag_pairs(self.v.get("Tags"))
    }

    pub(crate) fn account(&self) -> &str {
        &self.call.caller.account_id
    }

    pub(crate) fn region(&self) -> &str {
        &self.call.caller.region
    }
}

pub(crate) fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// A `Dimensions` list as sorted (name, value) pairs.
pub(crate) fn dimensions(v: Option<&Value>) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = v
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|d| {
                    let get = |k: &str| {
                        d.get(k)
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned()
                    };
                    (get("Name"), get("Value"))
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
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

/// Seconds since the epoch as milliseconds.
#[allow(clippy::cast_possible_truncation)]
pub(crate) fn secs_to_ms(secs: f64) -> i64 {
    (secs * 1000.0).round() as i64
}

/// Milliseconds as the seconds a timestamp member carries.
#[allow(clippy::cast_precision_loss)]
pub(crate) fn secs(ms: i64) -> Value {
    serde_json::json!(ms as f64 / 1000.0)
}

/// Sets a member of a JSON object (indexing a `Value` can panic; this cannot).
pub(crate) trait Put {
    fn put(&mut self, key: &str, value: Value);
}

impl Put for Value {
    fn put(&mut self, key: &str, value: Value) {
        if let Self::Object(m) = self {
            m.insert(key.to_owned(), value);
        }
    }
}

/// A `Dimensions` list from sorted pairs.
pub(crate) fn dimension_list(dims: &[(String, String)]) -> Value {
    Value::Array(
        dims.iter()
            .map(|(n, v)| serde_json::json!({"Name": n, "Value": v}))
            .collect(),
    )
}

/// `InvalidParameterValue`, the code CloudWatch uses for most bad values.
pub(crate) fn bad_value(message: impl Into<String>) -> ApiError {
    ApiError::modeled("InvalidParameterValueException", message)
}

/// `ValidationException` (the Query code is `ValidationError`).
pub(crate) fn validation(message: impl Into<String>) -> ApiError {
    ApiError::modeled("ValidationException", message)
}
