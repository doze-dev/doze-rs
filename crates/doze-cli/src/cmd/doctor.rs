//! `doze doctor`: the checks that need no core, each said as it is (rule 3: a tick only for what was verified; what
//! could not be checked is marked and makes the run partial, never "all checks passed"). F5 was the Go doctor
//! passing while the registry was down: the registry is checked here, host by host.

use crate::ctx::Ctx;
use crate::diag::{Diagnostic, ids};
use crate::output::{Done, Mode, Outcome};
use crate::paint::{Paint, pad};
use crate::registry::{FetchError, signed};
use crate::view::View;
use crate::{lock, platform, project};
use serde::Serialize;

/// How one check came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Ok,
    /// Works, with something worth knowing.
    Warn,
    Fail,
    NotChecked,
}

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub name: &'static str,
    pub state: State,
    pub detail: String,
}

/// What `doctor` shows.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub checks: Vec<Check>,
}

pub(crate) fn run(ctx: &mut Ctx<'_>, mode: &Mode) -> Outcome {
    let mut checks = vec![
        Check {
            name: "platform",
            state: State::Ok,
            detail: platform::triple(),
        },
        home(ctx),
    ];
    checks.extend(config(ctx, mode));
    checks.push(registry(ctx));
    checks.push(names(ctx));
    checks.push(Check {
        name: "daemon",
        state: State::NotChecked,
        detail: "not checked: the Rust core is not built yet".into(),
    });
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

fn home(ctx: &Ctx<'_>) -> Check {
    let h = &ctx.home;
    let (state, detail) = match std::fs::metadata(h) {
        Ok(m) if m.is_dir() && !m.permissions().readonly() => (State::Ok, h.display().to_string()),
        Ok(_) => (
            State::Fail,
            format!("{} is not a directory doze can write to", h.display()),
        ),
        Err(_) => (State::Ok, format!("{} (made on first use)", h.display())),
    };
    Check {
        name: "home",
        state,
        detail,
    }
}

fn config(ctx: &Ctx<'_>, mode: &Mode) -> Vec<Check> {
    let Some(p) = project::find(&ctx.cwd, mode.config.as_deref()) else {
        let detail = "no doze.hcl here or in any parent: checking the machine only".into();
        return vec![Check {
            name: "config",
            state: State::Ok,
            detail,
        }];
    };
    let mut out = vec![Check {
        name: "config",
        state: State::NotChecked,
        detail: format!(
            "{} found; what is in it is checked by doze lint, which needs the core",
            p.config.display()
        ),
    }];
    let lock = match std::fs::read_to_string(p.lock()) {
        Err(_) => Check {
            name: "doze.lock",
            state: State::Ok,
            detail: "none yet (doze up writes it)".into(),
        },
        Ok(text) => match lock::parse(&text) {
            Ok(l) => Check {
                name: "doze.lock",
                state: State::Ok,
                detail: match l.modules.len() {
                    1 => "1 module pinned".to_string(),
                    n => format!("{n} modules pinned"),
                },
            },
            Err(e) => Check {
                name: "doze.lock",
                state: State::Fail,
                detail: format!("can't be read: {} (doze modules says where)", e.message),
            },
        },
    };
    out.push(lock);
    out
}

/// F1 and F5: every registry host, and whether the one that answers serves the key built into this doze.
fn registry(ctx: &Ctx<'_>) -> Check {
    let check = |state, detail| Check {
        name: "registry",
        state,
        detail,
    };
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

/// The `.doze` zone, read without writing: the names file every doze program shares.
fn names(ctx: &Ctx<'_>) -> Check {
    let check = |state, detail: String| Check {
        name: "names",
        state,
        detail,
    };
    let off = ctx.var(doze_names::ENV_ZONE).is_some_and(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "off" | "0" | "false" | "no"
        )
    });
    if off {
        return check(
            State::Ok,
            format!(
                "the .doze zone is switched off ({}=off)",
                doze_names::ENV_ZONE
            ),
        );
    }
    let file = ctx.home.join(doze_names::registry::FILE_NAME);
    let Ok(raw) = std::fs::read(&file) else {
        return check(
            State::Ok,
            "no names yet: each doze program registers its own when it starts".into(),
        );
    };
    let Ok(entries) =
        serde_json::from_slice::<std::collections::BTreeMap<String, doze_names::Entry>>(&raw)
    else {
        return check(
            State::Warn,
            format!(
                "{} can't be read; the next doze program to start rewrites it",
                file.display()
            ),
        );
    };
    let live = entries
        .iter()
        .filter(|(k, e)| {
            !k.starts_with('_') && u32::try_from(e.pid).is_ok_and(doze_os::process::alive)
        })
        .count();
    check(
        State::NotChecked,
        format!(
            "{live} live names; whether the resolver answers them is not checked (it needs the daemon)"
        ),
    )
}

impl Report {
    pub(crate) fn human(&self, p: Paint) -> String {
        let mut out = String::new();
        for c in &self.checks {
            let mark = match c.state {
                State::Ok => p.ok("✓"),
                State::Warn => p.warn("!"),
                State::Fail => p.fail("✗"),
                State::NotChecked => p.dim("·"),
            };
            out.push_str(&format!("  {mark}  {}{}\n", pad(c.name, 11), c.detail));
        }
        out
    }
}
