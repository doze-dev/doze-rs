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
    pub(crate) fn differs_from(&self, q: &Queue) -> Option<&'static str> {
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
        numeric
            .into_iter()
            .find(|(_, asked, have)| asked.is_some_and(|n| n != *have))
            .map(|(name, ..)| name)
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
    let inflight = q.messages.iter().filter(|m| m.in_flight(now_ms)).count();
    let delayed = q.messages.iter().filter(|m| m.delayed(now_ms)).count();
    put(
        "ApproximateNumberOfMessages",
        (q.messages.len() - inflight - delayed).to_string(),
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
    for (name, value) in &q.stored {
        if name != "SqsManagedSseEnabled" {
            put(name, value.clone());
        }
    }
    out
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
