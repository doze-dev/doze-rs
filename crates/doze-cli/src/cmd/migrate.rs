//! `doze migrate <name>`: a module instance's data moved to the major version doze.hcl now declares. The move is the
//! module's own (protocol call `migrate`: postgres dumps with the old major, sets the old folder aside, restores into
//! the new one); the daemon stops the instance for it and puts it back. Nothing module-specific is here.
//!
//! Go's `migrate` was postgres's code in the CLI and wanted the stack down. Here the stack may be up: the daemon reads
//! doze.hcl when it starts, so when the block changed since then (the version edit, read from `run/blocks.json`) the
//! daemon is restarted first, under the stack lock, and what was awake wakes again. With doze stopped it is started
//! for the move. `--dry-run` says which of those would happen, and the move; nothing is started or asked.

use crate::cli::args::OneName;
use crate::cmd::ask::{Asker, Stop};
use crate::cmd::progress::Boot;
use crate::cmd::sync::Drift;
use crate::cmd::{down, up};
use crate::ctx::Ctx;
use crate::diag::{Diagnostic, ids};
use crate::output::{Done, Mode, Outcome};
use crate::plan::{Op as Step, Plan};
use crate::stack::{self, Stack};
use crate::view::View;
use doze_config::Kind;
use doze_control::{Connection, Error, Op, Request, Verb};
use serde::Serialize;
use std::time::Duration;

/// A move reads and writes all of the data, and may fetch two engine builds first: minutes, not seconds.
const MIGRATE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// What `migrate` did: the instance and what its module said of the move.
#[derive(Debug, Serialize)]
pub struct Migrated {
    pub name: String,
    pub result: String,
    /// The daemon was restarted first, to read doze.hcl as it is now.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub restarted: bool,
    /// doze was stopped: the daemon was started for the move and stopped after it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub started: bool,
}

impl Migrated {
    pub(crate) fn human(&self, p: crate::paint::Paint) -> String {
        let mut out = format!("{} {} migrated\n", p.ok("✓"), self.name);
        for line in self.result.lines().filter(|l| !l.trim().is_empty()) {
            out.push_str(&format!("  {}\n", p.dim(line)));
        }
        out
    }
}

/// What has to happen to the daemon before it is asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Before {
    /// It runs doze.hcl as it is now.
    Nothing,
    /// doze is stopped.
    Start,
    /// It started before the block was changed.
    Restart,
}

pub(crate) fn run(ctx: &mut Ctx<'_>, mode: &Mode, a: &OneName) -> Outcome {
    let name = a.name.clone();
    let stack = Stack::find(ctx, mode)?;
    let cfg = stack.loader.shape(&stack.project)?;
    stack::check_declared(
        &crate::cmd::declared::names(&cfg),
        std::slice::from_ref(&name),
    )?;
    let (ty, version) = module_of(&cfg, &name)?;
    let conn = stack.open(mode.timeout)?;
    // A Go daemon (no `plan` op) has no migrate call: refused before anything, its blocks record (Go's prints are made
    // another way) never read as a reason to stop it.
    if let Some(c) = &conn
        && crate::cmd::sync::planned(c, &name)
            .map_err(|e| stack::daemon_error(&e, None))?
            .is_none()
    {
        return Err(go_daemon(&name));
    }
    let before = match &conn {
        None => Before::Start,
        Some(_) if Drift::of(&stack, &cfg).differs(&name) => Before::Restart,
        Some(_) => Before::Nothing,
    };
    if a.dry.dry_run {
        return Ok(Done::ok(View::Plan(plan(&name, (&ty, &version), before))));
    }
    let (conn, again) = match (conn, before) {
        (Some(c), Before::Nothing) => (c, Vec::new()),
        (Some(c), _) => restart(ctx, mode, &stack, c, &name)?,
        (None, _) => (
            up::start_for(ctx, mode, &stack, "doze migrate")?,
            Vec::new(),
        ),
    };
    let answer = moved(ctx, mode, (&stack, conn), &name, &again);
    let started = before == Before::Start;
    if started {
        // Started for the move only: stopped again, as `sync` stops what it starts (Go's migrate ran with doze down).
        crate::cmd::sync::stop(ctx, &stack)?;
    }
    let answer = answer?;
    Ok(Done::ok(View::Migrated(Migrated {
        name,
        result: answer.result,
        restarted: before == Before::Restart,
        started,
    })))
}

/// `name`'s module type and declared version; refused for what is not a module instance.
fn module_of(cfg: &doze_config::Config, name: &str) -> Result<(String, String), Diagnostic> {
    match cfg.instance(name) {
        Some(i) if i.kind() == Kind::Module => Ok((i.ty.clone(), i.version.clone())),
        Some(i) => {
            let kind = i.kind().as_str();
            Err(Diagnostic::new(
                ids::NOTHING_TO_DO,
                format!(
                    "{name} is {} {kind}; doze migrate moves a module's data between majors",
                    crate::cmd::connect::article(kind)
                ),
            )
            .fix(format!("doze reset {name} starts it fresh instead")))
        }
        None => Err(stack::not_in(&crate::cmd::declared::names(cfg), name)),
    }
}

