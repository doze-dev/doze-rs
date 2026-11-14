//! `--dry-run` for the verbs (proposal, "`--dry-run`, command by command"): read the daemon's snapshot, plan, ask
//! nothing. `wake` and `restart` list what would start, dependencies first, in the order the daemon starts them;
//! `sleep`, `off`, `on` and `pin` each service and the state it would move to; `reset` each data folder and its
//! size; `task` the task and what it would wake first.

use crate::cmd::pin::Want;
use crate::cmd::status::awake;
use crate::plan::{Op, Plan};
use doze_control::{InstanceView, Snapshot};
use std::path::Path;

fn find<'a>(snap: &'a Snapshot, name: &str) -> Option<&'a InstanceView> {
    snap.instances.iter().find(|v| v.name == name)
}

/// `name` and everything it needs, needs first (depth first, each once), as the daemon wakes them. A cycle (which the
/// daemon refuses) is cut where it closes.
fn order<'a>(
    snap: &'a Snapshot,
    name: &'a str,
    visiting: &mut Vec<&'a str>,
    out: &mut Vec<&'a str>,
) {
    if out.contains(&name) || visiting.contains(&name) {
        return;
    }
    visiting.push(name);
    if let Some(v) = find(snap, name) {
        for n in &v.needs {
            order(snap, n, visiting, out);
        }
    }
    out.push(name);
}

/// What a wake (or a task, or a restart's dependencies) would start, in order.
fn wake_order<'a>(snap: &'a Snapshot, targets: &'a [String]) -> Vec<&'a str> {
    let (mut visiting, mut out) = (Vec::new(), Vec::new());
    for t in targets {
        order(snap, t, &mut visiting, &mut out);
    }
    out
}

/// Why `name` would wake: asked for, or needed by the one after it.
fn reason(snap: &Snapshot, name: &str, asked: &[String]) -> String {
    if asked.iter().any(|a| a == name) {
        return "asked for".into();
    }
    let by: Vec<&str> = snap
        .instances
        .iter()
        .filter(|v| v.needs.iter().any(|n| n == name))
        .map(|v| v.name.as_str())
        .collect();
    format!("{} needs it", by.join(", "))
}

/// `doze wake --dry-run`.
pub(crate) fn wake(snap: &Snapshot, targets: &[String], command: &str, named: &[String]) -> Plan {
    let mut plan = Plan::new(applies(command, named, &[]));
    for name in wake_order(snap, targets) {
        match find(snap, name) {
            Some(v) if awake(&v.state) => plan.unchanged(name),
            _ => plan.step(name, Op::Do, "wake", reason(snap, name, targets)),
        }
    }
    plan
}

/// `doze restart --dry-run`: what it needs and is asleep wakes first; then each stops and starts.
pub(crate) fn restart(snap: &Snapshot, names: &[String]) -> Plan {
    let mut plan = Plan::new(applies("doze restart", names, &[]));
    for name in wake_order(snap, names) {
        let v = find(snap, name);
        let up = v.is_some_and(|v| awake(&v.state));
        if names.iter().any(|n| n == name) {
            let how = if up {
                "stop it, then start it".to_owned()
            } else {
                let word = v.map_or_else(
                    || "asleep".to_owned(),
                    |v| doze_display::State::parse(&v.state).word().to_owned(),
                );
                format!("start it (it is {word})")
            };
            plan.step(name, Op::Do, "restart", how);
        } else if !up {
            plan.step(name, Op::Do, "wake", reason(snap, name, names));
        }
    }
    plan
}

