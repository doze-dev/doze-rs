//! The plan executor (promoted from `doze-module-test::run`, which now uses this one): the processes of a plan start in
//! `after` order as doze-os trees, each gated on its readiness check, then its hooks; a failure stops what already
//! started, in reverse. What runs is then one [`Unit`]: if any of its processes exits, the rest are stopped (Go's
//! `Composite`), and every process's whole tree goes with it (Go's `StartTree`).

use crate::logs::RunLog;
use crate::pidfile::Pids;
use crate::ready::{self, NotReady};
use doze_module_proto::msg::run::{Plan, Process, StopHow};
use doze_os::tree::{Stop, Stopped, Tree};
use std::future::Future;
use std::pin::Pin;
use std::process::{ExitStatus, Stdio};
use std::task::Poll;
use std::time::Duration;

/// The running processes of one plan, in start order.
#[derive(Debug)]
pub struct Unit {
    procs: Vec<(Process, Tree)>,
    /// Where its short commands are recorded (a module's stop command among them).
    pids: Option<Pids>,
}

/// The start order: each process after the ones it names; an unknown name or a cycle is an error (Go's `orderSpecs`,
/// a depth-first visit in declaration order).
pub fn order(plan: &Plan) -> Result<Vec<&Process>, String> {
    let mut out: Vec<&Process> = Vec::new();
    let mut on_stack: Vec<&str> = Vec::new();
    for p in &plan.processes {
        visit(plan, &p.name, &mut on_stack, &mut out)?;
    }
    Ok(out)
}

fn visit<'a>(
    plan: &'a Plan,
    name: &'a str,
    on_stack: &mut Vec<&'a str>,
    out: &mut Vec<&'a Process>,
) -> Result<(), String> {
    let Some(p) = plan.processes.iter().find(|p| p.name == name) else {
        return Err(format!("spawn spec depends on unknown spec \"{name}\""));
    };
    if on_stack.contains(&name) {
        return Err(format!("spawn spec dependency cycle at \"{name}\""));
    }
    if out.iter().any(|d| d.name == name) {
        return Ok(());
    }
    on_stack.push(name);
    for a in &p.after {
        visit(plan, a, on_stack, out)?;
    }
    on_stack.pop();
    out.push(p);
    Ok(())
}

fn spawn_one(p: &Process, log: &RunLog) -> Result<Tree, String> {
    let mut cmd = tokio::process::Command::new(crate::short::program_in(&p.program, Some(&p.env)));
    cmd.args(&p.args)
        .envs(&p.env)
        .stdin(Stdio::null())
        .stdout(log.stdio().map_err(|e| e.to_string())?)
        .stderr(log.stdio().map_err(|e| e.to_string())?);
    if let Some(d) = &p.dir {
        cmd.current_dir(d);
    }
    Tree::spawn(cmd).map_err(|e| format!("starting \"{}\": {e}", p.name))
}

/// Runs a hook (program and arguments) in the process's folder and environment, its output into the log; a failure
/// quotes that output (Go's `<name> hook "<line>": exit status 1: <output>`).
async fn run_hook(
    p: &Process,
    hook: &[String],
    log: &RunLog,
    pids: Option<&Pids>,
) -> Result<(), String> {
    let from = log.size();
    let short = crate::short::Short {
        argv: hook,
        env: Some(&p.env),
        dir: p.dir.as_deref().map(std::path::Path::new),
        out: Some(log),
    };
    let ran = crate::short::run(short, crate::steps::STEP_LIMIT, pids).await;
    ran.map_err(|e| {
        let out = log.since(from);
        let line = hook.join(" ");
        let msg = format!("{} hook \"{line}\": {e}", p.name);
        match out.trim() {
            "" => msg,
            out => format!("{msg}: {out}"),
        }
    })
}

/// Why a plan did not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartError {
    pub message: String,
    /// A readiness check ran out of time (Go's `fault.Timeout`).
    pub timeout: bool,
}

impl From<String> for StartError {
    fn from(message: String) -> StartError {
        StartError {
            message,
            timeout: false,
        }
    }
}

/// Starts every process of `plan`, each ready (within `budget`) before the next, running its hooks after it. With
/// `pids`, each process is recorded as it is spawned and each hook or exec probe while it runs (a host killed during
/// a slow start must leave nothing unrecorded). On any failure, what started is stopped again and the error quotes the
/// log's last lines.
pub async fn start(
    plan: &Plan,
    log: &RunLog,
    budget: Duration,
    pids: Option<&Pids>,
) -> Result<Unit, StartError> {
    let ordered = order(plan)?;
    if ordered.is_empty() {
        return Err(StartError::from("spawn plan has no specs".to_string()));
    }
    let mut unit = Unit {
        procs: Vec::new(),
        pids: pids.cloned(),
    };
    for p in ordered {
        let tree = match spawn_one(p, log) {
            Ok(t) => t,
            Err(e) => {
                unit.stop(STOP_GRACE).await;
                return Err(e.into());
            }
        };
        unit.procs.push((p.clone(), tree));
        if let Some(rec) = pids {
            rec.write(&unit.pids());
        }
        if let Err(e) = gate(&mut unit, p, log, budget, pids).await {
            unit.stop(STOP_GRACE).await;
            return Err(e);
        }
    }
    Ok(unit)
}

