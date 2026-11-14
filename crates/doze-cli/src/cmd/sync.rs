//! `doze sync [service] [-y]` (Go cmd/doze/sync.go): the stack brought in line with doze.hcl, through the daemon's
//! `apply` (each module creates what is new, updates what changed, and drops what it applied before and is no longer
//! declared; a service turned off is left alone). Names are checked against doze.hcl first (exit 4). It can drop data,
//! so it asks on a terminal and refuses without one unless `-y` (exit 9), as Go does. With doze stopped the daemon is
//! started for it (Go ran a runtime of its own, then stopped it).
//!
//! Go showed the change list before asking, from each module's plan; that list is not on the wire yet, so `--dry-run`
//! says it waits for it (DZ0001) rather than show a plan it cannot make, and the question names what can change.

use crate::cli::args::Sync;
use crate::cmd::stub;
use crate::ctx::Ctx;
use crate::diag::{Diagnostic, ids};
use crate::output::{Done, Mode, Outcome};
use crate::view::View;
use doze_control::Verb;

pub(crate) fn run(ctx: &mut Ctx<'_>, mode: &Mode, a: &Sync) -> Outcome {
    let names: Vec<String> = a.service.iter().cloned().collect();
    // The names first, as every command checks them (and doze.hcl, exit 5), before anything is asked.
    let stack = crate::stack::Stack::find(ctx, mode)?;
    crate::stack::check_offline(&stack, &names)?;
    if a.dry.dry_run {
        return Err(stub::not_built("doze sync --dry-run", stub::Needs::Daemon).note(
            "the change list comes from each module's plan, which the daemon does not give yet; doze sync -y applies",
        ));
    }
    let target = a.service.clone().unwrap_or_default();
    if !mode.yes && !confirm(ctx, mode, &target)? {
        let _ = writeln!(ctx.out, "sync cancelled");
        return Ok(Done::ok(View::Silent));
    }
    crate::cmd::env::asked(ctx, mode, &names, "doze sync")?;
    let conn = stack.require(mode.timeout, "doze sync")?;
    conn.run_verb(Verb::Sync, &target).map_err(|e| {
        crate::stack::daemon_error(&e, (!target.is_empty()).then_some(target.as_str()))
    })?;
    let line = format!("{} sync complete", mode.out().ok("✓"));
    Ok(Done::ok(View::Raw(format!("{line}\n"))))
}

/// Asks on a terminal; refuses where no one can answer (exit 9).
fn confirm(ctx: &mut Ctx<'_>, mode: &Mode, target: &str) -> Result<bool, Diagnostic> {
    let what = if target.is_empty() {
        "the stack".to_owned()
    } else {
        target.to_owned()
    };
    if ctx.interactive() && !mode.json {
        let _ = write!(
            ctx.err,
            "Sync {what} with doze.hcl? What is new is created, what changed is updated, and what is no longer declared is dropped. [y/N] "
        );
        let _ = ctx.err.flush();
        if let Some(yes) = ctx.host.ask() {
            return Ok(yes);
        }
    }
    Err(Diagnostic::new(
        ids::NEEDS_YES,
        "sync needs confirmation and no one can answer here",
    )
    .note("it drops what was applied before and is no longer declared")
    .fix(format!("doze sync -y {target}").trim_end().to_owned()))
}
