//! The words beside an instance: what it is and what it is doing about it, and what its state is down to.

use std::time::SystemTime;

use crate::format::{ago_nanos, countdown_nanos, plural};
use crate::gotime::{self, MILLISECOND, SECOND};
use crate::look::{awake_now, troubled};
use crate::model::{Holder, Instance, Kind, Snapshot, State, TaskRecord};
use crate::note::first_line;
use crate::reach::{engine_version, first_unmet, holder_named, short_run};

/// The third column: what the thing is, then what it is doing about it. (Go: `What`.)
#[must_use]
pub fn what(v: &Instance, snap: &Snapshot, now: SystemTime) -> String {
    let mut base = match v.kind {
        Kind::App | Kind::Service => short_run(&v.run).into_owned(),
        Kind::Task => match &v.task {
            Some(t) => {
                let ran = format!("ran {} ago in {}", ran_ago(t, now), took(t, MILLISECOND));
                if t.failed() {
                    format!("exit {} · {ran}", t.exit_code)
                } else {
                    ran
                }
            }
            None => "not run yet".to_owned(),
        },
        Kind::Module | Kind::Unspecified | Kind::Other => engine_version(v).into_owned(),
    };
    let mut extra = String::new();
    if troubled(v) && v.task.is_none() {
        // What went wrong is the long part, so it goes last and is the part that gets cut.
        base = first_line(&v.last_error).to_owned();
        if v.restart_max > 0 {
            extra = std::mem::replace(
                &mut base,
                format!("try {} of {}", v.restart_count, v.restart_max),
            );
        }
    } else if v.state == State::Booting {
        if let Some(n) = first_unmet(v, snap) {
            base = format!("waiting on {n}");
        } else if let Some(by) = holder_named(v) {
            extra = format!("woken by {by}");
        }
    } else if v.state == State::Idle
        && v.idle_since.is_some()
        && !v.always_on
        && let Some(left) = sleeps_in(v, snap, now)
    {
        extra = format!("sleeps in {}", countdown_nanos(left));
    }
    match (base.is_empty(), extra.is_empty()) {
        (true, _) => extra,
        (_, true) => base,
        _ => format!("{base} · {extra}"),
    }
}

/// How long until a dozing instance sleeps, when it has not passed.
fn sleeps_in(v: &Instance, snap: &Snapshot, now: SystemTime) -> Option<i64> {
    let after = gotime::nanos(if v.idle_after.is_zero() {
        snap.idle_timeout
    } else {
        v.idle_after
    });
    let left = after.wrapping_sub(gotime::since(now, v.idle_since));
    (left > 0).then_some(left)
}

fn ran_ago(t: &TaskRecord, now: SystemTime) -> String {
    ago_nanos(gotime::since(now, t.ran_at))
}

/// How long a task's run took, rounded to `to`.
pub(crate) fn took(t: &TaskRecord, to: i64) -> String {
    gotime::duration_string(gotime::round(gotime::nanos(t.took), to))
}

/// What an instance's state is down to, in a few words: what holds it awake, what it waits for, when it will
/// sleep, why it is not well. (Go: `Because`.)
#[must_use]
pub fn because(v: &Instance, snap: &Snapshot, now: SystemTime) -> String {
    if v.paused {
        "enabled = false in doze.hcl".to_owned()
    } else if v.disabled {
        "turned off; nothing wakes it until it is switched on".to_owned()
    } else if v.kind == Kind::Task {
        task_because(v, now)
    } else if troubled(v) {
        trouble_because(v, now)
    } else if v.state == State::Booting {
        hint(v, snap, now).unwrap_or_else(|| "starting".to_owned())
    } else if awake_now(v) {
        awake_because(v, snap, now)
    } else if v.kind == Kind::Module {
        "wakes on a connection".to_owned()
    } else {
        "starts when needed".to_owned()
    }
}

