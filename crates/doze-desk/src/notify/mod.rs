//! When the app says something outside its window (Go `internal/desk/notify`, design D8): only for a crash, a service
//! that gave up, one that did not become ready, a wake stuck for five minutes with two of them quiet, or a failed
//! task; once per episode, and never for what is on screen. Pure: the app hands it the hub's stacks and what is on
//! screen, and gets back what to say. [`outside`] is what pressing a notification's fix (or a menu-bar fix) does, and
//! [`first_fix`] which fix a notification offers (Go `app/notify.go`).

mod fix;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::time::SystemTime;

use doze_display::{self as words, Kind, Look, State};

use crate::attention::stuck_waking;
use crate::view::StackView;

pub use fix::{Native, Outside, first_fix, native, outside};

/// One notification: what to say, and where its click goes (Go `notify.Note`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Note {
    /// One per episode, so a repeat replaces rather than piles up.
    pub id: String,
    /// The stack's key (its state folder, as Needs you's items name it).
    pub key: String,
    pub dir: String,
    pub instance: String,
    /// "api crashed · shop".
    pub title: String,
    /// The error, or the task's result.
    pub body: String,
    /// The line under the body: "try 3 of 5 · next in 4s".
    pub sub: String,
}

/// Which episodes were already said (Go `notify.Rules`).
#[derive(Debug, Default)]
pub struct Rules {
    /// stack/instance in a crash (or stuck wake) episode.
    crashed: HashSet<String>,
    /// stack/task → the last run seen.
    ran: HashMap<String, String>,
    /// The stacks looked at once: what a stack already shows when it is first seen is not news, whenever it
    /// appears.
    primed: HashSet<String>,
    /// The ids of notes whose episode ended since [`Rules::ended`] was last asked: a crash that is over, a failed run
    /// that a good one followed. A notification still showing for one is withdrawn.
    over: Vec<String>,
}

/// A crash episode has ended: the instance is up, dozing, asleep, off or paused. Between the tries of a crash loop
/// it boots again, which is the same episode, as the Crash board's "try 3 of 5" says.
fn episode_over(v: &words::Instance) -> bool {
    matches!(
        words::look(v),
        Look::Awake | Look::Dozing | Look::Asleep | Look::Off | Look::Paused
    )
}

impl Rules {
    /// Rules that say nothing about what is already the case when they first look at a stack: a crash that was there
    /// before the app opened is on screen, not news.
    #[must_use]
    pub fn new() -> Rules {
        Rules::default()
    }

    /// The ids of the notes whose episode ended since this was last asked (Go left a notification up after its
    /// crash was over; the app withdraws it).
    pub fn ended(&mut self) -> Vec<String> {
        std::mem::take(&mut self.over)
    }

