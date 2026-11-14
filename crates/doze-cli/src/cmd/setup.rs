//! `doze dns-setup` and `doze doctor --fix`: the one-time machine setup for `.doze` names, and the repair of whatever
//! doctor finds missing (the names and this stack's domains) under **one** prompt. The setup is shared with doze-aws
//! and doze-kafka: whichever is installed sets the machine up for all three.
//!
//! The plan is data (`doze_doctor::repair`): `--dry-run` (`--print`, `--print-fix`) shows its script and runs
//! nothing, the run hands that same script to the host (sudo on a terminal, the system's prompt from the app), and
//! `--check` reports each part from the same steps.

use crate::cmd::doctor::Report;
use crate::ctx::Ctx;
use crate::diag::{Diagnostic, ids};
use crate::machine;
use crate::output::{Done, Mode, Outcome};
use crate::paint::Paint;
use crate::project;
use crate::view::View;
use doze_doctor::domains::StackDomains;
use doze_doctor::repair::{self, Repair};
use doze_doctor::{Check, Machine, State};
use doze_names::setup::{Plan, Shell, pasteable};
use serde::Serialize;

/// The script a privileged command would run, shown instead of run.
#[derive(Debug, Clone)]
pub struct Script {
    pub applies_with: String,
    pub plan: Option<Plan>,
}

impl Script {
    /// Go's `--print`: the purpose as a comment, then something a person can paste.
    pub(crate) fn human(&self) -> String {
        match &self.plan {
            Some(p) => format!("# {}\n{}", p.what, pasteable(p)),
            None => "✓ already set up — nothing to do\n".into(),
        }
    }
}

impl Serialize for Script {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Body<'a> {
            dry_run: bool,
            applies_with: &'a str,
            nothing_to_do: bool,
            #[serde(skip_serializing_if = "Option::is_none")]
            what: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            shell: Option<&'static str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            script: Option<&'a str>,
        }
        let p = self.plan.as_ref();
        Body {
            dry_run: true,
            applies_with: &self.applies_with,
            nothing_to_do: p.is_none(),
            what: p.map(|p| p.what.as_str()),
            shell: p.map(|p| match p.shell {
                Shell::Sh => "sh",
                Shell::PowerShell => "powershell",
            }),
            script: p.map(|p| p.script.as_str()),
        }
        .serialize(s)
    }
}

/// What a privileged run did: what it said, then each part as it stands now.
#[derive(Debug, Clone, Serialize)]
pub struct Ran {
    pub said: Vec<String>,
    pub checks: Vec<Check>,
}

impl Ran {
    pub(crate) fn human(&self, p: Paint) -> String {
        let mut out = String::new();
        for line in &self.said {
            out.push_str(line);
            out.push('\n');
        }
        for c in &self.checks {
            let mark = match c.state {
                State::Ok | State::Warn => p.ok("✓"),
                State::Fail => p.fail("✗"),
                State::NotChecked => p.dim("·"),
            };
            out.push_str(&format!("{mark} {} · {}\n", c.name, c.detail));
        }
        out
    }
}

/// `doze dns-setup`.
pub(crate) fn dns_setup(ctx: &mut Ctx<'_>, a: &crate::cli::setup::DnsSetup) -> Outcome {
    let m = machine::machine(ctx);
    if a.check {
        let checks = doze_doctor::names::checks(&m);
        let ok = checks.iter().all(|c| c.state != State::Fail);
        return Ok(Done {
            view: View::Doctor(Report::new(checks)),
            problem: (!ok).then(|| machine::missing(".doze names are", "doze dns-setup")),
        });
    }
    if a.uninstall {
        return uninstall(ctx, &m, a.dry_run);
    }
    if m.zone_off {
        return Err(Diagnostic::new(
            ids::NAMES_OFF,
            "names are switched off (DOZE_ZONE=off), so there is nothing to set up",
        )
        .fix("unset DOZE_ZONE to use .doze names, then run it again"));
    }
    let r = repair::plan(&m, None);
    finish(ctx, &m, r, a.dry_run, "doze dns-setup", None)
}