fn task_because(v: &Instance, now: SystemTime) -> String {
    if v.state == State::Booting {
        return match v.started_at {
            Some(at) if now > at => {
                format!("running now · {}", ago_nanos(gotime::since(now, Some(at))))
            }
            _ => "running now".to_owned(),
        };
    }
    match &v.task {
        // The word already says "not run"; the because says what it needs.
        None if !v.needs.is_empty() => format!("needs {} · runs when you say", v.needs.join(", ")),
        None => "runs when you say".to_owned(),
        Some(t) if t.failed() => format!("exit {} · ran {} ago", t.exit_code, ran_ago(t, now)),
        Some(t) => format!(
            "ran {} ago in {}",
            ran_ago(t, now),
            took(t, 100 * MILLISECOND)
        ),
    }
}

fn trouble_because(v: &Instance, now: SystemTime) -> String {
    if v.restart_max == 0 {
        return first_line(&v.last_error).to_owned();
    }
    let mut out = format!("try {} of {}", v.restart_count, v.restart_max);
    let raw = gotime::until(v.next_restart, now);
    if gotime::round(raw, SECOND) > 0 {
        out.push_str(&format!(" · next in {}", crate::grain::seconds_left(raw)));
    }
    out
}

fn awake_because(v: &Instance, snap: &Snapshot, now: SystemTime) -> String {
    let mut by: Vec<String> = v
        .holders
        .iter()
        .filter_map(|h| match h {
            Holder::Dependant(name) => Some(format!("{name} needs it")),
            Holder::Connections(n) => Some(format!("{n} connection{}", plural(u64::from(*n)))),
            Holder::Pin => Some("pinned awake".to_owned()),
            Holder::Other => None,
        })
        .collect();
    // what declares it a dependency; a dozing instance is held by nothing, whatever declared it
    if v.state != State::Idle {
        for o in &snap.instances {
            for n in &o.needs {
                if *n == v.name
                    && awake_now(o)
                    && !by.join(" ").contains(&format!("{} needs", o.name))
                {
                    by.push(format!("{} needs it", o.name));
                }
            }
        }
    }
    if !by.is_empty() {
        by.join(" · ")
    } else if v.always_on {
        "runs until stopped".to_owned()
    } else if v.state == State::Idle {
        hint(v, snap, now).unwrap_or_else(|| "quiet; sleeps soon".to_owned())
    } else {
        "no connection right now".to_owned()
    }
}

/// The part of what a row says that is about its moment: that it is pinned, waits on something, sleeps soon, or
/// who woke it; `None` when there is nothing of the kind. (Go: `Hint`.)
#[must_use]
pub fn hint(v: &Instance, snap: &Snapshot, now: SystemTime) -> Option<String> {
    if v.holders.contains(&Holder::Pin) {
        return Some("pinned awake".to_owned());
    }
    match v.state {
        State::Booting => first_unmet(v, snap)
            .map(|n| format!("waiting on {n}"))
            .or_else(|| holder_named(v).map(|by| format!("woken by {by}"))),
        State::Idle if v.idle_since.is_some() => {
            sleeps_in(v, snap, now).map(|left| format!("sleeps in {}", countdown_nanos(left)))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;

    // Go: TestAlwaysOnRowsSayTheyRunUntilStopped. An instance that never sleeps says so, and is never counted
    // down to sleep; one that sleeps and has nothing holding it says that instead.
    #[test]
    fn always_on_rows_say_they_run_until_stopped() {
        let epoch = UNIX_EPOCH + Duration::from_secs(1_791_727_480); // 2026-10-11 14:04:40 UTC
        let snap = Snapshot {
            idle_timeout: Duration::from_secs(300),
            instances: Vec::new(),
        };
        let mut idle = Instance {
            name: "worker".into(),
            kind: Kind::App,
            state: State::Idle,
            idle_since: Some(epoch - Duration::from_secs(1)),
            always_on: true,
            ..Instance::default()
        };
        assert_eq!(because(&idle, &snap, epoch), "runs until stopped");
        idle.always_on = false;
        idle.endpoint = "127.0.0.1:3000".into();
        assert!(because(&idle, &snap, epoch).starts_with("sleeps in"));
        let active = Instance {
            name: "api".into(),
            kind: Kind::App,
            state: State::Active,
            ..Instance::default()
        };
        assert_eq!(because(&active, &snap, epoch), "no connection right now");
    }
}
