//! The acknowledgements of a ShareFetch or a ShareAcknowledge, partition by partition, as Kafka 4.4.0 takes them
//! (`KafkaApis.handleAcknowledgements`): the batches are checked, then the caller's READ on the topic, then the partition,
//! then the share-partition acknowledges them all or none.

use kafka_auth::authz::Operation;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError as E;
use kafka_wire::Uuid;
use kafka_wire::ids::{GroupId, PartitionId, TopicId};

use crate::share::Key;
use crate::share::partition::AckBatch;
use crate::{Broker, Caller, clock};

/// One partition's acknowledgements, as a request names them.
#[derive(Debug, Clone)]
pub(crate) struct Asked {
    pub(crate) topic: Uuid,
    pub(crate) partition: i32,
    pub(crate) batches: Vec<AckBatch>,
}

/// What came of one partition's acknowledgements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Acked {
    pub(crate) topic: Uuid,
    pub(crate) partition: i32,
    pub(crate) error: Option<E>,
    pub(crate) message: Option<String>,
}

/// Kafka's `validateAcknowledgementBatches`: each batch in order, not overlapping the one before, with one type or one per
/// offset, each a type the version knows (RENEW only in a request that says it carries renewals).
fn batches_valid(batches: &[AckBatch], version: i16, renew: bool) -> bool {
    let most = if version >= 2 { 4 } else { 3 };
    let mut previous_last: Option<i64> = None;
    for b in batches {
        if b.first > b.last || previous_last.is_some_and(|p| b.first < p) || b.types.is_empty() {
            return false;
        }
        let span = b.last - b.first + 1;
        if b.types.len() > 1 && i64::try_from(b.types.len()).ok() != Some(span) {
            return false;
        }
        if b.types
            .iter()
            .any(|t| *t < 0 || *t > most || (*t == 4 && !renew))
        {
            return false;
        }
        previous_last = Some(b.last);
    }
    true
}

impl<F: Fs> Broker<F> {
    /// Acknowledges each partition's batches for `member`. A partition with no batches is answered only if another went
    /// as far as its share-partition (Kafka's quirk, L3623).
    pub(crate) fn share_acknowledge_all(
        &self,
        caller: &Caller,
        (group, member): (&GroupId, &str),
        asked: &[Asked],
        (version, renew): (i16, bool),
    ) -> Vec<Acked> {
        let guard = self.guard(caller);
        let limits = crate::share::limits_of(&self.share_settings(group));
        let now = clock::now().get();
        let mut out = Vec::new();
        let mut reached = false;
        let mut empties = Vec::new();
        for a in asked {
            let answer = |error: Option<E>, message: Option<String>| Acked {
                topic: a.topic,
                partition: a.partition,
                error,
                message,
            };
            let topic = TopicId::from_uuid(a.topic).and_then(|id| self.meta.topic_by_id(id));
            let Some(topic) = topic else {
                out.push(answer(Some(E::UnknownTopicId), None));
                continue;
            };
            if !batches_valid(&a.batches, version, renew) {
                out.push(answer(Some(E::InvalidRequest), None));
                continue;
            }
            if !guard.topic(Operation::Read, topic.name.as_str()) {
                out.push(answer(Some(E::TopicAuthorizationFailed), None));
                continue;
            }
            let Some(partition) =
                PartitionId::new(a.partition).filter(|p| p.get() < topic.partitions.get())
            else {
                out.push(answer(Some(E::UnknownTopicOrPartition), None));
                continue;
            };
            if a.batches.is_empty() {
                empties.push(answer(None, None));
                continue;
            }
            reached = true;
            let key = Key {
                group: group.clone(),
                topic: a.topic,
                partition: partition.get(),
            };
            // Only a share-partition this broker has in memory (one fetched since it started) can be acknowledged.
            let Some(held) = self.shares.get(&key) else {
                out.push(answer(Some(E::UnknownTopicOrPartition), None));
                continue;
            };
            match self.change_share(group, &held, false, |p| {
                p.acknowledge(member, &a.batches, now, &limits)
            }) {
                Ok(_) => out.push(answer(None, None)),
                Err(e) => out.push(answer(Some(e.code), Some(e.message))),
            }
        }
        if reached {
            out.extend(empties);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(first: i64, last: i64, types: &[i8]) -> AckBatch {
        AckBatch {
            first,
            last,
            types: types.to_vec(),
        }
    }

    #[test]
    fn batches_are_held_to_kafkas_shape() {
        assert!(batches_valid(&[b(0, 4, &[1]), b(5, 6, &[1, 2])], 1, false));
        assert!(
            !batches_valid(&[b(4, 0, &[1])], 1, false),
            "first after last"
        );
        assert!(
            !batches_valid(&[b(0, 4, &[1]), b(3, 6, &[1])], 1, false),
            "overlapping"
        );
        assert!(
            batches_valid(&[b(0, 4, &[1]), b(4, 6, &[1])], 1, false),
            "the same offset is not refused"
        );
        assert!(!batches_valid(&[b(0, 4, &[])], 1, false), "no type");
        assert!(
            !batches_valid(&[b(0, 4, &[1, 1])], 1, false),
            "two types for five offsets"
        );
        assert!(
            !batches_valid(&[b(0, 0, &[4])], 1, true),
            "RENEW is version 2's"
        );
        assert!(
            !batches_valid(&[b(0, 0, &[4])], 2, false),
            "RENEW without IsRenewAck"
        );
        assert!(batches_valid(&[b(0, 0, &[4])], 2, true));
        assert!(!batches_valid(&[b(0, 0, &[5])], 2, true));
        assert!(!batches_valid(&[b(0, 0, &[-1])], 2, true));
        assert!(batches_valid(&[b(0, 0, &[0])], 1, false), "a gap");
    }
}