/// `doze doctor --fix`: the names and this stack's domains, in one prompt.
pub(crate) fn fix(ctx: &mut Ctx<'_>, mode: &Mode, dry_run: bool) -> Outcome {
    let m = machine::machine(ctx);
    let stack = project::find(&ctx.cwd, mode.config.as_deref())
        .map(|p| StackDomains::recorded(m.home(), &p.dir));
    let r = repair::plan(&m, stack.as_ref());
    finish(ctx, &m, r, dry_run, "doze doctor --fix", stack.as_ref())
}

/// Shows the repair's script, or runs it and says what stands after.
fn finish(
    ctx: &mut Ctx<'_>,
    m: &Machine,
    mut r: Repair,
    dry_run: bool,
    applies_with: &str,
    stack: Option<&StackDomains>,
) -> Outcome {
    let mut said: Vec<String> = r
        .foreign
        .iter()
        .map(|d| format!("✗ route {d} · another program routes it; doze leaves it alone"))
        .collect();
    said.extend(r.cannot.iter().map(|w| format!("✗ cannot repair {w}")));
    let cannot = r
        .cannot
        .first()
        .map(|w| Diagnostic::new(ids::SETUP_FAILED, format!("cannot repair {w}")));
    let Some(plan) = r.one() else {
        if dry_run {
            return Ok(Done::ok(View::Script(Script {
                applies_with: applies_with.into(),
                plan: None,
            })));
        }
        if cannot.is_none() {
            said.push("✓ already set up — nothing to do".into());
        }
        return Ok(Done {
            view: View::Ran(Ran {
                said,
                checks: Vec::new(),
            }),
            problem: cannot,
        });
    };
    if dry_run {
        return Ok(Done::ok(View::Script(Script {
            applies_with: applies_with.into(),
            plan: Some(plan),
        })));
    }
    run(ctx, &plan)?;
    let still = r
        .after(m, ctx.host.settle())
        .map_err(|e| machine::failed(&e))?;
    let mut checks = doze_doctor::names::checks(m);
    if stack.is_some() {
        checks.extend(doze_doctor::domains::checks(m, stack));
    }
    // A tick only for what is in place: a part still missing after the script is the run's problem, names or domains.
    let problem = still
        .first()
        .cloned()
        .or_else(|| {
            checks
                .iter()
                .find(|c| c.state == State::Fail)
                .map(|c| format!("{} is still not in place", c.name))
        })
        .map(|why| Diagnostic::new(ids::SETUP_FAILED, format!("setup ran, but {why}")))
        .or(cannot);
    Ok(Done {
        view: View::Ran(Ran { said, checks }),
        problem,
    })
}

/// Runs `plan` as root through the host: one prompt. Always the host, even when this process is root already (the
/// real host runs it by its own shell then): a test's recorder must never be bypassed.
pub(crate) fn run(ctx: &Ctx<'_>, plan: &Plan) -> Result<(), Diagnostic> {
    ctx.host
        .privileged(plan, machine::prompt(ctx))
        .map_err(|r| machine::refused(&r, plan))
}

/// `doze dns-setup --uninstall`: the zone's setup, every recorded route and every doze authority's trust, in one
/// prompt; then the authority's files and the routes record, which are the person's own.
fn uninstall(ctx: &mut Ctx<'_>, m: &Machine, dry_run: bool) -> Outcome {
    let plan = m.setup.uninstall_plan().map_err(|e| machine::failed(&e))?;
    if dry_run {
        return Ok(Done::ok(View::Script(Script {
            applies_with: "doze dns-setup --uninstall".into(),
            plan: Some(plan),
        })));
    }
    run(ctx, &plan)?;
    let _ = std::fs::remove_dir_all(doze_names::ca::ca_dir(m.home()));
    doze_names::routes::Routes::open(m.home()).remove_all();
    Ok(Done::ok(View::Ran(Ran {
        said: vec!["✓ removed".into()],
        checks: Vec::new(),
    })))
}
