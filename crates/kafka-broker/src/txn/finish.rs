//! Carrying out a transaction whose decision is stored: the markers on disk, then a commit's offsets, then the record.

use kafka_meta::{OffsetCommit, StoredOffset, TxnState};
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError as E;
use kafka_wire::batch::{Batch, Marker, TimestampType};
use kafka_wire::ids::{GroupId, PartitionId, ProducerEpoch, ProducerId, TopicName};

use super::entries::Entry;
use crate::partitions::Written;
use crate::{Broker, blocking, clock};

/// What went wrong and why, for the log.
type Failure = (String, String);

fn failure(what: &str, why: impl std::fmt::Display) -> Failure {
    (what.to_owned(), why.to_string())
}

impl<F: Fs> Broker<F> {
    /// Carries out the stored decision: writes the markers, applies a commit's offsets and records the Complete state.
    /// Safe to repeat. Success, and the transaction recorded complete, only when every marker is on disk (each partition
    /// is flushed, whoever appended the marker) and, for a commit, the offsets are stored: a power cut the moment after
    /// cannot take a committed transaction back. When it cannot finish, the decision stands and the answer is
    /// CONCURRENT_TRANSACTIONS, which is what Kafka answers while a decided transaction's markers are being written;
    /// the coordinator keeps trying on its own.
    pub(crate) async fn finish_txn(&self, e: &mut Entry, commit: bool) -> Result<(), E> {
        match self.carry_out(e, commit).await {
            Ok(()) => {
                e.failure = None;
                Ok(())
            }
            Err((what, why)) => {
                // Once per failure, not once per attempt: the coordinator tries again twice a second.
                let text = format!("{what}: {why}");
                if e.failure.as_deref() != Some(text.as_str()) {
                    tracing::error!(transactional_id = %e.rec.id, commit, "transactions: {text}; the decision stands and will be carried out when it can be");
                    e.failure = Some(text);
                }
                Err(E::ConcurrentTransactions)
            }
        }
    }

    async fn carry_out(&self, e: &mut Entry, commit: bool) -> Result<(), Failure> {
        let producer = ProducerId::new(e.rec.producer_id)
            .ok_or_else(|| failure("the transaction has no producer", e.rec.producer_id))?;
        let epoch = ProducerEpoch::from_wire(e.rec.epoch);
        let kind = if commit {
            Marker::Commit
        } else {
            Marker::Abort
        };
        for (topic, index) in e.rec.partitions.clone() {
            let found = TopicName::parse(&topic)
                .ok()
                .and_then(|n| self.meta.topic(&n))
                .zip(PartitionId::new(index))
                .filter(|(t, id)| id.get() < t.partitions.get());
            // A topic that was deleted has nothing to end.
            let Some((t, id)) = found else { continue };
            let marker = Batch::parse(kind.batch(producer, epoch, 0, clock::now()))
                .map_err(|err| failure("could not build a transaction marker", err))?;
            let partition =
                self.partitions.get(&t, id).await.map_err(|err| {
                    failure("could not open a partition to end a transaction", err)
                })?;
            self.write_marker(&partition, marker).await?;
        }
        if commit {
            self.apply_offsets(e)?;
        }
        let complete = if commit {
            TxnState::CompleteCommit
        } else {
            TxnState::CompleteAbort
        };
        self.update_txn(e, |rec| {
            rec.state = complete;
            rec.partitions.clear();
            rec.groups.clear();
            rec.offsets.clear();
        })
        .await
        .map_err(|code| {
            failure(
                "could not record that the transaction is complete",
                format!("{code:?}"),
            )
        })
    }

    /// Appends a marker and waits until it is on disk: it is the decision readers act on.
    async fn write_marker(
        &self,
        partition: &crate::partitions::Partition<F>,
        marker: Batch,
    ) -> Result<(), Failure> {
        let pending = partition
            .submit(marker, clock::now(), TimestampType::Create, true)
            .await
            .map_err(|err| failure("could not write a transaction marker", err))?;
        match pending.await {
            Ok(Ok(Written::Appended(a))) => a
                .ticket
                .durable()
                .await
                .map_err(|err| failure("could not flush a transaction marker", err)),
            Ok(Ok(other)) => Err(failure(
                "a transaction marker was refused",
                format!("{other:?}"),
            )),
            Ok(Err(err)) => Err(failure("could not write a transaction marker", err)),
            Err(_) => Err(failure(
                "could not write a transaction marker",
                "the partition went away",
            )),
        }
    }

    /// Stores the consumer offsets a committed transaction carried.
    fn apply_offsets(&self, e: &Entry) -> Result<(), Failure> {
        let mut groups: Vec<&str> = e.rec.offsets.iter().map(|o| o.group.as_str()).collect();
        groups.sort_unstable();
        groups.dedup();
        for group in groups {
            let Ok(gid) = GroupId::parse(group) else {
                continue;
            };
            let commits: Vec<OffsetCommit> = e
                .rec
                .offsets
                .iter()
                .filter(|o| o.group == group)
                .filter_map(|o| {
                    Some(OffsetCommit {
                        topic: TopicName::parse(&o.topic).ok()?,
                        partition: PartitionId::new(o.partition)?,
                        offset: StoredOffset {
                            offset: o.offset,
                            leader_epoch: o.leader_epoch,
                            metadata: o.metadata.clone(),
                            committed_ms: clock::now().get(),
                        },
                    })
                })
                .collect();
            blocking::run(|| self.meta.commit_offsets(&gid, &commits)).map_err(|err| {
                failure(
                    &format!("could not apply a transaction's offsets for {group}"),
                    err,
                )
            })?;
        }
        Ok(())
    }
}
