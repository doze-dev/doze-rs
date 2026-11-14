//! `doze doctor`: what doze needs from this machine and this project, each said as it is (rule 3: a tick only for what
//! was verified; what could not be checked is marked and makes the run partial, never "all checks passed"). F5 was the
//! Go doctor passing while the registry was down: the registry is checked here, host by host. The machine's parts
//! (the `.doze` names, this stack's domains, what was left behind) come from `doze_doctor`, as the repair that
//! `doze doctor --fix` runs does, so the report and the repair cannot disagree.

use crate::ctx::Ctx;
use crate::diag::{Diagnostic, ids};
use crate::output::{Done, Mode, Outcome};
use crate::paint::{Paint, pad};
use crate::registry::{FetchError, signed};
use crate::view::View;
use crate::{lock, machine, platform, project};
use doze_doctor::domains::StackDomains;
use doze_doctor::{Check, State};
use serde::Serialize;

/// What `doctor` shows.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub checks: Vec<Check>,
}

pub(crate) fn run(ctx: &mut Ctx<'_>, mode: &Mode) -> Outcome {
    let mut checks = vec![
        Check::new("platform", State::Ok, platform::triple()),
        home(ctx),
    ];
    let found = project::find(&ctx.cwd, mode.config.as_deref());
    checks.extend(config(found.as_ref()));
    checks.push(registry(ctx));
    let m = machine::machine(ctx);
    checks.extend(doze_doctor::names::checks(&m));
    if let Some(p) = &found {
        checks.push(daemon(ctx, p));
        let stack = StackDomains::recorded(m.home(), &p.dir);
        checks.extend(doze_doctor::domains::checks(&m, Some(&stack)));
    }
    checks.extend(doze_doctor::leftovers::checks(m.home()));
    let count = |s: State| checks.iter().filter(|c| c.state == s).count();
    let problem = if count(State::Fail) > 0 {
        Some(Diagnostic::new(
            ids::CHECKS_FAILED,
            "some checks need attention (marked ✗ above)",
        ))
    } else {
        let n = count(State::NotChecked);
        (n > 0).then(|| {
            Diagnostic::new(
                ids::NOT_ALL_CHECKED,
                format!(
                    "{} of {} checked; {n} could not be (marked ·)",
                    checks.len() - n,
                    checks.len()
                ),
            )
        })
    };
    Ok(Done {
        view: View::Doctor(Report { checks }),
        problem,
    })
}

/// Whether this stack's daemon answers on its socket (stopped is fine: it starts on first use).
fn daemon(ctx: &Ctx<'_>, p: &project::Project) -> Check {
    let key = doze_projects::StackKey::for_config(&ctx.home, &p.config);
    let client = doze_control::Client::socket(key.socket())
        .named("cli")
        .with_timeout(Some(std::time::Duration::from_secs(2)));
    match client.dial() {
        Ok(_) => Check::new("daemon", State::Ok, "running"),
        Err(doze_control::Error::Connect(_)) => Check::new(
            "daemon",
            State::Ok,
            "stopped (starts automatically on first use)",
        ),
        Err(e) => Check::new(
            "daemon",
            State::Fail,
            format!("something answers on its socket and cannot be talked to: {e}"),
        ),
    }
}

fn home(ctx: &Ctx<'_>) -> Check {
    let h = &ctx.home;
    match std::fs::metadata(h) {
        Ok(m) if m.is_dir() && !m.permissions().readonly() => {
            Check::new("home", State::Ok, h.display().to_string())
        }
        Ok(_) => Check::new(
            "home",
            State::Fail,
            format!("{} is not a directory doze can write to", h.display()),
        ),
        Err(_) => Check::new(
            "home",
            State::Ok,
            format!("{} (made on first use)", h.display()),
        ),
    }
}

fn config(found: Option<&project::Project>) -> Vec<Check> {
    let Some(p) = found else {
        return vec![Check::new(
            "config",
            State::Ok,
            "no doze.hcl here or in any parent: checking the machine only",
        )];
    };
    let mut out = vec![Check::new(
        "config",
        State::NotChecked,
        format!(
            "{} found; what is in it is checked by doze lint, which needs the core",
            p.config.display()
        ),
    )];
    let lock = match std::fs::read_to_string(p.lock()) {
        Err(_) => Check::new("doze.lock", State::Ok, "none yet (doze up writes it)"),
        Ok(text) => match lock::parse(&text) {
            Ok(l) => Check::new(
                "doze.lock",
                State::Ok,
                match l.modules.len() {
                    1 => "1 module pinned".to_string(),
                    n => format!("{n} modules pinned"),
                },
            ),
            Err(e) => Check::new(
                "doze.lock",
                State::Fail,
                format!("can't be read: {} (doze modules says where)", e.message),
            ),
        },
    };
    out.push(lock);
    out
}

/// F1 and F5: every registry host, and whether the one that answers serves the key built into this doze.
fn registry(ctx: &Ctx<'_>) -> Check {
    let check = |state, detail| Check::new("registry", state, detail);
    match ctx.registry.get("doze/keys.json") {
        Err(FetchError::Unreachable(why)) => check(
            State::Fail,
            format!(
                "can't be reached: {} · modules this machine has keep working",
                FetchError::Unreachable(why).reasons()
            ),
        ),
        Err(e @ FetchError::Missing { .. }) => check(
            State::Fail,
            format!("answers, but serves no key ({})", e.reasons()),
        ),
        Ok(got) => {
            let key = serde_json::from_slice::<serde_json::Value>(&got.body)
                .ok()
                .and_then(|v| {
                    v.get("key")
                        .and_then(|k| k.as_str())
                        .and_then(signed::parse_key)
                });
            if key != Some(ctx.registry.key) {
                return check(
                    State::Fail,
                    format!(
                        "{} serves a key other than the one built into this doze",
                        got.from
                    ),
                );
            }
            let skipped: Vec<String> = got
                .skipped
                .iter()
                .map(|(b, w)| format!("{b}: {w}"))
                .collect();
            if skipped.is_empty() {
                check(
                    State::Ok,
                    format!("{} answers with the key built into this doze", got.from),
                )
            } else {
                check(
                    State::Warn,
                    format!(
                        "{}; {} answers with the key built into this doze",
                        skipped.join("; "),
                        got.from
                    ),
                )
            }
        }
    }
}

impl Report {
    /// One line a check, the names in one column as wide as the longest (at least 11); a multi-line detail continues
    /// under its first line.
    pub(crate) fn human(&self, p: Paint) -> String {
        let width = self
            .checks
            .iter()
            .map(|c| c.name.chars().count() + 2)
            .max()
            .unwrap_or(0)
            .max(11);
        let mut out = String::new();
        for c in &self.checks {
            let mark = match c.state {
                State::Ok => p.ok("✓"),
                State::Warn => p.warn("!"),
                State::Fail => p.fail("✗"),
                State::NotChecked => p.dim("·"),
            };
            let mut lines = c.detail.lines();
            let first = lines.next().unwrap_or_default();
            out.push_str(&format!("  {mark}  {}{first}\n", pad(&c.name, width)));
            for l in lines {
                out.push_str(&format!("     {}{l}\n", " ".repeat(width)));
            }
        }
        out
    }
}
