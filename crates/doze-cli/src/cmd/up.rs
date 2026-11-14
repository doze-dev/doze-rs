//! `doze up [service…]` (Go cmd/doze/up.go): what to start (the names given, or with none the choice of
//! [`remember::choose`]), remembered for the next bare `up`, the daemon started when it is not running (D1's `ctl`,
//! under the stack lock `down` takes too), then each one and everything it needs asked to come up (op `up`: converge,
//! then wake), needs first, a line for each as it is ready. Then it returns: the daemon supervises the stack in the
//! background. `-f` then streams the process logs until Ctrl-C, which only detaches.
//!
//! Better than Go: `-f` with `--json` is refused before anything is asked or remembered (Go remembered the run, then
//! refused it); two `up`s at once start one daemon (the stack lock; Go had none); `--dry-run` shows the start.

use crate::cli::args::Up;
use crate::cmd::ask::Asker;
use crate::cmd::{dry, logs, remember, verbs};
use crate::ctx::Ctx;
use crate::diag::{Diagnostic, ids};
use crate::notice::Notices;
use crate::output::{Done, Mode, Outcome};
use crate::stack::{self, Stack};
use crate::view::View;
use doze_config::Config;
use doze_config::model::COMMAND_ENGINE;
use doze_control::{Connection, Snapshot, Subscription, Topic, Verb};
use doze_daemon::ctl::{self, RunDir, StackLock};

pub(crate) fn run(ctx: &mut Ctx<'_>, mode: &Mode, a: &Up, notices: &mut Notices) -> Outcome {
    if a.follow && mode.json {
        return Err(Diagnostic::new(
            ids::FLAGS_DISAGREE,
            "-f streams logs; it can't be combined with --json",
        )
        .fix("doze up --json, then doze logs -f --json"));
    }
    let stack = Stack::find(ctx, mode)?;
    let cfg = stack.loader.shape(&stack.project)?;
    let asked = stack::unique(&a.services);
    check(&cfg, &asked, a)?;
    let conn = stack.open(mode.timeout)?;
    let snap = conn.as_ref().map(stack::snapshot).transpose()?;
    let dir = remember::record_dir(&cfg, stack.key.dir());
    let targets = if asked.is_empty() {
        let quiet = Mode {
            json: mode.json || a.dry.dry_run, // a plan never asks
            ..mode.clone()
        };
        remember::choose(ctx, &quiet, notices, (&cfg, &dir), (a.last, a.all))
    } else {
        asked.clone()
    };
    let closure = remember::closure(&cfg, &targets);
    if a.dry.dry_run {
        let shown = snap.unwrap_or_else(|| Snapshot {
            instances: crate::cmd::declared::views(&cfg),
            ..Snapshot::default()
        });
        let starts = conn.is_none();
        let plan = dry::wake_as("doze up", &shown, &closure, (&targets, &asked), starts);
        return Ok(Done::ok(View::Plan(plan)));
    }
    remember::remember_up(ctx, notices, (&cfg, &dir), &targets, snap.as_ref());
    let conn = match conn {
        Some(c) => c,
        None => start(ctx, mode, &stack)?,
    };
    boot(ctx, mode, (&stack, &cfg), conn, (&closure, a.follow))
}

/// Starts the stack's daemon (Go `startDaemon`) under the stack lock, unless another `up` started it while this one
/// waited for the lock, and connects to it.
fn start(ctx: &Ctx<'_>, mode: &Mode, stack: &Stack) -> Result<Connection, Diagnostic> {
    let run = RunDir::of(stack.key.dir());
    let refused = |what: String| {
        Diagnostic::new(ids::DAEMON_REFUSED, what)
            .fix("doze bug writes a report with the daemon's log")
    };
    let _lock = StackLock::take(&run).map_err(|e| {
        refused(format!(
            "could not take the stack's lock in {}: {e}",
            run.path().display()
        ))
    })?;
    if let Some(c) = stack.open(mode.timeout)? {
        return Ok(c);
    }
    let exe = ctx.host.daemon_exe().ok_or_else(|| {
        refused("could not find this doze program to start the daemon with".into())
    })?;
    let config = ctx.cwd.join(
        stack
            .project
            .source
            .as_ref()
            .unwrap_or(&stack.project.config),
    );
    let vars: Vec<(String, String)> = mode.vars.clone().into_iter().collect();
    let start = ctl::Start {
        exe: &exe,
        config: &config,
        vars: &vars,
    };
    ctl::start(&run, &start).map_err(|e| refused(format!("the doze daemon did not start: {e}")))?;
    stack.require(mode.timeout, "doze up")
}

