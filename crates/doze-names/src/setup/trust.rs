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
    let (mut lines, mut what) = (Vec::new(), Vec::new());
    let (foreign, mine): (Vec<String>, Vec<String>) =
        req.routes().into_iter().partition(|d| route_foreign(d));
    let missing: Vec<String> = mine.into_iter().filter(|d| !domain_routed(d).0).collect();
    if !missing.is_empty() || !req.prune.is_empty() {
        let keep = req.keep(&req.prune);
        let s = scripts::routes(os, &keep, &missing, &req.prune, &route_foreign);
        if !s.is_empty() {
            lines.push(scripts::Lines {
                root: s,
                user: String::new(),
            });
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
        authority_lines(os, ca, &CaFacts::read(os, ca), (&mut lines, &mut what));
    }
    (plan(os, &what, lines), foreign)
}

/// Where an authority stands on this machine: what its part of a trust plan is decided from, read once so the
/// decision itself ([`authority_lines`]) is a function of it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CaFacts {
    /// The machine trusts it already (macOS: `security verify-cert` passes).
    pub(crate) trusted: bool,
    /// macOS: the System keychain holds it, trusted or not.
    pub(crate) in_keychain: bool,
    /// Its rotated-out roots the machine still trusts.
    pub(crate) old_trusted: Vec<String>,
}

impl CaFacts {
    pub(crate) fn read(os: Os, ca: &Ca) -> CaFacts {
        let fp = ca.fingerprint();
        CaFacts {
            trusted: ca_trusted(ca).0,
            in_keychain: os == Os::Mac
                && crate::trust_store::trusted_doze_roots()
                    .iter()
                    .any(|have| have.eq_ignore_ascii_case(&fp)),
            old_trusted: ca
                .previous()
                .iter()
                .filter(|fp| fingerprint_trusted(Some(ca), fp))
                .cloned()
                .collect(),
        }
    }
}

/// An authority's part of a trust plan: rotated-out roots still trusted are untrusted, and the authority is trusted
/// unless it is already. A trusted authority is left alone: nothing here rotates or removes it.
pub(crate) fn authority_lines(
    os: Os,
    ca: &Ca,
    facts: &CaFacts,
    (lines, what): (&mut Vec<scripts::Lines>, &mut Vec<String>),
) {
    for fp in &facts.old_trusted {
        lines.push(scripts::untrust_fingerprint(os, Some(ca), fp));
        what.push("stop trusting a rotated-out authority".into());
    }
    if !facts.trusted {
        lines.push(scripts::trust(os, ca, facts.in_keychain));
        what.push(trust_words(ca.dir()));
    }
}

/// What trusting the authority means, said in the prompt.
fn trust_words(dir: &Path) -> String {
    format!(
        "trust the doze certificate authority for TLS on this machine (its key stays in {}; anyone with that key \
         could sign any name for this machine)",
        dir.display()
    )
}

/// What a plan with a part for the person ([`Plan::as_user`]) says after what it does: macOS asks for that part in
/// its own dialog, so a second password dialog, which is not doze's, comes as no surprise.
pub const MAC_TRUST_DIALOG: &str = "then macOS asks for your password once more, in its own dialog, to change the System trust settings";

/// The script that has this machine believe the front door's https:// for every name (`doze doctor --fix` and `doze
/// dns-setup`, beside the zone's setup): the authority under `home` trusted, and rotated-out roots no longer. With
/// `ca` `None` the authority is not made yet: the script names the root file it will be made at, and the caller makes
/// it ([`Ca::load_or_create`]) before running the script. `None` when there is nothing to do: an authority already
/// trusted is left as it is.
pub fn https_plan(os: Os, home: &Path, ca: Option<&Ca>) -> Option<Plan> {
    let facts = ca.map(|ca| CaFacts::read(os, ca));
    https_plan_with(os, home, ca.zip(facts.as_ref()))
}

