//! `doze update [--dry-run]` and `doze __refresh`, the background check behind the update line and the module copies.
//!
//! The newest doze comes from the state file when it is under a day old, else the registry's signed release file,
//! within the budget (through doze's one HTTP client, `doze_cli::http`). A doze installed by Homebrew is updated by
//! running `brew upgrade doze`; the app and a download update themselves another way, and the message gives the
//! exact one. Then each running stack is handed over to the new doze ([`super::handover`]: its daemon restarts on the
//! new doze and what was awake wakes again; the services stop for that moment, as keeping them running needs the
//! daemon to pass its listeners on, compat.md rule 5). A stack that could not be moved keeps the old doze, is named
//! with why, and the run exits 11 (rule 3: no tick for work not done).
//! Exits 0 updated or already the newest; 1 this install cannot update itself, or brew failed; 10 the newest is not
//! known (the network, or no release file yet); 11 updated, a stack not handed over.

use super::fetch::{self, Prefer};
use crate::cli::setup::Refresh;
use crate::ctx::{Ctx, VERSION};
use crate::diag::{Diagnostic, ids};
use crate::install::Install;
use crate::output::{Done, Outcome};
use crate::paint::Paint;
use crate::plan::{Op, Plan};
use crate::registry::FetchError;
use crate::registry::latest::{self, Latest};
use crate::registry::source::Source;
use crate::update::{self, newer};
use crate::view::View;
use serde::Serialize;

/// What `doze update` did.
#[derive(Debug, Serialize)]
pub struct Updated {
    pub from: &'static str,
    pub to: String,
    /// Homebrew, the app, a download.
    pub through: &'static str,
    /// The install tool ran and succeeded (false: this doze is the newest). Which version it installed is not read back:
    /// `doze version` says.
    pub installed: bool,
    /// The stacks that were running when the update started.
    pub running: usize,
    /// Each running stack's hand-over to the new doze. Not in the first `update` result; left out when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stacks: Vec<super::handover::Moved>,
}

/// What a hand-over does to a stack.
const HANDOVER: &str = "each daemon restarts on the new doze and what was awake wakes again (services stop for that moment)";

pub(crate) fn run(ctx: &mut Ctx<'_>, dry_run: bool) -> Outcome {
    let install = Install::of(ctx.host.exe().as_deref());
    let latest = newest(ctx)?;
    let running = doze_projects::running(&ctx.home).len();
    if dry_run {
        return Ok(Done::ok(View::Plan(plan(&latest, install, running))));
    }
    let mut done = Updated {
        from: VERSION,
        to: latest.version.clone(),
        through: install.name(),
        installed: false,
        running,
        stacks: Vec::new(),
    };
    if !newer(&latest.version, VERSION) {
        return Ok(Done::ok(View::Updated(done)));
    }
    if install != Install::Brew {
        return Err(Diagnostic::new(
            ids::SELF_UPDATE_UNSUPPORTED,
            format!(
                "doze {} is out; this doze came through {}, which updates it",
                latest.version,
                install.name()
            ),
        )
        .fix(install.how()));
    }
    brew(ctx)?;
    done.installed = true;
    let problem = hand_over(ctx, &mut done);
    Ok(Done {
        view: View::Updated(done),
        problem,
    })
}

/// Hands the running stacks over to the doze brew installed (the `doze` on the PATH, else this program's path), only
/// when it is this doze (`handover::all` refuses an older one); a diagnostic (exit 11) naming those that keep the old
/// doze.
fn hand_over(ctx: &mut Ctx<'_>, done: &mut Updated) -> Option<Diagnostic> {
    if done.running == 0 {
        return None;
    }
    let path = ctx.var("PATH").unwrap_or_default();
    let Some(exe) =
        doze_os::which::which_in("doze", path.as_ref(), None).or_else(|| ctx.host.daemon_exe())
    else {
        return Some(
            Diagnostic::new(
                ids::NOT_ALL_CHECKED,
                format!(
                    "{} running stack(s) keep doze {VERSION}: the new doze is not on the PATH",
                    done.running
                ),
            )
            .fix("doze down, then doze up, in each one moves it to the new doze"),
        );
    };
    let _ = writeln!(
        ctx.err,
        "handing {} running stack(s) over to the new doze…",
        done.running
    );
    done.stacks = super::handover::all(&ctx.home, &exe);
    let left: Vec<String> = done
        .stacks
        .iter()
        .filter(|m| !m.moved)
        .map(|m| format!("{}: {}", m.stack, m.detail))
        .collect();
    (!left.is_empty()).then(|| {
        Diagnostic::new(
            ids::NOT_ALL_CHECKED,
            format!("{} stack(s) keep doze {VERSION}", left.len()),
        )
        .note(left.join("\n"))
        .fix("doze down, then doze up, in each one moves it to the new doze")
    })
}

