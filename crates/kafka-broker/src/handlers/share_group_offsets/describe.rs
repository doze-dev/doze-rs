//! DescribeShareGroupOffsets (90): each share-partition's stored start and, from version 1, how many records from it to
//! the log's end are not yet done with.

use kafka_auth::authz::Operation;
use kafka_meta::{ShareRecordState, ShareState};
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError as E;
use kafka_wire::ids::{GroupId, PartitionId, TopicName};
use kafka_wire::messages::describe_share_group_offsets_request::DescribeShareGroupOffsetsRequestTopic;
use kafka_wire::messages::describe_share_group_offsets_response::{
    DescribeShareGroupOffsetsResponseGroup, DescribeShareGroupOffsetsResponsePartition,
    DescribeShareGroupOffsetsResponseTopic,
};
use kafka_wire::messages::{
    DescribeShareGroupOffsetsRequest, DescribeShareGroupOffsetsResponse, TopicName as WireTopic,
};

use super::{TOPIC_DENIED, text};
use crate::{Broker, Caller, Outcome, blocking};

/// Records done with (acknowledged or archived) from the stored start: Kafka's `deliveryCompleteCount`, -1 for a
/// share-partition that has no start yet.
fn delivery_complete(s: &ShareState) -> i64 {
    if s.start_offset < 0 {
        return -1;
    }
    s.batches
        .iter()
        .filter(|b| {
            matches!(
                b.state,
                ShareRecordState::Acknowledged | ShareRecordState::Archived
            )
        })
        .map(|b| b.last - b.first.max(s.start_offset) + 1)
        .filter(|n| *n > 0)
        .sum()
}

impl<F: Fs> Broker<F> {
    pub(crate) async fn describe_share_group_offsets(
        &self,
        version: i16,
        caller: &Caller,
        req: &DescribeShareGroupOffsetsRequest,
    ) -> Outcome {
        let guard = self.guard(caller);
        let mut resp = DescribeShareGroupOffsetsResponse::default();
        for g in &req.groups {
            let mut out = DescribeShareGroupOffsetsResponseGroup::default();
            out.group_id = g.group_id.clone();
            let name = g.group_id.0.as_str();
            if !guard.group(Operation::Describe, name) {
                out.error_code = E::GroupAuthorizationFailed.code();
            } else if let Ok(group) = GroupId::parse(name) {
                let may = |t: &str| guard.topic(Operation::Describe, t);
                let states = blocking::run(|| self.meta.share_states(&group)).unwrap_or_default();
                out.topics = match g.topics.as_deref() {
                    None => self.describe_every_share_topic(version, &states, may).await,
                    Some(asked) => {
                        self.describe_asked_share_topics(version, &states, asked, may)
                            .await
                    }
                };
            }
            resp.groups.push(out);
        }
        Outcome::Respond(kafka_wire::api::Response::DescribeShareGroupOffsets(resp))
    }

    /// Every partition the group has initialized, of the topics the caller may describe.
    async fn describe_every_share_topic(
        &self,
        version: i16,
        states: &[(TopicName, PartitionId, ShareState)],
        may: impl Fn(&str) -> bool,
    ) -> Vec<DescribeShareGroupOffsetsResponseTopic> {
        let mut out: Vec<DescribeShareGroupOffsetsResponseTopic> = Vec::new();
        for (name, p, s) in states {
            if !may(name.as_str()) {
                continue;
            }
            let Some(topic) = self.meta.topic(name) else {
                continue;
            };
            let d = self
                .share_offset_entry(version, &topic, p.get(), Some(s))
                .await;
            match out
                .iter_mut()
                .find(|t| t.topic_name.0.as_str() == name.as_str())
            {
                Some(t) => t.partitions.push(d),
                None => {
                    let mut t = DescribeShareGroupOffsetsResponseTopic::default();
                    t.topic_name = WireTopic(text(name.as_str()));
                    t.topic_id = topic.id.to_uuid();
                    t.partitions.push(d);
                    out.push(t);
                }
            }
        }
        out
    }

