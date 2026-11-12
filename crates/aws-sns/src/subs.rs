//! Subscriptions: subscribe (and who must confirm), confirm, unsubscribe, attributes, lists.

use std::collections::BTreeMap;

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::arn;
use crate::core::Core;
use crate::model::{Subscription, TOKEN_LIFE_MS, Topic};
use crate::req::{Req, invalid, invalid_because, no_subscription};
use crate::sub_attrs;
use crate::topics::page;

/// What AWS answers to `Subscribe` for a subscription that is not confirmed (unless the caller asks for its ARN).
const PENDING: &str = "pending confirmation";
/// AWS: 200 subscriptions with a filter policy to a topic, 10,000 to an account.
const MAX_FILTERED_PER_TOPIC: usize = 200;
const MAX_FILTERED_PER_ACCOUNT: usize = 10_000;

/// The protocols SNS has.
const PROTOCOLS: &[&str] = &[
    "http",
    "https",
    "email",
    "email-json",
    "sms",
    "sqs",
    "application",
    "lambda",
    "firehose",
];

impl Core {
    /// `Subscribe`.
    pub(crate) fn subscribe(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let topic = self.find_topic(r.s("TopicArn"), "TopicArn", r)?.clone();
        let protocol = r.s("Protocol");
        // Recorded 2026-11-09.
        if !PROTOCOLS.contains(&protocol) {
            return Err(invalid(format!(
                "Amazon SNS does not support this protocol string: {protocol}"
            )));
        }
        let endpoint = r.opt("Endpoint").unwrap_or_default();
        self.check_endpoint(protocol, endpoint, &topic)?;
        let mut attrs = r.map("Attributes");
        sub_attrs::check_all(&attrs, protocol, topic.fifo)?;
        sub_attrs::normalize(&mut attrs);
        let want_arn = r.flag("ReturnSubscriptionArn").unwrap_or(false);
        let shown = |s: &Subscription| {
            if s.confirmed || want_arn {
                s.arn.clone()
            } else {
                PENDING.to_owned()
            }
        };
        if let Some(same) = self
            .subs
            .iter()
            .map(|(_, s)| s)
            .find(|s| s.topic == topic.name && s.protocol == protocol && s.endpoint == endpoint)
        {
            return same_subscription(same, &attrs)
                .map(|()| json!({"SubscriptionArn": shown(same)}));
        }
        self.check_filter_limits(&topic, &attrs)?;
        if attrs.contains_key("FilterPolicy") {
            attrs
                .entry("FilterPolicyScope".into())
                .or_insert_with(|| "MessageAttributes".into());
        }
        let sub = self.new_subscription(&topic, protocol, endpoint, attrs, r);
        let answer = shown(&sub);
        self.subs.insert(sub.arn.clone(), sub);
        Ok(json!({"SubscriptionArn": answer}))
    }

    /// The subscription as it is made: confirmed at once for a protocol whose endpoint is the owner's own, else waiting for a token.
    fn new_subscription(
        &self,
        topic: &Topic,
        protocol: &str,
        endpoint: &str,
        attrs: BTreeMap<String, String>,
        r: &Req<'_>,
    ) -> Subscription {
        let owned_by_account = arn::split(endpoint, "Endpoint")
            .map(|a| a.account == topic.account)
            .unwrap_or(true);
        let confirmed = match protocol {
            "http" | "https" | "email" | "email-json" => false,
            "sqs" | "lambda" | "firehose" => owned_by_account,
            _ => true,
        };
        let id = aws_seal::uuid_v4().unwrap_or_default();
        let now = self.now_ms();
        Subscription {
            arn: arn::subscription(&topic.arn, &id),
            topic: topic.name.clone(),
            topic_arn: topic.arn.clone(),
            owner: r.account().to_owned(),
            protocol: protocol.to_owned(),
            endpoint: endpoint.to_owned(),
            attrs,
            confirmed,
            token: (!confirmed).then(token),
            token_expires_ms: now + TOKEN_LIFE_MS,
            authenticated: confirmed,
            authenticate_on_unsubscribe: false,
            // The caller's own ARN is not known here: every caller whose key IAM did not issue is the account root.
            principal: format!("arn:aws:iam::{}:root", r.account()),
            created_ms: now,
        }
    }

