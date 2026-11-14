//! The repair (`doze doctor --fix`, the app's Settings › Machine › Repair): whatever the checks found missing, the
//! zone's setup and the stack's domains, as **one** script under **one** prompt. Go asked once per part; a person asked
//! twice in a row learns to type the password without reading what it is for.
//!
//! The parts are the same plans `doze dns-setup` and `doze trust` run on their own, so the words, the script and the
//! report come from one place. The caller runs [`Repair::one`] its own way and then calls [`Repair::after`].

use crate::domains::{self, Authority, StackDomains};
use crate::{Machine, names};
use doze_names::Error;
use doze_names::routes::Routes;
use doze_names::setup::trust::{TrustRequest, trust_plan};
use doze_names::setup::{Plan, Shell};
use doze_names::trust_store::domain_routed;
use std::time::Duration;

/// Which part of the machine a part of the repair sets up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The `.doze` zone (what `doze dns-setup` does).
    Names,
    /// The stack's domains and the authority (what `doze trust` does).
    Domains,
}

/// One part of the repair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    pub kind: Kind,
    pub plan: Plan,
}

/// What the repair will do.
#[derive(Debug)]
pub struct Repair {
    pub parts: Vec<Part>,
    /// Routes the stack wants that another program on the machine owns: never touched, said.
    pub foreign: Vec<String>,
    request: Option<TrustRequest>,
}

/// The repair for what is missing on `m` for `stack`: nothing when everything is in place.
pub fn plan(m: &Machine, stack: Option<&StackDomains>) -> Result<Repair, Error> {
    let mut parts = Vec::new();
    if !m.zone_off
        && !names::set_up(m)
        && let Some(plan) = m.setup.install_plan()?
    {
        parts.push(Part {
            kind: Kind::Names,
            plan,
        });
    }
    let (mut foreign, mut request) = (Vec::new(), None);
    if let Some(stack) = stack.filter(|s| !s.domains.is_empty()) {
        let req = domains::request(m.home(), stack, Authority::probe(m.home()).ca);
        let (plan, theirs) = trust_plan(m.setup.os, &req);
        foreign = theirs;
        if let Some(plan) = plan {
            parts.push(Part {
                kind: Kind::Domains,
                plan,
            });
            request = Some(req);
        }
    }
    Ok(Repair {
        parts,
        foreign,
        request,
    })
}

impl Repair {
    /// Whether there is nothing to do.
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    /// Every part as one script under one prompt; `None` when there is nothing to do.
    pub fn one(&self) -> Option<Plan> {
        combine(self.parts.iter().map(|p| &p.plan))
    }

    /// What the person owns, put in step with what the script did: stale routes that are gone leave the record, and
    /// rotated-out authorities no longer trusted are forgotten. Then, for the zone, waits (up to `wait`) for the
    /// network daemon launchd starts a beat after the script returns. Says what is still missing, if anything.
    pub fn after(&mut self, m: &Machine, wait: Duration) -> Result<Vec<String>, Error> {
        if let Some(req) = &mut self.request {
            if let Some(home) = &req.home {
                let routes = Routes::open(home);
                for d in req.prune.iter().filter(|d| !domain_routed(d).0) {
                    routes.removed(d)?;
                }
            }
            if let Some(ca) = &mut req.ca {
                ca.forget_untrusted_previous()?;
            }
        }
        let start = std::time::Instant::now();
        let names = self.parts.iter().any(|p| p.kind == Kind::Names);
        while names && !names::set_up(m) && start.elapsed() < wait {
            std::thread::sleep(Duration::from_millis(150));
        }
        let mut missing = Vec::new();
        if names && !names::set_up(m) {
            missing.push(format!(
                "the .doze names are not in place: the network daemon did not come up (see {})",
                m.paths().helper_log.display()
            ));
        }
        Ok(missing)
    }
}

/// Plans of one shell as one: each part in its own subshell (sh) or script block (PowerShell), so one part's `set -e`
/// or variables do not leak into the next, and a failing part stops the run.
pub fn combine<'a>(plans: impl IntoIterator<Item = &'a Plan>) -> Option<Plan> {
    let plans: Vec<&Plan> = plans.into_iter().collect();
    let (first, rest) = plans.split_first()?;
    if rest.is_empty() {
        return Some((*first).clone());
    }
    let shell = first.shell;
    let what = plans
        .iter()
        .map(|p| p.what.trim_end_matches('.'))
        .collect::<Vec<_>>()
        .join("; and ");
    let mut script = String::new();
    match shell {
        Shell::Sh => {
            script.push_str("set -e\n");
            for p in &plans {
                script.push_str("(\n");
                script.push_str(p.script.trim_end_matches('\n'));
                script.push_str("\n)\n");
            }
        }
        Shell::PowerShell => {
            script.push_str("$ErrorActionPreference = 'Stop'\n");
            for p in &plans {
                script.push_str("& {\n");
                script.push_str(p.script.trim_end_matches('\n'));
                script.push_str("\n}\n");
            }
        }
    }
    Some(Plan {
        what,
        shell,
        script,
    })
}

#[cfg(test)]
#[path = "repair_tests.rs"]
mod tests;
