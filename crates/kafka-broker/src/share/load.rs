//! The broker's side of the share-partitions: finding one (from memory or the store), making the states of a group's new
//! partitions, and changing one under its lock with the change stored before it counts.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use kafka_meta::{ShareState, Topic};
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError as E;
use kafka_wire::ids::{GroupId, PartitionId, TimestampMs, TopicName};

use super::partition::{AckError, Acked, Limits, SharePartition};
use super::{Held, Key};
use crate::group_configs::{Reset, ShareSettings};
use crate::{Broker, blocking, clock};

/// Why a share-partition cannot be had.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Unloaded {
    /// The group never made it (Kafka: "Read operation on uninitialized share partition not allowed.").
    Uninitialized,
    /// The store or the log could not be read.
    Storage,
}

impl Unloaded {
    pub(crate) fn code(&self) -> E {
        match self {
            Self::Uninitialized => E::UnknownServerError,
            Self::Storage => E::KafkaStorageError,
        }
    }
}

/// The limits a share-partition of a group with these settings is held to.
#[must_use]
pub(crate) fn limits_of(s: &ShareSettings) -> Limits {
    Limits {
        max_delivery: i32::try_from(s.delivery_limit).unwrap_or(i32::MAX),
        max_in_flight: s.record_locks,
        lock_ms: s.lock_ms,
        renew: s.renew,
    }
}

impl<F: Fs> Broker<F> {
    /// What a share group runs by: its configs where set, else the broker's defaults.
    pub(crate) fn share_settings(&self, group: &GroupId) -> ShareSettings {
        self.group_configs.settings(group)
    }

    /// The share-partition of `group` for one partition of `topic`, from memory, else from the store (Kafka's
    /// `maybeInitialize`): a start not yet set is set now, by the group's `share.auto.offset.reset`.
    pub(crate) async fn share_partition(
        &self,
        group: &GroupId,
        topic: &Topic,
        partition: PartitionId,
    ) -> Result<Arc<Held>, Unloaded> {
        let key = Key {
            group: group.clone(),
            topic: topic.id.to_uuid(),
            partition: partition.get(),
        };
        if let Some(held) = self.shares.get(&key) {
            return Ok(held);
        }
        let stored = blocking::run(|| self.meta.share_state(group, &topic.name, partition))
            .map_err(|e| {
                tracing::error!(%group, topic = %topic.name, error = %e, "a share-partition's state could not be read");
                Unloaded::Storage
            })?
            .ok_or(Unloaded::Uninitialized)?;
        let part = if stored.start_offset < 0 {
            let start = self
                .reset_start(topic, partition, self.share_settings(group).reset)
                .await?;
            SharePartition::new(start, stored.state_epoch)
        } else {
            SharePartition::restore(&stored).map_err(|e| {
                tracing::error!(%group, topic = %topic.name, why = e.0, "a share-partition's stored state is not usable");
                Unloaded::Storage
            })?
        };
        Ok(self
            .shares
            .insert(key, Held::new(topic.name.clone(), partition, part)))
    }

    /// Where a share-partition with no start yet begins (Kafka's `startOffsetDuringInitialization`): the log's start, its
    /// end, or the first record no older than the duration (the end if every record is older: Kafka fails the
    /// share-partition with OFFSET_NOT_AVAILABLE until a newer record comes, SG-D4).
    async fn reset_start(
        &self,
        topic: &Topic,
        partition: PartitionId,
        reset: Reset,
    ) -> Result<i64, Unloaded> {
        let p = self.partitions.get(topic, partition).await.map_err(|e| {
            tracing::error!(topic = %topic.name, error = %e, "a share fetch could not open a partition");
            Unloaded::Storage
        })?;
        let reader = p.reader();
        Ok(match reset {
            Reset::Earliest => reader.start_offset().get(),
            Reset::Latest => reader.end_offset().get(),
            Reset::ByDuration(ms) => {
                let since = TimestampMs::from_wire(clock::now().get().saturating_sub(ms));
                blocking::run(|| reader.offset_for_timestamp(since))
                    .map_err(|_| Unloaded::Storage)?
                    .map_or_else(|| reader.end_offset().get(), |(o, _)| o.get())
            }
        })
    }

