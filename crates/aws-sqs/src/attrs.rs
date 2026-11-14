//! Queue attributes: which names exist, what values they take, and how they are written back.
//!
//! The ranges are the documented ones, recorded against AWS on 2026-11-09 (`test_attribute_ranges`): the SQS model states
//! none of them as traits.

use std::collections::BTreeMap;

use aws_wire::ApiError;

use crate::model::{Attrs, Queue};

/// Attribute names a caller may set that this build stores without acting on.
const STORED: &[&str] = &[
    "Policy",
    "RedrivePolicy",
    "RedriveAllowPolicy",
    "KmsMasterKeyId",
    "KmsDataKeyReusePeriodSeconds",
    "SqsManagedSseEnabled",
    "ContentBasedDeduplication",
    "DeduplicationScope",
    "FifoThroughputLimit",
    "FifoQueue",
];

/// What a caller asked to set, checked.
#[derive(Debug, Default)]
pub(crate) struct Requested {
    pub(crate) visibility_timeout: Option<u32>,
    pub(crate) delay_seconds: Option<u32>,
    pub(crate) max_message_size: Option<u32>,
    pub(crate) retention_seconds: Option<u32>,
    pub(crate) wait_seconds: Option<u32>,
    pub(crate) stored: BTreeMap<String, String>,
}

fn invalid(name: &str) -> ApiError {
    ApiError::new(
        400,
        "InvalidAttributeValue",
        format!("Invalid value for the parameter {name}."),
    )
}

fn number(name: &str, value: &str, lo: u32, hi: u32) -> Result<u32, ApiError> {
    value
        .parse::<u32>()
        .ok()
        .filter(|n| (lo..=hi).contains(n))
        .ok_or_else(|| invalid(name))
}

/// Checks `(name, value)` pairs a caller supplied.
pub(crate) fn parse<'a>(
    pairs: impl Iterator<Item = (&'a str, &'a str)>,
) -> Result<Requested, ApiError> {
    let mut r = Requested::default();
    for (name, value) in pairs {
        match name {
            "VisibilityTimeout" => r.visibility_timeout = Some(number(name, value, 0, 43_200)?),
            "DelaySeconds" => r.delay_seconds = Some(number(name, value, 0, 900)?),
            "MaximumMessageSize" => {
                r.max_message_size = Some(number(name, value, 1024, 1_048_576)?)
            }
            "MessageRetentionPeriod" => {
                r.retention_seconds = Some(number(name, value, 60, 1_209_600)?)
            }
            "ReceiveMessageWaitTimeSeconds" => r.wait_seconds = Some(number(name, value, 0, 20)?),
            "KmsDataKeyReusePeriodSeconds" => {
                number(name, value, 60, 86_400)?;
                r.stored.insert(name.to_owned(), value.to_owned());
            }
            other if STORED.contains(&other) => {
                r.stored.insert(other.to_owned(), value.to_owned());
            }
            other => {
                return Err(ApiError::new(
                    400,
                    "InvalidAttributeName",
                    format!("Unknown Attribute {other}."),
                ));
            }
        }
    }
    Ok(r)
}

impl Requested {
    /// The attributes a new queue gets: the defaults, overridden by what was asked.
    pub(crate) fn into_attrs(self) -> (Attrs, BTreeMap<String, String>) {
        let mut a = Attrs::default();
        self.apply_to(&mut a);
        (a, self.stored)
    }

    /// Overrides `a` with the numeric settings that were asked for.
    pub(crate) fn apply_to(&self, a: &mut Attrs) {
        a.visibility_timeout = self.visibility_timeout.unwrap_or(a.visibility_timeout);
        a.delay_seconds = self.delay_seconds.unwrap_or(a.delay_seconds);
        a.max_message_size = self.max_message_size.unwrap_or(a.max_message_size);
        a.retention_seconds = self.retention_seconds.unwrap_or(a.retention_seconds);
        a.wait_seconds = self.wait_seconds.unwrap_or(a.wait_seconds);
    }

