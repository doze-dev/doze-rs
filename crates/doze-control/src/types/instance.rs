//! What the daemon says about one instance: its view, its processes, why it is awake, how its task last ran, and its
//! meters. Field order and `omitempty` follow Go's `internal/control/types.go` exactly, so a view re-encodes byte for
//! byte.

use crate::json::{is_false, is_zero_f64, is_zero_i64};
use crate::time::{GoDuration, Timestamp};
use serde::{Deserialize, Serialize};

/// A serialisable snapshot of one backend's state (Go `InstanceView`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InstanceView {
    pub name: String,
    /// The block that declared it: `module`, `app`, `service` or `task`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub kind: String,
    /// How a task last ran; `None` for anything else, and for a task that has not run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<TaskView>,
    /// The command line of an app, service or task.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub run: String,
    /// The instances this one boots first, in the order it declared them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub needs: Vec<String>,
    pub engine: String,
    pub state: String,
    pub version: String,
    pub pid: i64,
    pub conns: i64,
    /// Why the instance is awake.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub holders: Vec<HolderView>,
    pub started_at: Timestamp,
    /// When `conns` hit 0; drives the reap countdown. Go's `omitempty` never omits a time.
    pub idle_since: Timestamp,
    /// This instance's own idle time; zero means the stack's (`Response::idle_timeout`).
    #[serde(skip_serializing_if = "GoDuration::is_zero")]
    pub idle_after: GoDuration,
    /// Never sleeps: a portless app, or a block with `sleep = "never"`.
    #[serde(skip_serializing_if = "is_false")]
    pub always_on: bool,
    /// Resident bytes of the backend, 0 when reaped.
    #[serde(skip_serializing_if = "is_zero_i64")]
    pub ram: i64,
    /// CPU usage percent (one core = 100), 0 when reaped.
    #[serde(skip_serializing_if = "is_zero_f64")]
    pub cpu: f64,
    /// The client-facing address.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub endpoint: String,
    /// The address the backend actually occupies (`host:port` or `unix:/path`).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub bind: String,
    /// The local DNS name, e.g. `orders-pg.<stack>.doze`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub domain: String,
    /// An app's own names outside the zone.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub domains: Vec<String>,
    /// The first of `domains` as the URL the front door serves over TLS.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub https: String,
    /// Reachable at its name but not at `127.0.0.1:<port>`, which something else holds.
    #[serde(skip_serializing_if = "is_false")]
    pub name_only: bool,
    /// The URL of the instance's web console, when it has one.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub console: String,
    /// The module describes a console of tables and streams.
    #[serde(skip_serializing_if = "is_false")]
    pub has_console: bool,
    /// The tab the console asks to open on (`console`, `web` or empty).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub console_front: String,
    /// Where the module serves a web console, its address filled in.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub web_url: String,
    /// A name this instance wanted that another program holds, and which.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name_held: String,
    /// The full connection string.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub url: String,
    /// The directly addressable path behind a shared front door (a bucket URL, a queue URL, a topic ARN).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub resource: String,
    /// The conventional env var (`DATABASE_URL`, …).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub env_var: String,
    /// Where this instance's data is written.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub data_dir: String,
    /// The most recent boot or crash failure.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub last_error: String,
    /// The last convergence failed or did not finish.
    #[serde(skip_serializing_if = "is_false")]
    pub tainted: bool,
    pub declared: bool,
    /// Not running by choice: `enabled = false` in the config, or `doze off`.
    #[serde(skip_serializing_if = "is_false")]
    pub disabled: bool,
    /// The config's half of `disabled`: only an edit to the file brings it back.
    #[serde(skip_serializing_if = "is_false")]
    pub paused: bool,
    /// Pinned: exempt from the idle reaper.
    #[serde(skip_serializing_if = "is_false")]
    pub keep_awake: bool,
    /// The display heading; empty means "infer from the engine".
    #[serde(skip_serializing_if = "String::is_empty")]
    pub group: String,
    /// Restarts after an unexpected exit.
    #[serde(skip_serializing_if = "is_zero_i64")]
    pub restart_count: i64,
    /// The restart budget, set only while `state` is `crashing`.
    #[serde(skip_serializing_if = "is_zero_i64")]
    pub restart_max: i64,
    /// When the next restart is due, set only while `state` is `crashing`.
    pub next_restart: Timestamp,
    /// The instance's descendant processes, each with its own meters.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<ProcView>,
    /// The latest liveness probe: `None` = not probed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub healthy: Option<bool>,
}

/// How a task last ran (Go `TaskView`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskView {
    /// `done` or `failed`.
    pub result: String,
    #[serde(skip_serializing_if = "is_zero_i64")]
    pub exit_code: i64,
    pub took_ms: i64,
    pub ran_at: Timestamp,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub why: String,
}

/// One reason an instance is awake (Go `HolderView`): a `dependant` (with `name`), a `pin`, or open client
/// `connection`s (with `count`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HolderView {
    pub kind: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(skip_serializing_if = "is_zero_i64")]
    pub count: i64,
}

/// One descendant process of a supervised instance (Go `ProcView`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProcView {
    pub pid: i64,
    pub rss: i64,
    pub cpu: f64,
    pub cmd: String,
    pub depth: i64,
}

/// One running instance's meters, sampled for the `stats` topic (Go `StatView`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StatView {
    pub name: String,
    pub pid: i64,
    pub ram: i64,
    pub cpu: f64,
    pub conns: i64,
    /// An app's own subprocesses, each with its meters.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<ProcView>,
}