    /// Looks at the stacks and returns what to say now (Go `Next`). `visible(key, instance)` says an instance is on
    /// screen, so nothing is said about it; the episode is still marked as seen.
    pub fn next(
        &mut self,
        stacks: &[StackView],
        visible: &dyn Fn(&str, &str) -> bool,
        now: SystemTime,
    ) -> Vec<Note> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for st in stacks {
            let key = st.key.dir().to_string_lossy().into_owned();
            let primed = self.primed.contains(&key);
            let snap = st.words();
            for v in &snap.instances {
                let id = format!("{key}/{}", v.name);
                seen.insert(id.clone());
                let shown = || visible(&key, &v.name);
                if v.kind == Kind::Task {
                    self.task(&mut out, st, v, &id, primed && !shown());
                    continue;
                }
                if let Some(it) = stuck_waking(st, v, now) {
                    // D8's third: a wake that hangs without failing. Once per wake, like a crash episode.
                    if self.crashed.insert(id.clone()) && primed && !shown() {
                        out.push(Note {
                            id: format!("{id}/stuck"),
                            title: format!("{} · {}", it.title, st.name),
                            body: it.detail,
                            ..note(st, &key, v)
                        });
                    }
                    continue;
                }
                if episode_over(v) {
                    if self.crashed.remove(&id) {
                        self.over.push(format!("{id}/crash"));
                        self.over.push(format!("{id}/stuck"));
                    }
                    continue;
                }
                if !words::troubled(v) || !self.crashed.insert(id.clone()) {
                    continue;
                }
                if primed && !shown() {
                    out.push(crash(st, &key, v, &id, now));
                }
            }
            self.primed.insert(key);
        }
        // Instances that are gone end their episodes.
        let gone: Vec<String> = self
            .crashed
            .iter()
            .filter(|id| !seen.contains(*id))
            .cloned()
            .collect();
        for id in gone {
            self.crashed.remove(&id);
            self.over.push(format!("{id}/crash"));
            self.over.push(format!("{id}/stuck"));
        }
        out
    }

    /// A task's run that failed, once per run (Go `task`).
    fn task(
        &mut self,
        out: &mut Vec<Note>,
        st: &StackView,
        v: &words::Instance,
        id: &str,
        say: bool,
    ) {
        let Some(t) = &v.task else { return };
        let run = format!(
            "{}/{:?}",
            t.ran_at
                .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos()),
            t.outcome
        );
        let prev = self.ran.insert(id.to_owned(), run.clone());
        // A run that went well ends the failed one before it.
        if let Some(p) = prev
            .as_ref()
            .filter(|p| **p != run && p.ends_with("/Failed"))
            && t.outcome == doze_display::Outcome::Done
        {
            self.over.push(format!("{id}/{p}"));
        }
        if !say
            || prev.as_ref().is_none_or(|p| *p == run)
            || t.outcome != doze_display::Outcome::Failed
        {
            return;
        }
        let key = st.key.dir().to_string_lossy();
        out.push(Note {
            id: format!("{id}/{run}"),
            title: format!("{} · {}", words::task_result(&v.name, t).text, st.name),
            body: format!(
                "task \"{}\" failed: exit {}; its last lines are on its page",
                v.name, t.exit_code
            ),
            sub: "a task failed; nothing retries it".into(),
            ..note(st, &key, v)
        });
    }
}

/// A note about `v` in `st`, its words still to fill.
fn note(st: &StackView, key: &str, v: &words::Instance) -> Note {
    Note {
        key: key.to_owned(),
        dir: st.dir.clone(),
        instance: v.name.clone(),
        ..Note::default()
    }
}

/// A crash, a service that gave up, or one that did not become ready.
fn crash(st: &StackView, key: &str, v: &words::Instance, id: &str, now: SystemTime) -> Note {
    let first = words::first_line(&v.last_error);
    let mut n = Note {
        id: format!("{id}/crash"),
        body: first.to_owned(),
        ..note(st, key, v)
    };
    if v.last_error.contains("not become ready") {
        // The third of D8's three: it started but never answered.
        n.title = format!("{} {first} · {}", v.name, st.name);
        n.body = "its logs usually say why".into();
        n.sub = "ready check in doze.hcl".into();
    } else if v.state == State::Crashing {
        n.title = format!("{} crashed · {}", v.name, st.name);
        if v.restart_max > 0 {
            n.sub = format!("try {} of {}", v.restart_count, v.restart_max);
            if let Some(d) = v.next_restart.and_then(|t| t.duration_since(now).ok())
                && !d.is_zero()
            {
                n.sub = format!("{} · next in {}s", n.sub, (d.as_secs_f64() + 0.5) as u64);
            }
        }
    } else {
        n.title = format!("{} failed · {}", v.name, st.name);
        n.sub = "it stopped and waits for you".into();
    }
    if n.body.is_empty() {
        n.body = "no error text; its logs usually say why".into();
    }
    n
}

/// The stack's key and the instance a note's id names (`<key>/<instance>/crash`, `…/stuck`, or a task's
/// `<key>/<instance>/<ran>/<outcome>`): where a click on a notification posted before this launch goes. `None` for an
/// id that names no instance.
#[must_use]
pub fn place_of(id: &str) -> Option<(String, String)> {
    let rest = id
        .strip_suffix("/crash")
        .or_else(|| id.strip_suffix("/stuck"))
        .or_else(|| {
            // A task's run: the time in nanoseconds, then the outcome.
            let (head, outcome) = id.rsplit_once('/')?;
            let (head, ran) = head.rsplit_once('/')?;
            (matches!(outcome, "Done" | "Failed" | "Other")
                && ran.bytes().all(|b| b.is_ascii_digit()))
            .then_some(head)
        })?;
    let (key, instance) = rest.rsplit_once('/')?;
    (!key.is_empty() && !instance.is_empty()).then(|| (key.to_owned(), instance.to_owned()))
}
