//! The verbs on one service at a time (Go cmd/doze/verbs.go): `wake` (`boot`), `sleep` (`reap`), `restart`, `reset`,
//! `off`, `on`. Each asks the daemon, one service after another, and says what happened to each as it happens; the
//! first that fails stops the run, and Ctrl-C stops it with exit 130 ([`crate::cmd::ask`]). A name given twice is
//! acted on once. Every one takes `--dry-run`: the plan, from the daemon's snapshot, and nothing asked
//! ([`crate::cmd::dry`]).
//!
//! The Go doze starts the daemon for `wake`, `restart`, `reset` and `pin` when it is not running; starting it is the
//! core's (`up`), so here those say the stack is not running (exit 7) until the Rust daemon lands.

use crate::cli::args::{Names, SomeNames};
use crate::cmd::ask::Asker;
use crate::cmd::dry;
use crate::cmd::ready::Ready;
use crate::cmd::report::Report;
use crate::ctx::Ctx;
use crate::diag::{Diagnostic, ids};
use crate::output::{Done, Mode, Outcome};
use crate::stack::{self, Stack};
use crate::view::View;
use doze_control::{Snapshot, Verb};
use std::time::{Duration, Instant};

/// `doze wake [name…]`: starts each with what it needs, needs first (Go `bootClosure`), and waits until each is
/// ready. With no name, every service doze.hcl does not turn off.
pub(crate) fn wake(ctx: &mut Ctx<'_>, mode: &Mode, a: &Names) -> Outcome {
    let (stack, ask, snap) = open(ctx, mode, "doze wake", &a.names)?;
    let asked = stack::unique(&a.names);
    let targets = stack::targets(&snap, &asked)?;
    if targets.is_empty() {
        return Err(Diagnostic::new(
            ids::NOTHING_TO_DO,
            "nothing to wake: the stack declares no services",
        )
        .fix("doze add <module>"));
    }
    let closure: Vec<String> = dry::wake_order(&snap, &targets)
        .into_iter()
        .map(str::to_owned)
        .collect();
    if a.dry.dry_run {
        return Ok(Done::ok(View::Plan(dry::wake(
            &snap, &closure, &targets, &asked,
        ))));
    }
    timed(
        ctx,
        mode,
        (&stack, &ask),
        Verb::Wake,
        &closure,
        ("awake", "awake"),
    )
}

/// `doze restart <name…>`.
pub(crate) fn restart(ctx: &mut Ctx<'_>, mode: &Mode, a: &SomeNames) -> Outcome {
    let (stack, ask, snap) = open(ctx, mode, "doze restart", &a.names)?;
    let names = stack::unique(&a.names);
    stack::check_names(&snap, &names)?;
    if a.dry.dry_run {
        return Ok(Done::ok(View::Plan(dry::restart(&snap, &names))));
    }
    let what = ("restarted", "restarted");
    timed(ctx, mode, (&stack, &ask), Verb::Restart, &names, what)
}

/// Runs a verb that starts something on each target, with how long each took to be ready (from the journal, on a
/// connection of its own so a busy journal never holds the verb's answer up).
fn timed(
    ctx: &mut Ctx<'_>,
    mode: &Mode,
    (stack, ask): (&Stack, &Asker),
    verb: Verb,
    targets: &[String],
    (state, line): (&str, &str),
) -> Outcome {
    let mut r = Report::new(mode);
    let mut ready = Ready::watch(stack);
    for name in targets {
        let start = Instant::now();
        if let Err(stop) = ask.verb(verb, name) {
            ready.stop();
            let d = r.stopped(name, &stop);
            return Ok(Done {
                view: View::Verbs(r),
                problem: Some(d),
            });
        }
        let took = ready.of(name, start.elapsed());
        r.ok(ctx.out, name, state, line, took);
    }
    ready.stop();
    Ok(Done::ok(View::Verbs(r)))
}

/// `doze sleep [name…]`: with no name, everything awake; the daemon keeps running.
pub(crate) fn sleep(ctx: &mut Ctx<'_>, mode: &Mode, a: &Names) -> Outcome {
    let stack = Stack::find(ctx, mode)?;
    let Some(conn) = stack.open(mode.timeout)? else {
        // Nothing runs, so nothing is awake: done, with no results, once the names are doze.hcl's (Go checks first).
        stack::check_offline(&stack, &stack::unique(&a.names))?;
        let r = Report::new(mode);
        r.say(
            ctx.out,
            &mode
                .out()
                .dim("nothing is awake (the daemon is not running)"),
        );
        return Ok(Done::ok(View::Verbs(r)));
    };
    let snap = stack::snapshot(&conn)?;
    let names = stack::unique(&a.names);
    stack::check_names(&snap, &names)?;
    if a.dry.dry_run {
        return Ok(Done::ok(View::Plan(dry::sleep(&snap, &names))));
    }
    let ask = Asker::new(conn, ctx, mode);
    let mut r = Report::new(mode);
    if !names.is_empty() {
        return each(ctx, &ask, r, Verb::Sleep, &names, ("asleep", "asleep"));
    }
    if let Err(stop) = ask.verb(Verb::Sleep, "") {
        let d = r.stopped("everything", &stop);
        return Ok(Done {
            view: View::Verbs(r),
            problem: Some(d),
        });
    }
    for v in
        stack::declared(&snap).filter(|v| matches!(v.state.as_str(), "active" | "idle" | "booting"))
    {
        r.quiet(&v.name, "asleep");
    }
    r.say(
        ctx.out,
        &format!("{} everything is asleep", mode.out().ok("✓")),
    );
    Ok(Done::ok(View::Verbs(r)))
}

