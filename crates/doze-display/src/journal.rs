//! The journal's events as sentences: what happened to an instance, and why.

use std::borrow::Cow;
use std::time::{Duration, SystemTime};

use crate::gotime;
use crate::note::first_line;

/// What happened (`kind`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventKind {
    /// `wake`: a start began.
    Wake,
    /// `ready`: the start finished.
    Ready,
    /// `sleep`: stopped on purpose.
    Sleep,
    /// `crash`: exited by itself; the detail is how.
    Crash,
    /// `restart`: a crashed instance is being started again.
    Restart,
    /// `give_up`: the restart budget is spent.
    GiveUp,
    /// `failed`: a start did not complete; the detail is why.
    Failed,
    /// `step`: a setup or `on_ready` command ran; the detail names it.
    Step,
    /// `pin`: pinned awake (detail `on`) or unpinned.
    Pin,
    /// A kind this crate does not know, said as it came.
    Other(String),
}

impl EventKind {
    /// Reads the journal's word.
    #[must_use]
    pub fn parse(word: &str) -> Self {
        match word {
            "wake" => Self::Wake,
            "ready" => Self::Ready,
            "sleep" => Self::Sleep,
            "crash" => Self::Crash,
            "restart" => Self::Restart,
            "give_up" => Self::GiveUp,
            "failed" => Self::Failed,
            "step" => Self::Step,
            "pin" => Self::Pin,
            other => Self::Other(other.to_owned()),
        }
    }
}

/// Why something happened (`cause`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Cause {
    /// No cause given, or one this crate does not know.
    #[default]
    Unknown,
    /// `connection`: a client connected.
    Connection,
    /// `dependency`: an instance that needs it started; it is named.
    Dependency(String),
    /// `command`: someone asked (the CLI, the app, the API).
    Command,
    /// `restart`: the restart policy.
    Restart,
    /// `idle`: nobody held it for the idle time.
    Idle,
    /// `shutdown`: the daemon is stopping.
    Shutdown,
    /// `watch`: a file it watches changed; the path is named.
    Watch(String),
}

impl Cause {
    /// Reads the journal's `kind` and `by`.
    #[must_use]
    pub fn parse(kind: &str, by: &str) -> Self {
        match kind {
            "connection" => Self::Connection,
            "dependency" => Self::Dependency(by.to_owned()),
            "command" => Self::Command,
            "restart" => Self::Restart,
            "idle" => Self::Idle,
            "shutdown" => Self::Shutdown,
            "watch" => Self::Watch(by.to_owned()),
            _ => Self::Unknown,
        }
    }
}

/// One line of the journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// `time`.
    pub time: SystemTime,
    /// `kind`.
    pub kind: EventKind,
    /// `instance`: whose event it is.
    pub instance: String,
    /// `cause`.
    pub cause: Cause,
    /// `took`: for a ready, the time since its wake.
    pub took: Duration,
    /// `detail`.
    pub detail: String,
}

/// An event as a sentence: "woke (api needed it)", "ready in 1.2s". (Go: `Event`.)
#[must_use]
pub fn sentence(e: &Event) -> String {
    let because: Cow<'_, str> = match &e.cause {
        Cause::Connection => " (a connection)".into(),
        Cause::Dependency(by) => format!(" ({by} needed it)").into(),
        Cause::Command => " (asked for)".into(),
        Cause::Restart => " (restart policy)".into(),
        Cause::Idle => " (idle)".into(),
        Cause::Shutdown => " (shutting down)".into(),
        Cause::Watch(path) => format!(" ({path} changed)").into(),
        Cause::Unknown => "".into(),
    };
    let took = if e.took.is_zero() {
        String::new()
    } else {
        format!(
            " in {}",
            gotime::duration_string(gotime::round(gotime::nanos(e.took), gotime::MILLISECOND))
        )
    };
    let detail = &e.detail;
    match &e.kind {
        // the detail is often the cause again ("asked for"): say it once
        EventKind::Wake if !detail.is_empty() && !because.contains(detail.as_str()) => {
            format!("woke{because} — {detail}")
        }
        EventKind::Wake => format!("woke{because}"),
        EventKind::Ready => format!("ready{took}"),
        EventKind::Sleep => format!("went to sleep{because}"),
        EventKind::Crash => format!("crashed: {detail}"),
        EventKind::Restart => format!("restarting{because}"),
        EventKind::GiveUp => format!("gave up restarting: {detail}"),
        EventKind::Failed => format!("failed{took}: {}", first_line(detail)),
        EventKind::Step => format!("step {detail}{took}"),
        EventKind::Pin if detail == "on" => "pinned awake".to_owned(),
        EventKind::Pin => "unpinned".to_owned(),
        EventKind::Other(kind) => kind.clone(),
    }
}

/// What made a run happen, in a few words: "api needs it", "you asked". (Go: `CauseWords`.)
#[must_use]
pub fn cause_words(c: &Cause) -> Cow<'_, str> {
    match c {
        Cause::Dependency(by) => format!("{by} needs it").into(),
        Cause::Command => "you asked".into(),
        Cause::Watch(path) => format!("{path} changed").into(),
        Cause::Connection => "a connection".into(),
        Cause::Restart => "restart policy".into(),
        Cause::Unknown | Cause::Idle | Cause::Shutdown => "doze up".into(),
    }
}
