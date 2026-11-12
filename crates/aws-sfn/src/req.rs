//! A request read as JSON (the gateway decoded it against the model first), and the errors handlers word the same way. Step
//! Functions spells its members lowercase-initial (`stateMachineArn`). The messages are AWS's where recorded (`Execution Does Not
//! Exist: '<arn>'`, `State Machine Does Not Exist: '<arn>'`, `Invalid Name: '<name>'`, `Invalid Role Arn: '<arn>'`); the others
//! follow the same pattern from memory.

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

    pub(crate) fn account(&self) -> &str {
        &self.call.caller.account_id
    }

    pub(crate) fn region(&self) -> &str {
        &self.call.caller.region
    }

    /// `tags` as pairs, in the order given.
    pub(crate) fn tags(&self) -> Vec<(String, String)> {
        self.v
            .get("tags")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|t| {
                let get = |k: &str| {
                    t.get(k)
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned()
                };
                (get("key"), get("value"))
            })
            .collect()
    }

    /// The paging position of `nextToken`, or 0.
    pub(crate) fn page_from(&self) -> Result<usize, ApiError> {
        match self.opt("nextToken") {
            None | Some("") => Ok(0),
            Some(t) => t
                .strip_prefix("doze-")
                .and_then(|n| n.parse().ok())
                .ok_or_else(|| ApiError::modeled("InvalidToken", format!("Invalid Token: '{t}'"))),
        }
    }

    /// `maxResults`, 0 meaning the service's default.
    pub(crate) fn page_size(&self, default: usize) -> usize {
        match self.int("maxResults") {
            Some(n) if n > 0 => usize::try_from(n).unwrap_or(default).min(1000),
            _ => default,
        }
    }
}

/// The token for the page that starts at `at`.
pub(crate) fn token(at: usize) -> String {
    format!("doze-{at}")
}

pub(crate) fn no_machine(arn: &str) -> ApiError {
    ApiError::modeled(
        "StateMachineDoesNotExist",
        format!("State Machine Does Not Exist: '{arn}'"),
    )
}

pub(crate) fn no_execution(arn: &str) -> ApiError {
    ApiError::modeled(
        "ExecutionDoesNotExist",
        format!("Execution Does Not Exist: '{arn}'"),
    )
}

pub(crate) fn no_activity(arn: &str) -> ApiError {
    ApiError::modeled(
        "ActivityDoesNotExist",
        format!("Activity Does Not Exist: '{arn}'"),
    )
}

pub(crate) fn invalid_arn(what: &str, arn: &str) -> ApiError {
    ApiError::modeled("InvalidArn", format!("Invalid {what}: '{arn}'"))
}

pub(crate) fn not_found(message: impl Into<String>) -> ApiError {
    ApiError::modeled("ResourceNotFound", message)
}

pub(crate) fn invalid(message: impl Into<String>) -> ApiError {
    ApiError::modeled("ValidationException", message)
}

pub(crate) fn conflict(message: impl Into<String>) -> ApiError {
    ApiError::modeled("ConflictException", message)
}

pub(crate) fn invalid_definition(message: impl Into<String>) -> ApiError {
    ApiError::modeled("InvalidDefinition", message)
}

/// The error an operation that is refused by name gives if it is reached anyway.
pub(crate) fn not_built() -> ApiError {
    ApiError::new(
        501,
        "NotImplemented",
        "This operation is not built in doze-aws.",
    )
}

/// Whether `name` is a name Step Functions accepts: 1 to 80 characters, none of whitespace, `< > { } [ ] ? *`, `" # % \ ^ | ~ `
/// $ & , ; : /`, or control characters.
pub(crate) fn valid_name(name: &str) -> bool {
    let count = name.chars().count();
    (1..=80).contains(&count)
        && !name.chars().any(|c| {
            c.is_whitespace()
                || c.is_control()
                || "<>{}[]?*\"#%\\^|~`$&,;:/".contains(c)
                || ('\u{7f}'..='\u{9f}').contains(&c)
        })
}

/// `InvalidName`, worded as recorded.
pub(crate) fn invalid_name(name: &str) -> ApiError {
    ApiError::modeled("InvalidName", format!("Invalid Name: '{name}'"))
}

/// Seconds since the epoch, as a JSON number, for a timestamp member.
pub(crate) fn secs(ms: i64) -> Value {
    #[allow(clippy::cast_precision_loss)]
    Value::from(ms as f64 / 1000.0)
}

/// A JSON object member set only when the value is there.
pub(crate) fn put(m: &mut Map<String, Value>, k: &str, v: Option<Value>) {
    if let Some(v) = v {
        m.insert(k.to_owned(), v);
    }
}
