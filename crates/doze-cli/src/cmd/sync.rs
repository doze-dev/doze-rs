//! `doze sync [service] [-y] [--dry-run]` (Go cmd/doze/sync.go): the stack brought in line with doze.hcl, through the
//! daemon's `apply` (each module creates what is new, updates what changed, and drops what it applied before and is no
//! longer declared; a service turned off is left alone). Names are checked against doze.hcl first (exit 4).
//!
//! As Go: the change list first (the daemon's `plan` op: each module's own objects against what was applied, nothing
//! woken or written), then "no changes" and exit 0 when it is empty, else the question (a terminal; exit 9 with no
//! one to answer and no `-y`). `--dry-run` is the list. With doze stopped the daemon is started for it and stopped
//! after, as Go's own runtime was. A Go daemon has no `plan` op ("unknown op"): then every sync asks, and `--dry-run`
//! says it cannot be answered there.

use crate::cli::args::Sync;
use crate::ctx::Ctx;
use crate::diag::{Diagnostic, ids};
use crate::output::{Done, Mode, Outcome};
use crate::plan::{Op as Step, Plan};
use crate::stack::{self, Stack};
use crate::view::View;
use doze_control::{ChangeView, Connection, Error, Op, Request, Snapshot, Verb};

pub(crate) fn run(ctx: &mut Ctx<'_>, mode: &Mode, a: &Sync) -> Outcome {
    let names: Vec<String> = a.service.iter().cloned().collect();
    // The names first, as every command checks them (and doze.hcl, exit 5), before anything is asked or started.
    let stack = Stack::find(ctx, mode)?;
    stack::check_offline(&stack, &names)?;
    let started = stack.open(mode.timeout)?.is_none();
    let done = synced(ctx, mode, a, &names);
    if started {
        // Started for this sync: stopped again, as Go's own runtime stopped everything it ran.
        let stopped = stop(ctx, &stack);
        let done = done?;
        stopped?;
        return Ok(done);
    }
    done
}

/// Everything after the daemon is there: the plan, the question, the apply.
fn synced(ctx: &mut Ctx<'_>, mode: &Mode, a: &Sync, names: &[String]) -> Outcome {
    let target = a.service.clone().unwrap_or_default();
    let (_, _, conn) = crate::cmd::env::opened(ctx, mode, names, "doze sync")?;
    let snap = stack::snapshot(&conn)?;
    let subject = (!target.is_empty()).then_some(target.as_str());
    match planned(&conn, &target).map_err(|e| stack::daemon_error(&e, subject))? {
        Some(changes) => {
            let plan = plan(&snap, &changes, &target);
            if a.dry.dry_run {
                return Ok(Done::ok(View::Plan(plan)));
            }
            if changes.is_empty() {
                let line = "no changes — declared structure is up to date\n";
                return Ok(Done::ok(View::Raw(line.to_owned())));
            }
            if !mode.json {
                let _ = write!(ctx.out, "{}", plan.steps(mode.out()));
                let _ = ctx.out.flush();
            }
            if !mode.yes && !confirm(ctx, mode, &target, "Apply these changes?")? {
                return Ok(cancelled(ctx));
            }
        }
        None if a.dry.dry_run => return Err(go_daemon()),
        None => {
            let what = if target.is_empty() {
                "the stack"
            } else {
                &target
            };
            let question = format!(
                "Sync {what} with doze.hcl? What is new is created, what changed is updated, and what is no longer declared is dropped."
            );
            if !mode.yes && !confirm(ctx, mode, &target, &question)? {
                return Ok(cancelled(ctx));
            }
        }
    }
    conn.run_verb(Verb::Sync, &target)
        .map_err(|e| stack::daemon_error(&e, subject))?;
    let line = format!("{} sync complete", mode.out().ok("✓"));
    Ok(Done::ok(View::Raw(format!("{line}\n"))))
}

/// The daemon's change list for `target` (empty: the stack), or `None` from a daemon without the `plan` op (Go's).
fn planned(conn: &Connection, target: &str) -> Result<Option<Vec<ChangeView>>, Error> {
    match conn.call(Request::on(Op::Plan, target)) {
        Ok(r) => Ok(Some(r.changes)),
        Err(Error::Daemon { message, .. }) if message.starts_with("unknown op") => Ok(None),
        Err(e) => Err(e),
    }
}

/// The change list as a plan: a group per instance (`db (postgres)`), a step per object (`+ database shop`), in the
/// order apply makes them; each declared module that would not change is listed as having nothing to change.
pub(crate) fn plan(snap: &Snapshot, changes: &[ChangeView], target: &str) -> Plan {
    let title = |name: &str| match stack::declared(snap).find(|v| v.name == name) {
        Some(v) if !v.engine.is_empty() => format!("{name} ({})", v.engine),
        _ => name.to_owned(),
    };
    let mut plan = Plan::new(if target.is_empty() {
        "doze sync".to_owned()
    } else {
        format!("doze sync {target}")
    });
    for c in changes {
        let step = match c.change.as_str() {
            "create" => Step::Add,
            "delete" => Step::Remove,
            _ => Step::Change,
        };
        plan.step(
            &title(&c.instance),
            step,
            format!("{} {}", c.kind, c.name),
            "",
        );
    }
    let quiet = stack::declared(snap).filter(|v| {
        v.kind == "module"
            && v.state != "off"
            && (target.is_empty() || v.name == target)
            && !changes.iter().any(|c| c.instance == v.name)
    });
    for v in quiet {
        plan.unchanged(&title(&v.name));
    }
    plan
}

/// Asks on a terminal; refuses where no one can answer (exit 9).
fn confirm(
    ctx: &mut Ctx<'_>,
    mode: &Mode,
    target: &str,
    question: &str,
) -> Result<bool, Diagnostic> {
    if ctx.interactive() && !mode.json {
        let _ = write!(ctx.err, "{question} [y/N] ");
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

fn cancelled(ctx: &mut Ctx<'_>) -> Done {
    let _ = writeln!(ctx.out, "sync cancelled");
    Done::ok(View::Silent)
}

/// `--dry-run` against a daemon that cannot say what would change.
fn go_daemon() -> Diagnostic {
    Diagnostic::new(
        ids::DAEMON_REFUSED,
        "this stack's daemon cannot say what sync would change (it has no plan op: a Go doze daemon)",
    )
    .note("doze sync -y applies without the list")
    .fix("doze down && doze up runs the stack on this doze")
}

/// Stops the daemon this sync started, and clears what it left.
fn stop(ctx: &mut Ctx<'_>, stack: &Stack) -> Result<(), Diagnostic> {
    let _lock = stack::lock(ctx, stack)?;
    if let Some(pid) = crate::cmd::down::identify(&stack.key) {
        crate::cmd::down::stop(pid)?;
    }
    crate::cmd::down::clear(&crate::cmd::down::leftovers(&stack.key));
    Ok(())
}