async fn gate(
    unit: &mut Unit,
    p: &Process,
    log: &RunLog,
    budget: Duration,
    pids: Option<&Pids>,
) -> Result<(), StartError> {
    let Some((_, tree)) = unit.procs.last_mut() else {
        return Err(StartError::from("nothing started".to_string()));
    };
    let alive = || matches!(tree.running(), Ok(true));
    let waited = ready::wait_ready(p.ready.as_ref(), budget, log.path(), pids, alive).await;
    if let Err(e) = waited {
        let timeout = matches!(e, NotReady::Timeout(_));
        return Err(StartError {
            message: not_ready(&p.name, &e, log),
            timeout,
        });
    }
    for hook in &p.hooks {
        run_hook(p, hook, log, pids).await?;
    }
    Ok(())
}

fn not_ready(name: &str, e: &NotReady, log: &RunLog) -> String {
    // Go quotes the newest run's last 200 lines.
    let tail = log.tail(200).join("\n");
    let tail = tail.trim();
    if tail.is_empty() {
        format!("{name} not ready: {e}")
    } else {
        format!("{name} not ready: {e}\n{tail}")
    }
}

/// How long a process gets to stop after it is asked, before its tree is killed (Go: 8 s for SIGTERM).
pub const STOP_GRACE: Duration = Duration::from_secs(8);

/// Stops one process the way its plan says. A module's stop command runs as a short command, recorded in the
/// instance's pidfiles while it runs (audit 2, M2: a host killed during it must leave nothing unswept); the process's
/// tree then gets `grace` to exit before it is killed.
async fn stop_one(
    p: &Process,
    tree: &mut Tree,
    grace: Duration,
    pids: Option<&Pids>,
) -> std::io::Result<Stopped> {
    let how = match &p.stop {
        Some(StopHow::Interrupt) => Stop::Interrupt,
        Some(StopHow::Terminate) | None => Stop::Terminate,
        Some(StopHow::Command(argv)) => {
            let short = crate::short::Short {
                argv,
                env: None,
                dir: None,
                out: None,
            };
            let asked = crate::short::run(short, grace, pids).await;
            return tree
                .stop_asked(asked.map_err(std::io::Error::other), grace)
                .await;
        }
    };
    tree.stop(&how, grace).await
}

impl Unit {
    /// The first process's pid (the unit's, as Go's `Composite.PID`).
    pub fn pid(&self) -> u32 {
        self.procs.first().and_then(|(_, t)| t.pid()).unwrap_or(0)
    }

    /// The pids of every process, for the pidfile sweep.
    pub fn pids(&self) -> Vec<u32> {
        self.procs.iter().filter_map(|(_, t)| t.pid()).collect()
    }

    /// Waits until any process exits; how it ended.
    pub async fn wait_any(&mut self) -> std::io::Result<ExitStatus> {
        let mut waits: Vec<Pin<Box<dyn Future<Output = std::io::Result<ExitStatus>> + Send + '_>>> =
            self.procs
                .iter_mut()
                .map(|(_, t)| Box::pin(t.wait()) as _)
                .collect();
        if waits.is_empty() {
            return Err(std::io::Error::other("an empty unit"));
        }
        std::future::poll_fn(|cx| {
            for w in &mut waits {
                if let Poll::Ready(r) = w.as_mut().poll(cx) {
                    return Poll::Ready(r);
                }
            }
            Poll::Pending
        })
        .await
    }

    /// Kills every tree now (after an exit: whatever the exited process left behind goes too).
    pub fn kill(&mut self) {
        // A reaped leader's group id may belong to someone else now: only trees whose leader is not reaped.
        for (_, t) in self
            .procs
            .iter_mut()
            .rev()
            .filter(|(_, t)| t.pid().is_some())
        {
            let _ = t.kill();
        }
    }

    /// Stops every process in reverse start order, each the way its plan said; how each ended.
    pub async fn stop(&mut self, grace: Duration) -> Vec<(String, std::io::Result<Stopped>)> {
        let mut out = Vec::new();
        while let Some((p, mut tree)) = self.procs.pop() {
            // A process that already exited (and was reaped) is not signalled again (audit 3, minor 2).
            if tree.pid().is_none() {
                continue;
            }
            let stopped = stop_one(&p, &mut tree, grace, self.pids.as_ref()).await;
            out.push((p.name.clone(), stopped));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(name: &str, after: &[&str]) -> Process {
        Process {
            name: name.into(),
            program: "x".into(),
            args: vec![],
            env: Default::default(),
            dir: None,
            after: after.iter().map(|s| (*s).to_string()).collect(),
            ready: None,
            hooks: vec![],
            stop: None,
        }
    }

    #[test]
    fn order_follows_after_in_declaration_order() {
        let plan = Plan {
            processes: vec![proc("c", &["b"]), proc("a", &[]), proc("b", &["a"])],
        };
        let names: Vec<_> = order(&plan)
            .unwrap()
            .iter()
            .map(|p| p.name.clone())
            .collect();
        assert_eq!(names, ["a", "b", "c"]);
    }

    #[test]
    fn order_names_the_cycle_and_the_unknown() {
        let cyc = Plan {
            processes: vec![proc("a", &["b"]), proc("b", &["a"])],
        };
        assert_eq!(
            order(&cyc).err().unwrap(),
            "spawn spec dependency cycle at \"a\""
        );
        let unknown = Plan {
            processes: vec![proc("a", &["zz"])],
        };
        assert_eq!(
            order(&unknown).err().unwrap(),
            "spawn spec depends on unknown spec \"zz\""
        );
    }
}
