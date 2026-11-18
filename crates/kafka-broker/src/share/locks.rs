//! The acquisition locks running out (Kafka's `AcquisitionLockTimerTask`): one task for every share-partition, asleep until
//! the soonest lock is due (the [`Alarm`](crate::alarm::Alarm) every acquisition and renewal sets). With no record
//! acquired it waits on the alarm with no timer at all: an idle broker, or one whose share groups are all caught up, does
//! not wake for it.

use std::sync::Arc;

use kafka_storage::fs::Fs;

use super::load::limits_of;
use crate::{Broker, clock};

impl<F: Fs + Send + Sync + 'static> Broker<F> {
    /// Lets go of the records whose locks have run out, when the soonest is due, as long as the broker lives.
    pub fn start_share_locks(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let broker = Arc::downgrade(self);
        let bell = self.shares.alarm.bell();
        tokio::spawn(async move {
            loop {
                let next = match broker.upgrade() {
                    Some(b) => b.shares.alarm.next_ms(),
                    None => break,
                };
                if next == i64::MAX {
                    bell.notified().await;
                    continue;
                }
                let now = clock::now().get();
                if now < next {
                    let wait = u64::try_from(next.saturating_sub(now)).unwrap_or(u64::MAX);
                    tokio::select! {
                        () = tokio::time::sleep(std::time::Duration::from_millis(wait)) => {}
                        () = bell.notified() => {}
                    }
                    continue;
                }
                let Some(broker) = broker.upgrade() else {
                    break;
                };
                broker.woke();
                broker.shares.alarm.clear();
                broker.expire_share_locks(now);
            }
        })
    }

    /// Every lock due by `now` runs out; the alarm is set to the soonest of those left.
    pub(crate) fn expire_share_locks(&self, now: i64) {
        for (key, held) in self.shares.all() {
            let due = held.lock().next_deadline();
            match due {
                None => continue,
                Some(at) if at > now => {
                    self.shares.alarm.set(at);
                    continue;
                }
                Some(_) => {}
            }
            let limits = limits_of(&self.share_settings(&key.group));
            // Kafka keeps a lock's end whether or not its write went through.
            let _ = self.change_share(&key.group, &held, true, |p| Ok(p.expire(now, &limits)));
        }
    }
}
