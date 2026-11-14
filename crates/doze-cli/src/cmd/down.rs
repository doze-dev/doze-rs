//! `doze down` (Go cmd/doze/down.go and internal/daemonctl `Stop`): the daemon is asked to stop with SIGTERM to the
//! exact pid its pid file names, and its own shutdown puts every service to sleep. Then, better than Go (proposal
//! rule 8, "leave nothing behind"), it is checked: the process gone, no socket and no pid file left. A leftover of a
//! daemon that is already gone is removed and said; a daemon that does not stop is named with its pid.
//!
//! The result is Go's: `{"stopped": true}` and "✓ doze stopped", or `{"stopped": false}` and "doze is not running".
//! What was awake is not yet remembered for the next `up` (Go's `rememberDown`): that comes with `up`'s choice of
//! services (C2c, pending).

use crate::cli::args::DryRun;
use crate::ctx::Ctx;
use crate::diag::{Diagnostic, ids};
use crate::output::{Done, Mode, Outcome};
use crate::paint::Paint;
use crate::plan::{Op, Plan};
use crate::stack::Stack;
use crate::view::View;
use serde::Serialize;
use std::path::Path;
use std::time::{Duration, Instant};

/// Go's budget: the daemon's shutdown reap is 15 s (a backend that ignores everything until SIGKILL), so wait past it.
const STOP_WAIT: Duration = Duration::from_secs(20);
const POLL: Duration = Duration::from_millis(100);

/// What `down` did.
#[derive(Debug, Serialize)]
pub struct Stopped {
    /// False: it was not running.
    pub stopped: bool,
    /// Leftovers of a daemon that was already gone, removed (a socket, a pid file). Not in Go's result.
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

/// `doze down [--dry-run]`.
pub(crate) fn run(ctx: &mut Ctx<'_>, mode: &Mode, a: &DryRun) -> Outcome {
    let stack = Stack::find(ctx, mode)?;
    if a.dry_run {
        return Ok(Done::ok(View::Plan(plan(&stack)?)));
    }
    let pid = doze_projects::daemon_pid(&stack.key).filter(|p| p.alive);
    let Some(pid) = pid else {
        // A socket that answers with no live pid behind it is a daemon this doze cannot stop by pid: say so.
        if stack.read()?.is_some() {
            return Err(Diagnostic::new(
                ids::DAEMON_REFUSED,
                format!(
                    "doze is running this stack but its pid file ({}) names no live process",
                    stack.key.pid_file().display()
                ),
            )
            .fix("doze bug writes a report with the daemon's log"));
        }
        let cleared = clear(&stack.key.socket(), &stack.key.pid_file());
        return Ok(Done::ok(View::Stopped(Stopped {
            stopped: false,
            cleared,
        })));
    };
    stop(pid.pid)?;
    let cleared = clear(&stack.key.socket(), &stack.key.pid_file());
    Ok(Done::ok(View::Stopped(Stopped {
        stopped: true,
        cleared,
    })))
}

/// SIGTERM to exactly `pid`, then wait for it to go.
fn stop(pid: u32) -> Result<(), Diagnostic> {
    if let Err(err) = doze_os::process::terminate(pid) {
        if err.kind() == std::io::ErrorKind::Unsupported {
            return Err(Diagnostic::new(
                ids::NOT_BUILT,
                "`doze down` cannot stop the daemon on Windows yet: that comes with the Rust daemon's named pipe (core plan W)",
            )
            .fix("the Go doze does this today"));
        }
        if doze_os::process::alive(pid) {
            return Err(Diagnostic::new(
                ids::DAEMON_REFUSED,
                format!("could not signal the doze daemon (pid {pid}): {err}"),
            )
            .fix(format!("kill {pid}")));
        }
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
            .fix(format!(
                "doze bug writes a report with its log · kill {pid} stops it"
            )));
        }
        std::thread::sleep(POLL);
    }
    Ok(())
}

/// Removes what a daemon that is gone left behind; says what it removed.
fn clear(socket: &Path, pid_file: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for (path, what) in [(socket, "control socket"), (pid_file, "pid file")] {
        if path.exists() && std::fs::remove_file(path).is_ok() {
            out.push(format!("{what} ({})", path.display()));
        }
    }
    out
}

/// `doze down --dry-run`: what would sleep (from the daemon's snapshot) and that the daemon would stop.
fn plan(stack: &Stack) -> Result<Plan, Diagnostic> {
    let mut plan = Plan::new("doze down");
    let snap = stack.read()?.and_then(|c| c.snapshot().ok());
    let Some(snap) = snap else {
        plan.unchanged("the stack (doze is not running)");
        return Ok(plan);
    };
    for v in snap
        .instances
        .iter()
        .filter(|v| crate::cmd::status::awake(&v.state))
    {
        plan.step(&v.name, Op::Do, "sleep", "its data stays");
    }
    plan.step(
        "doze",
        Op::Remove,
        "stop the daemon",
        "nothing is left listening",
    );
    Ok(plan)
}
