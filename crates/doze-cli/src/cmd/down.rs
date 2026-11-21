//! `doze down` (Go cmd/doze/down.go and internal/daemonctl `Stop`): the daemon is asked to stop with SIGTERM to the
//! exact pid its pid file names, and its own shutdown puts every service to sleep. Better than Go:
//! - the pid is signalled only when it is still the daemon: a live process that leads its own group (the daemon is
//!   started in a session of its own) and started no later than the pid file was written. Go signalled whatever held
//!   the pid, so a stale file could stop a stranger;
//! - after the stop, nothing is left (proposal rule 8): a leftover socket or pid file is removed and said;
//! - a socket that answers with no daemon pid behind it is refused, not reported as "not running" (Go's `Pid()` was 0
//!   and it said exit 0 while the daemon kept running).
//!
//! The run and `--dry-run` share one decision ([`decide`]), so the plan says exactly what the run would do.
//! The result is Go's: `{"stopped": true}` and "✓ doze stopped", or `{"stopped": false}` and "doze is not running";
//! `cleared` (the leftovers removed) is added. What was awake is remembered for the next `up` first (Go's
//! `rememberDown`, [`crate::cmd::remember`]).

use crate::cli::args::DryRun;
use crate::ctx::Ctx;
use crate::diag::{Diagnostic, ids};
use crate::notice::Notices;
use crate::output::{Done, Mode, Outcome};
use crate::paint::Paint;
use crate::plan::{Op, Plan};
use crate::stack::Stack;
use crate::view::View;
use doze_projects::StackKey;
use serde::Serialize;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

/// Go's budget: the daemon's shutdown reap is 15 s (a backend that ignores everything until SIGKILL), so wait past it.
const STOP_WAIT: Duration = Duration::from_secs(20);
const POLL: Duration = Duration::from_millis(100);
/// The daemon writes its pid file just after its socket accepts; a `down` in that instant waits this long for it.
const PID_WAIT: Duration = Duration::from_secs(2);
/// A process's start time is read to the second; the pid file is written just after the start.
const START_SLACK: Duration = Duration::from_secs(2);

/// What `down` did.
#[derive(Debug, Serialize)]
pub struct Stopped {
    /// False: it was not running.
    pub stopped: bool,
    /// Leftovers removed (a socket, a pid file). Not in Go's result; left out when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cleared: Vec<String>,
}

impl Stopped {
    pub(crate) fn human(&self, p: Paint) -> String {
        let mut out = String::new();
        for c in &self.cleared {
            out.push_str(&format!("{}\n", p.dim(&format!("removed a leftover {c}"))));
        }
        if self.stopped {
            out.push_str(&format!("{} doze stopped\n", p.ok("✓")));
        } else {
            out.push_str(&format!("{}\n", p.dim("doze is not running")));
        }
        out
    }
}

