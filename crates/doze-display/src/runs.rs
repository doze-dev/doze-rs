//! A task's runs, as the journal tells them: when, why, how long, how it ended.

use std::borrow::Cow;
use std::time::{Duration, SystemTime};

use crate::journal::{Event, EventKind, cause_words};
use crate::model::{Outcome, TaskRecord};

/// One run of a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run<'a> {
    /// When it started.
    pub at: SystemTime,
    /// Why, in a few words.
    pub why: Cow<'a, str>,
    /// How long it took.
    pub took: Duration,
    /// It ended well.
    pub ok: bool,
    /// It has ended; false while it is still running.
    pub done: bool,
}

/// A task's runs from the journal, oldest first. A run is a wake and what follows it, a ready or a failure. When
/// the journal has kept none, the task's own record of its last run (`last`) stands in; empty when there is nothing
/// to say. (Go: `RunHistory`.)
#[must_use]
pub fn run_history<'a>(
    events: &'a [Event],
    name: &str,
    last: Option<&'a TaskRecord>,
) -> Vec<Run<'a>> {
    let mut runs: Vec<Run<'a>> = Vec::new();
    for e in events.iter().filter(|e| e.instance == name) {
        let open = runs.last_mut().filter(|r| !r.done);
        match (&e.kind, open) {
            (EventKind::Wake, _) => {
                runs.push(Run {
                    at: e.time,
                    why: cause_words(&e.cause),
                    took: Duration::ZERO,
                    ok: false,
                    done: false,
                });
            }
            (EventKind::Ready, Some(run)) => (run.took, run.ok, run.done) = (e.took, true, true),
            (EventKind::Failed, Some(run)) => (run.took, run.done) = (e.took, true),
            _ => {}
        }
    }
    if runs.is_empty()
        && let Some((t, at)) = last.and_then(|t| Some((t, t.ran_at?)))
    {
        let why = if t.why.is_empty() {
            Cow::Borrowed("doze up")
        } else {
            Cow::Borrowed(t.why.as_str())
        };
        runs.push(Run {
            at,
            why,
            took: t.took,
            ok: t.outcome == Outcome::Done,
            done: true,
        });
    }
    runs
}
