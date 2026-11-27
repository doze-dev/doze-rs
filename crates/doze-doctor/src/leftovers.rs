//! What doze left behind on the machine that no running doze program owns: names held for processes that are gone,
//! and routes recorded for a project in the system's temporary folder (a test's or a fixture's run against this home).
//! Stale routes are the domains' report ([`crate::domains`]), since `doze trust` removes them in its own prompt. The
//! authority is not a leftover: it serves every `.doze` name over https, domain or not.
//!
//! None of these is failing: the next doze program to start clears dead names, and a temporary project's route is
//! said with the command that takes it away. They are said so a person reading doctor knows what is on the machine.

use crate::{Check, State};
use doze_names::routes::Routes;
use std::path::Path;

/// The leftovers under `home`.
pub fn checks(home: &Path) -> Vec<Check> {
    let mut out = Vec::new();
    let dead = dead_names(home);
    if !dead.is_empty() {
        let n = dead.len();
        out.push(Check::new(
            "leftovers: names",
            State::Warn,
            format!(
                "{n} name{} held for processes that are gone ({}); the next doze program to start clears them",
                if n == 1 { "" } else { "s" },
                dead.join(", ")
            ),
        ));
    }
    for (domain, dir) in Routes::open(home).temporary() {
        out.push(Check::new(
            format!("leftovers: route {domain}"),
            State::Warn,
            format!(
                "recorded for {dir}, a project in the temporary folder (a test or a fixture run against this home); \
                 cd {dir} && doze trust --remove takes it away"
            ),
        ));
    }
    out
}

/// The names whose holder's process is gone, sorted.
pub fn dead_names(home: &Path) -> Vec<String> {
    let file = home.join(doze_names::registry::FILE_NAME);
    let Ok(raw) = std::fs::read(&file) else {
        return Vec::new();
    };
    let Ok(entries) =
        serde_json::from_slice::<std::collections::BTreeMap<String, doze_names::Entry>>(&raw)
    else {
        return Vec::new();
    };
    entries
        .into_iter()
        // A parked name (a stopped stack's, kept resolving until it is back) is held by nobody on purpose.
        .filter(|(_, e)| !e.is_parked() && !u32::try_from(e.pid).is_ok_and(doze_os::process::alive))
        .map(|(k, _)| k)
        .collect()
}
