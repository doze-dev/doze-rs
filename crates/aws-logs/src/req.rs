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

    /// The log group the request names: `logGroupName`, or `logGroupIdentifier` (a name or an ARN).
    pub(crate) fn group_name(&self) -> Option<String> {
        self.opt("logGroupName")
            .map(str::to_owned)
            .or_else(|| self.opt("logGroupIdentifier").map(name_of_identifier))
    }
}

/// The name in a log group name or ARN (`arn:aws:logs:r:a:log-group:NAME[:*]`).
pub(crate) fn name_of_identifier(id: &str) -> String {
    match id.split_once(":log-group:") {
        Some((_, rest)) => rest.strip_suffix(":*").unwrap_or(rest).to_owned(),
        None => id.to_owned(),
    }
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

/// `InvalidParameterException`.
pub(crate) fn invalid(message: impl Into<String>) -> ApiError {
    ApiError::modeled("InvalidParameterException", message)
}

/// Recorded on AWS (2026-11-09): the words of a missing group.
pub(crate) fn no_group() -> ApiError {
    ApiError::modeled(
        "ResourceNotFoundException",
        "The specified log group does not exist.",
    )
}

/// Recorded on AWS (2026-11-09): the words of a missing stream.
pub(crate) fn no_stream() -> ApiError {
    ApiError::modeled(
        "ResourceNotFoundException",
        "The specified log stream does not exist.",
    )
}
