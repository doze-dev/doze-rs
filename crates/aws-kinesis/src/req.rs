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

    /// A string-to-string map member (tags).
    pub(crate) fn map(&self, name: &str) -> Vec<(String, String)> {
        self.v
            .get(name)
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_owned()))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn account(&self) -> &str {
        &self.call.caller.account_id
    }

    pub(crate) fn region(&self) -> &str {
        &self.call.caller.region
    }

    /// The stream the request names: by `StreamName`, or by the name inside `StreamARN`.
    pub(crate) fn stream_name(&self) -> Option<String> {
        self.opt("StreamName")
            .map(str::to_owned)
            .or_else(|| self.opt("StreamARN").and_then(name_of_arn))
            .or_else(|| self.opt("ResourceARN").and_then(name_of_arn))
    }
}

/// The stream name in a stream or consumer ARN (`arn:aws:kinesis:r:a:stream/NAME[/consumer/...]`).
pub(crate) fn name_of_arn(arn: &str) -> Option<String> {
    let rest = arn.split_once(":stream/")?.1;
    Some(rest.split('/').next()?.to_owned())
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

/// The seconds since the epoch, as a timestamp member.
#[allow(clippy::cast_precision_loss)]
pub(crate) fn secs(ms: i64) -> Value {
    serde_json::json!(ms as f64 / 1000.0)
}

/// The refusals Kinesis words the same way everywhere.
pub(crate) fn invalid(message: impl Into<String>) -> ApiError {
    ApiError::modeled("InvalidArgumentException", message)
}
