//! What one instance is doing, as everyone outside the engine sees it: the state words of control protocol 2 and the
//! fields a status snapshot carries (Go's `registry.Instance`).

use std::time::{Duration, SystemTime};

/// The lifecycle phase of an instance. The words are control protocol 2's, byte for byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum State {
    /// No process is running.
    #[default]
    Reaped,
    /// A start is in flight (one at a time: every waiter shares it).
    Booting,
    /// Running, and something holds it (a connection, a dependant), or it never sleeps.
    Active,
    /// Running with nothing holding it; the idle time is counting down.
    Idle,
    /// It exited by itself and a restart is scheduled.
    Crashing,
    /// Nothing is running and something went wrong; `last_error` says what.
    Failed,
    /// A command that runs to completion exited 0.
    Done,
}

impl State {
    /// The protocol word: `reaped`, `booting`, `active`, `idle`, `crashing`, `failed`, `done`.
    pub fn as_str(self) -> &'static str {
        match self {
            State::Reaped => "reaped",
            State::Booting => "booting",
            State::Active => "active",
            State::Idle => "idle",
            State::Crashing => "crashing",
            State::Failed => "failed",
            State::Done => "done",
        }
    }

    /// Whether a process is up (Go's `Active || Idle`).
    pub fn running(self) -> bool {
        matches!(self, State::Active | State::Idle)
    }
}

/// A copy of one instance's state at a moment.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Instance {
    pub name: String,
    pub state: State,
    /// The process (or a plan's first process), 0 when none runs.
    pub pid: u32,
    /// Where the running process is reached: `127.0.0.1:<port>` or a socket path; empty when none runs.
    pub backend: String,
    /// Live holds: client connections through doze plus running dependants.
    pub conns: u32,
    /// When the running process started.
    pub started_at: Option<SystemTime>,
    /// When it last had nothing holding it, while running.
    pub idle_since: Option<SystemTime>,
    pub last_error: String,
    /// Its last convergence failed; cleared only by a successful one (survives a new start).
    pub tainted: bool,
    /// Pinned awake: the reaper passes it by.
    pub keep_awake: bool,
    /// Taken out of this run (`doze off`): stopped, and every start is refused until `on`.
    pub off: bool,
    /// Restarts after an unexpected exit since the last intentional stop.
    pub restart_count: u32,
    /// The restart budget and when the next attempt is due, while crashing.
    pub restart_max: u32,
    pub next_restart: Option<SystemTime>,
    /// Never put to sleep for being idle (a portless app, `sleep = "never"`): with nothing holding it, it is active.
    pub always_on: bool,
    /// Its own idle time; `None` is the stack's default.
    pub idle_after: Option<Duration>,
    /// The last periodic health probe: `None` before the first, or with no probe.
    pub healthy: Option<bool>,
}

/// One reason a running instance is awake (Go's `runtime.Holder`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Holder {
    /// A running instance that needs it.
    Dependant(String),
    /// The user pinned it awake.
    Pin,
    /// Client connections open to it through doze (dependants not counted).
    Connections(u32),
}

impl Holder {
    /// Go's kind word: `dependant`, `pin`, `connection`.
    pub fn kind(&self) -> &'static str {
        match self {
            Holder::Dependant(_) => "dependant",
            Holder::Pin => "pin",
            Holder::Connections(_) => "connection",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_are_the_protocol_words() {
        let all = [
            (State::Reaped, "reaped"),
            (State::Booting, "booting"),
            (State::Active, "active"),
            (State::Idle, "idle"),
            (State::Crashing, "crashing"),
            (State::Failed, "failed"),
            (State::Done, "done"),
        ];
        for (s, w) in all {
            assert_eq!(s.as_str(), w);
            assert_eq!(s.running(), matches!(s, State::Active | State::Idle));
        }
    }
}
