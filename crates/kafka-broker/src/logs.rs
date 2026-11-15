//! The logs' own upkeep: segments that retention no longer keeps are deleted, now and then, in every topic.

use std::sync::Arc;
use std::time::Duration;

use kafka_storage::fs::Fs;
use kafka_wire::ids::PartitionId;

use crate::partitions::{Chore, Verdict, settings_print};
use crate::{Broker, clock, topic_config};

impl<F: Fs> Broker<F> {
    /// Applies every topic's retention to every partition once, now. A partition that is not open and has never been
    /// written to is left as it is; one that has files is opened for it, so that the writer, which owns the files of an open
    /// partition, does the deleting, and closed again after if the sweep was what opened it. A partition whose last pass said
    /// nothing could go before a time still to come, and that has not changed since, is not opened at all (an age bound
    /// takes the oldest segment first, and a size bound moves only with writes; see `partitions::upkeep`).
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
            let settings = settings_print(&(policy.max_bytes, policy.max_age_ms));
            for n in 0..topic.partitions.get() {
                let Some(id) = PartitionId::new(n) else {
                    continue;
                };
                let upkeep = self.partitions.upkeep(topic.id, id);
                if upkeep.skips(Chore::Retain, settings, now.get()) {
                    continue;
                }
                let changes = upkeep.changes();
                let was_open = self.partitions.is_open(topic.id, id);
                let swept = match self
                    .partitions
                    .get_unless_empty_in_background(&topic, id)
                    .await
                {
                    Ok(Some(partition)) => {
                        let from = partition.reader().start_offset().get();
                        let retained = partition.retain(policy, now).await;
                        drop(partition);
                        if !was_open {
                            self.partitions.release_if_idle(topic.id, id).await;
                        }
                        note.looked += 1;
                        retained.map(|r| {
                            note_move(&mut note, topic.name.as_ref(), n, (from, r.start.get()));
                            upkeep.note(
                                Chore::Retain,
                                Verdict {
                                    changes: r.changes,
                                    settings,
                                    at_ms: r.due_ms,
                                },
                            );
                        })
                    }
                    Ok(None) => {
                        upkeep.note(
                            Chore::Retain,
                            Verdict {
                                changes,
                                settings,
                                at_ms: None,
                            },
                        );
                        Ok(())
                    }
                    Err(e) => Err(e),
                };
                if let Err(error) = swept {
                    tracing::warn!(topic = %topic.name, partition = n, %error, "applying retention to a partition failed");
                }
            }
        }
        self.log_notes.pass(note);
    }

    /// Closes the partitions nothing has asked for in `Config::partition_idle_close`, as long as the broker lives: it sleeps
    /// until the next of the open ones will have been quiet that long, and, with none open, until one is opened.
    pub fn start_partition_closer(self: &Arc<Self>) -> tokio::task::JoinHandle<()>
    where
        F: Send + Sync + 'static,
    {
        let broker = Arc::downgrade(self);
        let quiet = self.config.partition_idle_close;
        let opened = self.partitions.opened_bell();
        tokio::spawn(async move {
            if quiet.is_zero() {
                return;
            }
            loop {
                let next = match broker.upgrade() {
                    Some(b) => {
                        b.woke();
                        b.partitions.close_quiet(quiet).await
                    }
                    None => break,
                };
                match next {
                    // A partition opened meanwhile is due later than this.
                    Some(at) => tokio::time::sleep_until(at).await,
                    None => opened.notified().await,
                }
            }
        })
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
