//! Coalescing: the folds say what changed as often as frames arrive; the UI hears it at most once a frame, and not
//! at all while it is hidden (PLAN.md §4.1, §4.3). The rule is a plain value ([`Coalescer`]), tested with made-up
//! instants; [`run`] is the thread that applies it to the hub's channels.

use crate::changed::{Batch, Changed};
use crate::select::block_on;
use async_channel::{Receiver, Sender, TrySendError};
use std::time::{Duration, Instant};

/// One frame at 60 Hz: the most often the UI hears of a change.
pub const FRAME: Duration = Duration::from_millis(16);

/// How often the UI may hear of changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    /// While visible: at most one batch per this.
    pub every: Duration,
    /// While hidden: at most one batch per this, or (`None`) none until it is shown again.
    pub hidden: Option<Duration>,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            every: FRAME,
            hidden: None,
        }
    }
}

/// The coalescing rule: changes merge into one pending batch, which is handed out when the UI may hear of it.
#[derive(Debug, Clone, Default)]
pub struct Coalescer {
    policy: Policy,
    pending: Batch,
    last: Option<Instant>,
    visible: bool,
}

impl Coalescer {
    /// A coalescer for a UI that starts visible.
    pub fn new(policy: Policy) -> Coalescer {
        Coalescer {
            policy,
            pending: Batch::default(),
            last: None,
            visible: true,
        }
    }

    /// A change happened.
    pub fn push(&mut self, c: Changed) {
        self.pending.add(c);
    }

    /// A batch that could not be handed over goes back, merged with what came since.
    pub fn put_back(&mut self, b: Batch) {
        self.pending.merge(b);
    }

    /// The UI was shown or hidden. Shown, what built up is due at once.
    pub fn set_visible(&mut self, visible: bool) {
        if visible && !self.visible {
            self.last = None;
        }
        self.visible = visible;
    }

    /// Whether the UI is visible.
    pub fn visible(&self) -> bool {
        self.visible
    }

    fn gap(&self) -> Option<Duration> {
        if self.visible {
            Some(self.policy.every)
        } else {
            self.policy.hidden
        }
    }

    /// When the pending batch is due: `None` when there is none, or while hidden with no hidden rate.
    pub fn due(&self) -> Option<Instant> {
        if self.pending.is_empty() {
            return None;
        }
        let gap = self.gap()?;
        Some(match self.last {
            None => Instant::now(),
            Some(last) => last + gap,
        })
    }

    /// The batch to hand to the UI at `now`, if one is due.
    pub fn take(&mut self, now: Instant) -> Option<Batch> {
        if self.pending.is_empty() {
            return None;
        }
        let gap = self.gap()?;
        if self.last.is_some_and(|last| now < last + gap) {
            return None;
        }
        self.last = Some(now);
        Some(std::mem::take(&mut self.pending))
    }
}

/// What the coalescing thread hears.
#[derive(Debug)]
pub(crate) enum Input {
    Changed(Changed),
    Visible(bool),
}

/// Applies `c` to what comes in on `input` and hands batches to `out` until every sender of `input` is gone. `out`
/// holds one batch: while the UI has not taken it, changes keep merging into the next.
pub(crate) fn run(mut c: Coalescer, input: &Receiver<Input>, out: &Sender<Batch>) {
    loop {
        let deadline = c.due();
        match block_on(input.recv(), deadline) {
            Some(Ok(Input::Changed(ch))) => {
                c.push(ch);
                while let Ok(more) = input.try_recv() {
                    match more {
                        Input::Changed(ch) => c.push(ch),
                        Input::Visible(v) => c.set_visible(v),
                    }
                }
            }
            Some(Ok(Input::Visible(v))) => c.set_visible(v),
            Some(Err(_)) => return,
            None => {}
        }
        if let Some(b) = c.take(Instant::now()) {
            match out.try_send(b) {
                Ok(()) => {}
                Err(TrySendError::Full(b)) => c.put_back(b),
                Err(TrySendError::Closed(_)) => return,
            }
        }
    }
}
