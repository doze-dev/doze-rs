//! What doze left behind on the machine that no running doze program owns: names held for processes that are gone,
//! and an authority kept although no stack declares a domain. Stale routes are the domains' report
//! ([`crate::domains`]), since `doze trust` removes them in its own prompt.
//!
//! None of these is failing: the next doze program to start clears dead names, and an unused authority is trusted by
//! nothing that asks for it. They are said so a person reading doctor knows what is on the machine.

use crate::{Check, State};
use doze_names::ca::ca_dir;
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
    if ca_dir(home).join("root.crt").is_file() && !Routes::open(home).any_declared() {
        out.push(Check::new(
            "leftovers: certificate authority",
            State::Warn,
            format!(
                "{} is kept although no stack declares a domain; doze trust --remove in the last such stack takes it away",
                ca_dir(home).display()
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
        .filter(|(_, e)| !u32::try_from(e.pid).is_ok_and(doze_os::process::alive))
        .map(|(k, _)| k)
        .collect()
}
