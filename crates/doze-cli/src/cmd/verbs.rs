//! The verbs on one service at a time (Go cmd/doze/verbs.go): `wake` (`boot`), `sleep` (`reap`), `restart`, `reset`,
//! `off`, `on`. Each asks the daemon, one service after another, and says what happened to each as it happens; the
//! first that fails stops the run, and Ctrl-C stops it with exit 130 ([`crate::cmd::ask`]). A name given twice is
//! acted on once. Every one takes `--dry-run`: the plan, from the daemon's snapshot, and nothing asked
//! ([`crate::cmd::dry`]).
//!
//! `wake`, `restart` and `reset` (and `pin`, `task <name>`) start the daemon when it is not running, as Go's did
//! (`startDaemon`), under the stack lock and said on stderr ([`Looked`]); a dry run with doze stopped plans from
//! doze.hcl, the daemon's start first, and starts nothing. `sleep`, `off` and `on` do not start it (Go's did not).

use crate::cli::args::{Names, SomeNames};
use crate::cmd::ask::Asker;
use crate::cmd::dry;
use crate::cmd::progress::Boot;
use crate::cmd::ready::Ready;
use crate::cmd::report::Report;
use crate::ctx::Ctx;
use crate::diag::{Diagnostic, ids};
use crate::output::{Done, Mode, Outcome};
use crate::stack::{self, Stack};
use crate::view::View;
use doze_control::{Connection, Snapshot, Verb};
use std::time::{Duration, Instant};

/// `doze wake [name…]`: starts each with what it needs, needs first (Go `bootClosure`), and waits until each is
/// ready. With no name, every service doze.hcl does not turn off.
pub(crate) fn wake(ctx: &mut Ctx<'_>, mode: &Mode, a: &Names) -> Outcome {
    let asked = stack::unique(&a.names);
    let looked = Looked::new(ctx, mode, &asked)?;
    let snap = looked.snap.clone();
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
        let plan = dry::wake_as(
            "doze wake",
            &snap,
            &closure,
            (&targets, &asked),
            looked.starts(),
        );
        return Ok(Done::ok(View::Plan(plan)));
    }
    let (stack, ask) = looked.connect(ctx, mode, "doze wake")?;
    timed(ctx, mode, (&stack, &ask), Verb::Wake, &closure, &|_| {
        ("awake", "awake")
    })
}

/// `doze restart <name…>`.
pub(crate) fn restart(ctx: &mut Ctx<'_>, mode: &Mode, a: &SomeNames) -> Outcome {
    let names = stack::unique(&a.names);
    let looked = Looked::new(ctx, mode, &names)?;
    if a.dry.dry_run {
        let plan = dry::restart(&looked.snap, &names);
        return Ok(Done::ok(View::Plan(looked.planned(plan))));
    }
    let (stack, ask) = looked.connect(ctx, mode, "doze restart")?;
    let what = |_: &str| ("restarted", "restarted");
    timed(ctx, mode, (&stack, &ask), Verb::Restart, &names, &what)
}

/// Runs a verb that starts something on each target, with how long each took to be ready (from the journal, on a
/// connection of its own so a busy journal never holds the verb's answer up). `label` gives each target's state and
/// line (`up` says a task is done).
pub(crate) fn timed(
    ctx: &mut Ctx<'_>,
    mode: &Mode,
    (stack, ask): (&Stack, &Asker),
    verb: Verb,
    targets: &[String],
    label: &dyn Fn(&str) -> (&'static str, &'static str),
) -> Outcome {
    let mut r = Report::new(mode);
    let mut ready = Ready::watch(stack);
    // `up` says each boot as it goes (Go `bootOne`); the others say only the result.
    let log = (verb == Verb::Up).then(|| (stack.run_dir().log_file(), ctx.term.stderr));
    for name in targets {
        let start = Instant::now();
        let asked = match &log {
            Some(log) => {
                let mut boot = Boot::begin(ctx.err, mode.err(), log.clone(), name);
                let asked = ask.verb_while(verb, name, &mut || boot.poll(ctx.err));
                boot.end(ctx.err);
                asked
            }
            None => ask.verb(verb, name),
        };
        if let Err(stop) = asked {
            ready.stop();
            let d = r.stopped(name, &stop);
            return Ok(Done {
                view: View::Verbs(r),
                problem: Some(d),
            });
        }
        let took = ready.of(name, start.elapsed());
        let (state, line) = label(name);
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
    let names = stack::unique(&a.names);
    let looked = Looked::new(ctx, mode, &names)?;
    if a.dry.dry_run {
        let plan = dry::reset(&looked.snap, &names);
        return Ok(Done::ok(View::Plan(looked.planned(plan))));
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
    // Asked first, as Go asks: nothing is started for a reset the user declines.
    let (_, ask) = looked.connect(ctx, mode, "doze reset")?;
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

/// A stack looked at for a verb that starts its daemon when it is down (Go `startDaemon`): the names checked against
/// the running stack, or with doze stopped against doze.hcl (exit 4, a broken config exit 5), and the snapshot, which
/// with doze stopped is doze.hcl's instances, all asleep. Nothing is started until [`Looked::connect`], so a dry run
/// and a declined question start nothing.
pub(crate) struct Looked {
    stack: Stack,
    conn: Option<Connection>,
    pub(crate) snap: Snapshot,
}

impl Looked {
    pub(crate) fn new(ctx: &Ctx<'_>, mode: &Mode, names: &[String]) -> Result<Looked, Diagnostic> {
        let stack = Stack::find(ctx, mode)?;
        let conn = stack.open(mode.timeout)?;
        let snap = match &conn {
            Some(c) => {
                let snap = stack::snapshot(c)?;
                stack::check_names(&snap, names)?;
                snap
            }
            None => {
                stack::check_offline(&stack, names)?;
                let cfg = stack.loader.shape(&stack.project)?;
                Snapshot {
                    instances: crate::cmd::declared::views(&cfg),
                    ..Snapshot::default()
                }
            }
        };
        Ok(Looked { stack, conn, snap })
    }

    /// Whether running the verb would start the daemon first.
    pub(crate) fn starts(&self) -> bool {
        self.conn.is_none()
    }

    /// `plan`, with the daemon's start in front when the run would start it.
    pub(crate) fn planned(&self, plan: crate::plan::Plan) -> crate::plan::Plan {
        if self.starts() {
            dry::after_start(plan)
        } else {
            plan
        }
    }

    /// The stack and a connection to its daemon that watches for Ctrl-C, the daemon started when it is down.
    pub(crate) fn connect(
        self,
        ctx: &mut Ctx<'_>,
        mode: &Mode,
        what: &str,
    ) -> Result<(Stack, Asker), Diagnostic> {
        let conn = match self.conn {
            Some(c) => c,
            None => crate::cmd::up::start_for(ctx, mode, &self.stack, what)?,
        };
        Ok((self.stack, Asker::new(conn, ctx, mode)))
    }
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
