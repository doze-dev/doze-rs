//! Putting an event on the account's default bus from another service (S3 object events now; Step Functions, CloudWatch, SSM and Secrets
//! Manager events when those services have something to say). The event goes through the peer bus as a `PutEvents` made by the
//! sending service, so it is matched against the rules, archived and delivered like any other, and is not asked of the authorizer
//! again (the service's own caller was).

use aws_peers::Peers;
use aws_wire::ApiError;
use aws_wire::service::Caller;
use serde_json::{Value, json};

/// An event to put on the default bus.
#[derive(Debug)]
pub struct Event<'a> {
    /// `aws.s3`.
    pub source: &'a str,
    /// `Object Created`.
    pub detail_type: &'a str,
    /// The event's `detail` (a JSON object).
    pub detail: &'a Value,
    /// The ARNs the event is about.
    pub resources: &'a [String],
    /// When it happened, in milliseconds since the epoch; now when `None`.
    pub time_ms: Option<i64>,
}

/// Puts the event on the default bus of the caller's account and region, as service `acting` (`s3`).
///
/// # Errors
/// EventBridge's own error, or an internal one saying no EventBridge is running (see [`is_absent`]).
pub async fn put(
    peers: &Peers,
    acting: &str,
    caller: &Caller,
    event: &Event<'_>,
) -> Result<(), ApiError> {
    // WHY: a time in ms is far below 2^53, so the conversion to seconds loses nothing.
    #[allow(clippy::cast_precision_loss)]
    let time = event.time_ms.map(|ms| ms as f64 / 1000.0);
    let mut entry = serde_json::Map::new();
    entry.insert("Source".into(), json!(event.source));
    entry.insert("DetailType".into(), json!(event.detail_type));
    entry.insert("Detail".into(), json!(event.detail.to_string()));
    entry.insert("Resources".into(), json!(event.resources));
    if let Some(t) = time {
        entry.insert("Time".into(), json!(t));
    }
    let answer = peers
        .call(
            acting,
            caller,
            "events",
            "PutEvents",
            &json!({"Entries": [entry]}),
        )
        .await?;
    if answer.get("FailedEntryCount").and_then(Value::as_i64) == Some(0) {
        return Ok(());
    }
    let why = answer
        .pointer("/Entries/0/ErrorMessage")
        .and_then(Value::as_str)
        .unwrap_or("EventBridge refused the event");
    Err(ApiError::internal(why.to_owned()))
}

/// Whether the error says no EventBridge service is registered (a caller may then skip the event without complaint).
#[must_use]
pub fn is_absent(e: &ApiError) -> bool {
    e.message.starts_with("no service events")
}
