//! A request as the handlers read it: the JSON the gateway built from path, query, headers and body, and who is calling.

use aws_wire::service::{Call, Caller};
use serde_json::{Map, Value};

use crate::arn::{self, FnRef};
use aws_wire::ApiError;

/// One decoded request.
#[derive(Debug)]
pub(crate) struct Req {
    pub(crate) body: Value,
    pub(crate) caller: Caller,
}

impl Req {
    pub(crate) fn new(call: &Call) -> Self {
        Self {
            body: serde_json::from_str(&call.body).unwrap_or(Value::Null),
            caller: call.caller.clone(),
        }
    }

    pub(crate) fn region(&self) -> &str {
        &self.caller.region
    }

    pub(crate) fn account(&self) -> &str {
        &self.caller.account_id
    }

    /// A string member, or `""`.
    pub(crate) fn text(&self, key: &str) -> &str {
        self.opt(key).unwrap_or_default()
    }

    pub(crate) fn opt(&self, key: &str) -> Option<&str> {
        self.body.get(key).and_then(Value::as_str)
    }

    pub(crate) fn int(&self, key: &str) -> Option<i64> {
        self.body.get(key).and_then(Value::as_i64)
    }

    pub(crate) fn flag(&self, key: &str) -> Option<bool> {
        self.body.get(key).and_then(Value::as_bool)
    }

    pub(crate) fn value(&self, key: &str) -> Option<&Value> {
        self.body.get(key).filter(|v| !v.is_null())
    }

    /// The members of the request as an object (empty when the body was not one).
    pub(crate) fn members(&self) -> &Map<String, Value> {
        static EMPTY: std::sync::LazyLock<Map<String, Value>> = std::sync::LazyLock::new(Map::new);
        self.body.as_object().unwrap_or(&EMPTY)
    }

    /// The function the request names: `FunctionName` and, if given, `Qualifier`.
    ///
    /// # Errors
    /// A reference that is not one of the forms, or names another region.
    pub(crate) fn function(&self) -> Result<FnRef, ApiError> {
        self.function_in("FunctionName")
    }

    pub(crate) fn function_in(&self, key: &str) -> Result<FnRef, ApiError> {
        arn::parse(
            self.text(key),
            self.opt("Qualifier"),
            self.region(),
            self.account(),
        )
    }
}