    /// The first attribute this request sets to something other than `q` has: why `CreateQueue` for an existing name fails.
    pub(crate) fn differs_from(&self, q: &Queue) -> Option<&str> {
        let a = &q.attrs;
        let numeric = [
            (
                "VisibilityTimeout",
                self.visibility_timeout,
                a.visibility_timeout,
            ),
            ("DelaySeconds", self.delay_seconds, a.delay_seconds),
            (
                "MaximumMessageSize",
                self.max_message_size,
                a.max_message_size,
            ),
            (
                "MessageRetentionPeriod",
                self.retention_seconds,
                a.retention_seconds,
            ),
            (
                "ReceiveMessageWaitTimeSeconds",
                self.wait_seconds,
                a.wait_seconds,
            ),
        ];
        let stored = self
            .stored
            .iter()
            .find(|(name, value)| q.stored.get(*name).is_some_and(|have| have != *value));
        numeric
            .into_iter()
            .find(|(_, asked, have)| asked.is_some_and(|n| n != *have))
            .map(|(name, ..)| name)
            .or_else(|| stored.map(|(name, _)| name.as_str()))
    }
}

/// The attributes of `q` the caller named (`All` for every one), as name and value. `now_ms` and the counts are the queue's
/// weather: they are read, not stored.
pub(crate) fn render(
    q: &Queue,
    account: &str,
    region: &str,
    names: &[&str],
    now_ms: i64,
) -> Vec<(String, String)> {
    let all = names.contains(&"All");
    let want = |n: &str| all || names.contains(&n);
    let mut out = Vec::new();
    let mut put = |name: &str, value: String| {
        if want(name) {
            out.push((name.to_owned(), value));
        }
    };
    let a = &q.attrs;
    put("DelaySeconds", a.delay_seconds.to_string());
    put("MaximumMessageSize", a.max_message_size.to_string());
    put("MessageRetentionPeriod", a.retention_seconds.to_string());
    put(
        "QueueArn",
        format!("arn:aws:sqs:{region}:{account}:{}", q.name),
    );
    put("ReceiveMessageWaitTimeSeconds", a.wait_seconds.to_string());
    put("VisibilityTimeout", a.visibility_timeout.to_string());
    put("CreatedTimestamp", q.created_secs.to_string());
    put("LastModifiedTimestamp", q.modified_secs.to_string());
    // Counted against the clock: a message past its retention is gone whether or not it has been removed yet.
    let live = || q.messages.iter().filter(|m| !q.expired(m, now_ms));
    let inflight = live().filter(|m| m.in_flight(now_ms)).count();
    let delayed = live().filter(|m| m.delayed(now_ms)).count();
    put(
        "ApproximateNumberOfMessages",
        (live().count() - inflight - delayed).to_string(),
    );
    put(
        "ApproximateNumberOfMessagesNotVisible",
        inflight.to_string(),
    );
    put("ApproximateNumberOfMessagesDelayed", delayed.to_string());
    // AWS reports SSE by SQS-managed keys as on unless the caller said otherwise.
    let sse = q
        .stored
        .get("SqsManagedSseEnabled")
        .cloned()
        .unwrap_or_else(|| "true".to_owned());
    put("SqsManagedSseEnabled", sse);
    if q.is_fifo() {
        for (name, default) in fifo_defaults() {
            if !q.stored.contains_key(name) {
                put(name, default.to_owned());
            }
        }
    }
    for (name, value) in &q.stored {
        if name != "SqsManagedSseEnabled" {
            put(name, value.clone());
        }
    }
    out
}

fn unknown(name: &str) -> ApiError {
    ApiError::new(
        400,
        "InvalidAttributeName",
        format!("Unknown Attribute {name}."),
    )
}

fn is_bool(value: &str) -> bool {
    value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("false")
}

