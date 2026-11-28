//! One running [`Unit`] owned by its own task: it reports the first exit (and kills what is left of the unit), or
//! stops the unit when asked. The engine holds the [`Handle`]; dropping every handle stops the unit.

use crate::exec::{Killed, Unit};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Notify, mpsc, oneshot, watch};

/// How a unit ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ended {
    /// A process exited by itself: whether with 0, and Go's words for it.
    Exited { success: bool, text: String },
    /// It was stopped on purpose.
    Stopped,
}

#[derive(Debug)]
struct StopReq {
    /// How long to wait for it to exit by itself first (after `stop` steps asked it to).
    patience: Duration,
    grace: Duration,
    /// The processes that had to be killed.
    done: oneshot::Sender<Vec<Killed>>,
}

/// The engine's side of a supervised unit.
#[derive(Debug, Clone)]
pub struct Handle {
    pid: u32,
    pids: Vec<u32>,
    stop: mpsc::Sender<StopReq>,
    ended: watch::Receiver<Option<Ended>>,
    /// Kills it now, whatever it is doing (the shutdown ran out of time).
    kill: Arc<Notify>,
}

impl Handle {
    /// Hands `unit` to a task of its own.
    pub fn supervise(unit: Unit) -> Handle {
        let (stop, rx) = mpsc::channel(1);
        let (tx, ended) = watch::channel(None);
        let (pid, pids) = (unit.pid(), unit.pids());
        let kill = Arc::new(Notify::new());
        tokio::spawn(run(unit, rx, tx, kill.clone()));
        Handle {
            pid,
            pids,
            stop,
            ended,
            kill,
        }
    }

    /// The unit's pid (its first process).
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Every process's pid.
    pub fn pids(&self) -> &[u32] {
        &self.pids
    }

    /// Whether it is still running.
    pub fn alive(&self) -> bool {
        self.ended.borrow().is_none()
    }

    /// Waits until it ends.
    pub async fn ended(&self) -> Ended {
        let mut rx = self.ended.clone();
        match rx.wait_for(Option::is_some).await {
            Ok(v) => v.clone().unwrap_or(Ended::Stopped),
            // The task is gone without a word: it was stopped.
            Err(_) => Ended::Stopped,
        }
    }

    /// Stops it: waits up to `patience` for it to exit by itself, then asks each process (reverse order) and kills its
    /// tree after `grace`. Returns once it is gone, with the processes that had to be killed; a unit that already ended
    /// is not an error.
    pub async fn stop(&self, patience: Duration, grace: Duration) -> Vec<Killed> {
        let (done, wait) = oneshot::channel();
        let req = StopReq {
            patience,
            grace,
            done,
        };
        let killed = if self.stop.send(req).await.is_ok() {
            wait.await.unwrap_or_default()
        } else {
            Vec::new()
        };
        let _ = self.ended().await;
        killed
    }

    /// Kills it now, whatever it is doing, a stop in progress included; it reports itself ended once reaped.
    pub fn kill(&self) {
        self.kill.notify_one();
    }
}

async fn run(
    mut unit: Unit,
    mut stop: mpsc::Receiver<StopReq>,
    ended: watch::Sender<Option<Ended>>,
    kill: Arc<Notify>,
) {
    let req = tokio::select! {
        st = unit.wait_any() => {
            let how = match st {
                Ok(st) => Ended::Exited { success: st.success(), text: doze_os::process::exit_text(&st) },
                Err(e) => Ended::Exited { success: false, text: format!("exited: {e}") },
            };
            // The rest are stopped the way they asked, in reverse order; the exited one's leftovers are killed.
            unit.stop(crate::exec::STOP_GRACE).await;
            let _ = ended.send(Some(how));
            return;
        }
        req = stop.recv() => req,
        () = kill.notified() => {
            unit.kill_now().await;
            let _ = ended.send(Some(Ended::Stopped));
            return;
        }
    };
    let Some(req) = req else {
        // Every handle dropped: stop it.
        unit.stop(crate::exec::STOP_GRACE).await;
        let _ = ended.send(Some(Ended::Stopped));
        return;
    };
    // Asked to stop: patience, then each process its grace; a kill meanwhile cuts it short, and whatever has not
    // exited yet (the process being stopped included) is killed at once.
    let patience = req.patience;
    let grace = req.grace;
    let stopping = async {
        if !patience.is_zero() {
            let _ = tokio::time::timeout(patience, unit.wait_any()).await;
        }
        unit.stop(grace).await;
    };
    let cut_short = tokio::select! {
        () = stopping => false,
        () = kill.notified() => true,
    };
    if cut_short {
        unit.kill_now().await;
    }
    let _ = ended.send(Some(Ended::Stopped));
    let _ = req.done.send(unit.take_killed());
}
