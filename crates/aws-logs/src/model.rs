//! What CloudWatch Logs holds: groups (with their filters), streams, events and stored documents.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// What a metric filter turns a matching event into.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Transformation {
    pub(crate) name: String,
    pub(crate) namespace: String,
    /// A number (`"1"`) or a selector (`"$.latency"`, `"$bytes"`).
    pub(crate) value: String,
    pub(crate) default: Option<f64>,
    pub(crate) dimensions: BTreeMap<String, String>,
    pub(crate) unit: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct MetricFilter {
    pub(crate) name: String,
    pub(crate) pattern: String,
    pub(crate) transformation: Transformation,
    pub(crate) created_ms: i64,
    pub(crate) apply_on_transformed: bool,
    pub(crate) field_selection: Option<String>,
    pub(crate) emit_system: Vec<String>,
}

/// A subscription filter: stored and returned. Nothing is delivered (the targets are Lambda, Kinesis, Firehose and
/// destinations in other accounts).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Subscription {
    pub(crate) name: String,
    pub(crate) pattern: String,
    pub(crate) destination: String,
    pub(crate) role: Option<String>,
    pub(crate) distribution: Option<String>,
    pub(crate) created_ms: i64,
    pub(crate) apply_on_transformed: bool,
    pub(crate) field_selection: Option<String>,
    pub(crate) emit_system: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Group {
    pub(crate) name: String,
    pub(crate) created_ms: i64,
    pub(crate) retention_days: Option<i64>,
    pub(crate) kms_key: Option<String>,
    pub(crate) class: String,
    pub(crate) deletion_protection: bool,
    pub(crate) tags: BTreeMap<String, String>,
    pub(crate) metric_filters: Vec<MetricFilter>,
    pub(crate) subscriptions: Vec<Subscription>,
}

impl Group {
    pub(crate) fn arn(&self, region: &str, account: &str) -> String {
        group_arn(region, account, &self.name)
    }
}

pub(crate) fn group_arn(region: &str, account: &str, name: &str) -> String {
    format!("arn:aws:logs:{region}:{account}:log-group:{name}")
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Stream {
    /// The number that prefixes the stream's event rows.
    pub(crate) no: u32,
    pub(crate) group: String,
    pub(crate) name: String,
    pub(crate) created_ms: i64,
    pub(crate) first_ms: Option<i64>,
    pub(crate) last_ms: Option<i64>,
    pub(crate) last_ingested_ms: Option<i64>,
    /// Events ever written: gives each a distinct row and the stream a sequence token that moves.
    pub(crate) written: u64,
    /// Bytes of events held: each message in UTF-8 plus 26 (what AWS counts a log event as). The group's is the sum.
    pub(crate) bytes: i64,
}

/// The key of a stream: group and stream name, with a byte neither may hold between them.
pub(crate) fn stream_key(group: &str, stream: &str) -> String {
    format!("{group}\u{1}{stream}")
}

/// A stored event: what a stream holds under (timestamp, sequence).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Event {
    pub(crate) ingested_ms: i64,
    pub(crate) message: String,
}

/// The row key of an event: stream, timestamp, sequence, as fixed-width hex so the keys sort as the events do.
pub(crate) fn event_key(stream: u32, ts: i64, seq: u64) -> String {
    format!("{stream:08x}{:016x}{:016x}", ts.max(0), seq)
}

/// A document kept as it was sent: a resource policy, an account policy, a query definition, a data protection policy. Stored,
/// returned, and applied to nothing.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Doc {
    pub(crate) json: String,
    pub(crate) updated_ms: i64,
}

pub(crate) fn doc_key(kind: &str, id: &str) -> String {
    format!("{kind}\u{1}{id}")
}
