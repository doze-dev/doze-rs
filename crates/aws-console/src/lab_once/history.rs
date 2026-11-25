//! What was forced, kept in memory: the last [`History::KEEP`] events newest first, and the services that were rested (so Home does not
//! call a service running that a forced rest closed after its last call).

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::model::LabOnce;
use crate::ring::now_ms;

/// What the Lab keeps of its forced events. A restart clears it.
#[derive(Debug, Default)]
pub struct History {
    next: AtomicU64,
    kept: Mutex<Kept>,
}

#[derive(Debug, Default)]
struct Kept {
    events: VecDeque<LabOnce>,
    /// The services a forced rest closed, by `(service, region)`: when, in milliseconds since 1970.
    rested: Vec<(String, String, u64)>,
}

/// The most rests remembered: one per service and region at most (the newest), and no more than there can be.
const MOST_RESTED: usize = 64 * 32;

impl History {
    /// The most events kept: the oldest go first.
    pub const KEEP: usize = 200;

    /// Keeps an event, newest first, and says how it was kept.
    pub fn add(
        &self,
        kind: &str,
        sentence: &str,
        ok: bool,
        subject: &str,
        region: &str,
    ) -> LabOnce {
        let once = LabOnce {
            id: self.next.fetch_add(1, Ordering::Relaxed) + 1,
            at_ms: now_ms(),
            kind: kind.to_owned(),
            sentence: sentence.to_owned(),
            ok,
            subject: subject.to_owned(),
            region: region.to_owned(),
        };
        let mut kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        kept.events.push_front(once.clone());
        kept.events.truncate(Self::KEEP);
        once
    }

    /// The events kept, newest first.
    #[must_use]
    pub fn recent(&self) -> Vec<LabOnce> {
        self.kept
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .events
            .iter()
            .cloned()
            .collect()
    }

    /// Forgets the events (not what was rested: that is what the services are, not what the Lab shows).
    pub fn clear(&self) {
        self.kept
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .events
            .clear();
    }

    /// Notes that `service` was rested in `region` now.
    pub fn rested(&self, service: &str, region: &str) {
        let mut kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        kept.rested
            .retain(|(s, r, _)| !(s == service && r == region));
        if kept.rested.len() < MOST_RESTED {
            kept.rested
                .push((service.to_owned(), region.to_owned(), now_ms()));
        }
    }

    /// Whether `service` was rested in `region` after the call made at `at_ms`: the call did not leave it running.
    #[must_use]
    pub fn rested_after(&self, service: &str, region: &str, at_ms: u64) -> bool {
        self.kept
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .rested
            .iter()
            .any(|(s, r, at)| s == service && r == region && *at > at_ms)
    }
}
