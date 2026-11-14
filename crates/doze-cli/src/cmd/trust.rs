//! `doze trust`: the one-time, one-prompt setup for a stack's own domains (Go `cmd/doze/trust.go`). Each declared
//! domain is routed to doze's resolver and, when the doze authority serves it, the authority is trusted here; routes
//! no stack declares any more go in the same prompt. `--check` looks without changing anything, `--remove` undoes
//! this stack's routes (and the authority's trust with the last stack), `--rotate` makes a new authority and trusts it.
//!
//! The stack's domains are what its daemon recorded when it last came up (`doze_doctor::domains`), until the Rust core
//! reads doze.hcl itself.

use crate::cli::setup::Trust;
use crate::cmd::doctor::Report;
use crate::cmd::setup::{Ran, Script, run};
use crate::ctx::Ctx;
use crate::diag::{Diagnostic, ids};
use crate::machine;
use crate::output::{Done, Mode, Outcome};
use crate::view::View;
use doze_doctor::domains::{Authority, StackDomains, request};
use doze_doctor::{Check, State};
use doze_names::ca::{Ca, ca_dir};
use doze_names::domains::wildcard;
use doze_names::routes::Routes;
use doze_names::setup::trust::{TrustRequest, trust_plan, trust_status, untrust_plan};
use std::path::Path;

/// `doze trust`.
pub(crate) fn run_trust(ctx: &mut Ctx<'_>, mode: &Mode, a: &Trust) -> Outcome {
    if a.rotate && a.remove {
        return Err(
            Diagnostic::new(ids::USAGE, "--rotate and --remove do not go together")
                .fix("doze trust --help"),
        );
    }
    let project = crate::cmd::modules::find(ctx, mode)?;
    let m = machine::machine(ctx);
    let home = m.home().to_path_buf();
    let stack = StackDomains::recorded(&home, &project.dir);
    if stack.domains.is_empty() && !a.remove {
        return Ok(Done::ok(View::Ran(Ran {
            said: vec![
                "no domains recorded for this stack: its daemon records the ones doze.hcl declares when the stack \
                 comes up (doze up), then doze trust sets them up"
                    .into(),
            ],
            checks: Vec::new(),
        })));
    }
    let mut said = Vec::new();
    let auth = authority(&home, a.rotate, &mut said)?;
    let mut req = request(&home, &stack, auth.ca);
    if a.check {
        return Ok(check(&req, auth.broken.as_deref()));
    }
    if a.remove {
        return remove(ctx, &home, &stack, req, a.dry_run);
    }
    let (plan, foreign) = trust_plan(m.setup.os, &req);
    said.extend(
        foreign
            .iter()
            .map(|d| format!("✗ route {d} · another program routes it; doze leaves it alone")),
    );
    if a.dry_run {
        return Ok(Done::ok(View::Script(Script {
            applies_with: "doze trust".into(),
            plan,
        })));
    }
    match plan {
        None => said.push("✓ already set up — nothing to do".into()),
        Some(plan) => {
            run(ctx, &plan)?;
            settle(&home, &mut req).map_err(|e| machine::failed(&e))?;
        }
    }
    req.prune.clear();
    for d in &stack.domains {
        let any = if wildcard(d) { "  any subdomain," } else { " " };
        said.push(format!("✓ https://{d}{any} once the stack is up"));
    }
    if a.rotate {
        said.push(
            "› processes already running trust the old authority: restart the stack to have them read the new one"
                .into(),
        );
    }
    let checks = steps(&req);
    // A tick only for what is in place: a script that ran and left a part missing is said, not passed over.
    let problem = checks.iter().find(|c| c.state == State::Fail).map(|c| {
        Diagnostic::new(
            ids::SETUP_FAILED,
            format!("the script ran, but {} is still not in place", c.name),
        )
    });
    Ok(Done {
        view: View::Ran(Ran { said, checks }),
        problem,
    })
}

