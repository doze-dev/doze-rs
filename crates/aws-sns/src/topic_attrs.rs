//! A topic's attributes: which names exist, what values they take, and the ones AWS fills in (the default policy, the effective
//! delivery policy). The one recorded refusal is for an unknown name (`Invalid parameter: AttributeName`, 2026-11-09); the wording of
//! the others is from memory.

use std::collections::BTreeMap;

use aws_iam_policy::Kind;
use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::model::Topic;
use crate::req::{invalid, invalid_because};

/// The most a topic may be told to carry: 1 MiB, and the default, 256 KiB.
pub(crate) const MAX_SIZE_CEILING: u64 = 1_048_576;
pub(crate) const DEFAULT_MAX_SIZE: u64 = 262_144;

/// The delivery-status feedback attributes: `<Protocol>SuccessFeedbackRoleArn` and the rest.
fn is_feedback(name: &str) -> Option<&'static str> {
    const PROTOCOLS: [&str; 5] = ["HTTP", "Firehose", "Lambda", "Application", "SQS"];
    PROTOCOLS.iter().find_map(|p| {
        let rest = name.strip_prefix(p)?;
        match rest {
            "SuccessFeedbackRoleArn" | "FailureFeedbackRoleArn" => Some("role"),
            "SuccessFeedbackSampleRate" => Some("rate"),
            _ => None,
        }
    })
}

/// Checks one attribute a caller sets. `fifo` is the kind of the topic.
pub(crate) fn check(name: &str, value: &str, fifo: bool) -> Result<(), ApiError> {
    match name {
        "DisplayName" if value.chars().count() > 100 => Err(invalid_because(
            "Attributes",
            "DisplayName must be less than 100 characters",
        )),
        "DeliveryPolicy" if !value.is_empty() => json_object(name, value),
        "Policy" => policy(value),
        "ContentBasedDeduplication" | "FifoTopic" => fifo_only(name, value, fifo),
        "FifoThroughputScope" | "ArchivePolicy" => only_fifo(name, fifo),
        "MaximumMessageSize" => max_size(value),
        "TracingConfig" if !matches!(value, "PassThrough" | "Active") => Err(invalid_because(
            "Attributes",
            "TracingConfig must be one of: PassThrough, Active",
        )),
        "SignatureVersion" if !matches!(value, "1" | "2") => Err(invalid_because(
            "Attributes",
            "SignatureVersion must be 1 or 2",
        )),
        "DisplayName" | "DeliveryPolicy" | "KmsMasterKeyId" | "SignatureVersion"
        | "TracingConfig" => Ok(()),
        other => match is_feedback(other) {
            Some("rate") => sample_rate(other, value),
            Some(_) => Ok(()),
            None => Err(invalid("AttributeName")),
        },
    }
}

fn only_fifo(name: &str, fifo: bool) -> Result<(), ApiError> {
    if fifo {
        return Ok(());
    }
    Err(invalid_because(
        "Attributes",
        format!("{name} is only valid for FIFO topics"),
    ))
}