/// `brew upgrade doze`, on this terminal.
fn brew(ctx: &mut Ctx<'_>) -> Result<(), Diagnostic> {
    let path = ctx.var("PATH").unwrap_or_default();
    let Some(brew) = doze_os::which::which_in("brew", path.as_ref(), None) else {
        return Err(Diagnostic::new(
            ids::SELF_UPDATE_UNSUPPORTED,
            "Homebrew's brew is not on the PATH",
        )
        .fix("brew upgrade doze, wherever brew is"));
    };
    match ctx.host.run(&brew, &["upgrade".into(), "doze".into()]) {
        Ok(0) => Ok(()),
        Ok(code) => Err(Diagnostic::new(
            ids::UPDATE_FAILED,
            format!("brew upgrade doze stopped with exit {code}"),
        )
        .fix("its words are above; run brew upgrade doze yourself to see them again")),
        Err(e) => Err(
            Diagnostic::new(ids::UPDATE_FAILED, format!("brew could not be run: {e}"))
                .fix("brew upgrade doze"),
        ),
    }
}

/// `--dry-run`: the version it would install, how, and what happens to the running daemons.
fn plan(latest: &Latest, install: Install, running: usize) -> Plan {
    let mut plan = Plan::new("doze update");
    if !newer(&latest.version, VERSION) {
        plan.unchanged(&format!("doze {VERSION} is the newest"));
        return plan;
    }
    let title = format!("doze {VERSION} → {}", latest.version);
    plan.step(
        &title,
        Op::Do,
        format!("install doze {}", latest.version),
        format!("through {}: {}", install.name(), install.how()),
    );
    if running > 0 {
        plan.step(
            &title,
            Op::Do,
            format!("hand {running} running stack(s) over to it"),
            HANDOVER,
        );
    }
    for news in &latest.news {
        plan.step(&title, Op::Add, news.clone(), "");
    }
    plan
}

impl Updated {
    pub(crate) fn human(&self, p: Paint) -> String {
        if self.installed {
            // brew exits 0 when its formula is behind too ("already installed"): what is installed now is not
            // known here, so it is not claimed (rule 3).
            let mut out = format!(
                "{} brew upgrade doze finished · the newest is {}; doze version says which this is now\n",
                p.ok("✓"),
                self.to
            );
            for m in self.stacks.iter().filter(|m| m.moved) {
                out.push_str(&format!(
                    "{} {} runs on the new doze · {}\n",
                    p.ok("✓"),
                    m.stack,
                    p.dim(&m.detail)
                ));
            }
            out
        } else {
            format!("{} doze {} is the newest\n", p.ok("✓"), self.from)
        }
    }
}

/// The newest doze: the background check's answer if it is fresh, else the registry's release file now.
fn newest(ctx: &mut Ctx<'_>) -> Result<Latest, Diagnostic> {
    let state = update::load(&ctx.home);
    if let Some(l) = state
        .latest
        .filter(|_| ctx.unix_now().saturating_sub(state.checked_at) < 86_400)
    {
        return Ok(l);
    }
    let got = ctx.registry.get(latest::PATH).map_err(|e| match e {
        FetchError::Missing { from } => Diagnostic::new(
            ids::LATEST_UNKNOWN,
            "the newest doze is not known: the registry publishes no release file yet",
        )
        .note(format!("{from} has no {}", latest::PATH))
        .fix(format!("see {}", crate::install::RELEASES)),
        e => Diagnostic::new(
            ids::LATEST_UNKNOWN,
            "the newest doze is not known: the registry can't be reached",
        )
        .note(e.reasons())
        .fix("check the network and run it again"),
    })?;
    let l = latest::parse(&String::from_utf8_lossy(&got.body), &ctx.registry.key).map_err(|e| {
        Diagnostic::new(
            ids::REGISTRY_UNSIGNED,
            format!("the registry's release file is refused: {}", e.message),
        )
        .fix("do not work around this: report it with doze bug")
    })?;
    remember(ctx, &l);
    Ok(l)
}

fn remember(ctx: &Ctx<'_>, l: &Latest) {
    let mut state = update::load(&ctx.home);
    state.checked_at = ctx.unix_now();
    state.latest = Some(l.clone());
    let _ = update::save(&ctx.home, &state);
}

/// The detached check: silent, bounded by the budget, and only ever writes the doze home.
pub(crate) fn refresh(ctx: &mut Ctx<'_>, r: &Refresh) -> Outcome {
    if r.latest
        && let Ok(got) = ctx.registry.get(latest::PATH)
        && let Ok(l) = latest::parse(&String::from_utf8_lossy(&got.body), &ctx.registry.key)
    {
        remember(ctx, &l);
    }
    for m in &r.modules {
        if let Some(src) = Source::parse(m) {
            let _ = fetch::module(ctx, &src, true, Prefer::Network);
        }
    }
    Ok(Done::ok(View::Silent))
}