/// `doze sleep --dry-run`: with no name, everything awake.
pub(crate) fn sleep(snap: &Snapshot, names: &[String]) -> Plan {
    let mut plan = Plan::new(applies("doze sleep", names, &[]));
    let all: Vec<&InstanceView> = snap.instances.iter().filter(|v| v.declared).collect();
    for v in all
        .iter()
        .filter(|v| names.is_empty() || names.contains(&v.name))
    {
        if awake(&v.state) {
            plan.step(
                &v.name,
                Op::Do,
                "sleep",
                "its data stays; it wakes on its next connection",
            );
        } else if !names.is_empty() {
            plan.unchanged(&v.name);
        }
    }
    if plan.groups.is_empty() {
        plan.unchanged("the stack");
    }
    plan
}

/// `doze off --dry-run` and `doze on --dry-run`.
pub(crate) fn on_off(snap: &Snapshot, names: &[String], off: bool) -> Plan {
    let command = if off { "doze off" } else { "doze on" };
    let mut plan = Plan::new(applies(command, names, &[]));
    for name in names {
        let is_off = find(snap, name).is_some_and(|v| v.disabled && !v.paused);
        match (off, is_off) {
            (true, true) | (false, false) => plan.unchanged(name),
            (true, false) => plan.step(
                name,
                Op::Do,
                "off",
                "stop it; nothing wakes it until doze on",
            ),
            (false, true) => plan.step(
                name,
                Op::Do,
                "on",
                "back in the run; it wakes on its next connection",
            ),
        }
    }
    plan
}

/// `doze pin --dry-run`.
pub(crate) fn pin(snap: &Snapshot, names: &[String], want: Want) -> Plan {
    let flag: &[&str] = match want {
        Want::On => &[],
        Want::Off => &["--off"],
        Want::Toggle => &["--toggle"],
    };
    let mut plan = Plan::new(applies("doze pin", names, flag));
    for name in names {
        let now = find(snap, name).is_some_and(|v| v.keep_awake);
        if !want.flips(now) {
            plan.unchanged(name);
        } else if now {
            plan.step(name, Op::Change, "unpin", "pinned → sleeps again when idle");
        } else {
            plan.step(
                name,
                Op::Change,
                "pin",
                "stays awake until unpinned or the daemon stops",
            );
        }
    }
    plan
}

/// `doze reset --dry-run`: each data folder and what is in it.
pub(crate) fn reset(snap: &Snapshot, names: &[String]) -> Plan {
    let mut plan = Plan::new(applies("doze reset -y", names, &[]));
    for name in names {
        let dir = find(snap, name)
            .map(|v| v.data_dir.as_str())
            .unwrap_or_default();
        if dir.is_empty() {
            plan.step(
                name,
                Op::Remove,
                "its data",
                "provisioned fresh on its next wake",
            );
        } else {
            let size = crate::cmd::status::human_bytes(
                i64::try_from(size_of(Path::new(dir))).unwrap_or(i64::MAX),
            );
            let size = if size.is_empty() {
                "empty".to_owned()
            } else {
                size
            };
            plan.step(
                name,
                Op::Remove,
                dir,
                format!("{size}; provisioned fresh on its next wake"),
            );
        }
    }
    plan
}

/// `doze task <name> --dry-run`: what it would wake first, then the run.
pub(crate) fn task(snap: &Snapshot, name: &str) -> Plan {
    let mut plan = Plan::new(format!("doze task {name}"));
    let target = [name.to_owned()];
    for n in wake_order(snap, &target) {
        if n == name {
            plan.step(
                name,
                Op::Do,
                "run",
                "now, whatever its state; the task does not run in a dry run",
            );
        } else if !find(snap, n).is_some_and(|v| awake(&v.state)) {
            plan.step(n, Op::Do, "wake", reason(snap, n, &target));
        }
    }
    plan
}

/// The command that does it for real.
fn applies(command: &str, names: &[String], flags: &[&str]) -> String {
    let mut s = command.to_owned();
    for f in flags {
        s.push(' ');
        s.push_str(f);
    }
    for n in names {
        s.push(' ');
        s.push_str(n);
    }
    s
}

/// The bytes under `dir` (files only; links are not followed).
fn size_of(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => size_of(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}