fn fifo_only(name: &str, value: &str, fifo: bool) -> Result<(), ApiError> {
    if name == "FifoTopic" {
        return if matches!(value, "true" | "false") {
            Ok(())
        } else {
            Err(invalid_because(
                "Attributes",
                "FifoTopic must be true or false",
            ))
        };
    }
    only_fifo(name, fifo)?;
    if matches!(value, "true" | "false") {
        return Ok(());
    }
    Err(invalid_because(
        "Attributes",
        format!("{name} must be true or false"),
    ))
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

fn policy(value: &str) -> Result<(), ApiError> {
    aws_iam_policy::check(value, Kind::Resource)
        .map(|_| ())
        .map_err(|e| invalid(format!("Policy Error: {}", e.0)))
}

fn max_size(value: &str) -> Result<(), ApiError> {
    match value.parse::<u64>() {
        Ok(n) if (1024..=MAX_SIZE_CEILING).contains(&n) => Ok(()),
        _ => Err(invalid_because(
            "Attributes",
            "MaximumMessageSize must be between 1024 and 1048576",
        )),
    }
}

fn sample_rate(name: &str, value: &str) -> Result<(), ApiError> {
    match value.parse::<u32>() {
        Ok(n) if n <= 100 => Ok(()),
        _ => Err(invalid_because(
            "Attributes",
            format!("{name} must be between 0 and 100"),
        )),
    }
}

/// The policy a new topic has (recorded 2026-11-09): any principal of the owner's account may use the account-level actions.
pub(crate) fn default_policy(topic_arn: &str, account: &str) -> String {
    json!({
        "Version": "2008-10-17",
        "Id": "__default_policy_ID",
        "Statement": [{
            "Sid": "__default_statement_ID",
            "Effect": "Allow",
            "Principal": {"AWS": "*"},
            "Action": [
                "SNS:GetTopicAttributes", "SNS:SetTopicAttributes", "SNS:AddPermission",
                "SNS:RemovePermission", "SNS:DeleteTopic", "SNS:Subscribe",
                "SNS:ListSubscriptionsByTopic", "SNS:Publish"
            ],
            "Resource": topic_arn,
            "Condition": {"StringEquals": {"AWS:SourceOwner": account}}
        }]
    })
    .to_string()
}

/// What the topic delivers with when nothing says otherwise (recorded 2026-11-09).
fn default_delivery() -> Value {
    json!({"http": {
        "defaultHealthyRetryPolicy": {
            "minDelayTarget": 20, "maxDelayTarget": 20, "numRetries": 3, "numMaxDelayRetries": 0,
            "numNoDelayRetries": 0, "numMinDelayRetries": 0, "backoffFunction": "linear"
        },
        "disableSubscriptionOverrides": false,
        "defaultRequestPolicy": {"headerContentType": "text/plain; charset=UTF-8"}
    }})
}

/// `base` with the members of `over` laid on it, object by object.
fn merge(base: &mut Value, over: &Value) {
    match (base, over) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                merge(b.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
        (b, o) => *b = o.clone(),
    }
}

impl Topic {
    /// The policy in force: the one set, or the default.
    pub(crate) fn policy(&self) -> String {
        self.attrs
            .get("Policy")
            .cloned()
            .unwrap_or_else(|| default_policy(&self.arn, &self.account))
    }

    /// The largest message the topic takes.
    pub(crate) fn max_size(&self) -> u64 {
        self.attrs
            .get("MaximumMessageSize")
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_MAX_SIZE)
    }

    pub(crate) fn content_based(&self) -> bool {
        self.attrs
            .get("ContentBasedDeduplication")
            .is_some_and(|v| v == "true")
    }

    /// `EffectiveDeliveryPolicy`: the defaults with the topic's own `DeliveryPolicy` laid over them (the merge is from memory).
    pub(crate) fn effective_delivery(&self) -> String {
        let mut effective = default_delivery();
        if let Some(Ok(own)) = self
            .attrs
            .get("DeliveryPolicy")
            .map(|p| serde_json::from_str::<Value>(p))
        {
            merge(&mut effective, &own);
        }
        effective.to_string()
    }
}

/// The attributes `GetTopicAttributes` answers: those the caller set, and those AWS adds. `confirmed`, `pending` count the subscriptions.
pub(crate) fn describe(t: &Topic, confirmed: usize, pending: usize) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = t.attrs.clone();
    out.insert("TopicArn".into(), t.arn.clone());
    out.insert("Owner".into(), t.account.clone());
    out.insert("Policy".into(), t.policy());
    out.insert("EffectiveDeliveryPolicy".into(), t.effective_delivery());
    out.entry("DisplayName".into()).or_default();
    out.insert("SubscriptionsConfirmed".into(), confirmed.to_string());
    out.insert("SubscriptionsPending".into(), pending.to_string());
    out.insert(
        "SubscriptionsDeleted".into(),
        t.deleted_subscriptions.to_string(),
    );
    if t.fifo {
        // (memory) A FIFO topic reports its kind and the default of its one switch.
        out.insert("FifoTopic".into(), "true".into());
        out.entry("ContentBasedDeduplication".into())
            .or_insert_with(|| "false".into());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_names_that_exist_are_accepted_and_others_are_the_recorded_refusal() {
        assert!(check("DisplayName", "Orders", false).is_ok());
        assert!(check("SQSFailureFeedbackRoleArn", "arn:x", false).is_ok());
        assert!(check("HTTPSuccessFeedbackSampleRate", "101", false).is_err());
        assert_eq!(
            check("NoSuchAttribute", "x", false).unwrap_err().message,
            "Invalid parameter: AttributeName"
        );
        assert!(check("ContentBasedDeduplication", "true", false).is_err());
        assert!(check("ContentBasedDeduplication", "true", true).is_ok());
        assert!(check("MaximumMessageSize", "1023", false).is_err());
        assert!(check("MaximumMessageSize", "1048576", false).is_ok());
    }

    #[test]
    fn a_delivery_policy_is_laid_over_the_default() {
        let mut t = Topic {
            name: "t".into(),
            arn: "arn:aws:sns:us-east-1:1:t".into(),
            account: "1".into(),
            fifo: false,
            created_ms: 0,
            attrs: BTreeMap::new(),
            tags: BTreeMap::new(),
            data_protection: None,
            deleted_subscriptions: 0,
            sequence: 0,
            dedup: Vec::new(),
        };
        t.attrs.insert(
            "DeliveryPolicy".into(),
            r#"{"http":{"defaultHealthyRetryPolicy":{"numRetries":5}}}"#.into(),
        );
        let eff: Value = serde_json::from_str(&t.effective_delivery()).unwrap();
        assert_eq!(eff["http"]["defaultHealthyRetryPolicy"]["numRetries"], 5);
        assert_eq!(
            eff["http"]["defaultHealthyRetryPolicy"]["minDelayTarget"],
            20
        );
    }
}
