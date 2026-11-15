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
    let instance = cfg
        .instance(&name)
        .map(|i| (i.kind(), i.ty.clone(), i.version.clone()));
    let Some((Kind::Module, ty, version)) = instance else {
        let kind = instance.map_or("", |(k, _, _)| k.as_str());
        return Err(Diagnostic::new(
            ids::NOTHING_TO_DO,
            format!(
                "{name} is {} {kind}; doze migrate moves a module's data between majors",
                article(kind)
            ),
        )
        .fix(format!("doze reset {name} starts it fresh instead")));
    };
    let conn = stack.open(mode.timeout)?;
    let before = match &conn {
        None => Before::Start,
        // Only a daemon that recorded what it started with (every Rust one) is compared; one without the record (Go's)
        // has no migrate call either, and says so.
        Some(_) if Drift::of(&stack, &cfg).differs(&name) => Before::Restart,
        Some(_) => Before::Nothing,
    };
    if a.dry.dry_run {
        return Ok(Done::ok(View::Plan(plan(&name, (&ty, &version), before))));
    }
    let (conn, restarted) = match (conn, before) {
        (Some(c), Before::Nothing) => (c, false),
        (Some(c), _) => (restart(ctx, mode, &stack, c, &name)?, true),
        (None, _) => (up::start_for(ctx, mode, &stack, "doze migrate")?, false),
    };
    let ask = Asker::new(conn, ctx, mode);
    let log = (stack.run_dir().log_file(), ctx.term.stderr && !mode.json);
    let mut boot = Boot::begin_as(ctx.err, mode.err(), log, &name, "migrating");
    let asked = ask.call_while(
        Request::on(Op::Migrate, &name),
        MIGRATE_TIMEOUT,
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
            go_daemon(&name)
        }
        Stop::Failed(e) => stack::daemon_error(&e, Some(&name)),
    })?;
    Ok(Done::ok(View::Migrated(Migrated {
        name,
        result: answer.result,
        restarted,
    })))
}

fn article(kind: &str) -> &'static str {
    if kind.starts_with(['a', 'e', 'i', 'o', 'u']) {
        "an"
    } else {
        "a"
    }
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
            "it supervises the stack in the background",
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

/// Stops the stack's daemon and starts it again from doze.hcl as it is now, under the stack lock, and wakes what was
/// awake again (but `name`, which the move stops anyway); the new connection.
fn restart(
    ctx: &mut Ctx<'_>,
    mode: &Mode,
    stack: &Stack,
    conn: Connection,
    name: &str,
) -> Result<Connection, Diagnostic> {
    let _lock = stack::lock(ctx, stack)?;
    let awake: Vec<String> = stack::snapshot(&conn)?
        .instances
        .iter()
        .filter(|v| v.declared && v.name != name && crate::cmd::status::awake(&v.state))
        .map(|v| v.name.clone())
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
    let conn = up::start(ctx, mode, stack)?;
    if !mode.quiet && !mode.json {
        let line = format!("restarted the doze daemon: {name}'s block changed since it started");
        let _ = writeln!(ctx.err, "{}", mode.err().dim(&line));
    }
    for n in &awake {
        if let Err(e) = conn.run_verb(Verb::Wake, n) {
            let line = format!(
                "{n} did not wake again: {}",
                stack::daemon_error(&e, Some(n)).message
            );
            let _ = writeln!(ctx.err, "{}", mode.err().warn(&line));
        }
    }
    Ok(conn)
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
