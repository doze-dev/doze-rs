//! `doze why <name>` (new; proposal "`why`"): why a service is in its state, in the app's words: what holds it
//! awake or what it is down to (`doze_display::because`), what woke it last and when, how it last stopped, and its
//! restarts. The live part comes from the daemon; the history from the journal on disk, so with doze stopped it
//! still says what happened, and says that doze is not running.

use crate::cli::args::Service;
use crate::ctx::Ctx;
use crate::output::{Done, Mode, Outcome};
use crate::paint::Paint;
use crate::stack::{self, Stack};
use crate::view::View;
use doze_control::{InstanceView, words};
use doze_display::{Event, EventKind};
use serde::Serialize;
use std::time::SystemTime;

pub(crate) fn run(ctx: &mut Ctx<'_>, mode: &Mode, a: &Service) -> Outcome {
    let stack = Stack::find(ctx, mode)?;
    let live = match stack.read()? {
        Some(c) => {
            let snap = stack::snapshot(&c)?;
            let Some(v) = stack::declared(&snap).find(|v| v.name == a.name).cloned() else {
                return Err(stack::not_declared(&snap, &a.name));
            };
            let words_snap = words::snapshot(snap.idle_timeout.to_std(), &snap.instances);
            let because = doze_display::because(&words::instance(&v), &words_snap, ctx.now);
            Some((v, because))
        }
        None => None,
    };
    if live.is_none() {
        stack::known_on_disk(&stack, &a.name)?;
    }
    let events = history(&stack, &a.name);
    let zone = crate::cmd::logs::zone(ctx);
    Ok(Done::ok(View::Why(Box::new(
        Why {
            name: a.name.clone(),
            running: live.is_some(),
            state: live.as_ref().map(|(v, _)| word(v)).unwrap_or_default(),
            because: live.as_ref().map(|(_, b)| b.clone()).unwrap_or_default(),
            restarts: live.as_ref().map_or(0, |(v, _)| v.restart_count),
            events: events.iter().map(|(k, e)| Said::of(k, e, &zone)).collect(),
            live: live.map(|(v, _)| v),
        }
        .trimmed(),
    ))))
}

/// The instance's events from the journal, newest last.
fn history(stack: &Stack, name: &str) -> Vec<(String, Event)> {
    let all = doze_journal::read(&stack.key.journal()).unwrap_or_default();
    all.into_iter()
        .filter(|e| e.instance == name)
        .map(|e| {
            let ev = Event {
                time: e.time.map_or(SystemTime::UNIX_EPOCH, SystemTime::from),
                kind: EventKind::parse(&e.kind),
                instance: e.instance.clone(),
                cause: doze_display::Cause::parse(&e.cause.kind, &e.cause.by),
                took: e.took().unwrap_or_default(),
                detail: e.detail.clone(),
            };
            (e.kind, ev)
        })
        .collect()
}

/// The state as a person reads it.
fn word(v: &InstanceView) -> String {
    if v.paused {
        return "turned off in doze.hcl".into();
    }
    if v.disabled {
        return "off".into();
    }
    doze_display::State::parse(&v.state).word().to_owned()
}

/// One event, as `why` says it.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Said {
    /// The wall-clock time on this machine, `14:02:11`.
    at: String,
    kind: String,
    said: String,
}

impl Said {
    fn of(kind: &str, e: &Event, zone: &jiff::tz::TimeZone) -> Said {
        let at = crate::clock::wall(&doze_control::Timestamp::from_system_time(e.time), zone);
        Said {
            at,
            kind: kind.to_owned(),
            said: doze_display::sentence(e),
        }
    }
}

/// What `why` found.
#[derive(Debug, Clone, Serialize)]
pub struct Why {
    name: String,
    running: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    state: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    because: String,
    restarts: i64,
    /// What the journal says happened to it, oldest first (the last ten).
    events: Vec<Said>,
    #[serde(skip_serializing_if = "Option::is_none")]
    live: Option<InstanceView>,
}

impl Why {
    pub(crate) fn human(&self, p: Paint) -> String {
        let mut out = String::new();
        if self.running {
            out.push_str(&format!("{} is {}\n", p.bold(&self.name), self.state));
            out.push_str(&format!("  {}\n", self.because));
        } else {
            out.push_str(&format!(
                "{} {}\n",
                p.bold(&self.name),
                p.dim("· doze is not running; from the journal:")
            ));
        }
        let last = |pick: fn(&EventKind) -> bool| {
            self.events
                .iter()
                .rev()
                .find(|e| pick(&EventKind::parse(&e.kind)))
        };
        if let Some(w) = last(|k| matches!(k, EventKind::Wake)) {
            out.push_str(&format!("  {} {}\n", p.dim(&w.at), w.said));
        }
        if let Some(r) = last(|k| matches!(k, EventKind::Ready | EventKind::Failed)) {
            out.push_str(&format!("  {} {}\n", p.dim(&r.at), r.said));
        }
        if let Some(s) =
            last(|k| matches!(k, EventKind::Sleep | EventKind::Crash | EventKind::GiveUp))
        {
            out.push_str(&format!("  last stop: {} {}\n", p.dim(&s.at), s.said));
        }
        if self.running {
            let restarts = match self.restarts {
                0 => "no restarts".to_owned(),
                1 => "restarted once".to_owned(),
                n => format!("restarted {n} times"),
            };
            out.push_str(&format!("  {}\n", p.dim(&restarts)));
        }
        if self.events.is_empty() {
            out.push_str(&format!(
                "  {}\n",
                p.dim("the journal has nothing about it yet")
            ));
        }
        out
    }

    /// Keeps the last ten events.
    pub(crate) fn trimmed(mut self) -> Why {
        let extra = self.events.len().saturating_sub(10);
        self.events.drain(..extra);
        self
    }
}