/// What `down` finds.
enum Found {
    /// The daemon, by its pid.
    Daemon(u32),
    /// A socket that answers, with no daemon pid behind it: refused.
    Unknown,
    /// Nothing running; these files are what a daemon that is gone left.
    Gone(Vec<(PathBuf, &'static str)>),
}

/// `doze down [--dry-run]`.
pub(crate) fn run(ctx: &mut Ctx<'_>, mode: &Mode, a: &DryRun, notices: &mut Notices) -> Outcome {
    let stack = Stack::find(ctx, mode)?;
    // Under the stack lock `up` and `run` take, so a stop never races a start or a boot (D1 audit). A stack that never
    // ran gets its run folder for the lock.
    let _lock = if a.dry_run {
        None
    } else {
        Some(crate::stack::lock(ctx, &stack)?)
    };
    let found = decide(&stack)?;
    if a.dry_run {
        return Ok(Done::ok(View::Plan(plan(&stack, &found)?)));
    }
    if !matches!(found, Found::Unknown) {
        remember(ctx, notices, &stack);
    }
    let stopped = match found {
        Found::Unknown => return Err(unknown()),
        Found::Gone(_) => false,
        Found::Daemon(pid) => stop(pid)?,
    };
    let cleared = clear(&leftovers(&stack.key));
    Ok(Done::ok(View::Stopped(Stopped { stopped, cleared })))
}

fn decide(stack: &Stack) -> Result<Found, Diagnostic> {
    if let Some(pid) = identify(&stack.key) {
        return Ok(Found::Daemon(pid));
    }
    if stack.read()?.is_some() {
        // A daemon that has just started answers a moment before its pid file is written; a pid file that names
        // another process is not waited for. A file that exists but names nothing yet is one being written.
        let waited = Instant::now();
        while waited.elapsed() < PID_WAIT && doze_projects::daemon_pid(&stack.key).is_none() {
            std::thread::sleep(POLL);
            if let Some(pid) = identify(&stack.key) {
                return Ok(Found::Daemon(pid));
            }
        }
        return Ok(identify(&stack.key).map_or(Found::Unknown, Found::Daemon));
    }
    Ok(Found::Gone(leftovers(&stack.key)))
}

/// The pid file's pid, if it is still the daemon that wrote it.
pub(crate) fn identify(key: &StackKey) -> Option<u32> {
    let pid = doze_projects::daemon_pid(key).filter(|p| p.alive)?.pid;
    let written = std::fs::metadata(key.pid_file())
        .and_then(|m| m.modified())
        .ok()?;
    let started = doze_os::process::started_at(pid)?;
    let started_before = started
        <= written
            .checked_add(START_SLACK)
            .unwrap_or(SystemTime::UNIX_EPOCH);
    (doze_os::process::leads_own_group(pid) && started_before).then_some(pid)
}

pub(crate) fn leftovers(key: &StackKey) -> Vec<(PathBuf, &'static str)> {
    [
        (key.socket(), "control socket"),
        (key.pid_file(), "pid file"),
    ]
    .into_iter()
    .filter(|(p, _)| p.exists())
    .collect()
}

fn unknown() -> Diagnostic {
    Diagnostic::new(
        ids::DAEMON_REFUSED,
        "a doze daemon answers for this stack, but its pid file (run/doze.pid in the stack's state folder) names no process doze started",
    )
    .fix("doze bug writes a report with the daemon's log")
}

/// What was awake, remembered first so the next `up` can offer it (Go `rememberDown`). A config that does not load
/// does not keep the stack from stopping (Go exited 5 and left running the stack the user asked to stop): a warning
/// names the config's error, and nothing is remembered.
fn remember(ctx: &Ctx<'_>, notices: &mut Notices, stack: &Stack) {
    let cfg = match stack.loader.shape(&stack.project) {
        Ok(cfg) => cfg,
        Err(d) => {
            notices.add(format!(
                "doze.hcl does not load ({}); down goes on, and what was awake is not remembered for the next up",
                d.message
            ));
            return;
        }
    };
    let snap = stack.read().ok().flatten().and_then(|c| c.snapshot().ok());
    let dir = crate::cmd::remember::record_dir(&cfg, stack.key.dir());
    crate::cmd::remember::remember_down(ctx, notices, (&cfg, &dir), snap.as_ref());
}

/// SIGTERM to exactly `pid`, then wait for it to go. False: the pid was not ours to signal after all (another user's,
/// since the check), so nothing was stopped.
pub(crate) fn stop(pid: u32) -> Result<bool, Diagnostic> {
    if let Err(err) = doze_os::process::terminate(pid) {
        return match err.kind() {
            std::io::ErrorKind::Unsupported => Err(Diagnostic::new(
                ids::NOT_BUILT,
                "`doze down` cannot stop the daemon on Windows yet: that comes with the Rust daemon's named pipe (core plan W)",
            )
            .fix("the Go doze does this today")),
            std::io::ErrorKind::PermissionDenied => Ok(false),
            _ if !doze_os::process::alive(pid) => Ok(true),
            _ => Err(Diagnostic::new(
                ids::DAEMON_REFUSED,
                format!("could not signal the doze daemon (pid {pid}): {err}"),
            )
            .fix("doze bug writes a report with the daemon's log")),
        };
    }
    let start = Instant::now();
    while doze_os::process::alive(pid) {
        if start.elapsed() > STOP_WAIT {
            return Err(Diagnostic::new(
                ids::TOO_SLOW,
                format!(
                    "the doze daemon (pid {pid}) did not stop within {}s",
                    STOP_WAIT.as_secs()
                ),
            )
            .fix("doze bug writes a report with its log; then doze down again"));
        }
        std::thread::sleep(POLL);
    }
    Ok(true)
}

/// Removes the files; says what it removed.
pub(crate) fn clear(files: &[(PathBuf, &'static str)]) -> Vec<String> {
    files
        .iter()
        .filter(|(path, _)| std::fs::remove_file(path).is_ok())
        .map(|(path, what)| format!("{what} ({})", path.display()))
        .collect()
}

/// `doze down --dry-run`: the same decision, said as a plan.
fn plan(stack: &Stack, found: &Found) -> Result<Plan, Diagnostic> {
    let mut plan = Plan::new("doze down");
    match found {
        Found::Unknown => return Err(unknown()),
        Found::Gone(files) => {
            for (path, what) in files {
                plan.step("leftovers", Op::Remove, *what, path.display().to_string());
            }
            plan.unchanged("the stack (doze is not running)");
        }
        Found::Daemon(pid) => {
            // The daemon answers or is starting; what is awake comes from its snapshot when it answers.
            if let Some(c) = stack.read()? {
                let snap = c
                    .snapshot()
                    .map_err(|e| crate::stack::daemon_error(&e, None))?;
                for v in snap
                    .instances
                    .iter()
                    .filter(|v| crate::cmd::status::awake(&v.state))
                {
                    plan.step(&v.name, Op::Do, "sleep", "its data stays");
                }
            }
            plan.step(
                "doze",
                Op::Remove,
                format!("stop the daemon (pid {pid})"),
                "nothing is left listening",
            );
        }
    }
    Ok(plan)
}
