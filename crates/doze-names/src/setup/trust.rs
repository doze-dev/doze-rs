//! Trusting a domain: the machine is told to ask the doze resolver for it (a route per domain), and the doze CA is
//! trusted so https from the front door is believed. Both in one privileged script under one prompt, as the zone's
//! setup is; both undone by [`untrust`]. Opt-in: nothing here runs for a machine that declared no domain, and the CA
//! is trusted only when a domain asked for https.
//!
//! The prompt says what the script does, and the script does only that: on macOS and Windows the routes written are the
//! request's own and the ones removed are the stale ones; on Linux the link's domain list is one setting, so every
//! change restates every route that should exist, and the prompt says so.

use super::run::run;
use super::trust_scripts as scripts;
use super::{How, Options, Os, Plan, Shell, Step};
use crate::Error;
use crate::ca::{Ca, ca_dir};
use crate::domains::route_domains;
use crate::routes::Routes;
use crate::trust_store::{ca_trusted, domain_routed, fingerprint_trusted, route_foreign};
use std::path::{Path, PathBuf};

/// What to set up, or take away.
#[derive(Debug, Default)]
pub struct TrustRequest {
    /// The declared names; wildcards fold onto the name under them.
    pub domains: Vec<String>,
    /// The authority to trust; `None` trusts none.
    pub ca: Option<Ca>,
    /// Routed domains nobody declares any more ([`Routes::stale`]): removed in the same prompt.
    pub prune: Vec<String>,
    /// Where the routes record is: the whole set of routes the machine should keep is read from it, and it is updated
    /// after a run.
    pub home: Option<PathBuf>,
    /// For [`untrust`]: remove every doze authority the machine trusts, named or not (the last project is leaving, and
    /// a root whose files are gone must not stay trusted).
    pub untrust_all: bool,
}

impl TrustRequest {
    fn routes(&self) -> Vec<String> {
        route_domains(&self.domains)
    }

    /// Every route the machine should keep after a run: the request's, what other projects recorded, minus `prune`.
    fn keep(&self, prune: &[String]) -> Vec<String> {
        match &self.home {
            Some(home) => Routes::open(home).keep(&self.routes(), prune),
            None => self
                .routes()
                .into_iter()
                .filter(|d| !prune.contains(d))
                .collect(),
        }
    }
}

/// What is in place for a request, without privilege: a step per route, one for the authority, one per rotated-out root
/// still trusted, one per stale route.
pub fn trust_status(req: &TrustRequest) -> Vec<Step> {
    let mut steps: Vec<Step> = req
        .routes()
        .iter()
        .map(|d| {
            let (done, detail) = domain_routed(d);
            Step::new(&format!("route {d}"), done, detail)
        })
        .collect();
    if let Some(ca) = &req.ca {
        let (done, detail) = ca_trusted(ca);
        steps.push(Step::new("certificate authority", done, detail));
        for fp in ca
            .previous()
            .iter()
            .filter(|fp| fingerprint_trusted(Some(ca), fp))
        {
            let short = fp.get(..12).unwrap_or(fp);
            steps.push(Step::new(
                &format!("old authority {short}"),
                false,
                "a rotated-out authority is still trusted; doze trust removes it",
            ));
        }
    }
    for d in &req.prune {
        steps.push(Step::new(
            &format!("stale route {d}"),
            false,
            format!("no stack declares {d} any more; doze trust removes its route"),
        ));
    }
    steps
}

fn shell(os: Os) -> Shell {
    if os == Os::Windows {
        Shell::PowerShell
    } else {
        Shell::Sh
    }
}

/// The trust script for a request on `os`, and the routes it found to be someone else's; `None` when everything is in
/// place.
pub fn trust_plan(os: Os, req: &TrustRequest) -> (Option<Plan>, Vec<String>) {
    let (mut script, mut what) = (Vec::new(), Vec::new());
    let (foreign, mine): (Vec<String>, Vec<String>) =
        req.routes().into_iter().partition(|d| route_foreign(d));
    let missing: Vec<String> = mine.into_iter().filter(|d| !domain_routed(d).0).collect();
    if !missing.is_empty() || !req.prune.is_empty() {
        let keep = req.keep(&req.prune);
        let s = scripts::routes(os, &keep, &missing, &req.prune, &route_foreign);
        if !s.is_empty() {
            script.push(s);
        }
        if !missing.is_empty() {
            what.push(format!("route {} to doze", missing.join(", ")));
        }
        if !req.prune.is_empty() {
            what.push(format!(
                "remove the stale route for {}",
                req.prune.join(", ")
            ));
        }
        let others: Vec<&String> = keep.iter().filter(|d| !missing.contains(d)).collect();
        if !others.is_empty() && scripts::restates_routes(os) {
            let others: Vec<&str> = others.iter().map(|s| s.as_str()).collect();
            what.push(format!(
                "restate the routes other stacks keep ({})",
                others.join(", ")
            ));
        }
    }
    if let Some(ca) = &req.ca {
        for fp in ca
            .previous()
            .iter()
            .filter(|fp| fingerprint_trusted(Some(ca), fp))
        {
            script.push(scripts::untrust_fingerprint(os, Some(ca), fp));
            what.push("stop trusting a rotated-out authority".into());
        }
        if !ca_trusted(ca).0 {
            script.push(scripts::trust(os, ca));
            what.push(format!(
                "trust the doze certificate authority for TLS on this machine (its key stays in {}; anyone with that key \
                 could sign any name for this machine)",
                ca.dir().display()
            ));
        }
    }
    (plan(os, &what, script), foreign)
}