/// The rules that tie attributes to the kind of queue and to each other. `fifo` is whether the queue is (or is being made)
/// a FIFO queue; `current` the stored attributes it already has; `creating` whether this is `CreateQueue`.
///
/// Messages are those AWS gave in `test_fifo_attribute_rules`, `test_boolean_attribute_spelling`, `test_queue_type_cannot_change`
/// and `test_kms_key_with_sqs_managed_encryption`. The booleans are case-insensitive, and the message for a bad one names the
/// *value* where the parameter's name would be (AWS's wording). The one for `FifoThroughputLimit` on a standard queue is
/// unrecorded.
pub(crate) fn check_kind(
    req: &Requested,
    current: &BTreeMap<String, String>,
    fifo: bool,
    creating: bool,
) -> Result<(), ApiError> {
    for (name, value) in &req.stored {
        let bad_bool = || {
            ApiError::new(
                400,
                "InvalidAttributeValue",
                format!("Invalid value for the parameter {value}."),
            )
        };
        match name.as_str() {
            "FifoQueue" if !creating => return Err(unknown(name)),
            "FifoQueue" if !is_bool(value) => return Err(bad_bool()),
            "ContentBasedDeduplication" if !fifo => return Err(unknown(name)),
            "ContentBasedDeduplication" if !is_bool(value) => return Err(bad_bool()),
            "DeduplicationScope" | "FifoThroughputLimit" if !fifo => {
                return Err(ApiError::new(
                    400,
                    "InvalidAttributeName",
                    format!("You can specify the {name} only when FifoQueue is set to true."),
                ));
            }
            "DeduplicationScope" if !matches!(value.as_str(), "messageGroup" | "queue") => {
                return Err(not_supported(name));
            }
            "FifoThroughputLimit"
                if !matches!(value.as_str(), "perQueue" | "perMessageGroupId") =>
            {
                return Err(not_supported(name));
            }
            _ => {}
        }
    }
    let merged = |name: &str| {
        req.stored
            .get(name)
            .or_else(|| current.get(name))
            .map(String::as_str)
    };
    if merged("FifoThroughputLimit") == Some("perMessageGroupId")
        && merged("DeduplicationScope") != Some("messageGroup")
    {
        return Err(ApiError::new(
            400,
            "InvalidAttributeValue",
            "Invalid value for the parameter FifoThroughputLimit. Reason: To set FifoThroughputLimit to perMessageGroupId, the DeduplicationScope must be messageGroup.",
        ));
    }
    let key = merged("KmsMasterKeyId").is_some_and(|k| !k.is_empty());
    if key
        && req
            .stored
            .get("SqsManagedSseEnabled")
            .is_some_and(|v| v.eq_ignore_ascii_case("true"))
    {
        return Err(ApiError::new(
            400,
            "InvalidAttributeName",
            "You can use one type of server-side encryption (SSE) at one time. You can either enable KMS SSE or SQS SSE.",
        ));
    }
    Ok(())
}

fn not_supported(name: &str) -> ApiError {
    ApiError::new(
        400,
        "InvalidAttributeValue",
        format!("Invalid value for the parameter {name}. Reason: not a supported value."),
    )
}

/// The attributes a FIFO queue reports with their defaults when the caller never set them.
pub(crate) fn fifo_defaults() -> [(&'static str, &'static str); 4] {
    [
        ("FifoQueue", "true"),
        ("ContentBasedDeduplication", "false"),
        ("DeduplicationScope", "queue"),
        ("FifoThroughputLimit", "perQueue"),
    ]
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn the_documented_bounds_are_inclusive() {
        for (name, lo, hi) in [
            ("VisibilityTimeout", "0", "43200"),
            ("DelaySeconds", "0", "900"),
            ("MaximumMessageSize", "1024", "1048576"),
            ("MessageRetentionPeriod", "60", "1209600"),
            ("ReceiveMessageWaitTimeSeconds", "0", "20"),
        ] {
            assert!(parse([(name, lo)].into_iter()).is_ok(), "{name} {lo}");
            assert!(parse([(name, hi)].into_iter()).is_ok(), "{name} {hi}");
        }
        for (name, bad) in [
            ("VisibilityTimeout", "43201"),
            ("VisibilityTimeout", "-1"),
            ("DelaySeconds", "901"),
            ("VisibilityTimeout", "soon"),
            ("MaximumMessageSize", "1023"),
        ] {
            let e = parse([(name, bad)].into_iter()).unwrap_err();
            assert_eq!(
                (e.status, e.code.as_ref(), e.message),
                (
                    400,
                    "InvalidAttributeValue",
                    format!("Invalid value for the parameter {name}.")
                )
            );
        }
    }

    #[test]
    fn a_name_nobody_has_or_nobody_may_set_is_unknown() {
        for name in [
            "NoSuchAttribute",
            "QueueArn",
            "CreatedTimestamp",
            "ApproximateNumberOfMessages",
        ] {
            let e = parse([(name, "1")].into_iter()).unwrap_err();
            assert_eq!(
                (e.code.as_ref(), e.message),
                ("InvalidAttributeName", format!("Unknown Attribute {name}."))
            );
        }
    }
}
