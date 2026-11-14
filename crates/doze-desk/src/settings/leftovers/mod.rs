//! Left behind (Go `serve/leftovers.go`): what stacks leave when they go out of step with doze. Engine processes
//! whose daemon died, daemons for stacks the Library does not list (another home, a deleted folder), state folders
//! with no stack, routes for domains no stack declares, and doze's certificate authority when nothing uses it. Each
//! has a way to clear it; nothing is cleared unasked.
//!
//! Looking only reads: `ps`, the home's folders, the routes record, `/etc/resolver`, the keychain's word on the
//! authority ([`gather`]). [`find`] is a pure function of what was read, so the tests feed it made-up machines.
//! Clearing ([`clear`]) goes through [`super::repair::Hands`] and looks again first.

mod clear;
mod gather;
mod routes;

use std::path::{Path, PathBuf};

use serde::Serialize;

pub use clear::{Cleared, chosen, clear, clear_all, clear_these, plan as plan_clear};
pub use gather::{gather, gather_with, parse_ps};
pub use routes::{forget_routes, parse_routes, read_routes, route_script, valid_domain};

/// One thing left behind (Go `view.Leftover`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Leftover {
    /// `daemon`, `engine`, `state`, `route`, `authority`.
    pub kind: &'static str,
    /// What [`clear`] takes: `pid:123`, `dir:/…`, `route:shop.test`, `ca:`.
    pub id: String,
    pub what: String,
    pub detail: String,
    /// Stop, Remove, Delete…
    pub action: String,
    /// A deletion of data: asked one by one, never cleared by Clean up.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub danger: bool,
    /// Removing it asks for the administrator's password.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub admin: bool,
}

impl Leftover {
    /// The heading of its kind in the list (Go `KINDS`).
    pub fn heading(kind: &str) -> &str {
        match kind {
            "daemon" => "daemons",
            "engine" => "engines",
            "state" => "data",
            "route" => "network routes",
            "authority" => "certificate authority",
            other => other,
        }
    }
}

/// One process, as `ps` lists it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    /// When it started: "Nov 6 20:00".
    pub since: String,
    /// The program, canonical.
    pub exe: String,
    /// The doze.hcl a `doze __daemon --config` process serves, canonical; and whether it is still there.
    pub daemon: String,
    pub config_there: bool,
}

/// A folder under `<home>/projects`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StateDir {
    pub dir: PathBuf,
    /// It has a project record: a stack the Library lists, missing files and all.
    pub listed: bool,
    /// A daemon holds it.
    pub running: bool,
    /// Its size on disk, in words.
    pub size: String,
}

/// doze's certificate authority, when its files are there.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Authority {
    pub dir: PathBuf,
    /// This machine trusts it.
    pub trusted: bool,
    /// Some stack still declares a domain.
    pub used: bool,
}

/// What a look at the machine read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Machine {
    pub home: PathBuf,
    /// The doze.hcl of every stack the Library lists, canonical.
    pub known: Vec<String>,
    pub procs: Vec<Proc>,
    pub dirs: Vec<StateDir>,
    /// Domains routed to doze that no stack declares.
    pub routes: Vec<String>,
    pub authority: Option<Authority>,
    /// This app can ask for the administrator's password.
    pub admin: bool,
    /// The person's home folder, for `~`.
    pub user_home: Option<String>,
}

impl Machine {
    fn tilde(&self, p: &Path) -> String {
        let s = p.to_string_lossy();
        doze_display::tilde(&s, self.user_home.as_deref()).into_owned()
    }
}

