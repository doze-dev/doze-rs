//! Forced events: a step a service takes by itself when its time comes, which the console's Lab asks it to take now.
//!
//! A forced event is never a transition of its own. The service runs the function its own timer or its own request path runs
//! (SQS's `ChangeMessageVisibility`, Scheduler's invocation, DynamoDB's time-to-live sweep, Lambda's retirement of a warm process),
//! behind the check its own call would meet, and says in words when there is nothing to do. It is not an AWS operation and is not
//! on the gateway: only a caller inside the process, holding the service ([`crate::service::Service::forced`]), can ask for one.

use serde_json::Value;

use crate::ApiError;
use crate::service::{BoxFuture, Caller};

/// The kinds of forced event, as a service is told them (the console's Lab names its rehearsals by the same words).
pub mod kind {
    /// SQS: the in-flight messages of a queue come back, as when their visibility timeout has run out.
    pub const EXPIRE_VISIBILITY: &str = "expire-visibility";
    /// EventBridge Scheduler: a schedule is invoked now, by the path its timer takes.
    pub const FIRE_SCHEDULE: &str = "fire-schedule";
    /// DynamoDB: the time-to-live sweep of a table.
    pub const SWEEP_TTL: &str = "sweep-ttl";
    /// Lambda: the warm processes of a function are ended, so its next invocation starts one.
    pub const COLD_START: &str = "cold-start";
}

/// A forced step that was carried out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Done {
    /// What it did, in a sentence (it is kept in the Lab's history as it is).
    pub sentence: String,
    /// What it acted on (a queue's name, a schedule's `group/name`, a table, a function).
    pub subject: String,
}

/// What a forced step came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// It ran.
    Done(Done),
    /// There was nothing to do, and why, in words: what the service's own timer would have left.
    Nothing(String),
}

/// A service that can carry out forced events.
pub trait Forced: Send + Sync {
    /// Carries out the forced event `kind` (see [`kind`]) for `caller`, on what `input` names (a JSON object: the kind says which
    /// members). The service asks the authorizer as it does for the matching call, so a caller without that right is refused in
    /// the words its own call is refused in.
    ///
    /// # Errors
    /// The authorizer's refusal, or the service's own words for a resource that is not there or an input that is not understood.
    fn force<'a>(
        &'a self,
        caller: &'a Caller,
        kind: &'a str,
        input: &'a Value,
    ) -> BoxFuture<'a, Result<Step, ApiError>>;
}

/// The text member `name` of a forced event's input, if there is one.
#[must_use]
pub fn text<'a>(input: &'a Value, name: &str) -> Option<&'a str> {
    input
        .get(name)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

/// The refusal for an input that lacks a member the kind needs.
#[must_use]
pub fn missing(member: &str) -> ApiError {
    ApiError::new(
        400,
        "ValidationException",
        format!("The forced event needs {member}."),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_member_is_read_when_it_is_text_and_not_empty() {
        let input = serde_json::json!({"queue": "orders", "empty": "", "n": 3});
        assert_eq!(text(&input, "queue"), Some("orders"));
        assert_eq!(text(&input, "empty"), None);
        assert_eq!(text(&input, "n"), None);
        assert_eq!(text(&input, "absent"), None);
    }

    #[test]
    fn a_missing_member_is_named() {
        let e = missing("a queue");
        assert_eq!((e.status, e.code.as_ref()), (400, "ValidationException"));
        assert_eq!(e.message, "The forced event needs a queue.");
    }
}
