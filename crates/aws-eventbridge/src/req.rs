//! A request read as JSON (the gateway decoded it against the model first), and the errors every handler words the same way.

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

    pub(crate) fn flag(&self, name: &str) -> bool {
        self.v.get(name).and_then(Value::as_bool).unwrap_or(false)
    }

    pub(crate) fn list(&self, name: &str) -> &[Value] {
        self.v
            .get(name)
            .and_then(Value::as_array)
            .map_or(&[], Vec::as_slice)
    }

    /// An object or array member as the JSON text it was sent as.
    pub(crate) fn json_text(&self, name: &str) -> Option<String> {
        self.v.get(name).map(Value::to_string)
    }

    pub(crate) fn account(&self) -> &str {
        &self.call.caller.account_id
    }

    pub(crate) fn region(&self) -> &str {
        &self.call.caller.region
    }

    /// The `Tags` member as pairs, in the order given.
    pub(crate) fn tags(&self) -> Vec<(String, String)> {
        self.list("Tags")
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

pub(crate) fn no_bus(name: &str) -> ApiError {
    not_found(format!("Event bus {name} does not exist."))
}

pub(crate) fn no_rule(bus: &str, name: &str) -> ApiError {
    not_found(format!("Rule {name} does not exist on EventBus {bus}."))
}

pub(crate) fn exists(message: impl Into<String>) -> ApiError {
    ApiError::modeled("ResourceAlreadyExistsException", message)
}

pub(crate) fn invalid(message: impl Into<String>) -> ApiError {
    ApiError::modeled("ValidationException", message)
}

pub(crate) fn invalid_pattern(message: impl Into<String>) -> ApiError {
    ApiError::modeled("InvalidEventPatternException", message)
}

pub(crate) fn limit(message: impl Into<String>) -> ApiError {
    ApiError::modeled("LimitExceededException", message)
}

/// The error an operation that is refused by name gives if it is reached anyway.
pub(crate) fn not_built() -> ApiError {
    ApiError::new(
        501,
        "NotImplemented",
        "This operation is not built in doze-aws.",
    )
}
