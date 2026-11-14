//! A task's run (Go's `runTask`): it is an instance like any other, so what it needs is woken and held while it runs, its
//! output is a log run and its start and end are journal events; what differs is that it decides whether it has to run,
//! ends `done`, and records how it went in the applied state.
//!
//! One deliberate difference from Go: the record is dropped when a run starts, so a run cut short (a stop, the daemon
//! killed) leaves none and the task runs again at its next trigger. Go kept the previous record, which could say `done`
//! for a migration that stopped half way.

use super::{Ask, TaskSpec, why_run};
use crate::applied::{Applied, TaskRecord};
use crate::engine::Engine;
use crate::error::Error;
use crate::events::Cause;
use crate::lifecycle::Now;
use crate::logs::RunLog;
use crate::stack::{Endpoint, Kind, When};
use std::process::Stdio;
use std::time::{Duration, Instant};

/// How many of a failed run's last lines its error quotes (Go's).
const QUOTE: usize = 10;

/// One read-modify-write of the state file at a time.
static RECORDS: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) fn records_lock() -> std::sync::MutexGuard<'static, ()> {
    RECORDS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// What a run that has no `timeout` may take.
const NO_LIMIT: Duration = Duration::from_secs(7 * 24 * 3600);

impl Engine {
    /// Whether `name` is a task.
    pub fn is_task(&self, name: &str) -> bool {
        self.service(name)
            .is_ok_and(|s| matches!(s.kind, Kind::Task(_)))
    }

    /// Runs task `name` now, whatever its state (`doze task <name>`); a run already in flight is joined.
    pub async fn run_task(&self, name: &str) -> Result<(), Error> {
        if !self.is_task(name) {
            self.service(name)?;
            return Err(Error::Failed(format!("{name} is not a task")));
        }
        // A start already in flight is joined; if it decided not to run (up to date), the record did not move, and the
        // forced run comes after it (E2 audit 2, M2). Bounded: each round either runs it or joins another start.
        for _ in 0..3 {
            let before = self.task_record(name);
            self.wake_inner(name, Cause::Command, When::Healthy, true)
                .await?;
            if self.task_record(name) != before {
                return Ok(());
            }
        }
        Err(Error::Failed(format!(
            "{name} did not run: other starts kept deciding it was up to date"
        )))
    }

    /// How task `name` last ran.
    pub fn task_record(&self, name: &str) -> Option<TaskRecord> {
        let _held = records_lock();
        Applied::load(&self.stack().state_file)
            .ok()
            .and_then(|a| a.tasks.get(name).cloned())
    }

    /// Records (or, with `None`, forgets) how task `name` ran, keeping the rest of the state.
    pub fn set_task_record(&self, name: &str, rec: Option<TaskRecord>) {
        let _held = records_lock();
        let path = &self.stack().state_file;
        let saved = Applied::load(path).and_then(|mut a| {
            match rec {
                Some(r) => a.tasks.insert(name.into(), r),
                None => a.tasks.remove(name),
            };
            a.save(path)
        });
        if let Err(err) = saved {
            tracing::warn!("cannot record task \"{name}\": {err}");
        }
    }
}

/// Start `boot` of task `name`: whether it has to run, then its needs, then its command. `force`: `doze task`.
pub(crate) async fn run(
    e: &Engine,
    name: &str,
    boot: u64,
    cause: &Cause,
    force: bool,
) -> Result<Endpoint, Error> {
    let svc = e.service(name)?.clone();
    let Kind::Task(ts) = &svc.kind else {
        return Err(Error::Failed(format!("{name} is not a task")));
    };
    let hash = if ts.inputs.is_empty() {
        String::new()
    } else {
        let (dir, globs) = (ts.dir.clone(), ts.inputs.clone());
        let hashed = tokio::task::spawn_blocking(move || super::inputs::hash(&dir, &globs).0);
        match hashed.await {
            Ok(h) => h,
            Err(err) => {
                // Go fails the task when its inputs cannot be read (E2 audit 2, n5).
                let fx = e.update(|c| c.m.task_skipped(name, boot, Now::current()));
                e.run(fx);
                return Err(Error::Failed(format!(
                    "task \"{name}\": reading its inputs: {err}"
                )));
            }
        }
    };
    let needed_by = match cause {
        Cause::Dependency(by) => Some(by.as_str()),
        _ => None,
    };
    let ask = if force { Ask::Forced } else { Ask::Needed };
    let last = e.task_record(name);
    let has_inputs = !ts.inputs.is_empty();
    let Some(why) = why_run(ask, ts.manual, has_inputs, &hash, last.as_ref(), needed_by) else {
        let fx = e.update(|c| c.m.task_skipped(name, boot, Now::current()));
        e.run(fx);
        return Ok(Endpoint::default());
    };
    let fx = e.update(|c| c.m.task_runs(name, boot, cause.clone(), &why));
    if fx.is_empty() {
        // Cancelled before it ran (E2 audit 2, n3): nothing is dropped or started; the cancel says why.
        return Err(Error::Failed(format!(
            "{name} was stopped while it was starting"
        )));
    }
    e.run(fx);
    tracing::info!("running task \"{name}\" ({why})…");
    e.set_task_record(name, None);
    let run = Run {
        e,
        name,
        boot,
        started: Instant::now(),
        ran_at: now_rfc3339(),
        why,
        hash,
    };
    run.go(&svc, ts).await
}

