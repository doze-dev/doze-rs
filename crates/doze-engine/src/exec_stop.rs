//! Stopping a unit: each process the way its plan says, within its grace, then killed; and the kills, said.

use super::{Pids, Process, StopHow, Unit};
use doze_os::tree::{Stop, Stopped, Tree};
use std::time::Duration;

/// A process a stop had to kill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Killed {
    pub name: String,
    pub why: Why,
}

/// Why a process was killed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// Its grace ran out: it had been asked to stop this long before.
    Grace(Duration),
    /// The shutdown's deadline passed while it was still stopping or waiting to.
    OutOfTime,
    /// The daemon was asked to stop a second time, and it had not exited 2 s later.
    AskedAgain,
}

impl Killed {
    /// What happened, in words (the daemon's log, the journal).
    pub fn words(&self) -> String {
        match self.why {
            Why::Grace(d) => format!(
                "\"{}\" did not exit within {:.1}s of being asked to stop; it was killed (SIGKILL)",
                self.name,
                d.as_secs_f64()
            ),
            Why::OutOfTime => format!(
                "\"{}\" was killed (SIGKILL): the shutdown ran out of time",
                self.name
            ),
            Why::AskedAgain => format!(
                "\"{}\" was killed (SIGKILL): asked to stop again, it had not exited",
                self.name
            ),
        }
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
                capture: None,
            };
            return match crate::short::run(short, grace, pids).await {
                Ok(()) => tree.stop_asked(Ok(()), grace).await,
                // The command could not ask it (missing, refused, timed out): the signal asks instead, with the same
                // grace, rather than a kill that skips whatever the process does on its way out.
                Err(e) => {
                    tracing::warn!(
                        "the stop command for \"{}\" failed ({e}); asking it with SIGTERM instead",
                        p.name
                    );
                    tree.stop(&Stop::Terminate, grace).await
                }
            };
        }
    };
    tree.stop(&how, grace).await
}

impl Unit {
    /// Stops every process in reverse start order, each the way its plan said; how each ended. A process stays in the
    /// unit until its stop is done, so a stop given up on (the shutdown's deadline, [`Unit::kill_now`]) still finds it.
    pub async fn stop(&mut self, grace: Duration) -> Vec<(String, std::io::Result<Stopped>)> {
        let mut out = Vec::new();
        while let Some((p, tree)) = self.procs.last_mut() {
            // A process that already exited (and was reaped) is not signalled again (audit 3, minor 2).
            if tree.pid().is_some() {
                let asked = std::time::Instant::now();
                let stopped = stop_one(p, tree, grace, self.pids.as_ref()).await;
                if let Ok(Stopped::Forced(_)) = &stopped {
                    // Whatever it meant to do on the way out (a snapshot, a flush, a goodbye) did not happen.
                    let k = Killed {
                        name: p.name.clone(),
                        why: Why::Grace(asked.elapsed()),
                    };
                    tracing::warn!("{}", k.words());
                    self.killed.push(k);
                }
                out.push((p.name.clone(), stopped));
            }
            self.procs.pop();
        }
        if let Some(log) = self.log.take() {
            log.close().await;
        }
        out
    }

    /// Kills every process left now (for `why`), waits until each is reaped, and closes the log.
    pub async fn kill_now(&mut self, why: Why) {
        for (_, t) in self.procs.iter_mut().filter(|(_, t)| t.pid().is_some()) {
            let _ = t.kill();
        }
        while let Some((p, mut tree)) = self.procs.pop() {
            if tree.pid().is_none() {
                continue;
            }
            let _ = tree.wait().await;
            let k = Killed {
                name: p.name.clone(),
                why,
            };
            tracing::warn!("{}", k.words());
            self.killed.push(k);
        }
        if let Some(log) = self.log.take() {
            log.close().await;
        }
    }

    /// The processes its stops had to kill so far (taken).
    pub fn take_killed(&mut self) -> Vec<Killed> {
        std::mem::take(&mut self.killed)
    }
}
