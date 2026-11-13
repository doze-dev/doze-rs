//! A subscription's attributes: which names exist and what values they take. The one recorded refusal is the filter policy that is not
//! JSON (2026-11-09); the wording of the rest is from memory.

use std::collections::BTreeMap;

use aws_wire::ApiError;
use serde_json::Value;

use crate::filter;
use crate::model::Subscription;
use crate::req::invalid_because;

/// The attributes a caller may give to `Subscribe` or `SetSubscriptionAttributes`.
const SETTABLE: &[&str] = &[
    "RawMessageDelivery",
    "FilterPolicy",
    "FilterPolicyScope",
    "DeliveryPolicy",
    "RedrivePolicy",
    "SubscriptionRoleArn",
    "ReplayPolicy",
];

pub(crate) fn is_settable(name: &str) -> bool {
    SETTABLE.contains(&name)
}

fn json_object(name: &str, value: &str) -> Result<(), ApiError> {
    match serde_json::from_str::<Value>(value) {
        Ok(Value::Object(_)) => Ok(()),
        _ => Err(invalid_because(
            "Attributes",
            format!("{name}: failed to parse JSON"),
        )),
    }
}

/// Checks the attributes a subscription would have: each on its own, and the filter policy against its scope. `fifo` is the topic's
/// kind, `protocol` the subscription's.
pub(crate) fn check_all(
    attrs: &BTreeMap<String, String>,
    protocol: &str,
    fifo: bool,
) -> Result<(), ApiError> {
    for (name, value) in attrs {
        check_one(name, value, fifo)?;
    }
    if let Some(policy) = attrs.get("FilterPolicy") {
        let body = attrs.get("FilterPolicyScope").map(String::as_str) == Some("MessageBody");
        filter::parse(policy, body)
            .map_err(|e| invalid_because("Attributes", format!("FilterPolicy: {e}")))?;
    }
    if protocol == "firehose" && !attrs.contains_key("SubscriptionRoleArn") {
        return Err(invalid_because(
            "Attributes",
            "Please specify the subscription role ARN for the Firehose subscription",
        ));
    }
    Ok(())
}

fn check_one(name: &str, value: &str, fifo: bool) -> Result<(), ApiError> {
    match name {
        "RawMessageDelivery" if !matches!(value, "true" | "false") => Err(invalid_because(
            "Attributes",
            format!("RawMessageDelivery: Invalid value [{value}]. Must be true or false."),
        )),
        "FilterPolicyScope" if !matches!(value, "MessageAttributes" | "MessageBody") => {
            Err(invalid_because(
                "Attributes",
                format!(
                    "FilterPolicyScope: Invalid value [{value}]. Please use either MessageBody or MessageAttributes"
                ),
            ))
        }
        "DeliveryPolicy" | "ReplayPolicy" => {
            if name == "ReplayPolicy" && !fifo {
                return Err(invalid_because(
                    "Attributes",
                    "ReplayPolicy is only valid for subscriptions to FIFO topics",
                ));
            }
            json_object(name, value)
        }
        "RedrivePolicy" => redrive(value),
        "FilterPolicy" | "RawMessageDelivery" | "FilterPolicyScope" | "SubscriptionRoleArn" => {
            Ok(())
        }
        other => Err(invalid_because(
            "Attributes",
            format!("Invalid attribute name: {other}"),
        )),
    }
}

/// A redrive policy names the queue that takes what cannot be delivered.
fn redrive(value: &str) -> Result<(), ApiError> {
    let ok = serde_json::from_str::<Value>(value).is_ok_and(|v| {
        v.get("deadLetterTargetArn")
            .and_then(Value::as_str)
            .is_some_and(|a| a.starts_with("arn:aws:sqs:"))
    });
    if ok {
        Ok(())
    } else {
        Err(invalid_because(
            "Attributes",
            "RedrivePolicy: deadLetterTargetArn must be the ARN of an SQS queue",
        ))
    }
}

