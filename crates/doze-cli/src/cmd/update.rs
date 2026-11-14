//! `doze update [--dry-run]` and `doze __refresh`, the background check behind the update line and the module copies.
//!
//! `--dry-run` is real now: the newest doze (from the state file when it is under a day old, else the registry's
//! release file, within the budget) and how this install would take it. Updating itself waits for the core: the new
//! doze must take the running daemon over without stopping services (compat.md rule 5). Exits 0; 10 the newest is
//! not known (the network, or a registry that publishes no release file yet); 1 the update itself, not built.

use super::fetch::{self, Prefer};
use super::stub::{Needs, not_built};
use crate::cli::setup::Refresh;
use crate::ctx::{Ctx, VERSION};
use crate::diag::{Diagnostic, ids};
use crate::install::Install;
use crate::output::{Done, Outcome};
use crate::plan::{Op, Plan};
use crate::registry::FetchError;
use crate::registry::latest::{self, Latest};
use crate::registry::source::Source;
use crate::update::{self, newer};
use crate::view::View;

pub(crate) fn run(ctx: &mut Ctx<'_>, dry_run: bool) -> Outcome {
    let install = Install::of(ctx.host.exe().as_deref());
    if !dry_run {
        return Err(
            not_built("doze update", Needs::Handover).fix(format!("until then: {}", install.how()))
        );
    }
    let latest = newest(ctx)?;
    let mut plan = Plan::new("doze update");
    if newer(&latest.version, VERSION) {
        let title = format!("doze {VERSION} → {}", latest.version);
        plan.step(
            &title,
            Op::Do,
            format!("install doze {}", latest.version),
            format!("through {}: {}", install.name(), install.how()),
        );
        plan.step(
            &title,
            Op::Do,
            "hand the running daemon over to it",
            "services keep running",
        );
        for news in &latest.news {
            plan.step(&title, Op::Add, news.clone(), "");
        }
    } else {
        plan.unchanged(&format!("doze {VERSION} is the newest"));
    }
    Ok(Done::ok(View::Plan(plan)))
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