fn plan(os: Os, what: &[String], script: Vec<String>) -> Option<Plan> {
    let script: Vec<String> = script
        .into_iter()
        .filter(|l| !l.trim().is_empty())
        .collect();
    if script.is_empty() {
        return None;
    }
    let mut body = script.join("\n");
    body.push('\n');
    if os == Os::Windows {
        body.insert_str(0, "$ErrorActionPreference = 'Stop'\n");
    }
    Some(Plan {
        what: what.join("; "),
        shell: shell(os),
        script: body,
    })
}

/// Installs the routes and trusts the authority, in one prompt. Idempotent: what is in place is left alone, and so is
/// a route that is someone else's.
pub fn trust(os: Os, o: &mut Options<'_>, req: &mut TrustRequest) -> Result<(), Error> {
    let (plan, foreign) = trust_plan(os, req);
    for d in &foreign {
        writeln!(o.out, "✗ route {d} · {}", domain_routed(d).1)?;
    }
    let Some(plan) = plan else {
        writeln!(o.out, "✓ already set up — nothing to do")?;
        return Ok(());
    };
    run(o, &plan)?;
    if matches!(o.how, How::Print) {
        return Ok(());
    }
    if let Some(home) = &req.home {
        let routes = Routes::open(home);
        for d in req.prune.iter().filter(|d| !domain_routed(d).0) {
            routes.removed(d)?;
        }
    }
    if let Some(ca) = &mut req.ca {
        ca.forget_untrusted_previous()?;
    }
    req.prune.clear();
    for st in trust_status(req) {
        let mark = if st.done { "✓" } else { "✗" };
        writeln!(o.out, "{mark} {} · {}", st.name, st.detail)?;
    }
    Ok(())
}

/// The untrust script: the request's routes removed (routes other projects still declare are kept), the authority's
/// trust removed (current root and every rotated-out one), and with `untrust_all` every doze authority.
pub fn untrust_plan(os: Os, req: &TrustRequest) -> Option<Plan> {
    let (mut script, mut what) = (Vec::new(), Vec::new());
    let removing = req.routes();
    if !removing.is_empty() {
        let keep = req.keep(&removing);
        let s = scripts::routes(os, &keep, &[], &removing, &route_foreign);
        if !s.is_empty() {
            script.push(s);
            what.push(format!("remove the route for {}", removing.join(", ")));
        }
    }
    if let Some(ca) = &req.ca {
        script.push(scripts::untrust(os, Some(ca)));
        for fp in ca.previous() {
            script.push(scripts::untrust_fingerprint(os, Some(ca), fp));
        }
        what.push("stop trusting the doze certificate authority".into());
    }
    if req.untrust_all {
        let unknown = scripts::untrust_unknown(os);
        if !unknown.is_empty() && req.ca.is_none() {
            what.push("stop trusting every doze certificate authority".into());
        }
        script.extend(unknown);
    }
    plan(os, &what, script)
}

/// Removes the request's routes and the authority's trust, in one prompt.
pub fn untrust(os: Os, o: &mut Options<'_>, req: &TrustRequest) -> Result<(), Error> {
    let Some(plan) = untrust_plan(os, req) else {
        writeln!(o.out, "nothing to remove")?;
        return Ok(());
    };
    run(o, &plan)?;
    if matches!(o.how, How::Print) {
        return Ok(());
    }
    if let Some(home) = &req.home {
        let routes = Routes::open(home);
        for d in req.routes() {
            routes.removed(&d)?;
        }
    }
    writeln!(o.out, "✓ removed")?;
    Ok(())
}

/// What setup's uninstall adds for domains: every recorded route removed, and every doze authority's trust, whether
/// or not the files that made it can still be read, so nothing of doze is left pointing at a resolver that will not
/// come back.
pub(crate) fn cleanup_script(os: Os, home: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let recorded = Routes::open(home).recorded();
    if !recorded.is_empty() {
        out.push(scripts::uninstall_routes(os, &recorded));
    }
    if let Ok(Some(ca)) = Ca::probe(&ca_dir(home)) {
        out.push(scripts::untrust(os, Some(&ca)));
        for fp in ca.previous() {
            out.push(scripts::untrust_fingerprint(os, Some(&ca), fp));
        }
    }
    out.extend(scripts::untrust_unknown(os));
    out.retain(|l| !l.trim().is_empty());
    out
}

/// What the person owns, removed once the privileged part ran: the authority's files and the routes record.
pub(crate) fn cleanup_after(home: &Path) {
    let _ = std::fs::remove_dir_all(ca_dir(home));
    Routes::open(home).remove_all();
}

#[cfg(test)]
#[path = "trust_tests.rs"]
mod tests;