    /// The partitions asked for: a topic the caller may not describe is refused on each partition, after the rest; one
    /// that does not exist has no start; a partition never initialized has none either.
    async fn describe_asked_share_topics(
        &self,
        version: i16,
        states: &[(TopicName, PartitionId, ShareState)],
        asked: &[DescribeShareGroupOffsetsRequestTopic],
        may: impl Fn(&str) -> bool,
    ) -> Vec<DescribeShareGroupOffsetsResponseTopic> {
        let mut out = Vec::new();
        let mut refused = Vec::new();
        for t in asked {
            let name = t.topic_name.0.as_str();
            let mut topic_out = DescribeShareGroupOffsetsResponseTopic::default();
            topic_out.topic_name = t.topic_name.clone();
            if !may(name) {
                topic_out.partitions = no_offsets(
                    &t.partitions,
                    Some((E::TopicAuthorizationFailed, TOPIC_DENIED)),
                );
                refused.push(topic_out);
                continue;
            }
            let Some(topic) = TopicName::parse(name)
                .ok()
                .and_then(|n| self.meta.topic(&n))
            else {
                topic_out.partitions = no_offsets(&t.partitions, None);
                out.push(topic_out);
                continue;
            };
            topic_out.topic_id = topic.id.to_uuid();
            for p in &t.partitions {
                let s = states
                    .iter()
                    .find(|(n, q, _)| *n == topic.name && q.get() == *p)
                    .map(|(_, _, s)| s);
                let d = self.share_offset_entry(version, &topic, *p, s).await;
                topic_out.partitions.push(d);
            }
            out.push(topic_out);
        }
        out.extend(refused);
        out
    }

    /// One partition as DescribeShareGroupOffsets shows it: its stored start (-1 if it has none, or no start yet) and, from
    /// version 1, its lag: the records from the start to the log's end not yet done with (Kafka's
    /// `latestOffset - startOffset - deliveryCompleteCount`, -1 where either is not known).
    async fn share_offset_entry(
        &self,
        version: i16,
        topic: &kafka_meta::Topic,
        p: i32,
        s: Option<&ShareState>,
    ) -> DescribeShareGroupOffsetsResponsePartition {
        let mut d = DescribeShareGroupOffsetsResponsePartition::default();
        d.partition_index = p;
        d.leader_epoch = 0;
        d.start_offset = s.map_or(-1, |s| s.start_offset);
        d.lag = -1;
        let Some(s) = s else {
            return d;
        };
        let done = delivery_complete(s);
        if version < 1 || s.start_offset < 0 || done < 0 {
            return d;
        }
        let Some(partition) = PartitionId::new(p) else {
            return d;
        };
        match self.partitions.get(topic, partition).await {
            Ok(log) => d.lag = (log.reader().end_offset().get() - s.start_offset - done).max(0),
            Err(e) => {
                tracing::error!(topic = %topic.name, partition = p, error = %e, "a share group's lag could not be read");
                d.error_code = E::KafkaStorageError.code();
                d.error_message = Some(text("The partition's end could not be read."));
            }
        }
        d
    }
}

/// Partitions with no start to tell: -1 and -1, with an error or none.
fn no_offsets(
    partitions: &[i32],
    error: Option<(E, &str)>,
) -> Vec<DescribeShareGroupOffsetsResponsePartition> {
    partitions
        .iter()
        .map(|p| {
            let mut d = DescribeShareGroupOffsetsResponsePartition::default();
            d.partition_index = *p;
            d.start_offset = -1;
            d.lag = -1;
            if let Some((e, words)) = error {
                d.error_code = e.code();
                d.error_message = Some(text(words));
            }
            d
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kafka_meta::ShareBatch;

    #[test]
    fn what_is_done_with_counts_from_the_start_and_an_unstarted_partition_counts_nothing() {
        let batch = |first, last, state| ShareBatch {
            first,
            last,
            state,
            delivery_count: 1,
        };
        let s = ShareState {
            state_epoch: 0,
            start_offset: 5,
            batches: vec![
                batch(5, 6, ShareRecordState::Acknowledged),
                batch(8, 8, ShareRecordState::Available),
                batch(9, 10, ShareRecordState::Archived),
            ],
        };
        assert_eq!(delivery_complete(&s), 4);
        let unstarted = ShareState {
            state_epoch: 0,
            start_offset: -1,
            batches: Vec::new(),
        };
        assert_eq!(delivery_complete(&unstarted), -1);
    }
}
