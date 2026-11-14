//! An instance in trouble (Go `instanceItem`, `stuckWaking`): crashing, failed, not ready in time, still waking, or a
//! task whose last run failed; with the port a crash names and who holds it.

use std::time::{Duration, SystemTime};

use doze_display::{self as words, EventKind, Instance, Outcome, State};

use super::items::{StackFacts, time_print};
use super::{Fix, FixKind, Item, Place};
use crate::view::StackView;

/// How long a wake may take before doze says so, and how long it must have been silent (Go `stuckFor`, `quietFor`).
const STUCK_FOR: Duration = Duration::from_secs(5 * 60);
const QUIET_FOR: Duration = Duration::from_secs(2 * 60);

/// An instance in trouble: crashing, failed, not ready in time, still waking, or a task whose last run failed.
pub(crate) fn instance_item(
    st: &StackView,
    stack: &str,
    key: &str,
    v: &Instance,
    f: &StackFacts<'_>,
    now: SystemTime,
) -> Option<Item> {
    if let Some(it) = stuck_waking(st, v, now) {
        return Some(it);
    }
    if !words::troubled(v) {
        return None;
    }
    let mut it = Item {
        id: format!("instance:{key}/{}", v.name),
        red: true,
        kind: "instance",
        key: key.to_owned(),
        stack: stack.to_owned(),
        instance: v.name.clone(),
        show: Some(Place {
            key: key.to_owned(),
            page: "instance",
            instance: v.name.clone(),
            section: "",
        }),
        ..Item::default()
    };
    if let Some(t) = v.task.as_ref().filter(|t| t.outcome == Outcome::Failed) {
        it.kind = "task";
        it.title = format!("{} failed", v.name);
        it.detail = format!("exit {}", t.exit_code);
        let e = words::first_line(&v.last_error);
        if !e.is_empty() {
            it.detail = format!("{} · {e}", it.detail);
        }
        (it.mono, it.since, it.print) = (true, t.ran_at, time_print(t.ran_at));
        it.fixes = vec![
            Fix::new("Run again", FixKind::Do).verb("task").primary(),
            Fix::new("Output", FixKind::Show),
        ];
        return Some(it);
    }
    let err = words::first_line(&v.last_error).to_owned();
    // Since is the last time the journal saw it crash or fail.
    it.since = st
        .events
        .iter()
        .rev()
        .find(|e| e.instance == v.name && matches!(e.kind.as_str(), "crash" | "failed" | "give_up"))
        .and_then(|e| e.time.to_system_time());
    if v.last_error.contains("not become ready") {
        it.title = format!("{} did not become ready", v.name);
        it.detail = "it started but never answered; its logs usually say why".into();
    } else if v.state == State::Crashing {
        it.title = format!("{} keeps crashing", v.name);
        it.detail = crash_detail(&err, v, now);
    } else {
        it.title = format!("{} stopped and waits for you", v.name);
        it.detail.clone_from(&err);
    }
    it.mono = !err.is_empty() && it.detail == err;
    // The daemon's own word for the state, as Go's print has it, so a choice put aside in either app holds in both.
    let wire = st.instance(&v.name).map_or("", |w| w.state.as_str());
    it.print = format!("{wire}|{err}");
    it.fixes = vec![
        Fix::new("Restart", FixKind::Do).verb("restart").primary(),
        Fix::new("Logs", FixKind::Show),
    ];
    held_port(&mut it, v, f);
    Some(it)
}

/// The first error line and the try count: "boom · try 3 of 5, next in 4s".
fn crash_detail(err: &str, v: &Instance, now: SystemTime) -> String {
    if v.restart_max == 0 {
        return err.to_owned();
    }
    let mut try_ = format!("try {} of {}", v.restart_count, v.restart_max);
    if let Some(next) = v.next_restart
        && let Ok(d) = next.duration_since(now)
    {
        try_ = format!("{try_}, next in {}", words::seconds_to_go(d));
    }
    if err.is_empty() {
        try_
    } else {
        format!("{err} · {try_}")
    }
}

/// A crash whose words name a port another process holds: say who, and offer to stop it.
fn held_port(it: &mut Item, v: &Instance, f: &StackFacts<'_>) {
    let Some(held) = f.held else { return };
    let Some(port) = crate::ports::port_in_use([v.last_error.as_str()]) else {
        return;
    };
    let Some(h) = held(port).filter(|h| h.pid > 0 && !h.doze) else {
        return;
    };
    it.detail = format!("port {port} is held by {} (pid {})", h.command, h.pid);
    it.mono = false;
    if v.state == State::Crashing && v.restart_max > 0 {
        it.detail = format!(
            "{} · try {} of {}",
            it.detail, v.restart_count, v.restart_max
        );
    }
    it.fixes = vec![
        Fix {
            pid: h.pid,
            ..Fix::new(
                format!("Stop pid {} and start {}", h.pid, v.name),
                FixKind::Holder,
            )
            .primary()
        },
        Fix::new("Show", FixKind::Show),
    ];
}

/// An instance waking for five minutes with no journal word from it for two (Go `stuckWaking`; D8's third rule).
#[must_use]
pub fn stuck_waking(st: &StackView, v: &Instance, now: SystemTime) -> Option<Item> {
    if v.state != State::Booting {
        return None;
    }
    let mine: Vec<_> = st
        .events
        .iter()
        .rev()
        .filter(|e| e.instance == v.name)
        .collect();
    let heard = mine.first().and_then(|e| e.time.to_system_time())?;
    let woke = mine
        .iter()
        .find(|e| EventKind::parse(&e.kind) == EventKind::Wake)
        .and_then(|e| e.time.to_system_time())?;
    let since = |t: SystemTime| now.duration_since(t).unwrap_or_default();
    if since(woke) < STUCK_FOR || since(heard) < QUIET_FOR {
        return None;
    }
    let key = st.key.dir().to_string_lossy().into_owned();
    Some(Item {
        id: format!("instance:{key}/{}", v.name),
        print: format!("stuck|{}", time_print(Some(woke))),
        red: true,
        kind: "instance",
        key: key.clone(),
        stack: st.name.clone(),
        instance: v.name.clone(),
        title: format!("{} is still waking", v.name),
        detail: format!(
            "for {}, with no word from it for {}; its logs usually say why",
            words::ago(since(woke)),
            words::ago(since(heard))
        ),
        since: Some(woke),
        fixes: vec![
            Fix::new("Restart", FixKind::Do).verb("restart").primary(),
            Fix::new("Logs", FixKind::Show),
        ],
        show: Some(Place {
            key,
            page: "instance",
            instance: v.name.clone(),
            section: "",
        }),
        ..Item::default()
    })
}