/// [`https_plan`], given what was read about the authority.
pub(crate) fn https_plan_with(os: Os, home: &Path, ca: Option<(&Ca, &CaFacts)>) -> Option<Plan> {
    let (mut lines, mut what) = (Vec::new(), Vec::new());
    let dir = ca_dir(home);
    match ca {
        Some((ca, facts)) => authority_lines(os, ca, facts, (&mut lines, &mut what)),
        None => {
            lines.push(scripts::trust_root(os, &crate::ca::root_file(&dir), false));
            what.push(trust_words(&dir));
        }
    }
    plan(os, &what, lines)
}

fn plan(os: Os, what: &[String], lines: Vec<scripts::Lines>) -> Option<Plan> {
    let join = |part: Vec<String>| {
        let part: Vec<String> = part.into_iter().filter(|l| !l.trim().is_empty()).collect();
        if part.is_empty() {
            return String::new();
        }
        let mut body = part.join("\n");
        body.push('\n');
        body
    };
    let (root, user): (Vec<String>, Vec<String>) =
        lines.into_iter().map(|l| (l.root, l.user)).unzip();
    let (mut script, as_user) = (join(root), join(user));
    if script.is_empty() && as_user.is_empty() {
        return None;
    }
    if os == Os::Windows && !script.is_empty() {
        script.insert_str(0, "$ErrorActionPreference = 'Stop'\n");
    }
    let mut what = what.join("; ");
    if !as_user.is_empty() {
        what.push_str("; ");
        what.push_str(MAC_TRUST_DIALOG);
    }
    Some(Plan {
        what,
        shell: shell(os),
        script,
        as_user,
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
/// trust removed (current root and every rotated-out one), and with `untrust_all` every doze authority. On macOS the
/// trust settings go in the person's part, through macOS's dialog, as they came.
pub fn untrust_plan(os: Os, req: &TrustRequest) -> Option<Plan> {
    let (mut lines, mut what) = (Vec::new(), Vec::new());
    let removing = req.routes();
    if !removing.is_empty() {
        let keep = req.keep(&removing);
        let s = scripts::routes(os, &keep, &[], &removing, &route_foreign);
        if !s.is_empty() {
            lines.push(scripts::Lines {
                root: s,
                user: String::new(),
            });
            what.push(format!("remove the route for {}", removing.join(", ")));
        }
    }
    if let Some(ca) = &req.ca {
        lines.push(scripts::untrust(os, Some(ca)));
        for fp in ca.previous() {
            lines.push(scripts::untrust_fingerprint(os, Some(ca), fp));
        }
        what.push("stop trusting the doze certificate authority".into());
    }
    if req.untrust_all {
        let unknown = scripts::untrust_unknown(os);
        if !unknown.is_empty() && req.ca.is_none() {
            what.push("stop trusting every doze certificate authority".into());
        }
        lines.extend(unknown);
    }
    plan(os, &what, lines)
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
/// come back. The lines for root's script, and the person's part ([`Plan::as_user`]: macOS's admin trust settings).
pub(crate) fn cleanup_script(os: Os, home: &Path) -> (Vec<String>, String) {
    let mut lines = Vec::new();
    let recorded = Routes::open(home).recorded();
    if !recorded.is_empty() {
        lines.push(scripts::Lines {
            root: scripts::uninstall_routes(os, &recorded),
            user: String::new(),
        });
    }
    if let Ok(Some(ca)) = Ca::probe(&ca_dir(home)) {
        lines.push(scripts::untrust(os, Some(&ca)));
        for fp in ca.previous() {
            lines.push(scripts::untrust_fingerprint(os, Some(&ca), fp));
        }
    }
    lines.extend(scripts::untrust_unknown(os));
    let (mut root, mut user): (Vec<String>, Vec<String>) =
        lines.into_iter().map(|l| (l.root, l.user)).unzip();
    root.retain(|l| !l.trim().is_empty());
    user.retain(|l| !l.trim().is_empty());
    let mut user = user.join("\n");
    if !user.is_empty() {
        user.push('\n');
    }
    (root, user)
}

/// What the person owns, removed once the privileged part ran: the authority's files and the routes record.
pub(crate) fn cleanup_after(home: &Path) {
    let _ = std::fs::remove_dir_all(ca_dir(home));
    Routes::open(home).remove_all();
}

#[cfg(test)]
#[path = "trust_tests.rs"]
mod tests;
