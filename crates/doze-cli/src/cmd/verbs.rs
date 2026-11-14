//! The verbs on one service at a time (Go cmd/doze/verbs.go): `wake` (`boot`), `sleep` (`reap`), `restart`, `reset`,
//! `off`, `on`. Each asks the daemon, one service after another in the order given, and says what happened to each
//! as it happens; the first that fails stops the run. Every one takes `--dry-run`: the plan, from the daemon's
//! snapshot, and nothing asked ([`crate::cmd::dry`]).
//!
//! The Go doze starts the daemon for `wake`, `restart`, `reset` and `pin` when it is not running; starting it is the
//! core's (`up`), so here those say the stack is not running (exit 7) until the Rust daemon lands.

use crate::cli::args::{Names, SomeNames};
use crate::cmd::dry;
use crate::cmd::ready::Ready;
use crate::cmd::report::Report;
use crate::ctx::Ctx;
use crate::diag::{Diagnostic, ids};
use crate::output::{Done, Mode, Outcome};
use crate::stack::{self, Stack};
use crate::view::View;
use doze_control::{Connection, Verb};
use std::time::Instant;

/// `doze wake [name…]`: starts each, with what it needs, and waits until it is ready.
pub(crate) fn wake(ctx: &mut Ctx<'_>, mode: &Mode, a: &Names) -> Outcome {
    let (conn, snap) = open(ctx, mode, "doze wake")?;
    let targets = stack::targets(&snap, &a.names)?;
    if targets.is_empty() {
        return Err(Diagnostic::new(
            ids::NO_SUCH_SERVICE,
            "nothing to wake: the stack declares no services",
        )
        .fix("doze add <module>"));
    }
    if a.dry.dry_run {
        return Ok(Done::ok(View::Plan(dry::wake(
            &snap,
            &targets,
            "doze wake",
            &a.names,
        ))));
    }
    timed(ctx, mode, &conn, Verb::Wake, &targets, ("awake", "awake"))
}

/// `doze restart <name…>`.
pub(crate) fn restart(ctx: &mut Ctx<'_>, mode: &Mode, a: &SomeNames) -> Outcome {
    let (conn, snap) = open(ctx, mode, "doze restart")?;
    stack::check_names(&snap, &a.names)?;
    if a.dry.dry_run {
        return Ok(Done::ok(View::Plan(dry::restart(&snap, &a.names))));
    }
    timed(
        ctx,
        mode,
        &conn,
        Verb::Restart,
        &a.names,
        ("restarted", "restarted"),
    )
}

/// Runs a verb that starts something on each target, with how long each took to be ready.
fn timed(
    ctx: &mut Ctx<'_>,
    mode: &Mode,
    conn: &Connection,
    verb: Verb,
    targets: &[String],
    (state, line): (&str, &str),
) -> Outcome {
    let mut r = Report::new(mode);
    let mut ready = Ready::watch(conn);
    for name in targets {
        let start = Instant::now();
        if let Err(e) = conn.run_verb(verb, name) {
            ready.stop();
            let d = r.fail(name, &e);
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
        // Nothing runs, so nothing is awake: done, with no results.
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
    stack::check_names(&snap, &a.names)?;
    if a.dry.dry_run {
        return Ok(Done::ok(View::Plan(dry::sleep(&snap, &a.names))));
    }
    let mut r = Report::new(mode);
    if a.names.is_empty() {
        let awake: Vec<String> = stack::declared(&snap)
            .filter(|v| matches!(v.state.as_str(), "active" | "idle" | "booting"))
            .map(|v| v.name.clone())
            .collect();
        if let Err(e) = conn.run_verb(Verb::Sleep, "") {
            return Err(stack::daemon_error(&e, None));
        }
        for n in &awake {
            r.quiet(n, "asleep");
        }
        r.say(
            ctx.out,
            &format!("{} everything is asleep", mode.out().ok("✓")),
        );
        return Ok(Done::ok(View::Verbs(r)));
    }
    each(ctx, &conn, r, Verb::Sleep, &a.names, ("asleep", "asleep"))
}

/// `doze reset <name…>`: deletes the data; asks first, or needs `-y` with no terminal.
pub(crate) fn reset(ctx: &mut Ctx<'_>, mode: &Mode, a: &SomeNames) -> Outcome {
    let (conn, snap) = open(ctx, mode, "doze reset")?;
    stack::check_names(&snap, &a.names)?;
    if a.dry.dry_run {
        return Ok(Done::ok(View::Plan(dry::reset(&snap, &a.names))));
    }
    let r = Report::new(mode);
    if !mode.yes {
        let names = a.names.join(", ");
        let loses = if a.names.len() == 1 {
            format!("{names} loses its data; it starts fresh on its next wake")
        } else {
            format!("{names} lose their data; each starts fresh on its next wake")
        };
        let asked = if ctx.interactive() && !mode.json {
            let _ = write!(
                ctx.err,
                "Reset {names}? The data is deleted and provisioned fresh on the next wake. [y/N] "
            );
            let _ = ctx.err.flush();
            ctx.host.ask()
        } else {
            None
        };
        match asked {
            Some(true) => {}
            Some(false) => {
                r.say(ctx.out, &mode.out().dim("nothing was reset"));
                return Ok(Done::ok(View::Verbs(r)));
            }
            None => {
                return Err(Diagnostic::new(
                    ids::NEEDS_YES,
                    "reset deletes data and will not guess",
                )
                .note(loses)
                .fix(format!(
                    "doze reset -y {} · --dry-run shows what goes",
                    a.names.join(" ")
                )));
            }
        }
    }
    let line = "reset — its data is gone; the next wake provisions it fresh";
    each(ctx, &conn, r, Verb::Reset, &a.names, ("reset", line))
}

/// `doze off <name…>` and `doze on <name…>`.
pub(crate) fn on_off(ctx: &mut Ctx<'_>, mode: &Mode, a: &SomeNames, off: bool) -> Outcome {
    let what = if off { "doze off" } else { "doze on" };
    let (conn, snap) = open(ctx, mode, what)?;
    stack::check_names(&snap, &a.names)?;
    if a.dry.dry_run {
        return Ok(Done::ok(View::Plan(dry::on_off(&snap, &a.names, off))));
    }
    let r = Report::new(mode);
    let (verb, state) = if off {
        (Verb::Off, "off")
    } else {
        (Verb::On, "on")
    };
    each(ctx, &conn, r, verb, &a.names, (state, state))
}

/// Runs a verb on each name in turn, a line for each.
fn each(
    ctx: &mut Ctx<'_>,
    conn: &Connection,
    mut r: Report,
    verb: Verb,
    names: &[String],
    (state, line): (&str, &str),
) -> Outcome {
    for name in names {
        if let Err(e) = conn.run_verb(verb, name) {
            let d = r.fail(name, &e);
            return Ok(Done {
                view: View::Verbs(r),
                problem: Some(d),
            });
        }
        r.ok(ctx.out, name, state, line, std::time::Duration::ZERO);
    }
    Ok(Done::ok(View::Verbs(r)))
}

/// The stack's daemon and its snapshot, for a verb that needs it running.
pub(crate) fn open(
    ctx: &Ctx<'_>,
    mode: &Mode,
    what: &str,
) -> Result<(Connection, doze_control::Snapshot), Diagnostic> {
    let stack = Stack::find(ctx, mode)?;
    let conn = stack.require(mode.timeout, what)?;
    let snap = stack::snapshot(&conn)?;
    Ok((conn, snap))
}
