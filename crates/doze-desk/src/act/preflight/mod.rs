//! The checks before Up (Go `serve/preflight.go`, the PreFlight board): a stopped stack's folder and doze.hcl, each
//! port the starting set declares and who holds it, its domains' setup. Nothing is started to find out. `blocked` says
//! something red is in the way and the sheet asks; with nothing red, Up just goes. Before one service of a running
//! stack wakes, the same for its port ([`instance`]).
//!
//! What is looked at (who listens, which ports this user can bind, which domains are set up) is gathered first
//! ([`Looked`]), so the checks themselves are a plain function the tests can feed.

mod instance;
mod ports;

pub use instance::preflight_instance;
pub use ports::{bindable, listening, parse_lsof_all, parse_ss_all};

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use doze_display as words;

use super::dialog::and_words;
use super::starting::{Starting, starting_set};
use crate::overview::{Decl, Shape, config_error};
use crate::ports::Holder;

/// What the checks found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Preflight {
    /// The stack's name, or the service's for one service.
    pub stack: String,
    pub blocked: bool,
    pub checks: Vec<PreCheck>,
}

/// How a check reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tone {
    #[default]
    Ok,
    /// It starts anyway.
    Warn,
    /// In the way.
    Bad,
}

/// One line: what was looked at, what was found, the ways out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PreCheck {
    pub tone: Tone,
    pub title: String,
    pub detail: String,
    /// The detail is a command line: drawn in mono.
    pub mono: bool,
    /// A second line, in plain words.
    pub note: String,
    pub fixes: Vec<Fix>,
}

/// A way out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixKind {
    /// Stop the process (by pid, checked again first), then start.
    Stop(i64),
    /// Up without this service.
    Without(String),
    /// Open this file in the editor.
    Edit(String),
    /// Set the machine's names up.
    Repair,
    /// Remove the stack from doze (its folder is gone).
    Remove,
}

/// A way out, and how its button reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    pub label: String,
    pub kind: FixKind,
    pub primary: bool,
}

/// What was looked at before the checks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Looked {
    /// Who listens on each port, each marked when it is one of doze's own ([`crate::ports::owned_by`]).
    pub held: HashMap<u16, Holder>,
    /// Ports nothing visible holds that this user still cannot bind (another user's).
    pub unbindable: BTreeSet<u16>,
    /// The domains not set up on this machine.
    pub untrusted: Vec<String>,
    /// The app can ask for the administrator's password (Go `s.admin != nil`): the domains' check offers Set up.
    pub admin: bool,
}

impl Looked {
    /// Looks: one `lsof`, a bind probe for each of `ports` that nothing visible holds, and the holders marked by
    /// `stacks` (stack, service, pid) as Go's `ownedBy` does.
    pub fn look<'a>(
        ports: impl IntoIterator<Item = u16>,
        stacks: &[(&'a str, &'a str, i64)],
        untrusted: Vec<String>,
    ) -> Looked {
        let mut held = listening();
        let mut unbindable = BTreeSet::new();
        for port in ports {
            match held.get_mut(&port) {
                Some(h) if !h.doze => crate::ports::owned_by(h, stacks.iter().copied()),
                Some(_) => {}
                None if !bindable(port) => {
                    unbindable.insert(port);
                }
                None => {}
            }
        }
        Looked {
            held,
            unbindable,
            untrusted,
            admin: false,
        }
    }
}

/// The port a declared instance asks for: its own `port`, or for a module the one its module gives it (the address the
/// CLI says it listens at), as Go's loader fills it in.
pub fn decl_port(d: &Decl) -> Option<u16> {
    let own = d.port.and_then(|p| u16::try_from(p).ok());
    let module = || {
        (d.kind == "module")
            .then(|| d.endpoint.rsplit_once(':'))
            .flatten()
            .and_then(|(_, p)| p.parse().ok())
    };
    own.or_else(module).filter(|p| *p > 0)
}

/// What a stopped stack's folder is missing: `the folder is gone`, `doze.hcl is gone`.
pub fn missing_files(dir: &Path) -> Option<&'static str> {
    if !dir.is_dir() {
        Some("the folder is gone")
    } else if !dir.join("doze.hcl").is_file() {
        Some("doze.hcl is gone")
    } else {
        None
    }
}

fn bad(title: String, note: String, fixes: Vec<Fix>) -> PreCheck {
    PreCheck {
        tone: Tone::Bad,
        title,
        note,
        fixes,
        ..PreCheck::default()
    }
}

fn edit(path: &Path) -> Fix {
    let name = path
        .file_name()
        .map_or_else(|| "doze.hcl".into(), |n| n.to_string_lossy().into_owned());
    Fix {
        label: format!("Edit {name}"),
        kind: FixKind::Edit(path.display().to_string()),
        primary: false,
    }
}

