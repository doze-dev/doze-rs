//! The events Activity reads: the stack's journal on disk (`<state>/run/journal.jsonl` and its rotated file), or,
//! when there is none, what the daemon sent (Go `Service.Activity`).

use std::time::{Duration, SystemTime};

use doze_display as words;

use crate::view::StackView;

/// One journal event, as Activity reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ev {
    pub time: SystemTime,
    pub kind: String,
    pub instance: String,
    pub cause_kind: String,
    pub cause_by: String,
    pub detail: String,
    /// For a ready: the time since its wake.
    pub took: Duration,
}

impl Ev {
    /// An event of `kind` for `instance` at `time`, caused by `cause_kind` (and `cause_by`): for tests and fixtures.
    pub fn new(time: SystemTime, kind: &str, instance: &str, cause: (&str, &str)) -> Ev {
        Ev {
            time,
            kind: kind.into(),
            instance: instance.into(),
            cause_kind: cause.0.into(),
            cause_by: cause.1.into(),
            detail: String::new(),
            took: Duration::ZERO,
        }
    }

    /// A line read from the journal file; `None` for one without a time.
    pub fn from_journal(e: &doze_journal::Event) -> Option<Ev> {
        Some(Ev {
            time: SystemTime::from(e.time?),
            kind: e.kind.clone(),
            instance: e.instance.clone(),
            cause_kind: e.cause.kind.clone(),
            cause_by: e.cause.by.clone(),
            detail: e.detail.clone(),
            took: e.took().unwrap_or_default(),
        })
    }

    /// An event the daemon sent; `None` for one without a time.
    pub fn from_wire(e: &doze_control::Event) -> Option<Ev> {
        Some(Ev {
            time: e.time.to_system_time()?,
            kind: e.kind.clone(),
            instance: e.instance.clone(),
            cause_kind: e.cause.kind.clone(),
            cause_by: e.cause.by.clone(),
            detail: e.detail.clone(),
            took: e.took.to_std(),
        })
    }

    /// The words' event, for its sentence.
    pub(crate) fn words(&self) -> words::Event {
        words::Event {
            time: self.time,
            kind: words::EventKind::parse(&self.kind),
            instance: self.instance.clone(),
            cause: words::Cause::parse(&self.cause_kind, &self.cause_by),
            took: self.took,
            detail: self.detail.clone(),
        }
    }
}

/// The stack's events: the journal on disk, or (none there, or unreadable) what its daemon sent.
pub fn events_of(st: &StackView) -> Vec<Ev> {
    let disk: Vec<Ev> = doze_journal::read(&st.key.journal())
        .unwrap_or_default()
        .iter()
        .filter_map(Ev::from_journal)
        .collect();
    if !disk.is_empty() {
        return disk;
    }
    st.events.iter().filter_map(Ev::from_wire).collect()
}