/// What the machine holds that no stack does (Go `Service.Leftovers`).
pub fn find(m: &Machine) -> Vec<Leftover> {
    let mut out = Vec::new();
    let modules = m.home.join("modules");
    for p in &m.procs {
        if !p.daemon.is_empty() && !m.known.contains(&p.daemon) {
            let why = if p.config_there {
                "for a stack Home does not list"
            } else {
                "for a doze.hcl that is gone"
            };
            out.push(Leftover {
                kind: "daemon",
                id: format!("pid:{}", p.pid),
                what: format!("a stack's daemon, {why}"),
                detail: format!(
                    "pid {} · {} · since {}",
                    p.pid,
                    m.tilde(Path::new(&p.daemon)),
                    p.since
                ),
                action: "Stop".into(),
                ..Leftover::default()
            });
        } else if p.ppid == 1 && (Path::new(&p.exe).starts_with(&modules) || is_plugin(&p.exe)) {
            let base = p.exe.rsplit(['/', '\\']).next().unwrap_or_default();
            out.push(Leftover {
                kind: "engine",
                id: format!("pid:{}", p.pid),
                what: format!("{base}, whose daemon is gone"),
                detail: format!("pid {} · since {}", p.pid, p.since),
                action: "Stop".into(),
                ..Leftover::default()
            });
        }
    }
    for d in m.dirs.iter().filter(|d| !d.listed && !d.running) {
        out.push(Leftover {
            kind: "state",
            id: format!("dir:{}", d.dir.to_string_lossy()),
            what: "data of a stack doze no longer lists".into(),
            detail: format!("{} · {}", m.tilde(&d.dir), d.size),
            action: "Delete…".into(),
            danger: true,
            ..Leftover::default()
        });
    }
    machine_parts(m, &mut out);
    out
}

/// Routes and the authority: the machine's, so removing them takes the administrator's password, asked once for all
/// the routes.
fn machine_parts(m: &Machine, out: &mut Vec<Leftover>) {
    let action = if m.admin {
        "Remove (one password)"
    } else {
        "Remove"
    };
    for d in &m.routes {
        out.push(Leftover {
            kind: "route",
            id: format!("route:{d}"),
            what: format!("a route sending {d} to doze"),
            detail: format!("/etc/resolver/{d} · no project with a doze.hcl declares it any more"),
            action: action.into(),
            admin: true,
            ..Leftover::default()
        });
    }
    if let Some(ca) = m.authority.as_ref().filter(|a| !a.used) {
        let mut l = Leftover {
            kind: "authority",
            id: "ca:".into(),
            what: "doze's certificate authority, which no stack uses now".into(),
            detail: format!("{} · not trusted; its files only", m.tilde(&ca.dir)),
            action: "Remove".into(),
            ..Leftover::default()
        };
        if ca.trusted {
            l.what = "doze's certificate authority, still trusted by this Mac".into();
            l.detail = format!("{} · no stack declares a domain any more", m.tilde(&ca.dir));
            l.action = action.into();
            l.admin = true;
        }
        out.push(l);
    }
}

/// A path is an engine plugin in some doze home's module cache (`<home>/modules/<ns>/<name>/<version>/bin/<name>-plugin`),
/// so an orphan from a test's or another home's daemon is found too (Go `isPlugin`).
pub fn is_plugin(exe: &str) -> bool {
    let parts: Vec<&str> = exe.split(['/', '\\']).collect();
    let n = parts.len();
    n >= 6
        && parts.get(n - 2) == Some(&"bin")
        && parts.get(n - 6) == Some(&"modules")
        && parts.get(n - 1).is_some_and(|b| b.ends_with("-plugin"))
}

/// How many there are: those Clean up clears, and the data folders, deleted one at a time.
pub fn counts(items: &[Leftover]) -> (usize, usize) {
    let data = items.iter().filter(|l| l.danger).count();
    (items.len() - data, data)
}

/// The list's line under its heading (Go `Leftovers`).
pub fn summary(items: &[Leftover]) -> String {
    match items.len() {
        0 => "nothing: every process, folder and route doze sees belongs to a stack on Home".into(),
        1 => "1 thing stacks left behind, out of step with doze".into(),
        n => format!("{n} things stacks left behind, out of step with doze"),
    }
}

/// The kinds present, in the order they were found.
pub fn kinds(items: &[Leftover]) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for l in items {
        if !out.contains(&l.kind) {
            out.push(l.kind);
        }
    }
    out
}

/// How many of a kind show before "and N more like these · show all".
pub const SHOWN: usize = 6;

/// A folder's size on disk, in words (Go `du`).
pub fn size_words(n: u64) -> String {
    const KB: u64 = 1 << 10;
    const MB: u64 = 1 << 20;
    const GB: u64 = 1 << 30;
    #[allow(clippy::cast_precision_loss)] // WHY: a size in words, one decimal
    match n {
        n if n >= GB => format!("{:.1} GB", n as f64 / GB as f64),
        n if n >= MB => format!("{} MB", n >> 20),
        n if n >= KB => format!("{} KB", n >> 10),
        n => format!("{n} bytes"),
    }
}

#[cfg(test)]
mod tests;