    /// Changes a share-partition with `change`, under its lock. A change that moved a record is stored before it counts;
    /// if the store refuses it, the share-partition is put back as it was (an acknowledgement, Kafka's rollback) or kept
    /// (`keep_unstored`: a lock that ran out, which Kafka keeps however its write went). Then the start moves past what is
    /// done with, the lock alarm is set, and waiting fetches are told if anything was made available.
    pub(crate) fn change_share(
        &self,
        group: &GroupId,
        held: &Held,
        keep_unstored: bool,
        change: impl FnOnce(&mut SharePartition) -> Result<Acked, AckError>,
    ) -> Result<Acked, AckError> {
        let mut part = held.lock();
        let before = part.clone();
        let acked = match change(&mut part) {
            Ok(acked) => acked,
            Err(e) => {
                *part = before;
                return Err(e);
            }
        };
        if acked.changed {
            let snapshot = part.snapshot();
            if let Err(e) = self.store_share(group, held, &snapshot) {
                tracing::error!(%group, topic = %held.topic, partition = held.partition.get(), error = %e.message, "a share-partition's state could not be stored");
                if !keep_unstored {
                    *part = before;
                    return Err(e);
                }
            }
        }
        let start = part.start;
        part.advance();
        if let Some(at) = part.next_deadline() {
            self.shares.alarm.set(at);
        }
        if acked.released || part.start != start {
            self.shares.notify();
        }
        Ok(acked)
    }

    /// Writes one share-partition's state.
    fn store_share(
        &self,
        group: &GroupId,
        held: &Held,
        state: &ShareState,
    ) -> Result<(), AckError> {
        let stored = blocking::run(|| {
            self.meta
                .set_share_states(group, &[(&held.topic, held.partition, state)])
        });
        match stored {
            Ok(done) if done.first() == Some(&true) => Ok(()),
            Ok(_) => Err(AckError {
                code: E::UnknownTopicOrPartition,
                message: "This server does not host this topic-partition.".to_owned(),
            }),
            Err(e) => Err(AckError {
                code: E::KafkaStorageError,
                message: format!("The share-partition's state could not be stored: {e}"),
            }),
        }
    }

    /// Makes the share-partitions of `topics` that `group` has none of yet (Kafka's InitializeShareGroupState, which the
    /// group coordinator sends after a heartbeat that finds new ones): a topic the group has never read starts where its
    /// `share.auto.offset.reset` says when first fetched (start -1); a new partition of a topic it reads starts at 0.
    pub(crate) fn initialize_shares(&self, group: &GroupId, epoch: i32, topics: &BTreeSet<String>) {
        let have = match blocking::run(|| self.meta.share_states(group)) {
            Ok(have) => have,
            Err(e) => {
                tracing::error!(%group, error = %e, "a share group's states could not be read");
                return;
            }
        };
        let mut known: BTreeMap<TopicName, BTreeSet<i32>> = BTreeMap::new();
        for (t, p, _) in &have {
            known.entry(t.clone()).or_default().insert(p.get());
        }
        let mut new: Vec<(TopicName, PartitionId, ShareState)> = Vec::new();
        for name in topics {
            let Some(topic) = TopicName::parse(name)
                .ok()
                .and_then(|n| self.meta.topic(&n))
            else {
                continue;
            };
            let had = known.get(&topic.name);
            let start = if had.is_some() { 0 } else { -1 };
            for p in 0..topic.partitions.get() {
                if had.is_some_and(|ps| ps.contains(&p)) {
                    continue;
                }
                let Some(partition) = PartitionId::new(p) else {
                    continue;
                };
                new.push((
                    topic.name.clone(),
                    partition,
                    ShareState {
                        state_epoch: epoch,
                        start_offset: start,
                        batches: Vec::new(),
                    },
                ));
            }
        }
        if new.is_empty() {
            return;
        }
        let writes: Vec<kafka_meta::ShareStateWrite<'_>> =
            new.iter().map(|(t, p, s)| (t, *p, s)).collect();
        if let Err(e) = blocking::run(|| self.meta.set_share_states(group, &writes)) {
            tracing::error!(%group, error = %e, "a share group's new partitions could not be initialized");
        }
    }

    /// Lets go of what `member` holds in the share-partitions of its session (it closed it, or its connection went: Kafka's
    /// `releaseSession`).
    pub(crate) fn release_share_member(
        &self,
        group: &GroupId,
        member: &str,
        parts: &[(kafka_wire::Uuid, i32)],
    ) {
        let limits = limits_of(&self.share_settings(group));
        for (topic, partition) in parts {
            let key = Key {
                group: group.clone(),
                topic: *topic,
                partition: *partition,
            };
            let Some(held) = self.shares.get(&key) else {
                continue;
            };
            let _ = self.change_share(group, &held, false, |p| {
                Ok(p.release_member(member, &limits))
            });
        }
    }

    /// A connection went: the share sessions it made close, and what their members held is let go of. A group left with no
    /// session on the broker has its share-partitions let go of too (read again from the store when next fetched).
    pub fn connection_closed(&self, connection: u64) {
        if connection == 0 {
            return;
        }
        let gone = self.shares.sessions().close_connection(connection);
        for ((group, member), parts) in gone {
            self.release_share_member(&group, member.as_str(), &parts);
            if self.shares.sessions().of_group(&group) == 0 {
                self.shares.forget(&group, None);
            }
        }
    }
}
