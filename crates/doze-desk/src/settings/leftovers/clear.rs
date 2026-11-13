//! Clearing what was left behind (Go `ClearLeftover`): one by its id, or every one that is not a deletion of data.
//! Each looks again first, against a fresh look at the machine: only what the list named, and only while it still is
//! a leftover, is stopped, deleted or removed. A process is stopped by its exact pid; a folder is deleted only under
//! `<home>/projects` and only without a project record; routes are removed together, behind one password.

use std::path::Path;

use super::{Leftover, Machine, find, forget_routes, route_script};
use crate::settings::repair::{Fail, Hands};

/// What a clear came to: what to say, and whether it is bad news.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cleared {
    pub said: String,
    pub bad: bool,
}

impl Cleared {
    fn ok(said: impl Into<String>) -> Cleared {
        Cleared {
            said: said.into(),
            bad: false,
        }
    }

    fn bad(said: impl Into<String>) -> Cleared {
        Cleared {
            said: said.into(),
            bad: true,
        }
    }
}

/// Clears the leftover `id`, given a fresh look at the machine.
pub fn clear(id: &str, m: &Machine, hands: &mut dyn Hands) -> Cleared {
    let items = find(m);
    let Some(l) = items.iter().find(|l| l.id == id) else {
        return Cleared::bad(format!(
            "{} is no longer left behind; nothing was done",
            id.split_once(':').map_or(id, |(_, a)| a)
        ));
    };
    match one(l, m, hands) {
        Ok(said) => Cleared::ok(said),
        Err(Fail::Cancelled) => Cleared::ok("cancelled; nothing was removed"),
        Err(Fail::Failed(why)) => Cleared::bad(why),
    }
}

/// Clears every leftover that is not data: processes one by one, routes in one prompt, then the authority.
pub fn clear_all(m: &Machine, hands: &mut dyn Hands) -> Cleared {
    let items = find(m);
    let mut failed = Vec::new();
    let routes: Vec<String> = items
        .iter()
        .filter(|l| l.kind == "route")
        .map(|l| l.id.trim_start_matches("route:").to_owned())
        .collect();
    for l in items.iter().filter(|l| !l.danger && l.kind != "route") {
        match one(l, m, hands) {
            Ok(_) => {}
            Err(Fail::Cancelled) => failed.push(format!("{}: cancelled", l.what)),
            Err(Fail::Failed(why)) => failed.push(format!("{}: {why}", l.what)),
        }
    }
    if !routes.is_empty()
        && let Err(e) = clear_routes(m, &routes, hands)
    {
        failed.push(format!(
            "routes: {}",
            match e {
                Fail::Cancelled => "cancelled".into(),
                Fail::Failed(why) => why,
            }
        ));
    }
    if failed.is_empty() {
        Cleared::ok("cleaned up what was left behind")
    } else {
        Cleared::bad(failed.join("; "))
    }
}

fn one(l: &Leftover, m: &Machine, hands: &mut dyn Hands) -> Result<String, Fail> {
    let (kind, arg) = l.id.split_once(':').unwrap_or((&l.id, ""));
    match kind {
        "pid" => stop(arg, hands),
        "dir" => delete(Path::new(arg), m, hands),
        "route" => clear_routes(m, &[arg.to_owned()], hands)
            .map(|()| format!("removed the route for {arg}")),
        "ca" => clear_authority(m, hands),
        _ => Err(Fail::Failed(format!("no leftover called “{}”", l.id))),
    }
}

/// Asks the process to stop, waits up to five seconds, then stops it at once.
fn stop(arg: &str, hands: &mut dyn Hands) -> Result<String, Fail> {
    let pid: u32 = arg
        .parse()
        .map_err(|_| Fail::Failed(format!("“{arg}” is not a process id")))?;
    hands.terminate(pid)?;
    for _ in 0..50 {
        if !hands.alive(pid) {
            break;
        }
        hands.pause();
    }
    if hands.alive(pid) {
        hands.kill(pid)?;
    }
    Ok(format!("stopped pid {pid}"))
}

/// Deletes a state folder: only under `<home>/projects`, only without a project record.
fn delete(dir: &Path, m: &Machine, hands: &mut dyn Hands) -> Result<String, Fail> {
    let projects = format!("{}/", m.home.join("projects").to_string_lossy());
    let canon = super::gather::canon(dir);
    if !canon.starts_with(&projects) || dir.components().any(|c| c.as_os_str() == "..") {
        return Err(Fail::Failed(format!(
            "{} is not a doze state folder",
            dir.display()
        )));
    }
    if dir.join("project.json").is_file() {
        return Err(Fail::Failed(format!(
            "{} belongs to a listed stack; nothing was deleted",
            dir.display()
        )));
    }
    hands.remove_dir(dir)?;
    Ok(format!("deleted {}", m.tilde(dir)))
}

/// Removes the routes' resolver files behind one password, then forgets them from the record.
fn clear_routes(m: &Machine, domains: &[String], hands: &mut dyn Hands) -> Result<(), Fail> {
    if !m.admin {
        return Err(Fail::Failed(NO_PROMPT.into()));
    }
    let script = route_script(domains);
    if script.is_empty() {
        return Err(Fail::Failed("no valid domain to remove".into()));
    }
    let what = if domains.len() == 1 {
        "remove a stale route"
    } else {
        "remove stale routes"
    };
    hands.admin(what, &script)?;
    forget_routes(&m.home, domains).map_err(|e| Fail::Failed(e.to_string()))
}

/// Stops trusting doze's authority, when this Mac does, and deletes its files.
fn clear_authority(m: &Machine, hands: &mut dyn Hands) -> Result<String, Fail> {
    let Some(ca) = &m.authority else {
        return Ok("doze's certificate authority is gone already".into());
    };
    if ca.used {
        return Err(Fail::Failed(
            "a stack declares a domain again; the authority stays".into(),
        ));
    }
    if ca.trusted {
        if !m.admin {
            return Err(Fail::Failed(NO_PROMPT.into()));
        }
        hands.admin(
            "stop trusting doze's certificate authority",
            &untrust_script(&ca.dir),
        )?;
    }
    hands.remove_dir(&ca.dir)?;
    Ok("removed doze's certificate authority".into())
}

/// Every doze root out of the System keychain (Go names `Untrust` with `UntrustAll`): its trust setting by file, then
/// each certificate named doze's, until none is left.
fn untrust_script(ca: &Path) -> String {
    let root = crate::settings::quote(&ca.join("root.crt").to_string_lossy());
    format!(
        "security remove-trusted-cert -d {root} 2>/dev/null || true\nwhile security delete-certificate -c 'doze local CA' /Library/Keychains/System.keychain >/dev/null 2>&1; do :; done"
    )
}

/// A machine leftover needs the administrator's password, and this face cannot ask for it (Go `errNoPrompt`).
pub(crate) const NO_PROMPT: &str = "this needs the administrator's password, which this app cannot ask for here: in a terminal, doze doctor --fix removes stale routes";