/// Asks for the move and waits for it (Ctrl-C ends the wait), with its progress; then wakes `again` (what a restart
/// stopped). The module's answer, or why there is none. The connection is closed on return.
fn moved(
    ctx: &mut Ctx<'_>,
    mode: &Mode,
    (stack, conn): (&Stack, Connection),
    name: &str,
    again: &[String],
) -> Result<doze_control::Response, Diagnostic> {
    let ask = Asker::new(conn, ctx, mode);
    let log = (stack.run_dir().log_file(), ctx.term.stderr && !mode.json);
    let mut boot = Boot::begin_as(ctx.err, mode.err(), log, name, "migrating");
    let asked = ask.call_while(
        Request::on(Op::Migrate, name),
        mode.timeout.max(MIGRATE_TIMEOUT),
        &mut || boot.poll(ctx.err),
    );
    boot.end(ctx.err);
    let answer = asked.map_err(|stop| match stop {
        Stop::Interrupted => Diagnostic::new(
            ids::INTERRUPTED,
            format!("stopped waiting for {name}'s move"),
        )
        .note("the daemon finishes the move; doze logs shows how it went")
        .fix(format!("doze logs {name}")),
        Stop::Failed(Error::Daemon { message, .. }) if message.starts_with("unknown op") => {
            go_daemon(name)
        }
        // The module and the daemon name the instance in their words already.
        Stop::Failed(e) => stack::daemon_error(&e, None),
    });
    // What was awake before a restart wakes again once the move is over (before it, a dependant would wake the
    // instance on the new major with the old data).
    for n in again {
        if let Err(stop) = ask.verb(Verb::Wake, n) {
            let why = match stop {
                Stop::Interrupted => "interrupted".to_owned(),
                Stop::Failed(e) => stack::daemon_error(&e, Some(n)).message,
            };
            let _ = writeln!(
                ctx.err,
                "{}",
                mode.err().warn(&format!("{n} did not wake again: {why}"))
            );
        }
    }
    answer
}

/// `--dry-run`: the daemon's start or restart, then the move.
fn plan(name: &str, (ty, version): (&str, &str), before: Before) -> Plan {
    let mut plan = Plan::new(format!("doze migrate {name}"));
    match before {
        Before::Nothing => {}
        Before::Start => plan.step(
            "doze",
            Step::Do,
            "start the daemon",
            "for the move, and stopped again after it",
        ),
        Before::Restart => plan.step(
            "doze",
            Step::Do,
            "restart the daemon",
            format!("{name}'s block changed since it started; what is awake wakes again"),
        ),
    }
    let to = if version.is_empty() {
        ty.to_owned()
    } else {
        format!("{ty} {version}")
    };
    plan.step(
        name,
        Step::Do,
        "migrate",
        format!(
            "its module moves the data to {to}, as doze.hcl declares; {name} is stopped meanwhile"
        ),
    );
    plan
}

/// Stops the stack's daemon and starts it again from doze.hcl as it is now, under the stack lock; the new connection
/// and what was awake (but `name`), for waking again after the move. What a restart does not carry over is said.
fn restart(
    ctx: &mut Ctx<'_>,
    mode: &Mode,
    stack: &Stack,
    conn: Connection,
    name: &str,
) -> Result<(Connection, Vec<String>), Diagnostic> {
    let _lock = stack::lock(ctx, stack)?;
    let snap = stack::snapshot(&conn)?;
    let awake: Vec<String> = snap
        .instances
        .iter()
        .filter(|v| v.declared && v.name != name && crate::cmd::status::awake(&v.state))
        .map(|v| v.name.clone())
        .collect();
    let lost: Vec<String> = snap
        .instances
        .iter()
        .filter_map(|v| match () {
            () if !v.declared => Some(format!("{} (added with doze add)", v.name)),
            () if v.keep_awake => Some(format!("{}'s pin", v.name)),
            () if v.state == "off" => Some(format!("{} being off", v.name)),
            () => None,
        })
        .collect();
    drop(conn);
    let pid = down::identify(&stack.key).ok_or_else(|| {
        Diagnostic::new(
            ids::DAEMON_REFUSED,
            format!("{name}'s block changed since the daemon started, and its pid file names no daemon doze started"),
        )
        .note("the daemon reads doze.hcl when it starts")
        .fix("doze down && doze up, then doze migrate again")
    })?;
    if !down::stop(pid)? {
        return Err(Diagnostic::new(
            ids::DAEMON_REFUSED,
            "the daemon could not be restarted (it is another user's)",
        )
        .fix("doze down && doze up, then doze migrate again"));
    }
    down::clear(&down::leftovers(&stack.key));
    let conn = up::start(ctx, mode, stack).map_err(|d| {
        d.note("the old daemon was stopped: the stack is down now")
            .fix("doze up")
    })?;
    if !mode.quiet {
        let mut line =
            format!("restarted the doze daemon: {name}'s block changed since it started");
        if !lost.is_empty() {
            line.push_str(&format!("; not carried over: {}", lost.join(", ")));
        }
        let _ = writeln!(ctx.err, "{}", mode.err().dim(&line));
    }
    Ok((conn, awake))
}

/// A daemon without the `migrate` call (Go's: its migrate was the CLI's own, with the stack down).
fn go_daemon(name: &str) -> Diagnostic {
    Diagnostic::new(
        ids::DAEMON_REFUSED,
        format!(
            "this stack's daemon cannot migrate {name} (it has no migrate call: a Go doze daemon)"
        ),
    )
    .fix("doze down && doze up runs the stack on this doze; then doze migrate again")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plan_says_what_happens_to_the_daemon_first() {
        let p = crate::paint::Paint::new(false);
        let restart = plan("db", ("postgres", "17"), Before::Restart).render(p);
        assert!(restart.contains("restart the daemon"), "{restart}");
        assert!(
            restart.contains("its module moves the data to postgres 17"),
            "{restart}"
        );
        let start = plan("db", ("postgres", "17"), Before::Start).render(p);
        assert!(start.contains("start the daemon"), "{start}");
        let nothing = plan("db", ("postgres", ""), Before::Nothing).render(p);
        assert!(!nothing.contains("daemon"), "{nothing}");
        assert!(nothing.contains("moves the data to postgres,"), "{nothing}");
    }
}
