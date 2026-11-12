//! What SNS keeps: topics, subscriptions, mobile-push applications and endpoints, and the region's SMS settings.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// How long a deduplication id is remembered on a FIFO topic (AWS: five minutes).
pub(crate) const DEDUP_WINDOW_MS: i64 = 5 * 60 * 1000;
/// How long a confirmation token is good for (AWS: two days).
pub(crate) const TOKEN_LIFE_MS: i64 = 2 * 24 * 60 * 60 * 1000;

/// A message id remembered for deduplication on a FIFO topic.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Remembered {
    pub(crate) key: String,
    pub(crate) expires_ms: i64,
    pub(crate) message_id: String,
    pub(crate) sequence: String,
}

/// A topic.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Topic {
    pub(crate) name: String,
    pub(crate) arn: String,
    pub(crate) account: String,
    pub(crate) fifo: bool,
    pub(crate) created_ms: i64,
    /// The attributes a caller set, as they gave them. Defaults (the policy, the delivery policy) are not stored.
    pub(crate) attrs: BTreeMap<String, String>,
    pub(crate) tags: BTreeMap<String, String>,
    pub(crate) data_protection: Option<String>,
    /// Subscriptions removed (the count `GetTopicAttributes` reports).
    pub(crate) deleted_subscriptions: u64,
    /// The last sequence number given to a message on a FIFO topic.
    pub(crate) sequence: u64,
    pub(crate) dedup: Vec<Remembered>,
}

impl Topic {
    /// The topic without what a delivery does not need (its attributes, tags and remembered ids), cheap to hold across an await.
    pub(crate) fn light(&self) -> Self {
        Self {
            name: self.name.clone(),
            arn: self.arn.clone(),
            account: self.account.clone(),
            fifo: self.fifo,
            created_ms: self.created_ms,
            attrs: BTreeMap::new(),
            tags: BTreeMap::new(),
            data_protection: None,
            deleted_subscriptions: 0,
            sequence: 0,
            dedup: Vec::new(),
        }
    }
}

/// A subscription of an endpoint to a topic.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Subscription {
    pub(crate) arn: String,
    pub(crate) topic: String,
    pub(crate) topic_arn: String,
    pub(crate) owner: String,
    pub(crate) protocol: String,
    pub(crate) endpoint: String,
    /// The attributes a caller set: `RawMessageDelivery`, `FilterPolicy`, `FilterPolicyScope`, `RedrivePolicy`, ...
    pub(crate) attrs: BTreeMap<String, String>,
    pub(crate) confirmed: bool,
    pub(crate) token: Option<String>,
    pub(crate) token_expires_ms: i64,
    pub(crate) authenticated: bool,
    /// Whether an unsubscribe must be signed (`ConfirmSubscription` with `AuthenticateOnUnsubscribe`).
    pub(crate) authenticate_on_unsubscribe: bool,
    pub(crate) principal: String,
    pub(crate) created_ms: i64,
}

impl Subscription {
    pub(crate) fn raw(&self) -> bool {
        self.attrs.get("RawMessageDelivery").map(String::as_str) == Some("true")
    }

    pub(crate) fn body_scope(&self) -> bool {
        self.attrs.get("FilterPolicyScope").map(String::as_str) == Some("MessageBody")
    }
}

/// A mobile-push application.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Application {
    pub(crate) arn: String,
    pub(crate) name: String,
    pub(crate) platform: String,
    pub(crate) attrs: BTreeMap<String, String>,
}

/// A device registered with an application.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Endpoint {
    pub(crate) arn: String,
    pub(crate) application: String,
    pub(crate) token: String,
    pub(crate) attrs: BTreeMap<String, String>,
}

/// The region's SMS settings.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Sms {
    pub(crate) attrs: BTreeMap<String, String>,
}
