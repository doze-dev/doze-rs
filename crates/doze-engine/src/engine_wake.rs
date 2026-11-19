//! A wake, in the shell: the machine decides (start, join, refuse, already running) under the lock, waiting while a
//! previous run is still being stopped; a start it decided is spawned here, and the caller waits for its phase.

use super::{Engine, Phase};
use crate::error::Error;
use crate::events::Cause;
use crate::machine::{Decision, Effect};
use crate::stack::{Endpoint, When};
use std::time::Duration;
use tokio::sync::watch;

impl Engine {
    /// Wakes `name` (and what it needs, first) and returns where it is reached once ready. Concurrent wakes of a cold
    /// instance share one start.
    pub async fn wake(&self, name: &str, cause: Cause) -> Result<Endpoint, Error> {
        self.wake_when(name, cause, When::Healthy).await
    }

    pub(crate) async fn wake_when(
        &self,
        name: &str,
        cause: Cause,
        when: When,
    ) -> Result<Endpoint, Error> {
        self.wake_inner(name, cause, when, false).await
    }

    /// A wake; `force`: a task it starts runs whatever its state (`doze task`).
    pub(crate) async fn wake_inner(
        &self,
        name: &str,
        cause: Cause,
        when: When,
        force: bool,
    ) -> Result<Endpoint, Error> {
        let (rx, start) = self.decide_wake(name, cause).await;
        // The journal's `wake` goes out before the start can wake (and journal) its needs.
        if let Some(mut s) = start {
            s.force = force;
            crate::boot::spawn(self, name, when, s);
        }
        let Some(mut rx) = rx? else {
            return Ok(self
                .lock()
                .live
                .get(name)
                .map(|l| l.endpoint.clone())
                .unwrap_or_default());
        };
        let phase = rx.wait_for(|p| !matches!(p, Phase::Booting)).await;
        match phase.as_deref() {
            Ok(Phase::Ready(ep)) => Ok(ep.clone()),
            Ok(Phase::Failed(e)) => Err(e.clone()),
            _ => Err(Error::Failed(format!(
                "the start of \"{name}\" was abandoned"
            ))),
        }
    }

    /// The machine's answer to a wake, waiting while a previous run is still being stopped or cancelled; the effects
    /// are run here, and a new start is handed back to be spawned.
    async fn decide_wake(
        &self,
        name: &str,
        cause: Cause,
    ) -> (
        Result<Option<watch::Receiver<Phase>>, Error>,
        Option<crate::boot::Start>,
    ) {
        let mut changes = self.changes();
        loop {
            // Decided and recorded under one lock: a wake that joins this start must find its receiver.
            let (answer, fx) = self.update(|c| {
                let (d, fx) = c.m.wake(name, cause.clone());
                let rerun = fx
                    .iter()
                    .any(|f| matches!(f, Effect::StartBoot { rerun: true, .. }));
                let answer = match d {
                    Decision::Busy => None,
                    Decision::Running => Some((Ok(None), None)),
                    Decision::Refuse(e) => Some((Err(e), None)),
                    Decision::Join(b) => Some(match c.boots.get(&b).cloned() {
                        Some(rx) => (Ok(Some(rx)), None),
                        None => (
                            Err(Error::Failed(format!("the start of \"{name}\" was lost"))),
                            None,
                        ),
                    }),
                    Decision::Start(b) => {
                        let (tx, rx) = watch::channel(Phase::Booting);
                        let (cancel_tx, cancel) = watch::channel(None);
                        c.boots.insert(b, rx.clone());
                        c.cancels.insert(b, cancel_tx);
                        let s = crate::boot::Start {
                            boot: b,
                            rerun,
                            tx,
                            cancel,
                            cause: cause.clone(),
                            force: false,
                        };
                        Some((Ok(Some(rx)), Some(s)))
                    }
                };
                (answer, fx)
            });
            self.run(fx);
            if let Some(a) = answer {
                return a;
            }
            // Bounded poll as well as the change feed: a stop's end always moves the version, but never wait blind.
            let _ = tokio::time::timeout(Duration::from_millis(50), changes.changed()).await;
        }
    }
}