/// The authority to trust: a new one with `--rotate` (whatever state the old one is in), else the one there is. Only
/// the daemon makes one; trust looks.
fn authority(home: &Path, rotate: bool, said: &mut Vec<String>) -> Result<Authority, Diagnostic> {
    if !rotate {
        return Ok(Authority::probe(home));
    }
    let ca = Ca::rotate(&ca_dir(home)).map_err(|e| machine::failed(&e))?;
    said.push(format!("✓ new certificate authority {}", ca.common_name()));
    if let Err(e) = ca.bundle() {
        said.push(format!("  the trust bundle could not be rewritten: {e}"));
    }
    Ok(Authority {
        ca: Some(ca),
        broken: None,
    })
}

/// After the script: stale routes that are gone leave the record, rotated-out authorities no longer trusted are
/// forgotten (what `doze_names`' own trust does after its run).
fn settle(home: &Path, req: &mut TrustRequest) -> Result<(), doze_names::Error> {
    let routes = Routes::open(home);
    for d in req
        .prune
        .iter()
        .filter(|d| !doze_names::trust_store::domain_routed(d).0)
    {
        routes.removed(d)?;
    }
    if let Some(ca) = &mut req.ca {
        ca.forget_untrusted_previous()?;
    }
    Ok(())
}

fn steps(req: &TrustRequest) -> Vec<Check> {
    trust_status(req)
        .into_iter()
        .map(|s| Check::step(s.name, s.done, s.detail).fix(doze_doctor::FIX_DOMAINS))
        .collect()
}

/// `--check`: each part, and the command that sets up what is missing.
fn check(req: &TrustRequest, broken: Option<&str>) -> Done {
    let mut checks = steps(req);
    if let Some(why) = broken {
        checks.push(Check::step("certificate authority", false, why).fix(doze_doctor::FIX_ROTATE));
    }
    let ok = checks.iter().all(|c| c.state != State::Fail);
    let fix = if broken.is_some() {
        doze_doctor::FIX_ROTATE
    } else {
        doze_doctor::FIX_DOMAINS
    };
    Done {
        view: View::Doctor(Report { checks }),
        problem: (!ok).then(|| machine::missing("the domains are", fix)),
    }
}

/// `--remove`: this stack's routes go unless another stack still declares the domain; the authority's trust and files
/// go with the last stack, every doze root the machine holds with them. With `--dry-run` nothing is changed.
fn remove(
    ctx: &mut Ctx<'_>,
    home: &Path,
    stack: &StackDomains,
    mut req: TrustRequest,
    dry_run: bool,
) -> Outcome {
    let routes = Routes::open(home);
    let others = |d: &String| routes.claimants(d).iter().any(|c| *c != stack.dir);
    req.domains = stack
        .domains
        .iter()
        .filter(|d| !others(d))
        .cloned()
        .collect();
    req.prune = Vec::new();
    let elsewhere = routes.recorded().iter().any(others);
    let last = !elsewhere;
    if last {
        req.untrust_all = true;
    } else {
        req.ca = None; // another stack still serves a domain from it
    }
    let plan = untrust_plan(doze_names::setup::Os::native(), &req);
    if dry_run {
        return Ok(Done::ok(View::Script(Script {
            applies_with: "doze trust --remove".into(),
            plan,
        })));
    }
    let _ = routes.forget_project(&stack.dir);
    let mut said = Vec::new();
    match plan {
        None => said.push("nothing to remove".into()),
        Some(plan) => {
            run(ctx, &plan)?;
            for d in &req.domains {
                routes.removed(d).map_err(|e| machine::failed(&e))?;
            }
            said.push("✓ removed".into());
        }
    }
    if last && ca_dir(home).exists() {
        std::fs::remove_dir_all(ca_dir(home)).map_err(|e| {
            Diagnostic::new(
                ids::SETUP_FAILED,
                format!("the authority's files could not be removed: {e}"),
            )
        })?;
        said.push("✓ the certificate authority's files are gone".into());
    }
    Ok(Done::ok(View::Ran(Ran {
        said,
        checks: Vec::new(),
    })))
}
