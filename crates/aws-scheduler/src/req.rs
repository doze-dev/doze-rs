//! A request read as JSON (the gateway decoded it against the model first), and the errors every handler words the same way.
//!
//! No Scheduler error has been recorded yet: every sentence here is from memory of AWS's answers and marked so.

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

    /// A timestamp member (seconds since the epoch on the wire) in milliseconds.
    pub(crate) fn time_ms(&self, name: &str) -> Option<i64> {
        #[allow(clippy::cast_possible_truncation)]
        // WHY: seconds since the epoch in ms fit an i64 by far.
        self.v
            .get(name)
            .and_then(Value::as_f64)
            .map(|s| (s * 1000.0).round() as i64)
    }

    /// The schedule's group: the one named, or `default`.
    pub(crate) fn group(&self) -> &str {
        self.opt("GroupName")
            .filter(|g| !g.is_empty())
            .unwrap_or(crate::model::DEFAULT_GROUP)
    }

    pub(crate) fn account(&self) -> &str {
        &self.call.caller.account_id
    }

    pub(crate) fn region(&self) -> &str {
        &self.call.caller.region
    }

    /// The `Tags` member as pairs, in the order given.
    pub(crate) fn tags(&self) -> Vec<(String, String)> {
        self.v
            .get("Tags")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .map(|t| {
                let get = |k: &str| t.get(k).and_then(Value::as_str).unwrap_or_default();
                (get("Key").to_owned(), get("Value").to_owned())
            })
            .collect()
    }
}

/// `ResourceNotFoundException`.
pub(crate) fn not_found(message: impl Into<String>) -> ApiError {
    ApiError::modeled("ResourceNotFoundException", message)
}

/// (memory)
pub(crate) fn no_schedule(name: &str) -> ApiError {
    not_found(format!("Schedule {name} does not exist."))
}

/// (memory)
pub(crate) fn no_group(name: &str) -> ApiError {
    not_found(format!("Schedule group {name} does not exist."))
}

/// `ConflictException`.
pub(crate) fn conflict(message: impl Into<String>) -> ApiError {
    ApiError::modeled("ConflictException", message)
}

/// `ValidationException`.
pub(crate) fn invalid(message: impl Into<String>) -> ApiError {
    ApiError::modeled("ValidationException", message)
}

/// The error an operation that is refused by name gives if it is reached anyway.
pub(crate) fn not_built() -> ApiError {
    ApiError::new(
        501,
        "NotImplemented",
        "This operation is not built in doze-aws.",
    )
}
