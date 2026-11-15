//! What EventBridge keeps. Every record is plain data that the store writes as postcard behind a version byte; descriptive blocks the
//! service only echoes (a bus's log configuration, a target's parameters) are kept as the JSON the caller sent, so nothing a client
//! put in is lost on the way back.

use std::collections::BTreeMap;

use aws_wire::service::Caller;
use serde::{Deserialize, Serialize};

/// An event bus. Keyed `<account>/<name>`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Bus {
    pub(crate) account: String,
    pub(crate) name: String,
    pub(crate) arn: String,
    pub(crate) description: Option<String>,
    pub(crate) kms_key: Option<String>,
    /// `DeadLetterConfig` as sent.
    pub(crate) dead_letter: Option<String>,
    /// `LogConfig` as sent.
    pub(crate) log_config: Option<String>,
    /// The bus's resource policy (a JSON document).
    pub(crate) policy: Option<String>,
    pub(crate) tags: BTreeMap<String, String>,
    pub(crate) created_ms: i64,
    pub(crate) modified_ms: i64,
}

/// A rule. Keyed `<account>/<bus>/<name>`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Rule {
    pub(crate) account: String,
    pub(crate) bus: String,
    pub(crate) name: String,
    pub(crate) arn: String,
    /// The pattern text exactly as given (AWS echoes it back unchanged, recorded 2026-11-09).
    pub(crate) pattern: Option<String>,
    pub(crate) schedule: Option<String>,
    pub(crate) enabled: bool,
    pub(crate) description: Option<String>,
    pub(crate) role_arn: Option<String>,
    pub(crate) managed_by: Option<String>,
    pub(crate) tags: BTreeMap<String, String>,
    pub(crate) targets: Vec<StoredTarget>,
    /// A rate counts from here: when the rule was made or its schedule last changed.
    pub(crate) anchor_ms: i64,
}

/// A target as `PutTargets` was given it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct StoredTarget {
    pub(crate) id: String,
    pub(crate) arn: String,
    /// The whole `Target` as JSON.
    pub(crate) json: String,
}

/// An archive of events. Keyed `<account>/<name>`; its events are rows of their own.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Archive {
    pub(crate) account: String,
    pub(crate) name: String,
    pub(crate) arn: String,
    pub(crate) source_arn: String,
    pub(crate) description: Option<String>,
    pub(crate) pattern: Option<String>,
    pub(crate) retention_days: i64,
    pub(crate) kms_key: Option<String>,
    pub(crate) created_ms: i64,
    pub(crate) events: u64,
    pub(crate) bytes: u64,
}

/// A replay. Keyed `<account>/<name>`. Replays finish as they start (no asynchronous replay, SCOPE.md), so a record is a finished one.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Replay {
    pub(crate) account: String,
    pub(crate) name: String,
    pub(crate) arn: String,
    pub(crate) description: Option<String>,
    pub(crate) source_arn: String,
    /// `Destination` as sent.
    pub(crate) destination: String,
    pub(crate) start_ms: i64,
    pub(crate) end_ms: i64,
    pub(crate) started_ms: i64,
    pub(crate) ended_ms: i64,
    pub(crate) last_replayed_ms: Option<i64>,
}

/// A connection. Keyed `<account>/<name>`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Connection {
    pub(crate) account: String,
    pub(crate) name: String,
    pub(crate) arn: String,
    pub(crate) description: Option<String>,
    pub(crate) auth_type: String,
    /// `AuthParameters` as sent. The secrets in it are never returned.
    pub(crate) auth_parameters: String,
    pub(crate) connectivity: Option<String>,
    pub(crate) kms_key: Option<String>,
    pub(crate) state: String,
    pub(crate) secret_arn: String,
    pub(crate) created_ms: i64,
    pub(crate) modified_ms: i64,
    pub(crate) authorized_ms: i64,
}

/// An API destination. Keyed `<account>/<name>`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ApiDestination {
    pub(crate) account: String,
    pub(crate) name: String,
    pub(crate) arn: String,
    pub(crate) description: Option<String>,
    pub(crate) connection_arn: String,
    pub(crate) endpoint: String,
    pub(crate) method: String,
    pub(crate) rate_limit: Option<i64>,
    pub(crate) created_ms: i64,
    pub(crate) modified_ms: i64,
}

/// A key made of account and names; none of them contains a slash.
pub(crate) fn key(account: &str, parts: &[&str]) -> String {
    let mut k = account.to_owned();
    for p in parts {
        k.push('/');
        k.push_str(p);
    }
    k
}

/// The caller a delivery is made as, kept on disk with work that waits: the region, account and endpoint it was accepted for.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Who {
    pub(crate) access_key_id: String,
    pub(crate) account_id: String,
    pub(crate) region: String,
    pub(crate) endpoint: String,
    pub(crate) service: Option<String>,
    pub(crate) session_token: Option<String>,
}

impl From<&Caller> for Who {
    fn from(c: &Caller) -> Self {
        Self {
            access_key_id: c.access_key_id.clone(),
            account_id: c.account_id.clone(),
            region: c.region.clone(),
            endpoint: c.endpoint.clone(),
            service: c.service.clone(),
            session_token: c.session_token.clone(),
        }
    }
}

impl From<Who> for Caller {
    fn from(w: Who) -> Self {
        Self {
            access_key_id: w.access_key_id,
            account_id: w.account_id,
            region: w.region,
            endpoint: w.endpoint,
            service: w.service,
            session_token: w.session_token,
        }
    }
}

/// An event `PutEvents` accepted and has not yet run through its bus. Keyed by the order it was accepted in (`sequence_key`).
///
/// AWS answers `PutEvents` once the events are accepted and invokes the targets afterwards; the event is on disk before the answer,
/// so a stop or a crash between the two delivers it when the service is next started.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Pending {
    pub(crate) account: String,
    pub(crate) bus: String,
    pub(crate) event: crate::event::Event,
    pub(crate) caller: Who,
}

/// The key of the `n`-th piece of waiting work: zero-padded so the keys sort in the order the work arrived.
pub(crate) fn sequence_key(n: u64) -> String {
    format!("{n:020}")
}