/// `text` without the whitespace between its tokens, keys in the order given. AWS answers a filter policy compact whatever the caller
/// sent (recorded 2026-11-09: `{"kind": ["order"]}` came back as `{"kind":["order"]}`); the other documents are kept as given (unrecorded).
pub(crate) fn compact(text: &str) -> String {
    let (mut out, mut in_string, mut escaped) = (String::with_capacity(text.len()), false, false);
    for c in text.chars() {
        if in_string {
            out.push(c);
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => in_string = false,
                _ => {}
            }
        } else if !c.is_whitespace() {
            in_string = c == '"';
            out.push(c);
        }
    }
    out
}

/// Puts the attributes in the form they are kept in.
pub(crate) fn normalize(attrs: &mut BTreeMap<String, String>) {
    if let Some(policy) = attrs.get_mut("FilterPolicy") {
        *policy = compact(policy);
    }
}

/// What `GetSubscriptionAttributes` answers for `s` (recorded 2026-11-09).
pub(crate) fn describe(s: &Subscription) -> BTreeMap<String, String> {
    let mut out = s.attrs.clone();
    let text = |b: bool| if b { "true" } else { "false" }.to_owned();
    out.insert("SubscriptionArn".into(), s.arn.clone());
    out.insert("TopicArn".into(), s.topic_arn.clone());
    out.insert("Owner".into(), s.owner.clone());
    out.insert("Protocol".into(), s.protocol.clone());
    out.insert("Endpoint".into(), s.endpoint.clone());
    out.insert("PendingConfirmation".into(), text(!s.confirmed));
    out.insert("ConfirmationWasAuthenticated".into(), text(s.authenticated));
    out.insert("SubscriptionPrincipal".into(), s.principal.clone());
    out.entry("RawMessageDelivery".into())
        .or_insert_with(|| "false".into());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attrs(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn a_filter_policy_is_kept_compact_with_its_strings_untouched() {
        assert_eq!(
            compact(r#"{"kind": ["or der" , "a\" b"]}"#),
            r#"{"kind":["or der","a\" b"]}"#
        );
        assert_eq!(compact("{ \n}"), "{}");
    }

    #[test]
    fn values_and_names_are_checked() {
        let ok = |p: &[(&str, &str)], proto: &str, fifo: bool| check_all(&attrs(p), proto, fifo);
        assert!(ok(&[("RawMessageDelivery", "true")], "sqs", false).is_ok());
        assert!(ok(&[("RawMessageDelivery", "yes")], "sqs", false).is_err());
        assert!(ok(&[("FilterPolicyScope", "Body")], "sqs", false).is_err());
        assert!(ok(&[("Nonsense", "x")], "sqs", false).is_err());
        assert!(ok(&[("ReplayPolicy", "{}")], "sqs", false).is_err());
        assert!(ok(&[("ReplayPolicy", "{}")], "sqs", true).is_ok());
        assert!(
            ok(
                &[(
                    "RedrivePolicy",
                    r#"{"deadLetterTargetArn":"arn:aws:sqs:us-east-1:1:dlq"}"#
                )],
                "sqs",
                false
            )
            .is_ok()
        );
        assert!(ok(&[("RedrivePolicy", "{}")], "sqs", false).is_err());
        assert!(ok(&[], "firehose", false).is_err());
    }

    #[test]
    fn a_filter_policy_is_read_in_its_scope() {
        let nested = r#"{"a":{"b":["x"]}}"#;
        assert!(check_all(&attrs(&[("FilterPolicy", nested)]), "sqs", false).is_err());
        let body = attrs(&[
            ("FilterPolicy", nested),
            ("FilterPolicyScope", "MessageBody"),
        ]);
        assert!(check_all(&body, "sqs", false).is_ok());
        let e = check_all(&attrs(&[("FilterPolicy", "{not json")]), "sqs", false).unwrap_err();
        assert!(e.message.starts_with(
            "Invalid parameter: Attributes Reason: FilterPolicy: failed to parse JSON."
        ));
    }
}
