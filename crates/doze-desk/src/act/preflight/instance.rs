//! The look before one service of a running stack starts, turned on or woken (Go `PreflightInstance`): the port it
//! declares, and who holds it. Nothing to say for one already awake, or a task.

use std::path::Path;

use super::{Fix, FixKind, Looked, PreCheck, Preflight, bad, decl_port, edit};
use crate::overview::Shape;

/// The checks before `name` of the running stack in `dir` (read as `shape`) starts; `awake` says it already is.
pub fn preflight_instance(
    name: &str,
    awake: bool,
    task: bool,
    dir: &Path,
    shape: &Shape,
    looked: &Looked,
) -> Preflight {
    let mut p = Preflight {
        stack: name.to_owned(),
        ..Preflight::default()
    };
    if awake || task || shape.error.is_some() {
        return p;
    }
    let Some(port) = shape
        .decls
        .iter()
        .find(|d| d.name == name)
        .and_then(decl_port)
    else {
        return p;
    };
    let conf = dir.join("doze.hcl");
    let crash = format!("{name} would crash on start.");
    let check: PreCheck = match looked.held.get(&port) {
        Some(h) if !h.doze || !h.stack.is_empty() => {
            if h.stack.is_empty() {
                let mut c = bad(
                    format!("Port {port} for {name} is taken"),
                    format!("{crash} doze never stops a process it did not start without asking."),
                    vec![
                        Fix {
                            label: format!("Stop pid {}, then start {name}", h.pid),
                            kind: FixKind::Stop(h.pid),
                            primary: true,
                        },
                        edit(&conf),
                    ],
                );
                (c.detail, c.mono) = (format!("{} (pid {})", h.command, h.pid), true);
                c
            } else {
                bad(
                    format!(
                        "Port {port} for {name} is used by {} in {}",
                        h.service, h.stack
                    ),
                    format!(
                        "Two services cannot both have it. Stop {} first.",
                        h.service
                    ),
                    vec![edit(&conf)],
                )
            }
        }
        None if looked.unbindable.contains(&port) => {
            let mut c = bad(
                format!("Port {port} for {name} is taken"),
                crash,
                vec![edit(&conf)],
            );
            c.detail = "by a process of another user, which doze cannot see or stop".into();
            c
        }
        _ => return p,
    };
    p.blocked = true;
    p.checks.push(check);
    p
}
