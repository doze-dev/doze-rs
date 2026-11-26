//! Tasks that are tracked and stopped together.
//!
//! Nothing is fire-and-forget: a task a service starts is spawned here, so a stop can ask it to end and
//! wait until it has.

use std::future::Future;
use std::sync::Arc;

use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::Rests;

/// A set of tasks and the signal that tells them to stop.
#[derive(Debug, Clone, Default)]
pub struct Tasks {
    stop: CancellationToken,
    tracker: TaskTracker,
    rests: Arc<Rests>,
}

impl Tasks {
    /// An empty set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a task that is waited for at shutdown.
    pub fn spawn<F>(&self, task: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.tracker.spawn(task);
    }

    /// The signal a task selects on to learn that it should end.
    #[must_use]
    pub fn stopping(&self) -> CancellationToken {
        self.stop.clone()
    }

    /// The lazy services made on this task set, which a tool may tell to rest (see [`Rests`]).
    #[must_use]
    pub fn rests(&self) -> Arc<Rests> {
        Arc::clone(&self.rests)
    }

    /// How many tasks are running.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tracker.len()
    }

    /// No task is running.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tracker.is_empty()
    }

    /// Tells every task to stop and waits until they have. A task that does not select on
    /// [`stopping`](Self::stopping) is waited for to its natural end.
    pub async fn shutdown(&self) {
        self.stop.cancel();
        self.tracker.close();
        self.tracker.wait().await;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    #[tokio::test]
    async fn shutdown_stops_the_tasks_and_waits_for_them() {
        let tasks = Tasks::new();
        let ended = Arc::new(AtomicBool::new(false));
        let (flag, stopping) = (Arc::clone(&ended), tasks.stopping());
        tasks.spawn(async move {
            stopping.cancelled().await;
            flag.store(true, Ordering::SeqCst);
        });
        assert_eq!(tasks.len(), 1);
        tasks.shutdown().await;
        assert!(ended.load(Ordering::SeqCst));
        assert!(tasks.is_empty());
    }
}
