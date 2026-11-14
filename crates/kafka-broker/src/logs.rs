//! The logs' own upkeep: segments that retention no longer keeps are deleted, now and then, in every topic.

use std::sync::Arc;
use std::time::Duration;

use kafka_storage::fs::Fs;
use kafka_wire::ids::PartitionId;

use crate::partitions::Trim;
use crate::{Broker, clock, topic_config};

impl<F: Fs> Broker<F> {
    /// Applies every topic's retention to every partition once, now. A partition that is not open and has never been
    /// written to is left as it is; one that has files is opened for it (and closed again when the limit on open
    /// partitions says), so that the writer, which owns the files of an open partition, does the deleting.
    pub async fn check_logs(&self) {
        let now = clock::now();
        let mut note = crate::log_notes::PassNote {
            at_ms: now.get(),
            looked: 0,
            deleted: Vec::new(),
            more: 0,
        };
        for topic in self.meta.topics() {
            let policy = topic_config::retention(&self.defaults.get(), &topic.config);
            if !policy.is_set() || topic_config::compacts_only(&topic.config) {
                continue;
            }
            for n in 0..topic.partitions.get() {
                let Some(id) = PartitionId::new(n) else {
                    continue;
                };
                let swept = match self.partitions.get_unless_empty(&topic, id).await {
                    Ok(Some(partition)) => {
                        let from = partition.reader().start_offset().get();
                        let to = partition.trim(Trim::Retention(policy, now)).await;
                        note.looked += 1;
                        if let Ok(to) = &to {
                            note_move(&mut note, topic.name.as_ref(), n, (from, to.get()));
                        }
                        to.map(drop)
                    }
                    Ok(None) => Ok(()),
                    Err(e) => Err(e),
                };
                if let Err(error) = swept {
                    tracing::warn!(topic = %topic.name, partition = n, %error, "applying retention to a partition failed");
                }
            }
        }
        self.log_notes.pass(note);
    }

    /// Applies retention now and then, as long as the broker lives.
    pub fn start_log_sweeper(self: &Arc<Self>) -> tokio::task::JoinHandle<()>
    where
        F: Send + Sync + 'static,
    {
        let broker = Arc::downgrade(self);
        let every: Duration = self.config.log_retention_check_every;
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(every).await;
                let Some(broker) = broker.upgrade() else {
                    break;
                };
                broker.check_logs().await;
            }
        })
    }
}

/// Notes that a pass moved a partition's log start, if it did: listed up to a bound, counted beyond it.
fn note_move(
    note: &mut crate::log_notes::PassNote,
    topic: &str,
    partition: i32,
    (from, to): (i64, i64),
) {
    if to <= from {
        return;
    }
    if note.deleted.len() < crate::log_notes::MOST_DELETED {
        note.deleted.push(crate::log_notes::DeletedNote {
            topic: topic.to_owned(),
            partition,
            from,
            to,
        });
    } else {
        note.more += 1;
    }
}
