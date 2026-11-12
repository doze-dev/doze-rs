//! The journal's events as the `journal` topic carries them (Go `internal/journal`: `Event`, `Cause`, the kinds).

use crate::time::{GoDuration, Timestamp};
use serde::{Deserialize, Serialize};

/// The kinds of event.
pub mod kind {
    /// A start began.
    pub const WAKE: &str = "wake";
    /// The start finished; `took` is the time since its wake.
    pub const READY: &str = "ready";
    /// Stopped on purpose.
    pub const SLEEP: &str = "sleep";
    /// Exited by itself; `detail` is how.
    pub const CRASH: &str = "crash";
    /// A crashed instance is being started again.
    pub const RESTART: &str = "restart";
    /// The restart budget is spent.
    pub const GIVE_UP: &str = "give_up";
    /// A start did not complete; `detail` is why.
    pub const FAILED: &str = "failed";
    /// A setup or `on_ready` command ran; `detail` names it.
    pub const STEP: &str = "step";
    /// Pinned awake or unpinned; `detail` is `on` or `off`.
    pub const PIN: &str = "pin";
    /// Every kind.
    pub const ALL: [&str; 9] = [
        WAKE, READY, SLEEP, CRASH, RESTART, GIVE_UP, FAILED, STEP, PIN,
    ];
}

/// The kinds of cause.
pub mod cause {
    /// A client connected.
    pub const CONNECTION: &str = "connection";
    /// An instance that depends on it started; `by` names it.
    pub const DEPENDENCY: &str = "dependency";
    /// Someone asked: the CLI, the dash, the API.
    pub const COMMAND: &str = "command";
    /// The restart policy.
    pub const RESTART: &str = "restart";
    /// Nobody held it for the idle time.
    pub const IDLE: &str = "idle";
    /// The daemon is stopping.
    pub const SHUTDOWN: &str = "shutdown";
    /// A file it watches changed; `by` names the path.
    pub const WATCH: &str = "watch";
    /// Every cause.
    pub const ALL: [&str; 7] = [
        CONNECTION, DEPENDENCY, COMMAND, RESTART, IDLE, SHUTDOWN, WATCH,
    ];
}

/// Why something happened (Go `journal.Cause`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Cause {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub kind: String,
    /// Who: the dependant, for a dependency.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub by: String,
}

impl Cause {
    /// Go's `omitzero`: a cause with neither field.
    pub fn is_zero(&self) -> bool {
        self.kind.is_empty() && self.by.is_empty()
    }
}

/// One line of the journal (Go `journal.Event`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Event {
    /// Unique across daemon restarts: a subscription resumes after it.
    pub seq: u64,
    pub time: Timestamp,
    pub kind: String,
    pub instance: String,
    #[serde(skip_serializing_if = "Cause::is_zero")]
    pub cause: Cause,
    #[serde(skip_serializing_if = "GoDuration::is_zero")]
    pub took: GoDuration,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub detail: String,
}
