//! A message as published, and what is checked of it before anything is delivered. The wording of the refusals is from memory except
//! for the two recorded on 2026-11-09: an empty message, and a FIFO topic without a group.

use std::collections::BTreeMap;

use aws_wire::ApiError;
use serde_json::Value;

use crate::filter_match::Attribute;
use crate::model::Topic;
use crate::req::{invalid, invalid_because};

/// The most message attributes one message carries.
const MAX_ATTRIBUTES: usize = 10;

/// One attribute of a message.
#[derive(Clone, Debug)]
pub(crate) struct Attr {
    pub(crate) data_type: String,
    pub(crate) string_value: Option<String>,
    /// A binary value, base64 as it came.
    pub(crate) binary_value: Option<String>,
}

/// What a caller asked to publish.
#[derive(Clone, Debug, Default)]
pub(crate) struct Message {
    pub(crate) body: String,
    pub(crate) subject: Option<String>,
    pub(crate) json_structure: bool,
    pub(crate) attrs: BTreeMap<String, Attr>,
    pub(crate) dedup_id: Option<String>,
    pub(crate) group_id: Option<String>,
}

impl Message {
    /// Reads a `Publish` request or a `PublishBatch` entry.
    pub(crate) fn from_json(v: &Value) -> Self {
        let text = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
        let attrs = v
            .get("MessageAttributes")
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .map(|(name, a)| {
                        let field = |k: &str| a.get(k).and_then(Value::as_str).map(str::to_owned);
                        (
                            name.clone(),
                            Attr {
                                data_type: field("DataType").unwrap_or_default(),
                                string_value: field("StringValue"),
                                binary_value: field("BinaryValue"),
                            },
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            body: text("Message").unwrap_or_default(),
            subject: text("Subject"),
            json_structure: v.get("MessageStructure").and_then(Value::as_str) == Some("json"),
            attrs,
            dedup_id: text("MessageDeduplicationId"),
            group_id: text("MessageGroupId"),
        }
    }

    /// The bytes that count against the topic's size limit: the body, and the name, type and value of each attribute.
    pub(crate) fn size(&self) -> usize {
        let attrs: usize = self
            .attrs
            .iter()
            .map(|(name, a)| {
                name.len()
                    + a.data_type.len()
                    + a.string_value.as_ref().map_or(0, String::len)
                    + a.binary_value.as_ref().map_or(0, |b| b.len() * 3 / 4)
            })
            .sum();
        self.body.len() + attrs
    }

    /// The attributes as the filter sees them.
    pub(crate) fn filterable(&self) -> BTreeMap<String, Attribute> {
        self.attrs
            .iter()
            .map(|(k, a)| {
                (
                    k.clone(),
                    Attribute {
                        data_type: a.data_type.clone(),
                        string_value: a.string_value.clone(),
                    },
                )
            })
            .collect()
    }

    /// The text to deliver to an endpoint of `protocol`: the whole message, or for `MessageStructure=json` the member for the protocol,
    /// else `default`.
    pub(crate) fn text_for(&self, protocol: &str) -> String {
        if !self.json_structure {
            return self.body.clone();
        }
        let parsed: Value = serde_json::from_str(&self.body).unwrap_or(Value::Null);
        [protocol, "default"]
            .iter()
            .find_map(|k| parsed.get(*k).and_then(Value::as_str))
            .unwrap_or_default()
            .to_owned()
    }
}

/// What is wrong with the message, if anything, for a publish to `topic`.
pub(crate) fn check(topic: &Topic, m: &Message) -> Result<(), ApiError> {
    check_body(m)?;
    check_attrs(&m.attrs)?;
    if m.size() as u64 > topic.max_size() {
        return Err(invalid("Message too long"));
    }
    check_kind(topic, m)
}

/// The checks that hold wherever a message goes (a topic, a phone, a device).
pub(crate) fn check_body(m: &Message) -> Result<(), ApiError> {
    if m.body.is_empty() {
        return Err(invalid("Empty message"));
    }
    if let Some(s) = &m.subject
        && (s.chars().count() > 100 || s.chars().any(char::is_control) || s.is_empty())
    {
        return Err(invalid("Subject"));
    }
    if m.json_structure {
        let parsed: Value = serde_json::from_str(&m.body).unwrap_or(Value::Null);
        if !parsed.is_object() {
            return Err(invalid(
                "Message Structure - JSON message body failed to parse",
            ));
        }
        if !parsed.get("default").is_some_and(Value::is_string) {
            return Err(invalid(
                "Message Structure - No default entry in JSON message body",
            ));
        }
    }
    Ok(())
}

fn check_attrs(attrs: &BTreeMap<String, Attr>) -> Result<(), ApiError> {
    if attrs.len() > MAX_ATTRIBUTES {
        return Err(invalid_because(
            "MessageAttributes",
            format!(
                "Number of message attributes [{}] exceeds the allowed maximum [{MAX_ATTRIBUTES}] for Publish API",
                attrs.len()
            ),
        ));
    }
    attrs.iter().try_for_each(|(name, a)| check_attr(name, a))
}

fn check_attr(name: &str, a: &Attr) -> Result<(), ApiError> {
    let kind = a.data_type.split('.').next().unwrap_or_default();
    if !matches!(kind, "String" | "Number" | "Binary") {
        return Err(invalid_because(
            "MessageAttributes",
            format!(
                "The message attribute '{name}' has an invalid message attribute type, the set of supported type prefixes is Binary, Number, and String."
            ),
        ));
    }
    let present = if kind == "Binary" {
        a.binary_value.as_ref().is_some_and(|v| !v.is_empty())
    } else {
        a.string_value.as_ref().is_some_and(|v| !v.is_empty())
    };
    if !present {
        return Err(invalid_because(
            "MessageAttributes",
            format!(
                "The message attribute '{name}' must contain non-empty message attribute value for message attribute type '{}'.",
                a.data_type
            ),
        ));
    }
    if kind == "Number"
        && a.string_value
            .as_deref()
            .is_some_and(|v| v.trim().parse::<f64>().is_err())
    {
        return Err(invalid_because(
            "MessageAttributes",
            format!("Could not cast message attribute '{name}' value to number."),
        ));
    }
    Ok(())
}

/// What the kind of topic requires of a publish.
fn check_kind(topic: &Topic, m: &Message) -> Result<(), ApiError> {
    if let Some(g) = &m.group_id
        && (g.is_empty() || g.len() > 128)
    {
        return Err(invalid("MessageGroupId"));
    }
    if let Some(d) = &m.dedup_id
        && (d.is_empty() || d.len() > 128)
    {
        return Err(invalid("MessageDeduplicationId"));
    }
    if !topic.fifo {
        return match &m.dedup_id {
            // (memory)
            Some(_) => Err(invalid(
                "The request includes MessageDeduplicationId parameter that is not valid for this topic type",
            )),
            None => Ok(()),
        };
    }
    // Recorded 2026-11-09.
    if m.group_id.is_none() {
        return Err(invalid(
            "The MessageGroupId parameter is required for FIFO topics",
        ));
    }
    if m.dedup_id.is_none() && !topic.content_based() {
        // (memory)
        return Err(invalid(
            "The topic should either have ContentBasedDeduplication enabled or MessageDeduplicationId provided explicitly",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn topic(fifo: bool) -> Topic {
        Topic {
            name: "t".into(),
            arn: "arn:aws:sns:us-east-1:1:t".into(),
            account: "1".into(),
            fifo,
            created_ms: 0,
            attrs: BTreeMap::new(),
            tags: BTreeMap::new(),
            data_protection: None,
            deleted_subscriptions: 0,
            sequence: 0,
            dedup: Vec::new(),
        }
    }

    fn message(v: &Value) -> Message {
        Message::from_json(v)
    }

    #[test]
    fn the_recorded_refusals_are_worded_as_recorded() {
        let t = topic(false);
        assert_eq!(
            check(&t, &message(&json!({"Message": ""})))
                .unwrap_err()
                .message,
            "Invalid parameter: Empty message"
        );
        assert_eq!(
            check(&topic(true), &message(&json!({"Message": "x"})))
                .unwrap_err()
                .message,
            "Invalid parameter: The MessageGroupId parameter is required for FIFO topics"
        );
    }

    #[test]
    fn attributes_and_structure_are_checked() {
        let t = topic(false);
        let attr = |a: Value| {
            check(
                &t,
                &message(&json!({"Message": "x", "MessageAttributes": {"a": a}})),
            )
        };
        assert!(attr(json!({"DataType": "String", "StringValue": "v"})).is_ok());
        assert!(attr(json!({"DataType": "String.Array", "StringValue": "[1]"})).is_ok());
        assert!(attr(json!({"DataType": "Number", "StringValue": "1.5"})).is_ok());
        assert!(attr(json!({"DataType": "Number", "StringValue": "x"})).is_err());
        assert!(attr(json!({"DataType": "Binary", "BinaryValue": "AAEC"})).is_ok());
        assert!(attr(json!({"DataType": "Binary", "StringValue": "x"})).is_err());
        assert!(attr(json!({"DataType": "Date", "StringValue": "x"})).is_err());
        assert!(attr(json!({"DataType": "String"})).is_err());
        let json_msg = |body: &str| {
            check(
                &t,
                &message(&json!({"Message": body, "MessageStructure": "json"})),
            )
        };
        assert!(json_msg(r#"{"default":"x","sqs":"y"}"#).is_ok());
        assert!(json_msg(r#"{"sqs":"y"}"#).is_err());
        assert!(json_msg("plain").is_err());
    }

    #[test]
    fn a_fifo_topic_needs_a_deduplication_id_unless_it_is_content_based() {
        let mut t = topic(true);
        let m = message(&json!({"Message": "x", "MessageGroupId": "g"}));
        assert!(check(&t, &m).is_err());
        t.attrs
            .insert("ContentBasedDeduplication".into(), "true".into());
        assert!(check(&t, &m).is_ok());
        let std_with_id = message(&json!({"Message": "x", "MessageDeduplicationId": "d"}));
        assert!(check(&topic(false), &std_with_id).is_err());
    }

    #[test]
    fn size_counts_the_attributes_and_the_topic_sets_the_limit() {
        let mut t = topic(false);
        t.attrs.insert("MaximumMessageSize".into(), "1024".into());
        let big = message(
            &json!({"Message": "x".repeat(1020), "MessageAttributes": {"a": {"DataType": "String", "StringValue": "vvvv"}}}),
        );
        assert_eq!(
            check(&t, &big).unwrap_err().message,
            "Invalid parameter: Message too long"
        );
        assert!(check(&topic(false), &big).is_ok());
    }

    #[test]
    fn a_json_structure_picks_the_member_for_the_protocol_or_default() {
        let m = message(
            &json!({"Message": r#"{"default":"d","sqs":"s"}"#, "MessageStructure": "json"}),
        );
        assert_eq!(m.text_for("sqs"), "s");
        assert_eq!(m.text_for("email"), "d");
        assert_eq!(
            message(&json!({"Message": "plain"})).text_for("sqs"),
            "plain"
        );
    }
}
