//! The one signal every open stream listens to, and the task that makes it: alive only while somebody listens.
//!
//! A [`Ticker`] holds a `watch` channel. [`Ticker::watch`] hands out a [`Viewer`]; the first viewer starts the task that
//! bumps the channel on a floor timer (2 s until the recorder and the group ring bump it themselves, slice 9e), and the
//! last viewer to go stops it. With nobody connected there is no task, no timer and no wake-up: the console's idle
//! cost is zero.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::sync::watch;
use tokio::task::JoinHandle;

/// The floor timer's period.
pub const FLOOR: Duration = Duration::from_secs(2);

#[derive(Debug, Default)]
struct Producer {
    viewers: usize,
    task: Option<JoinHandle<()>>,
}

/// The shared signal.
#[derive(Debug)]
pub struct Ticker {
    tx: watch::Sender<u64>,
    producer: Mutex<Producer>,
    period: Duration,
}

impl Ticker {
    /// A signal that ticks every `period` while somebody listens.
    #[must_use]
    pub fn new(period: Duration) -> Arc<Self> {
        Arc::new(Self {
            tx: watch::channel(0).0,
            producer: Mutex::new(Producer::default()),
            period,
        })
    }

    /// Starts listening. The first viewer starts the timer task; call it from inside the runtime.
    #[must_use]
    pub fn watch(self: &Arc<Self>) -> Viewer {
        let mut p = self.producer.lock().unwrap_or_else(PoisonError::into_inner);
        p.viewers += 1;
        if p.task.is_none() {
            let tx = self.tx.clone();
            let period = self.period;
            p.task = Some(tokio::spawn(async move {
                let mut every = tokio::time::interval(period);
                every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                // The first tick of an interval is at once: a new viewer already has its first event.
                every.tick().await;
                loop {
                    every.tick().await;
                    tx.send_modify(|n| *n = n.wrapping_add(1));
                }
            }));
        }
        Viewer {
            rx: self.tx.subscribe(),
            ticker: Arc::clone(self),
        }
    }

    /// Whether the timer task exists.
    #[must_use]
    pub fn running(&self) -> bool {
        self.producer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .task
            .is_some()
    }

    /// How many streams are listening.
    #[must_use]
    pub fn viewers(&self) -> usize {
        self.producer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .viewers
    }

    /// Ticks now, for whoever bumps the signal for a real reason (a change worth showing).
    pub fn bump(&self) {
        self.tx.send_modify(|n| *n = n.wrapping_add(1));
    }
}

/// One listener. Dropping it is leaving.
#[derive(Debug)]
pub struct Viewer {
    rx: watch::Receiver<u64>,
    ticker: Arc<Ticker>,
}

impl Viewer {
    /// Waits for the next tick; `false` if the signal is gone.
    pub async fn changed(&mut self) -> bool {
        self.rx.changed().await.is_ok()
    }
}

impl Drop for Viewer {
    fn drop(&mut self) {
        let mut p = self
            .ticker
            .producer
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        p.viewers = p.viewers.saturating_sub(1);
        if p.viewers == 0 {
            if let Some(task) = p.task.take() {
                task.abort();
            }
        }
    }
}

#[cfg(test)]
mod tests;
