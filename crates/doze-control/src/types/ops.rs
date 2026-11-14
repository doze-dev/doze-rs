//! The protocol's names: every op the daemon dispatches (Go `server.go`), the topics, the verbs, the fault kinds, and
//! the shapes a client asks with (Go `Subscription`, `LogQuery`, `Snapshot`), which are not sent as they are.

use super::frame::Response;
use super::instance::InstanceView;
use crate::time::{GoDuration, Timestamp};
use std::time::Duration;

macro_rules! names {
    ($(#[$meta:meta])* $name:ident { $($(#[$vm:meta])* $variant:ident = $text:literal,)* }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name {
            $($(#[$vm])* $variant,)*
        }

        impl $name {
            /// Every one, in Go's order.
            pub const ALL: &'static [$name] = &[$($name::$variant,)*];

            /// The name on the wire.
            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $text,)*
                }
            }

            /// The one with this wire name.
            pub fn parse(s: &str) -> Option<$name> {
                match s {
                    $($text => Some($name::$variant),)*
                    _ => None,
                }
            }
        }
    };
}

names! {
    /// Every op of protocol 2. Each answers with one response frame, except the streams: `subscribe`, `events`,
    /// and `logs` with `follow`. `cancel` is never answered: it ends the stream with its id.
    Op {
        Status = "status",
        Up = "up",
        Apply = "apply",
        Destroy = "destroy",
        Down = "down",
        Sleep = "sleep",
        Boot = "boot",
        Wake = "wake",
        Restart = "restart",
        Reset = "reset",
        Task = "task",
        Logs = "logs",
        Add = "add",
        Remove = "remove",
        KeepAwake = "keepawake",
        Pin = "pin",
        Off = "off",
        On = "on",
        Resources = "resources",
        Admin = "admin",
        Subscribe = "subscribe",
        Events = "events",
        Cancel = "cancel",
    }
}

names! {
    /// What a subscription can follow.
    Topic {
        /// A full snapshot, then each instance as it changes.
        Instances = "instances",
        /// CPU and memory per running instance, on an interval.
        Stats = "stats",
        /// Journal events, from a sequence number on.
        Journal = "journal",
        /// Log lines for chosen instances.
        Logs = "logs",
        /// A module's console: its spec, its tables, a stream.
        Console = "console",
    }
}

names! {
    /// The things one can do to an instance (Go `Verb*`); each is also the name of its op.
    Verb {
        /// Converge and wake; an empty target is the whole stack.
        Up = "up",
        /// Start now, with dependencies.
        Wake = "wake",
        /// Stop, keeping data; an empty target is everything awake.
        Sleep = "sleep",
        /// Stop and start.
        Restart = "restart",
        /// Stop, wipe the data, provision fresh on the next wake.
        Reset = "reset",
        /// Run a task now, whatever its state.
        Task = "task",
        /// Toggle: keep awake past the idle timeout.
        Pin = "pin",
        /// Take out of this run: stop it, and nothing wakes it until `on`.
        Off = "off",
        /// Put back into this run; it wakes on the next connection.
        On = "on",
        /// Reconcile declared structure (Go `VerbSync`).
        Sync = "apply",
    }
}

impl Verb {
    /// The op that carries it.
    pub fn op(self) -> Op {
        match self {
            Verb::Up => Op::Up,
            Verb::Wake => Op::Wake,
            Verb::Sleep => Op::Sleep,
            Verb::Restart => Op::Restart,
            Verb::Reset => Op::Reset,
            Verb::Task => Op::Task,
            Verb::Pin => Op::Pin,
            Verb::Off => Op::Off,
            Verb::On => Op::On,
            Verb::Sync => Op::Apply,
        }
    }
}

/// The control protocol this crate speaks.
pub const PROTO_VERSION: i64 = 2;

/// As [`Subscription::since`], asks for every journal event kept.
pub const JOURNAL_FROM_START: u64 = u64::MAX;

/// The kinds of error a response's `code` names (Go `internal/fault`). `failed` and `interrupted` are never sent.
pub mod fault {
    pub const FAILED: &str = "failed";
    pub const USAGE: &str = "usage";
    pub const NOT_FOUND: &str = "not_found";
    pub const CONFIG_INVALID: &str = "config_invalid";
    pub const NO_CONFIG: &str = "no_config";
    pub const DAEMON_DOWN: &str = "daemon_down";
    pub const TIMEOUT: &str = "timeout";
    pub const NEEDS_CONFIRMATION: &str = "needs_confirmation";
    pub const INTERRUPTED: &str = "interrupted";
}

/// What a subscribe request asks for (Go `Subscription`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Subscription {
    pub topics: Vec<Topic>,
    /// Scopes the `logs` and `console` topics (empty = every declared instance).
    pub names: Vec<String>,
    /// The journal sequence number to resume after: zero sends only events from now on, [`JOURNAL_FROM_START`]
    /// everything kept.
    pub since: u64,
    /// The stats sampling period (`None` = one second).
    pub interval: Option<Duration>,
    /// Recent log lines of each instance to send before following (zero = the default, negative = none).
    pub backlog: i64,
    /// The console stream panel to run; without one only tables are sent.
    pub panel: String,
    /// The row of the panel's table the stream follows.
    pub selector: String,
}

impl Subscription {
    /// A subscription to these topics, with every other choice the daemon's default.
    pub fn to(topics: &[Topic]) -> Subscription {
        Subscription {
            topics: topics.to_vec(),
            ..Subscription::default()
        }
    }

    /// Whether it follows the topic.
    pub fn wants(&self, topic: Topic) -> bool {
        self.topics.contains(&topic)
    }
}

/// Which lines to read (Go `LogQuery`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogQuery {
    pub instance: String,
    /// 0 is the newest run, -1 the one before, a positive number a run by its id.
    pub run: i64,
    /// How many lines from the end (zero = the default).
    pub tail: i64,
    /// Drops lines older than this (zero = keep all).
    pub since: Timestamp,
}

/// The answer to "what is the stack doing" (Go `Snapshot`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub listen: String,
    pub idle_timeout: GoDuration,
    pub stack: String,
    pub dir: String,
    /// When the daemon read doze.hcl; zero from an older daemon.
    pub loaded: Timestamp,
    pub instances: Vec<InstanceView>,
}

impl From<Response> for Snapshot {
    fn from(r: Response) -> Snapshot {
        Snapshot {
            listen: r.listen,
            idle_timeout: r.idle_timeout,
            stack: r.stack,
            dir: r.dir,
            loaded: r.loaded,
            instances: r.instances,
        }
    }
}