/// One run that has to happen.
struct Run<'a> {
    e: &'a Engine,
    name: &'a str,
    boot: u64,
    started: Instant,
    ran_at: String,
    why: String,
    hash: String,
}

impl Run<'_> {
    async fn go(&self, svc: &crate::stack::Service, ts: &TaskSpec) -> Result<Endpoint, Error> {
        crate::boot::wake_needs(self.e, svc, self.boot)
            .await
            .map_err(|err| self.fail(err, 0))?;
        let log = RunLog::begin(self.e.logs(), self.name).map_err(|err| {
            let msg = format!("opening the log for \"{}\": {err}", self.name);
            self.fail(Error::Failed(msg), 0)
        })?;
        let ended = exec(self.e, self.name, ts, &log).await;
        log.close().await;
        match ended {
            Ok(None) => Ok(self.done()),
            Ok(Some(code)) => {
                let mut msg = format!("task \"{}\" failed: exit {code}", self.name);
                for line in log.recent(QUOTE) {
                    msg.push('\n');
                    msg.push_str(&line);
                }
                Err(self.fail(Error::Failed(msg), code))
            }
            Err(err) => {
                let msg = format!("task \"{}\": {err}", self.name);
                Err(self.fail(Error::Failed(msg), 0))
            }
        }
    }

    fn record(&self, result: &str, exit_code: i32) -> TaskRecord {
        TaskRecord {
            result: result.into(),
            exit_code,
            took_ms: i64::try_from(self.started.elapsed().as_millis()).unwrap_or(i64::MAX),
            ran_at: self.ran_at.clone(),
            why: self.why.clone(),
            inputs: self.hash.clone(),
        }
    }

    fn fail(&self, err: Error, code: i32) -> Error {
        let (name, boot) = (self.name, self.boot);
        self.e
            .set_task_record(name, Some(self.record("failed", code)));
        let took = self.started.elapsed();
        let msg = err.to_string();
        let fx = self
            .e
            .update(|c| c.m.boot_failed(name, boot, &msg, took, Now::current()));
        self.e.run(fx);
        err
    }

    fn done(&self) -> Endpoint {
        let (name, boot) = (self.name, self.boot);
        self.e.set_task_record(name, Some(self.record("done", 0)));
        let took = self.started.elapsed();
        let fx = self
            .e
            .update(|c| c.m.task_done(name, boot, took, Now::current()));
        self.e.run(fx);
        tracing::info!(
            "task \"{name}\" done in {}",
            crate::godur::go_duration(took)
        );
        Endpoint::default()
    }
}

/// Runs the task's command to its end, its output into `log`: `None` when it exited 0, else its exit code (-1 for a
/// signal or an overrun). `Err`: it could not start.
async fn exec(e: &Engine, name: &str, ts: &TaskSpec, log: &RunLog) -> Result<Option<i32>, String> {
    let (prog, args) = ts.run.split_first().ok_or("an empty run line")?;
    let mut env = ts.env.clone();
    crate::process::with_extra(&mut env, &e.stack().extra_env);
    let mut cmd = tokio::process::Command::new(crate::short::program_in(prog, Some(&env)));
    cmd.args(args)
        .envs(&env)
        .current_dir(&ts.dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut tree = doze_os::tree::Tree::spawn(cmd).map_err(|err| err.to_string())?;
    let pids = e.pids(name);
    let _recorded = tree.pid().map(|pid| pids.short(pid));
    log.attach(&mut tree, "");
    let limit = ts.timeout.unwrap_or(NO_LIMIT);
    match tokio::time::timeout(limit, tree.wait()).await {
        Ok(Ok(st)) if st.success() => Ok(None),
        Ok(Ok(st)) => Ok(Some(st.code().unwrap_or(-1))),
        Ok(Err(err)) => Err(err.to_string()),
        Err(_) => {
            let _ = tree.kill();
            let _ = tokio::time::timeout(Duration::from_secs(2), tree.wait()).await;
            Ok(Some(-1))
        }
    }
}

/// Now as RFC 3339 with nanoseconds, in UTC (Go wrote local time with its offset; both read either).
fn now_rfc3339() -> String {
    doze_journal::go_time(time::OffsetDateTime::now_utc())
}
