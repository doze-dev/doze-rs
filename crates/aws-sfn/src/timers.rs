//! The driver's clock: one task per region that has something due (a Wait, a retry's wait, a task's timeout or heartbeat, a
//! machine's own timeout), asleep until the earliest deadline, and gone when nothing is due. Nothing ticks: a Wait of an hour costs
//! one sleeping task. The task is doze-runtime's [`Due`], which keeps a deadline a call arms while a wake-up is under way.

use std::sync::{Arc, Weak};
use std::time::Duration;

use aws_wire::service::Caller;
use doze_runtime::{Ctx, Due, Fire};

use crate::core::Request;
use crate::shared::Shared;

/// A deadline already past when it is handed back is moved on by this much, so a fault in the arithmetic cannot become a busy loop.
const AT_LEAST: Duration = Duration::from_millis(100);

/// What a region's wake-up asks with: what the actors live beside, and the caller its tick is made for.
type Armed = (Weak<Shared>, Caller);

/// The timers of every region.
#[derive(Debug)]
pub(crate) struct Timers {
    due: Arc<Due<Armed>>,
}

impl Timers {
    pub(crate) fn new(ctx: Arc<Ctx>) -> Arc<Self> {
        let fire: Fire<Armed> = Box::new(|region, (shared, caller), at| {
            Box::pin(async move {
                let shared = shared.upgrade()?;
                // The answer re-arms this timer through `Shared::ask`; what it says is read back here too.
                shared
                    .ask(&region, &caller, Request::Tick(caller.clone(), at))
                    .await
                    .ok()
                    .and_then(|a| a.next_due)
            })
        });
        Arc::new(Self {
            due: Due::new(ctx, AT_LEAST, fire),
        })
    }

    /// Tells the region's timer the deadline of the answer numbered `number` ([`doze_runtime::Lazy::call_numbered`]), starting the
    /// task if there is a deadline and no task. An answer older than one already armed from is dropped: a call that awaited its save
    /// is overtaken.
    pub(crate) fn arm(
        &self,
        shared: &Arc<Shared>,
        region: &str,
        caller: &Caller,
        (due, number): (Option<i64>, u64),
    ) {
        self.due.arm_ordered(
            region,
            &(Arc::downgrade(shared), caller.clone()),
            due,
            number,
        );
    }

    /// Whether a region has a timer task (for tests).
    #[cfg(test)]
    pub(crate) fn armed(&self, region: &str) -> bool {
        self.due.armed(region)
    }
}