    /// What each protocol wants of its endpoint. The wording is from memory, except the FIFO one (recorded 2026-11-09).
    fn check_endpoint(
        &self,
        protocol: &str,
        endpoint: &str,
        topic: &Topic,
    ) -> Result<(), ApiError> {
        let ok = match protocol {
            "http" => endpoint.starts_with("http://") && endpoint.len() > 7,
            "https" => endpoint.starts_with("https://") && endpoint.len() > 8,
            "email" | "email-json" => endpoint.contains('@'),
            "sms" => crate::sms::is_phone_number(endpoint),
            "lambda" => endpoint.starts_with("arn:aws:lambda:"),
            "firehose" => endpoint.starts_with("arn:aws:firehose:"),
            "application" => self.endpoints.get(endpoint).is_some(),
            _ => return check_queue(endpoint, topic),
        };
        if ok { Ok(()) } else { Err(invalid("Endpoint")) }
    }

    fn check_filter_limits(
        &self,
        topic: &Topic,
        attrs: &BTreeMap<String, String>,
    ) -> Result<(), ApiError> {
        if !attrs.contains_key("FilterPolicy") {
            return Ok(());
        }
        let filtered = |s: &&Subscription| s.attrs.contains_key("FilterPolicy");
        let in_topic = self
            .subs
            .iter()
            .map(|(_, s)| s)
            .filter(filtered)
            .filter(|s| s.topic == topic.name)
            .count();
        let in_account = self.subs.iter().map(|(_, s)| s).filter(filtered).count();
        if in_topic >= MAX_FILTERED_PER_TOPIC || in_account >= MAX_FILTERED_PER_ACCOUNT {
            // (memory)
            return Err(ApiError::modeled(
                "FilterPolicyLimitExceededException",
                "Filter policy limit exceeded",
            ));
        }
        Ok(())
    }

    /// `ConfirmSubscription`: the token must be one issued for a subscription of this topic and still good.
    pub(crate) fn confirm_subscription(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let topic = self
            .find_topic(r.s("TopicArn"), "TopicArn", r)?
            .name
            .clone();
        let (given, now) = (r.s("Token"), self.now_ms());
        let found = self
            .subs
            .iter()
            .map(|(_, s)| s)
            .find(|s| {
                s.topic == topic
                    && !s.confirmed
                    && s.token.as_deref() == Some(given)
                    && s.token_expires_ms > now
            })
            .map(|s| s.arn.clone());
        // (memory)
        let Some(key) = found else {
            return Err(invalid("Token"));
        };
        let strict = r.opt("AuthenticateOnUnsubscribe") == Some("true");
        if let Some(s) = self.subs.get_mut(&key) {
            s.confirmed = true;
            s.token = None;
            s.authenticated = true;
            s.authenticate_on_unsubscribe = strict;
        }
        Ok(json!({"SubscriptionArn": key}))
    }

    /// The key of the subscription an ARN names.
    fn find_subscription(&self, text: &str, r: &Req<'_>) -> Result<String, ApiError> {
        let parts = arn::split(text, "SubscriptionArn")?;
        if parts.service != "sns" || !parts.resource.contains(':') {
            // (memory)
            return Err(invalid("SubscriptionArn"));
        }
        if parts.region != r.region() || parts.account != r.account() {
            return Err(no_subscription());
        }
        self.subs
            .get(text)
            .map(|s| s.arn.clone())
            .ok_or_else(no_subscription)
    }

    /// `Unsubscribe`.
    pub(crate) fn unsubscribe(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let key = self.find_subscription(r.s("SubscriptionArn"), r)?;
        if let Some(sub) = self.subs.remove(&key)
            && let Some(topic) = self.topics.get_mut(&sub.topic)
        {
            topic.deleted_subscriptions += 1;
        }
        Ok(Value::Null)
    }

    /// `GetSubscriptionAttributes`.
    pub(crate) fn get_subscription_attributes(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let key = self.find_subscription(r.s("SubscriptionArn"), r)?;
        let sub = self.subs.get(&key).ok_or_else(no_subscription)?;
        Ok(json!({"Attributes": sub_attrs::describe(sub)}))
    }