/// The checks before `action` (`up-last`, `up-all`, `up` with `chosen`) starts the stopped stack `name` in `dir`,
/// read as `shape`, last up with `last`.
pub fn preflight(
    name: &str,
    dir: &Path,
    shape: &Shape,
    last: &[String],
    (action, chosen): (&str, &[String]),
    looked: &Looked,
) -> Preflight {
    let mut p = Preflight {
        stack: name.to_owned(),
        ..Preflight::default()
    };
    let shown = words::tilde(&dir.display().to_string(), words::home().as_deref()).into_owned();
    if let Some(m) = missing_files(dir) {
        p.blocked = true;
        let mut c = bad(
            format!("{name}'s {}", m.trim_start_matches("the ")),
            String::new(),
            vec![Fix {
                label: "Remove…".into(),
                kind: FixKind::Remove,
                primary: false,
            }],
        );
        c.detail = format!("{shown} was moved or deleted; nothing can start from it");
        p.checks.push(c);
        return p;
    }
    if let Some(msg) = &shape.error {
        p.blocked = true;
        let c = config_error(
            &dir.join("doze.hcl"),
            msg,
            None,
            std::time::SystemTime::now(),
        );
        let mut check = bad(
            "doze.hcl has a mistake".into(),
            c.hint.clone(),
            vec![edit(Path::new(&c.file))],
        );
        check.detail = c.problem();
        p.checks.push(check);
        return p;
    }
    let set = starting_set(&shape.decls, last, action, chosen);
    let and_need = if set.deps.is_empty() {
        ""
    } else {
        ", and what they need"
    };
    let names: Vec<&str> = set.names.iter().map(String::as_str).collect();
    p.checks.push(PreCheck {
        title: "The folder is there, and doze.hcl loads".into(),
        detail: format!("{shown} · starts {}{and_need}", nothing_or(&names)),
        ..PreCheck::default()
    });
    let free = ports_checks(&mut p, &set, dir, looked);
    domains_check(&mut p, name, &set, looked);
    if let Some(c) = free_check(free) {
        p.checks.push(c);
    }
    p
}

fn nothing_or(names: &[&str]) -> String {
    if names.is_empty() {
        "nothing".into()
    } else {
        and_words(names)
    }
}

/// A check for each declared port in the way; the free ones, to say so.
fn ports_checks(p: &mut Preflight, set: &Starting<'_>, dir: &Path, looked: &Looked) -> Vec<u16> {
    let mut free = Vec::new();
    let conf = dir.join("doze.hcl");
    for d in &set.decls {
        let Some(port) = decl_port(d) else { continue };
        let n = d.name.as_str();
        let without = |w: &mut Vec<Fix>| {
            if !set.needed.contains(n) && set.names.len() > 1 {
                w.push(Fix {
                    label: format!("Up without {n}"),
                    kind: FixKind::Without(n.to_owned()),
                    primary: false,
                });
            }
            w.push(edit(&conf));
        };
        let mut fixes = Vec::new();
        match looked.held.get(&port) {
            None if looked.unbindable.contains(&port) => {
                without(&mut fixes);
                let mut c = bad(
                    format!("Port {port} for {n} is taken"),
                    format!("{n} would crash on start."),
                    fixes,
                );
                c.detail = "by a process of another user, which doze cannot see or stop".into();
                p.checks.push(c);
            }
            None => free.push(port),
            Some(h) if h.doze && h.stack.is_empty() => free.push(port),
            Some(h) if !h.stack.is_empty() => {
                without(&mut fixes);
                let note = format!(
                    "Two stacks cannot both have it. Stop {} first, or start without {n}.",
                    h.stack
                );
                p.checks.push(bad(
                    format!(
                        "Port {port} for {n} is used by {} in {}",
                        h.service, h.stack
                    ),
                    note,
                    fixes,
                ));
            }
            Some(h) => {
                fixes.push(Fix {
                    label: format!("Stop pid {}, then Up", h.pid),
                    kind: FixKind::Stop(h.pid),
                    primary: true,
                });
                without(&mut fixes);
                let note = format!(
                    "{n} would crash on start. doze never stops a process it did not start without asking."
                );
                let mut c = bad(format!("Port {port} for {n} is taken"), note, fixes);
                (c.detail, c.mono) = (format!("{} (pid {})", h.command, h.pid), true);
                p.checks.push(c);
            }
        }
    }
    p.blocked |= p.checks.iter().any(|c| c.tone == Tone::Bad);
    free
}

fn domains_check(p: &mut Preflight, name: &str, set: &Starting<'_>, looked: &Looked) {
    let domains: Vec<&String> = set.decls.iter().flat_map(|d| &d.domains).collect();
    let Some(first) = domains.first() else { return };
    if !domains.iter().any(|d| looked.untrusted.contains(d)) {
        return;
    }
    p.checks.push(PreCheck {
        tone: Tone::Warn,
        title: format!("{first} is not set up on this Mac"),
        detail: format!(
            "{name} starts anyway; its .doze names answer until it is. This waits in Needs you."
        ),
        // Go offers Set up only where it can ask for the password; elsewhere the check stands alone (Needs you has
        // the command).
        fixes: if looked.admin {
            vec![Fix {
                label: "Set up (one password)".into(),
                kind: FixKind::Repair,
                primary: false,
            }]
        } else {
            Vec::new()
        },
        ..PreCheck::default()
    });
}

/// "Ports 3000 and 8080 are free", in Go's order (the numbers sorted as text).
fn free_check(free: Vec<u16>) -> Option<PreCheck> {
    let mut words: Vec<String> = free.iter().map(u16::to_string).collect();
    words.sort();
    words.dedup();
    let title = match words.as_slice() {
        [] => return None,
        [one] => format!("Port {one} is free"),
        many => {
            let refs: Vec<&str> = many.iter().map(String::as_str).collect();
            format!("Ports {} are free", and_words(&refs))
        }
    };
    Some(PreCheck {
        title,
        ..PreCheck::default()
    })
}

#[cfg(test)]
mod tests;
