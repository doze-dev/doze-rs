//! What the words are about: an instance as the daemon reports it, and the stack it belongs to.
//!
//! These carry only what the words read. The daemon's wire names are kept in the docs (`state`, `idle_since`, …) so
//! the protocol crate can fill them directly; a string the daemon sends that has a meaning here is parsed into an
//! enum once, at that boundary, and never compared as text again.

use std::time::{Duration, SystemTime};

/// The daemon's state word for an instance (`state`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum State {
    /// `active`: up, and something holds it.
    Active,
    /// `idle`: up with nobody holding it, counting down to sleep.
    Idle,
    /// `booting`: starting, or (for a task) running.
    Booting,
    /// `crashing`: exited by itself; the restart policy is trying again.
    Crashing,
    /// `failed`: a start did not complete.
    Failed,
    /// `error`.
    Error,
    /// `reaped`, or no state at all: asleep.
    #[default]
    Reaped,
    /// A word this crate does not know, drawn as it came.
    Other(String),
}

impl State {
    /// Reads the daemon's word. An empty word is asleep, as `reaped` is.
    #[must_use]
    pub fn parse(word: &str) -> Self {
        match word {
            "active" => Self::Active,
            "idle" => Self::Idle,
            "booting" => Self::Booting,
            "crashing" => Self::Crashing,
            "failed" => Self::Failed,
            "error" => Self::Error,
            "reaped" | "" => Self::Reaped,
            other => Self::Other(other.to_owned()),
        }
    }

    /// The word a person reads: awake, dozing (awake with nobody holding it, counting down to sleep), waking and
    /// asleep. Any other word is the daemon's own. (Go: `Word`.)
    #[must_use]
    pub fn word(&self) -> &str {
        match self {
            Self::Active => "awake",
            Self::Idle => "dozing",
            Self::Booting => "waking",
            Self::Reaped => "asleep",
            Self::Crashing => "crashing",
            Self::Failed => "failed",
            Self::Error => "error",
            Self::Other(word) => word,
        }
    }

    /// The process is up, counting one that is still booting. (Go: `Awake`.)
    #[must_use]
    pub fn awake(&self) -> bool {
        matches!(self, Self::Active | Self::Idle | Self::Booting)
    }
}

/// The block that declared an instance (`kind`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Kind {
    /// `module`: from the doze registry.
    Module,
    /// `app`: the project's own code.
    App,
    /// `service`: anything else the project runs.
    Service,
    /// `task`: runs once, then gets out of the way.
    Task,
    /// No kind given; listed with the modules.
    #[default]
    Unspecified,
    /// A kind this crate does not know.
    Other,
}

impl Kind {
    /// Reads the daemon's word.
    #[must_use]
    pub fn parse(word: &str) -> Self {
        match word {
            "module" => Self::Module,
            "app" => Self::App,
            "service" => Self::Service,
            "task" => Self::Task,
            "" => Self::Unspecified,
            _ => Self::Other,
        }
    }
}

/// One reason an instance is awake (`holders`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Holder {
    /// A running instance that needs it (`dependant`, with its name).
    Dependant(String),
    /// Open client connections (`connection`, with the count).
    Connections(u32),
    /// Pinned awake (`pin`).
    Pin,
    /// A holder this crate does not know.
    Other,
}

/// How a task's last run ended (`task.result`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// `done`.
    Done,
    /// `failed`.
    Failed,
    /// Anything else.
    Other,
}

impl Outcome {
    /// Reads the daemon's word.
    #[must_use]
    pub fn parse(word: &str) -> Self {
        match word {
            "done" => Self::Done,
            "failed" => Self::Failed,
            _ => Self::Other,
        }
    }
}

/// How a task last ran (`task`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRecord {
    /// `result`.
    pub outcome: Outcome,
    /// `exit_code`.
    pub exit_code: i64,
    /// `took_ms`: how long the run took.
    pub took: Duration,
    /// `ran_at`; `None` when the daemon sent none.
    pub ran_at: Option<SystemTime>,
    /// `why`: what made it run.
    pub why: String,
}

impl TaskRecord {
    pub(crate) fn failed(&self) -> bool {
        self.outcome == Outcome::Failed
    }
}

/// One instance of a stack, as the daemon reports it (`InstanceView`). Times the daemon leaves out are `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // WHY: each is a flag of the wire format, not a state machine
pub struct Instance {
    /// `name`.
    pub name: String,
    /// `kind`.
    pub kind: Kind,
    /// `task`: how a task last ran; `None` for anything else, and for a task that has not run.
    pub task: Option<TaskRecord>,
    /// `run`: the command line of an app, service or task.
    pub run: String,
    /// `needs`: what it boots first, in the order it declared them.
    pub needs: Vec<String>,
    /// `engine`.
    pub engine: String,
    /// `state`.
    pub state: State,
    /// `version`.
    pub version: String,
    /// `holders`: why it is awake.
    pub holders: Vec<Holder>,
    /// `started_at`.
    pub started_at: Option<SystemTime>,
    /// `idle_since`: when the last hold went; drives the countdown to sleep.
    pub idle_since: Option<SystemTime>,
    /// `idle_after`: its own idle time; zero means the stack's.
    pub idle_after: Duration,
    /// `always_on`: it never sleeps.
    pub always_on: bool,
    /// `ram`: resident bytes, 0 asleep.
    pub ram: u64,
    /// `endpoint`: the client-facing address.
    pub endpoint: String,
    /// `bind`: the address the backend occupies (`host:port` or `unix:/path`).
    pub bind: String,
    /// `domain`: its name in the local zone.
    pub domain: String,
    /// `name_only`: reachable at its name but not at `127.0.0.1:<port>`.
    pub name_only: bool,
    /// `console`: the URL of its web console, as the config said.
    pub console: String,
    /// `has_console`: the module describes a console of tables and streams.
    pub has_console: bool,
    /// `console_front`: the tab the console asks to open on (`console`, `web` or empty).
    pub console_front: String,
    /// `web_url`: where the module serves a web console.
    pub web_url: String,
    /// `name_held`: a name it wanted belongs to another program, and which.
    pub name_held: String,
    /// `url`: the full connection string.
    pub url: String,
    /// `resource`: the directly addressable path behind a shared front door.
    pub resource: String,
    /// `data_dir`: where its data is written.
    pub data_dir: String,
    /// `last_error`.
    pub last_error: String,
    /// `tainted`: its last sync did not finish.
    pub tainted: bool,
    /// `disabled`: not running by choice.
    pub disabled: bool,
    /// `paused`: `enabled = false` in doze.hcl.
    pub paused: bool,
    /// `keep_awake`: pinned.
    pub keep_awake: bool,
    /// `restart_count`.
    pub restart_count: u32,
    /// `restart_max`: the restart budget, set while crashing.
    pub restart_max: u32,
    /// `next_restart`: when the next attempt is due.
    pub next_restart: Option<SystemTime>,
}

impl Instance {
    pub(crate) fn is_task(&self) -> bool {
        self.kind == Kind::Task
    }

    pub(crate) fn last_run_failed(&self) -> bool {
        self.task.as_ref().is_some_and(TaskRecord::failed)
    }
}

/// What the stack is doing: its instances and the idle time they sleep after.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// The stack's idle time (`idle_timeout`).
    pub idle_timeout: Duration,
    /// Its instances, in the daemon's order.
    pub instances: Vec<Instance>,
}

impl Snapshot {
    /// The instance named `name`. (Go: `Find`.)
    #[must_use]
    pub fn find(&self, name: &str) -> Option<&Instance> {
        self.instances.iter().find(|v| v.name == name)
    }
}
