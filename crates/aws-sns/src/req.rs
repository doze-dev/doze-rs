//! A request read as JSON (the gateway turned a Query form into JSON before it was checked against the model), and the errors every
//! handler words the same way.

use std::collections::BTreeMap;

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

    /// A map of strings (`Attributes`).
    pub(crate) fn map(&self, name: &str) -> BTreeMap<String, String> {
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

    /// The `Tags` member as pairs, in the order given.
    pub(crate) fn tags(&self) -> Vec<(String, String)> {
        self.v
            .get("Tags")
            .and_then(Value::as_array)
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

    pub(crate) fn account(&self) -> &str {
        &self.call.caller.account_id
    }

    pub(crate) fn region(&self) -> &str {
        &self.call.caller.region
    }
}

/// `Invalid parameter: <what>`.
pub(crate) fn invalid(what: impl AsRef<str>) -> ApiError {
    ApiError::modeled(
        "InvalidParameterException",
        format!("Invalid parameter: {}", what.as_ref()),
    )
}

/// `Invalid parameter: <name> Reason: <reason>`.
pub(crate) fn invalid_because(name: &str, reason: impl AsRef<str>) -> ApiError {
    invalid(format!("{name} Reason: {}", reason.as_ref()))
}

pub(crate) fn not_found(message: &str) -> ApiError {
    ApiError::modeled("NotFoundException", message)
}

/// The words AWS gave for a topic that is not there (recorded 2026-11-09).
pub(crate) fn no_topic() -> ApiError {
    not_found("Topic does not exist")
}

pub(crate) fn no_subscription() -> ApiError {
    // (memory)
    not_found("Subscription does not exist")
}

pub(crate) fn no_resource() -> ApiError {
    // (memory)
    ApiError::modeled("ResourceNotFoundException", "Resource does not exist")
}
