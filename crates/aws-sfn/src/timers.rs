//! The driver's clock: one task per region that has something due (a Wait, a retry's wait, a task's timeout or heartbeat, a
//! machine's own timeout), asleep until the earliest deadline, and gone when nothing is due. Nothing ticks: a Wait of an hour costs
//! one sleeping task.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use aws_wire::service::Caller;
use doze_runtime::Ctx;
use tokio::sync::watch;

use crate::core::Request;
use crate::shared::Shared;

/// A sleep is cut into pieces no longer than this, so a deadline far ahead does not meet the limit of the timer wheel.
const LONGEST_SLEEP_MS: i64 = 86_400_000;
/// A deadline already past when it is handed back is moved on by this much, so a fault in the arithmetic cannot become a busy loop.
const AT_LEAST_MS: i64 = 100;

/// The timers of every region.
#[derive(Debug)]
pub(crate) struct Timers {
    ctx: Arc<Ctx>,
    regions: Mutex<HashMap<String, watch::Sender<Option<i64>>>>,
}

impl Timers {
    pub(crate) fn new(ctx: Arc<Ctx>) -> Arc<Self> {
        Arc::new(Self {
            ctx,
            regions: Mutex::new(HashMap::new()),
        })
    }

    /// Tells the region's timer its deadline, starting the task if there is a deadline and no task.
    pub(crate) fn arm(
        &self,
        shared: &Arc<Shared>,
        region: &str,
        caller: &Caller,
        due: Option<i64>,
    ) {
        let mut map = self.regions.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(tx) = map.get(region) {
            tx.send_replace(due);
            return;
        }
        let Some(at) = due else { return };
        let (tx, rx) = watch::channel(Some(at));
        map.insert(region.to_owned(), tx);
        self.ctx.tasks.spawn(run(
            Arc::clone(shared),
            region.to_owned(),
            caller.clone(),
            rx,
        ));
    }

    /// Whether a region has a timer task (for tests).
    #[cfg(test)]
    pub(crate) fn armed(&self, region: &str) -> bool {
        self.regions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(region)
    }

    fn set(&self, region: &str, due: Option<i64>) {
        let map = self.regions.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(tx) = map.get(region) {
            tx.send_replace(due);
        }
    }

    /// Ends the region's entry if its deadline is still `None`.
    fn finish(&self, region: &str) -> bool {
        let mut map = self.regions.lock().unwrap_or_else(PoisonError::into_inner);
        let idle = map.get(region).is_some_and(|tx| tx.borrow().is_none());
        if idle {
            map.remove(region);
        }
        idle
    }
}

async fn run(
    shared: Arc<Shared>,
    region: String,
    caller: Caller,
    mut rx: watch::Receiver<Option<i64>>,
) {
    let timers = Arc::clone(&shared.timers);
    let stopping = shared.ctx.tasks.stopping();
    loop {
        let due = *rx.borrow_and_update();
        let Some(at) = due else {
            if timers.finish(&region) {
                return;
            }
            continue;
        };
        let wait = at - shared.ctx.clock.now_ms();
        if wait > 0 {
            let nap = Duration::from_millis(u64::try_from(wait.min(LONGEST_SLEEP_MS)).unwrap_or(0));
            tokio::select! {
                () = tokio::time::sleep(nap) => {}
                changed = rx.changed() => {
                    if changed.is_err() {
                        return;
                    }
                }
                () = stopping.cancelled() => return,
            }
            continue;
        }
        // The answer re-arms this timer through `Shared::ask`; what it says is read back below.
        let next = shared
            .ask(&region, &caller, Request::Tick(caller.clone(), at))
            .await
            .ok()
            .and_then(|a| a.next_due);
        let now = shared.ctx.clock.now_ms();
        timers.set(&region, next.map(|n| n.max(now + AT_LEAST_MS)));
    }
}
