//! What the engine did, as journal events (Go's `internal/journal` kinds and causes). The engine publishes them; E2's
//! journal writer and D1's `journal` topic subscribe.

use std::time::Duration;

/// What happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A start began.
    Wake,
    /// The start finished; `took` is the time since its wake.
    Ready,
    /// Stopped on purpose.
    Sleep,
    /// Exited by itself; `detail` is how.
    Crash,
    /// The restart budget is spent.
    GiveUp,
    /// A start did not complete; `detail` is why.
    Failed,
    /// A `setup`, `on_ready` or `stop` command ran; `detail` names it.
    Step,
    /// Pinned awake or unpinned; `detail` is `on` or `off`.
    Pin,
}

impl Kind {
    /// The journal word.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Wake => "wake",
            Kind::Ready => "ready",
            Kind::Sleep => "sleep",
            Kind::Crash => "crash",
            Kind::GiveUp => "give_up",
            Kind::Failed => "failed",
            Kind::Step => "step",
            Kind::Pin => "pin",
        }
    }
}

/// Why something happened.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Cause {
    /// Not recorded.
    #[default]
    None,
    /// A client connected.
    Connection,
    /// A service that needs it started; names it.
    Dependency(String),
    /// Someone asked: the CLI, the app, the API.
    Command,
    /// The restart policy.
    Restart,
    /// Nothing held it for its idle time.
    Idle,
    /// The stack is stopping.
    Shutdown,
    /// A file it watches changed; names the path.
    Watch(String),
}

impl Cause {
    /// The journal's `cause.kind` word (empty for none).
    pub fn kind(&self) -> &'static str {
        match self {
            Cause::None => "",
            Cause::Connection => "connection",
            Cause::Dependency(_) => "dependency",
            Cause::Command => "command",
            Cause::Restart => "restart",
            Cause::Idle => "idle",
            Cause::Shutdown => "shutdown",
            Cause::Watch(_) => "watch",
        }
    }

    /// The journal's `cause.by`: who, for a dependency or a watched path.
    pub fn by(&self) -> &str {
        match self {
            Cause::Dependency(s) | Cause::Watch(s) => s,
            _ => "",
        }
    }
}

/// One thing the engine did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub kind: Kind,
    pub instance: String,
    pub cause: Cause,
    pub took: Option<Duration>,
    pub detail: String,
}

impl Event {
    /// An event with nothing but its kind and instance.
    pub fn new(kind: Kind, instance: &str) -> Event {
        Event {
            kind,
            instance: instance.into(),
            cause: Cause::None,
            took: None,
            detail: String::new(),
        }
    }

    pub fn cause(mut self, cause: Cause) -> Event {
        self.cause = cause;
        self
    }

    pub fn took(mut self, took: Duration) -> Event {
        self.took = Some(took);
        self
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Event {
        self.detail = detail.into();
        self
    }

    /// As a journal line (the journal stamps its sequence number and time).
    pub fn to_journal(&self) -> doze_journal::Event {
        doze_journal::Event {
            kind: self.kind.as_str().into(),
            instance: self.instance.clone(),
            cause: doze_journal::Cause {
                kind: self.cause.kind().into(),
                by: self.cause.by().into(),
            },
            took_ns: self
                .took
                .map_or(0, |d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX)),
            detail: self.detail.clone(),
            ..doze_journal::Event::default()
        }
    }
}