/// Asks for each of `closure` in turn, a line for each, then the background line, or the logs with `-f`.
fn boot(
    ctx: &mut Ctx<'_>,
    mode: &Mode,
    (stack, cfg): (&Stack, &Config),
    conn: Connection,
    (closure, follows): (&[String], bool),
) -> Outcome {
    let ask = Asker::new(conn, ctx, mode);
    let label = |name: &str| {
        if cfg.task(name).is_some() {
            ("done", "done")
        } else {
            ("awake", "awake")
        }
    };
    let done = verbs::timed(ctx, mode, (stack, &ask), Verb::Up, closure, &label)?;
    let (View::Verbs(r), None) = (&done.view, &done.problem) else {
        return Ok(done);
    };
    let p = mode.out();
    if !follows {
        let line = format!(
            "{} up — supervised in the background. {}",
            p.dim("›"),
            p.dim("doze for the dash · doze down to stop")
        );
        r.say(ctx.out, &line);
        return Ok(done);
    }
    follow(ctx, mode, &ask, &processes(cfg, closure));
    Ok(done)
}

/// Go's checks, in Go's order: the names are declared (exit 4), `--last`/`--all` go with a bare `up` (exit 2), and a
/// bare `up` has something to start.
fn check(cfg: &Config, asked: &[String], a: &Up) -> Result<(), Diagnostic> {
    if !asked.is_empty() {
        stack::check_declared(&crate::cmd::declared::names(cfg), asked)?;
        if a.last || a.all {
            return Err(Diagnostic::new(
                ids::FLAGS_DISAGREE,
                "--last and --all choose for a bare doze up; drop them, or the names",
            )
            .fix(format!("doze up {}", asked.join(" "))));
        }
        return Ok(());
    }
    if remember::enabled_names(cfg).is_empty() {
        return Err(Diagnostic::new(
            ids::NOTHING_TO_DO,
            "nothing to bring up (no enabled services declared)",
        )
        .fix("doze add <module>"));
    }
    Ok(())
}

/// The process services among `closure`, in its order (Go `processNames`).
fn processes(cfg: &Config, closure: &[String]) -> Vec<String> {
    closure
        .iter()
        .filter(|n| cfg.instance(n).is_some_and(|i| i.ty == COMMAND_ENGINE))
        .cloned()
        .collect()
}

/// `up -f`: the process logs as they are written, until Ctrl-C, which only detaches (the stack keeps running). A
/// stream that ends early is said; `up` itself succeeded.
fn follow(ctx: &mut Ctx<'_>, mode: &Mode, ask: &Asker, procs: &[String]) {
    let p = mode.out();
    if procs.is_empty() {
        let line = "up — no process services to follow; the dash (doze) has the backend logs";
        let _ = writeln!(ctx.out, "{}", p.dim(line));
        return;
    }
    let line = "streaming logs — press Ctrl-C to detach (the stack keeps running)";
    let _ = writeln!(ctx.out, "{}", p.dim(line));
    let sub = Subscription {
        names: procs.to_vec(),
        backlog: -1,
        ..Subscription::to(&[Topic::Logs])
    };
    let page = logs::Page::of(ctx, procs);
    let ended = logs::follow(ctx, mode, &ask.conn, &sub, None, &page);
    let problem = match ended {
        Ok(d) => d.problem,
        Err(d) => Some(d),
    };
    if let Some(d) = problem {
        let e = mode.err();
        let _ = writeln!(ctx.err, "{} log stream ended: {}", e.fail("✗"), d.message);
    }
    let line = "detached — the stack keeps running. doze down to stop";
    let _ = writeln!(ctx.out, "\n{}", p.dim(line));
}
