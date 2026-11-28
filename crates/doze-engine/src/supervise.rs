//! One running [`Unit`] owned by its own task: it reports the first exit (and kills what is left of the unit), or
//! stops the unit when asked. The engine holds the [`Handle`]; dropping every handle stops the unit.

use crate::exec::{Killed, Unit, Why};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, watch};

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
    done: oneshot::Sender<()>,
}

/// The engine's side of a supervised unit.
#[derive(Debug, Clone)]
pub struct Handle {
    pid: u32,
    pids: Vec<u32>,
    stop: mpsc::Sender<StopReq>,
    ended: watch::Receiver<Option<Ended>>,
    /// Set to kill it now, whatever it is doing, and why.
    kill: Arc<watch::Sender<Option<Why>>>,
    /// The processes it had to kill, however it ended: filled in before it reports itself ended.
    killed: Arc<Mutex<Vec<Killed>>>,
}

impl Handle {
    /// Hands `unit` to a task of its own.
    pub fn supervise(unit: Unit) -> Handle {
        let (stop, rx) = mpsc::channel(1);
        let (tx, ended) = watch::channel(None);
        let (pid, pids) = (unit.pid(), unit.pids());
        let (kill, kill_rx) = watch::channel(None);
        let killed = Arc::new(Mutex::new(Vec::new()));
        let task = Task {
            stop: rx,
            ended: tx,
            kill: kill_rx,
            killed: killed.clone(),
        };
        tokio::spawn(run(unit, task));
        Handle {
            pid,
            pids,
            stop,
            ended,
            kill: Arc::new(kill),
            killed,
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
    /// tree after `grace`. Returns once it is gone, with the processes that had to be killed (a kill that came before
    /// the stop was asked for included); a unit that already ended is not an error.
    pub async fn stop(&self, patience: Duration, grace: Duration) -> Vec<Killed> {
        let (done, wait) = oneshot::channel();
        let req = StopReq {
            patience,
            grace,
            done,
        };
        if self.stop.send(req).await.is_ok() {
            let _ = wait.await;
        }
        let _ = self.ended().await;
        self.killed
            .lock()
            .map(|mut k| std::mem::take(&mut *k))
            .unwrap_or_default()
    }

    /// Kills it now, whatever it is doing (a stop in progress included), for `why`; it reports itself ended once
    /// reaped. A second kill changes nothing.
    pub fn kill(&self, why: Why) {
        self.kill.send_if_modified(|w| {
            if w.is_some() {
                return false;
            }
            *w = Some(why);
            true
        });
    }
}

/// The supervising task's ends of the channels.
struct Task {
    stop: mpsc::Receiver<StopReq>,
    ended: watch::Sender<Option<Ended>>,
    kill: watch::Receiver<Option<Why>>,
    killed: Arc<Mutex<Vec<Killed>>>,
}

impl Task {
    /// Waits for a kill; why.
    async fn killed(kill: &mut watch::Receiver<Option<Why>>) -> Why {
        // The guard is let go of before any other await: the task must stay `Send`.
        let got = kill.wait_for(Option::is_some).await.ok().map(|w| *w);
        match got {
            Some(w) => w.unwrap_or(Why::OutOfTime),
            // Every handle is gone: nothing can ask any more.
            None => std::future::pending().await,
        }
    }

    /// Reports how it ended, with what had to be killed recorded first.
    fn end(&self, unit: &mut Unit, how: Ended) {
        if let Ok(mut k) = self.killed.lock() {
            k.extend(unit.take_killed());
        }
        let _ = self.ended.send(Some(how));
    }
}

async fn run(mut unit: Unit, mut t: Task) {
    let req = tokio::select! {
        st = unit.wait_any() => {
            let how = match st {
                Ok(st) => Ended::Exited { success: st.success(), text: doze_os::process::exit_text(&st) },
                Err(e) => Ended::Exited { success: false, text: format!("exited: {e}") },
            };
            // The rest are stopped the way they asked, in reverse order; the exited one's leftovers are killed.
            unit.stop(crate::exec::STOP_GRACE).await;
            t.end(&mut unit, how);
            return;
        }
        req = t.stop.recv() => req,
        why = Task::killed(&mut t.kill) => {
            unit.kill_now(why).await;
            t.end(&mut unit, Ended::Stopped);
            return;
        }
    };
    let Some(req) = req else {
        // Every handle dropped: stop it.
        unit.stop(crate::exec::STOP_GRACE).await;
        t.end(&mut unit, Ended::Stopped);
        return;
    };
    // Asked to stop: patience, then each process its grace; a kill meanwhile cuts it short, and whatever has not
    // exited yet (the process being stopped included) is killed at once.
    let (patience, grace) = (req.patience, req.grace);
    let stopping = async {
        if !patience.is_zero() {
            let _ = tokio::time::timeout(patience, unit.wait_any()).await;
        }
        unit.stop(grace).await;
    };
    let cut_short = tokio::select! {
        () = stopping => None,
        why = Task::killed(&mut t.kill) => Some(why),
    };
    if let Some(why) = cut_short {
        unit.kill_now(why).await;
    }
    t.end(&mut unit, Ended::Stopped);
    let _ = req.done.send(());
}
