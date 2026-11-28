//! Clearing what was left behind (Go `ClearLeftover`): one by its id, or every one that is not a deletion of data.
//! Each looks again first, against a fresh look at the machine: only what the list named, and only while it still is
//! a leftover, is stopped, deleted or removed. A process is stopped by its exact pid; a folder is deleted only under
//! `<home>/projects` and only without a project record; routes are removed together, behind one password.

use std::path::Path;

use super::{Leftover, Machine, authority, find, forget_routes, route_script};
use crate::settings::plan::Planned;
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
        let what = match id.split_once(':') {
            Some(("ca", _)) => "doze's certificate authority",
            Some((_, a)) if !a.is_empty() => a,
            _ => id,
        };
        return Cleared::bad(format!("{what} is no longer left behind; nothing was done"));
    };
    match one(l, m, hands) {
        Ok(said) => Cleared::ok(said),
        Err(Fail::Cancelled) => Cleared::ok("cancelled; nothing was removed"),
        // macOS's dialog closed partway: the person's choice, said with what stands.
        Err(Fail::Declined(why)) => Cleared::ok(why),
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
            Err(Fail::Failed(why) | Fail::Declined(why)) => {
                failed.push(format!("{}: {why}", l.what))
            }
        }
    }
    if !routes.is_empty()
        && let Err(e) = clear_routes(m, &routes, hands)
    {
        failed.push(format!(
            "routes: {}",
            match e {
                Fail::Cancelled => "cancelled".into(),
                Fail::Failed(why) | Fail::Declined(why) => why,
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
    let projects = std::path::PathBuf::from(super::gather::canon(&m.home.join("projects")));
    let canon = std::path::PathBuf::from(super::gather::canon(dir));
    if canon == projects
        || !canon.starts_with(&projects)
        || dir.components().any(|c| c.as_os_str() == "..")
    {
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
    if m.names_https {
        return Err(Fail::Failed(
            "the .doze names serve https from the authority; it stays".into(),
        ));
    }
    if ca.trusted {
        if !m.admin {
            return Err(Fail::Failed(NO_PROMPT.into()));
        }
        // Its trust first, as the person: a cancelled dialog stops here, with nothing deleted. With several roots the
        // dialog comes once for each, so one closed after the first allowed leaves that one untrusted.
        hands
            .as_user(UNTRUST_AS_YOU, &authority::untrust_as_user(&ca.dir))
            .map_err(|e| match e {
                Fail::Cancelled => Fail::Declined(UNTRUST_CANCELLED.into()),
                other => other,
            })?;
        hands.admin(UNTRUST, &authority::untrust_script(&ca.dir))?;
    }
    hands.remove_dir(&ca.dir)?;
    Ok("removed doze's certificate authority".into())
}

const UNTRUST: &str = "delete this home's roots from the System keychain";

/// A cancelled macOS dialog while the authority's trust was being removed (doze-names says the same for the CLI).
pub(crate) const UNTRUST_CANCELLED: &str = "cancelled in macOS's dialog: nothing was deleted, and each certificate \
     whose trust was not removed yet is still trusted";
const UNTRUST_AS_YOU: &str =
    "stop trusting doze's certificate authority (macOS asks in its own dialog, once for each root)";

/// A machine leftover needs the administrator's password, and this face cannot ask for it (Go `errNoPrompt`).
pub(crate) const NO_PROMPT: &str = "this needs the administrator's password, which this app cannot ask for here: in a terminal, doze doctor --fix removes stale routes";

/// What clearing `id` (or `all`) would do, step by step, doing nothing: shown before a password is asked for. It
/// follows [`clear`] and [`clear_all`]: processes one by one, the routes together behind one password, the authority.
pub fn plan(id: &str, m: &Machine) -> Vec<Planned> {
    let items = find(m);
    let chosen: Vec<&Leftover> = if id == "all" {
        items.iter().filter(|l| !l.danger).collect()
    } else {
        items.iter().filter(|l| l.id == id).collect()
    };
    let mut out = Vec::new();
    for l in chosen.iter().filter(|l| l.kind != "route") {
        let (kind, arg) = l.id.split_once(':').unwrap_or((&l.id, ""));
        match kind {
            "pid" => out.push(Planned::Stop {
                what: l.what.clone(),
                pid: arg.parse().unwrap_or_default(),
            }),
            "dir" => out.push(Planned::Delete {
                what: l.what.clone(),
                dir: Path::new(arg).to_path_buf(),
            }),
            "ca" => {
                if let Some(ca) = m.authority.as_ref() {
                    if ca.trusted {
                        out.push(Planned::AsYou {
                            what: UNTRUST_AS_YOU.into(),
                            script: authority::untrust_as_user(&ca.dir),
                        });
                        out.push(Planned::Admin {
                            what: UNTRUST.into(),
                            script: authority::untrust_script(&ca.dir),
                        });
                    }
                    out.push(Planned::Delete {
                        what: "the authority's files".into(),
                        dir: ca.dir.clone(),
                    });
                }
            }
            _ => {}
        }
    }
    let routes: Vec<String> = chosen
        .iter()
        .filter(|l| l.kind == "route")
        .map(|l| l.id.trim_start_matches("route:").to_owned())
        .collect();
    if !routes.is_empty() {
        out.push(Planned::Admin {
            what: if routes.len() == 1 {
                "remove a stale route".into()
            } else {
                "remove stale routes".into()
            },
            script: route_script(&routes),
        });
    }
    out
}

/// The leftovers (by id) clearing `id` (or `all`) takes: every one that is not data for `all`, else that one.
pub fn chosen(id: &str, m: &Machine) -> Vec<String> {
    find(m)
        .into_iter()
        .filter(|l| if id == "all" { !l.danger } else { l.id == id })
        .map(|l| l.id)
        .collect()
}

/// Clears exactly the leftovers `ids` names (what a plan showed), each only while it is a leftover still: processes
/// one by one, the routes together behind one password, the authority. Nothing found since is touched.
pub fn clear_these(ids: &[String], m: &Machine, hands: &mut dyn Hands) -> Cleared {
    if ids.is_empty() {
        return Cleared::ok("nothing was left behind to clear");
    }
    if let [one] = ids {
        return clear(one, m, hands);
    }
    let items: Vec<Leftover> = find(m)
        .into_iter()
        .filter(|l| ids.contains(&l.id) && !l.danger)
        .collect();
    let mut failed = Vec::new();
    for l in items.iter().filter(|l| l.kind != "route") {
        match one(l, m, hands) {
            Ok(_) => {}
            Err(Fail::Cancelled) => failed.push(format!("{}: cancelled", l.what)),
            Err(Fail::Failed(why) | Fail::Declined(why)) => {
                failed.push(format!("{}: {why}", l.what))
            }
        }
    }
    let routes: Vec<String> = items
        .iter()
        .filter(|l| l.kind == "route")
        .map(|l| l.id.trim_start_matches("route:").to_owned())
        .collect();
    if !routes.is_empty()
        && let Err(e) = clear_routes(m, &routes, hands)
    {
        failed.push(format!(
            "routes: {}",
            match e {
                Fail::Cancelled => "cancelled".into(),
                Fail::Failed(why) | Fail::Declined(why) => why,
            }
        ));
    }
    if failed.is_empty() {
        Cleared::ok("cleaned up what was left behind")
    } else {
        Cleared::bad(failed.join("; "))
    }
}