    /// `SetSubscriptionAttributes`: the whole set is checked as it will be, so a scope change re-reads the filter policy.
    pub(crate) fn set_subscription_attributes(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let key = self.find_subscription(r.s("SubscriptionArn"), r)?;
        let (name, value) = (
            r.s("AttributeName"),
            r.opt("AttributeValue").unwrap_or_default(),
        );
        if !sub_attrs::is_settable(name) {
            return Err(invalid("AttributeName"));
        }
        let Some(sub) = self.subs.get(&key) else {
            return Err(no_subscription());
        };
        let fifo = self.topics.get(&sub.topic).is_some_and(|t| t.fifo);
        let mut next = sub.attrs.clone();
        if value.is_empty() {
            next.remove(name);
        } else {
            next.insert(name.to_owned(), value.to_owned());
        }
        if name == "FilterPolicy" && !value.is_empty() {
            next.entry("FilterPolicyScope".into())
                .or_insert_with(|| "MessageAttributes".into());
        }
        sub_attrs::check_all(&next, &sub.protocol, fifo)?;
        sub_attrs::normalize(&mut next);
        if let Some(s) = self.subs.get_mut(&key) {
            s.attrs = next;
        }
        Ok(Value::Null)
    }

    /// `ListSubscriptions` and `ListSubscriptionsByTopic`.
    pub(crate) fn list_subscriptions(
        &self,
        r: &Req<'_>,
        by_topic: bool,
    ) -> Result<Value, ApiError> {
        let topic = if by_topic {
            Some(
                self.find_topic(r.s("TopicArn"), "TopicArn", r)?
                    .name
                    .clone(),
            )
        } else {
            None
        };
        let all: Vec<Value> = self
            .subs
            .iter()
            .map(|(_, s)| s)
            .filter(|s| topic.as_ref().is_none_or(|t| &s.topic == t))
            .map(|s| {
                json!({
                    "SubscriptionArn": if s.confirmed { s.arn.as_str() } else { "PendingConfirmation" },
                    "Owner": s.owner,
                    "Protocol": s.protocol,
                    "Endpoint": s.endpoint,
                    "TopicArn": s.topic_arn,
                })
            })
            .collect();
        let (shown, next) = page(&all, r.opt("NextToken"))?;
        Ok(json!({"Subscriptions": shown, "NextToken": next}))
    }
}

/// An SQS endpoint is a queue's ARN, and a FIFO queue goes with a FIFO topic only (the first message recorded 2026-11-09, the second from
/// memory).
fn check_queue(endpoint: &str, topic: &Topic) -> Result<(), ApiError> {
    let arn = arn::split(endpoint, "Endpoint")
        .ok()
        .filter(|a| a.service == "sqs" && !a.resource.is_empty() && !a.resource.contains(':'))
        .ok_or_else(|| invalid("SQS endpoint ARN"))?;
    let fifo_queue = arn.resource.ends_with(".fifo");
    if fifo_queue && !topic.fifo {
        return Err(invalid(
            "Invalid parameter: Endpoint Reason: FIFO SQS Queues can not be subscribed to standard SNS topics",
        ));
    }
    if !fifo_queue && topic.fifo {
        return Err(invalid(
            "Invalid parameter: Endpoint Reason: Please use FIFO SQS queue",
        ));
    }
    Ok(())
}

/// Subscribing again to the same endpoint answers with the same subscription, if the attributes agree (memory).
fn same_subscription(
    existing: &Subscription,
    attrs: &BTreeMap<String, String>,
) -> Result<(), ApiError> {
    let described = sub_attrs::describe(existing);
    let agrees = attrs.iter().all(|(k, v)| described.get(k) == Some(v));
    if agrees {
        Ok(())
    } else {
        Err(invalid_because(
            "Attributes",
            "Subscription already exists with different attributes",
        ))
    }
}

/// A confirmation token: 64 hex characters.
fn token() -> String {
    let a = aws_seal::uuid_v4().unwrap_or_default().replace('-', "");
    let b = aws_seal::uuid_v4().unwrap_or_default().replace('-', "");
    format!("{a}{b}")
}
