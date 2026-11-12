//! Topics: create (idempotent as AWS is), delete, list, and their attributes.

use std::collections::BTreeMap;

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::arn;
use crate::core::Core;
use crate::model::Topic;
use crate::req::{Req, invalid, invalid_because, no_topic};
use crate::topic_attrs;

/// What a page of a list holds (AWS: 100).
pub(crate) const PAGE: usize = 100;
/// AWS: 100,000 standard topics and 1,000 FIFO topics to an account.
const MAX_STANDARD: usize = 100_000;
const MAX_FIFO: usize = 1_000;
/// The longest a data protection policy may be.
const MAX_DATA_POLICY: usize = 30_720;

/// Whether `name` is a topic name: letters, digits, `_` and `-`, 1 to 256 characters; a FIFO topic's ends in `.fifo` (which counts).
fn valid_name(name: &str, fifo: bool) -> bool {
    let base = if fifo {
        match name.strip_suffix(".fifo") {
            Some(b) => b,
            None => return false,
        }
    } else {
        name
    };
    !base.is_empty()
        && name.len() <= 256
        && base
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// A page of `items` from the offset a `NextToken` holds, and the token for the next page.
pub(crate) fn page<T: Clone>(
    items: &[T],
    token: Option<&str>,
) -> Result<(Vec<T>, Option<String>), ApiError> {
    let start = match token {
        None => 0,
        Some(t) => t.parse::<usize>().map_err(|_| invalid("NextToken"))?,
    };
    let shown: Vec<T> = items.iter().skip(start).take(PAGE).cloned().collect();
    let next = (items.len() > start + PAGE).then(|| (start + PAGE).to_string());
    Ok((shown, next))
}

impl Core {
    /// The topic a topic ARN names.
    pub(crate) fn find_topic(
        &self,
        text: &str,
        param: &str,
        r: &Req<'_>,
    ) -> Result<&Topic, ApiError> {
        let name = arn::topic_name(text, param, r.region(), r.account())?.ok_or_else(no_topic)?;
        self.topics.get(name).ok_or_else(no_topic)
    }

    /// The name of the topic a topic ARN names, once it is known to exist.
    pub(crate) fn topic_key(
        &self,
        text: &str,
        param: &str,
        r: &Req<'_>,
    ) -> Result<String, ApiError> {
        self.find_topic(text, param, r).map(|t| t.name.clone())
    }

    /// `CreateTopic`.
    pub(crate) fn create_topic(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("Name");
        let mut attrs = r.map("Attributes");
        let fifo = attrs.get("FifoTopic").map(String::as_str) == Some("true");
        // Recorded 2026-11-09: a `.fifo` name without the attribute, and the attribute without a `.fifo` name, are both this.
        if !valid_name(name, fifo) || (!fifo && name.ends_with(".fifo")) {
            return Err(invalid("Topic Name"));
        }
        for (k, v) in &attrs {
            topic_attrs::check(k, v, fifo)?;
        }
        attrs.remove("FifoTopic");
        let tags = r.tags();
        crate::tags::check_new(&tags)?;
        let policy = r.opt("DataProtectionPolicy");
        if policy.is_some_and(|p| p.len() > MAX_DATA_POLICY) {
            return Err(invalid("DataProtectionPolicy"));
        }
        if let Some(existing) = self.topics.get(name) {
            same_definition(existing, &attrs, fifo, &tags)?;
            return Ok(json!({"TopicArn": existing.arn}));
        }
        self.check_topic_limit(fifo)?;
        let arn = arn::topic(r.region(), r.account(), name);
        let topic = Topic {
            name: name.to_owned(),
            arn: arn.clone(),
            account: r.account().to_owned(),
            fifo,
            created_ms: self.now_ms(),
            attrs,
            tags: tags.into_iter().collect(),
            data_protection: policy.map(str::to_owned),
            deleted_subscriptions: 0,
            sequence: 0,
            dedup: Vec::new(),
        };
        self.topics.insert(name.to_owned(), topic);
        Ok(json!({"TopicArn": arn}))
    }

    fn check_topic_limit(&self, fifo: bool) -> Result<(), ApiError> {
        let held = self.topics.iter().filter(|(_, t)| t.fifo == fifo).count();
        if held >= if fifo { MAX_FIFO } else { MAX_STANDARD } {
            // (memory)
            return Err(ApiError::modeled(
                "TopicLimitExceededException",
                "Topic limit exceeded",
            ));
        }
        Ok(())
    }

    /// `DeleteTopic`: a topic that is not there is not an error (recorded 2026-11-09), and its subscriptions go with it.
    pub(crate) fn delete_topic(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = match arn::topic_name(r.s("TopicArn"), "TopicArn", r.region(), r.account())? {
            Some(n) if self.topics.get(n).is_some() => n.to_owned(),
            _ => return Ok(Value::Null),
        };
        let gone: Vec<String> = self
            .subs
            .iter()
            .filter(|(_, s)| s.topic == name)
            .map(|(k, _)| k.to_owned())
            .collect();
        for key in gone {
            self.subs.remove(&key);
        }
        self.topics.remove(&name);
        Ok(Value::Null)
    }

    /// `ListTopics`.
    pub(crate) fn list_topics(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let all: Vec<Value> = self
            .topics
            .iter()
            .map(|(_, t)| json!({"TopicArn": t.arn}))
            .collect();
        let (topics, next) = page(&all, r.opt("NextToken"))?;
        Ok(json!({"Topics": topics, "NextToken": next}))
    }

    /// `GetTopicAttributes`.
    pub(crate) fn get_topic_attributes(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let topic = self.find_topic(r.s("TopicArn"), "TopicArn", r)?;
        let (confirmed, pending) = self.subscription_counts(&topic.name);
        Ok(json!({"Attributes": topic_attrs::describe(topic, confirmed, pending)}))
    }

    /// `SetTopicAttributes`.
    pub(crate) fn set_topic_attributes(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let key = self.topic_key(r.s("TopicArn"), "TopicArn", r)?;
        let (name, value) = (
            r.s("AttributeName"),
            r.opt("AttributeValue").unwrap_or_default(),
        );
        let fifo = self.topics.get(&key).is_some_and(|t| t.fifo);
        // The kind of a topic is fixed when it is made.
        if name == "FifoTopic" {
            return Err(invalid("AttributeName"));
        }
        topic_attrs::check(name, value, fifo)?;
        if name == "MaximumMessageSize" {
            self.check_large(&key, value)?;
        }
        if let Some(topic) = self.topics.get_mut(&key) {
            if value.is_empty() {
                topic.attrs.remove(name);
            } else {
                topic.attrs.insert(name.to_owned(), value.to_owned());
            }
        }
        Ok(Value::Null)
    }

    /// A topic that takes more than 256 KiB has at most 100 subscriptions, each SQS, Firehose or Lambda.
    fn check_large(&self, topic: &str, value: &str) -> Result<(), ApiError> {
        if value.parse::<u64>().unwrap_or(0) <= topic_attrs::DEFAULT_MAX_SIZE {
            return Ok(());
        }
        let subs: Vec<_> = self.subs.iter().filter(|(_, s)| s.topic == topic).collect();
        let only_big_ones = subs
            .iter()
            .all(|(_, s)| matches!(s.protocol.as_str(), "sqs" | "firehose" | "lambda"));
        if subs.len() > 100 || !only_big_ones {
            // (memory)
            return Err(invalid_because(
                "Attributes",
                "A topic with a MaximumMessageSize above 256 KiB must have 100 or fewer subscriptions, each an SQS, Firehose or Lambda subscription",
            ));
        }
        Ok(())
    }

    /// `PutDataProtectionPolicy`. The service is closed to new customers in AWS; the policy is kept and returned, nothing is masked.
    pub(crate) fn put_data_protection_policy(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let key = self.topic_key(r.s("ResourceArn"), "ResourceArn", r)?;
        let text = r.s("DataProtectionPolicy");
        if text.len() > MAX_DATA_POLICY
            || !matches!(serde_json::from_str::<Value>(text), Ok(Value::Object(_)))
        {
            // (memory)
            return Err(invalid("DataProtectionPolicy"));
        }
        if let Some(t) = self.topics.get_mut(&key) {
            t.data_protection = Some(text.to_owned());
        }
        Ok(Value::Null)
    }

    /// `GetDataProtectionPolicy`: absent when none was put.
    pub(crate) fn get_data_protection_policy(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let t = self.find_topic(r.s("ResourceArn"), "ResourceArn", r)?;
        Ok(json!({"DataProtectionPolicy": t.data_protection}))
    }

    /// How many subscriptions of a topic are confirmed, and how many wait for confirmation.
    pub(crate) fn subscription_counts(&self, topic: &str) -> (usize, usize) {
        let mine = self.subs.iter().filter(|(_, s)| s.topic == topic);
        let (mut confirmed, mut pending) = (0, 0);
        for (_, s) in mine {
            if s.confirmed {
                confirmed += 1;
            } else {
                pending += 1;
            }
        }
        (confirmed, pending)
    }
}

/// A topic of this name exists: asking to create it again is fine if the definition is the same (recorded 2026-11-09).
fn same_definition(
    existing: &Topic,
    attrs: &BTreeMap<String, String>,
    fifo: bool,
    tags: &[(String, String)],
) -> Result<(), ApiError> {
    let in_force = topic_attrs::describe(existing, 0, 0);
    let differs = fifo != existing.fifo
        || attrs.iter().any(|(k, v)| match in_force.get(k) {
            Some(have) => !same_value(k, have, v),
            None => true,
        });
    if differs {
        return Err(invalid_because(
            "Attributes",
            "Topic already exists with different attributes",
        ));
    }
    let given: BTreeMap<_, _> = tags.iter().cloned().collect();
    if !given.is_empty() && given != existing.tags {
        return Err(invalid_because(
            "Tags",
            "Topic already exists with different tags",
        ));
    }
    Ok(())
}

/// Whether two values of an attribute are the same: policies are compared as documents.
fn same_value(name: &str, a: &str, b: &str) -> bool {
    if matches!(name, "Policy" | "DeliveryPolicy" | "ArchivePolicy") {
        let (x, y) = (
            serde_json::from_str::<Value>(a),
            serde_json::from_str::<Value>(b),
        );
        if let (Ok(x), Ok(y)) = (x, y) {
            return x == y;
        }
    }
    a == b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_the_documented_rule() {
        assert!(valid_name("orders_1-a", false));
        assert!(!valid_name("not a topic name!", false));
        assert!(!valid_name("", false));
        assert!(valid_name("a.fifo", true));
        assert!(!valid_name("a", true));
        assert!(!valid_name(".fifo", true));
        assert!(!valid_name(&"a".repeat(257), false));
        assert!(valid_name(&"a".repeat(256), false));
    }

    #[test]
    fn a_page_is_a_hundred_and_the_token_is_an_offset() {
        let items: Vec<u32> = (0..250).collect();
        let (first, next) = page(&items, None).unwrap();
        assert_eq!((first.len(), next.as_deref()), (100, Some("100")));
        let (last, next) = page(&items, Some("200")).unwrap();
        assert_eq!((last.len(), next), (50, None));
        assert!(page(&items, Some("x")).is_err());
    }
}