/// `doze reset <name…>`: deletes the data; asks first, or needs `-y` with no terminal.
pub(crate) fn reset(ctx: &mut Ctx<'_>, mode: &Mode, a: &SomeNames) -> Outcome {
    let (_, ask, snap) = open(ctx, mode, "doze reset", &a.names)?;
    let names = stack::unique(&a.names);
    stack::check_names(&snap, &names)?;
    if a.dry.dry_run {
        return Ok(Done::ok(View::Plan(dry::reset(&snap, &names))));
    }
    let r = Report::new(mode);
    if !mode.yes {
        match confirm_reset(ctx, mode, &names) {
            Ok(true) => {}
            Ok(false) => {
                r.say(ctx.out, &mode.out().dim("nothing was reset"));
                return Ok(Done::ok(View::Verbs(r)));
            }
            Err(d) => return Err(d),
        }
    }
    let line = "reset — its data is gone; the next wake provisions it fresh";
    each(ctx, &ask, r, Verb::Reset, &names, ("reset", line))
}

/// Asks on a terminal; refuses (exit 9) where no one can answer.
fn confirm_reset(ctx: &mut Ctx<'_>, mode: &Mode, names: &[String]) -> Result<bool, Diagnostic> {
    let list = names.join(", ");
    if ctx.interactive() && !mode.json {
        let _ = write!(
            ctx.err,
            "Reset {list}? The data is deleted and provisioned fresh on the next wake. [y/N] "
        );
        let _ = ctx.err.flush();
        if let Some(yes) = ctx.host.ask() {
            return Ok(yes);
        }
    }
    let loses = if names.len() == 1 {
        format!("{list} loses its data; it starts fresh on its next wake")
    } else {
        format!("{list} lose their data; each starts fresh on its next wake")
    };
    Err(
        Diagnostic::new(ids::NEEDS_YES, "reset deletes data and will not guess")
            .note(loses)
            .fix(format!(
                "doze reset -y {} · --dry-run shows what goes",
                names.join(" ")
            )),
    )
}

/// `doze off <name…>` and `doze on <name…>`.
pub(crate) fn on_off(ctx: &mut Ctx<'_>, mode: &Mode, a: &SomeNames, off: bool) -> Outcome {
    let what = if off { "doze off" } else { "doze on" };
    let (_, ask, snap) = open(ctx, mode, what, &a.names)?;
    let names = stack::unique(&a.names);
    stack::check_names(&snap, &names)?;
    if a.dry.dry_run {
        return Ok(Done::ok(View::Plan(dry::on_off(&snap, &names, off))));
    }
    let r = Report::new(mode);
    let (verb, state) = if off {
        (Verb::Off, "off")
    } else {
        (Verb::On, "on")
    };
    each(ctx, &ask, r, verb, &names, (state, state))
}

/// Runs a verb on each name in turn, a line for each.
fn each(
    ctx: &mut Ctx<'_>,
    ask: &Asker,
    mut r: Report,
    verb: Verb,
    names: &[String],
    (state, line): (&str, &str),
) -> Outcome {
    for name in names {
        if let Err(stop) = ask.verb(verb, name) {
            let d = r.stopped(name, &stop);
            return Ok(Done {
                view: View::Verbs(r),
                problem: Some(d),
            });
        }
        r.ok(ctx.out, name, state, line, Duration::ZERO);
    }
    Ok(Done::ok(View::Verbs(r)))
}

/// The stack, a connection to its daemon that watches for Ctrl-C, and the snapshot, for a verb that needs it
/// running.
pub(crate) fn open(
    ctx: &Ctx<'_>,
    mode: &Mode,
    what: &str,
    names: &[String],
) -> Result<(Stack, Asker, Snapshot), Diagnostic> {
    let stack = Stack::find(ctx, mode)?;
    let Some(conn) = stack.open(mode.timeout)? else {
        // Go reads doze.hcl and checks the names before it finds the daemon down: a typo is exit 4, a broken
        // config exit 5, and only then "not running" (exit 7).
        stack::check_offline(&stack, &stack::unique(names))?;
        return Err(stack::down(what));
    };
    let snap = stack::snapshot(&conn)?;
    Ok((stack, Asker::new(conn, ctx, mode), snap))
}
