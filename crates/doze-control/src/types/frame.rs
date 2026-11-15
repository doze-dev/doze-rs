//! What the daemon sends: frames (Go `Frame`), the response a frame carries (Go `Response`), and what goes in them.

use super::console::ConsoleFrame;
use super::instance::{InstanceView, StatView};
use super::journal::Event;
use crate::json::{is_false, is_zero_i64, is_zero_u64};
use crate::time::{GoDuration, Timestamp};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The first frame in each direction (Go `Hello`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Hello {
    pub proto: i64,
    /// Who is connecting (`cli`, `dash`, `app`), for the daemon's log.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub client: String,
}

/// One message from the daemon (Go `Frame`). A unary request gets one frame with a response; a stream gets a
/// response as its acknowledgement, then topic frames, then one with `end`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Frame {
    /// The request this answers.
    #[serde(skip_serializing_if = "is_zero_u64")]
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hello: Option<Hello>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<Response>,
    /// Set on the frame that refuses a connection (a protocol mismatch), and on an `end` that failed.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
    /// See [`crate::Topic`].
    #[serde(skip_serializing_if = "String::is_empty")]
    pub topic: String,
    /// When a stats frame's readings were taken. Always sent; zero on other frames.
    pub at: Timestamp,
    /// An instances frame that carries every instance: the first, and any sent because a subscriber fell behind.
    #[serde(skip_serializing_if = "is_false")]
    pub snapshot: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub instances: Vec<InstanceView>,
    /// Instances gone since the last instances frame.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stats: Vec<StatView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<Event>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log: Option<LogFrame>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub console: Option<ConsoleFrame>,
    /// Closes a stream.
    #[serde(skip_serializing_if = "is_false")]
    pub end: bool,
}

/// The daemon's reply (Go `Response`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Response {
    pub ok: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
    /// The error's kind (see [`crate::fault`]) when it has one more precise than `failed`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub code: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub listen: String,
    /// The reap window, for the countdown.
    #[serde(skip_serializing_if = "GoDuration::is_zero")]
    pub idle_timeout: GoDuration,
    /// The stack's name.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub stack: String,
    /// The folder doze.hcl is in.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub dir: String,
    /// When the daemon read doze.hcl. Go's `omitzero`.
    #[serde(skip_serializing_if = "Timestamp::is_zero")]
    pub loaded: Timestamp,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub instances: Vec<InstanceView>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<String>,
    /// The same lines with their time, stream and run.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub log: Vec<LogFrame>,
    /// A builtin's sub-resources (`resources`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<ResourceView>,
    /// The data actions it offers (`resources`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<ActionView>,
    /// An admin action's result line (`admin`).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub result: String,
}

/// One instance-tagged log line (Go `LogFrame`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LogFrame {
    pub instance: String,
    pub line: String,
    /// `o` (stdout) or `e` (stderr), then `:<part>` for one process of several, or `s:<name>` for a step.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub stream: String,
    /// Numbers the start of the instance this line belongs to.
    #[serde(skip_serializing_if = "is_zero_i64")]
    pub run: i64,
    pub time: Timestamp,
}

/// One instance-state transition (Go `EventFrame`); the `events` op sends it as an instances frame.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EventFrame {
    pub instance: InstanceView,
}

/// A builtin's sub-resource (Go `ResourceView`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ResourceView {
    pub kind: String,
    pub name: String,
    pub status: String,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub info: BTreeMap<String, String>,
}

/// A builtin's data operation (Go `ActionView`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ActionView {
    pub id: String,
    pub label: String,
    pub kind: String,
    #[serde(skip_serializing_if = "is_false")]
    pub destructive: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub input_hint: String,
}

impl Frame {
    /// The topic, if it is one this protocol knows.
    pub fn known_topic(&self) -> Option<crate::Topic> {
        crate::Topic::parse(&self.topic)
    }
}
