//! The share groups' admin APIs (KIP-932): DescribeShareGroupOffsets (90), AlterShareGroupOffsets (91) and
//! DeleteShareGroupOffsets (92), as Kafka 4.4.0 answers them (`KafkaApis` L3853-4077, `GroupCoordinatorService` and
//! `GroupMetadataManager`). They read and write the stored share-partition states: the start offset (SPSO) and what is
//! done with past it, which a share group's "offsets" are.

use kafka_auth::authz::Operation;
use kafka_meta::{GroupLife, ShareLife, ShareRecordState, ShareState};
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError as E;
use kafka_wire::StrBytes;
use kafka_wire::Uuid;
use kafka_wire::ids::{GroupId, PartitionId, TopicName};
use kafka_wire::messages::alter_share_group_offsets_request::AlterShareGroupOffsetsRequestTopic;
use kafka_wire::messages::alter_share_group_offsets_response::{
    AlterShareGroupOffsetsResponsePartition, AlterShareGroupOffsetsResponseTopic,
};
use kafka_wire::messages::delete_share_group_offsets_request::DeleteShareGroupOffsetsRequestTopic;
use kafka_wire::messages::delete_share_group_offsets_response::DeleteShareGroupOffsetsResponseTopic;
use kafka_wire::messages::describe_share_group_offsets_request::DescribeShareGroupOffsetsRequestTopic;
use kafka_wire::messages::describe_share_group_offsets_response::{
    DescribeShareGroupOffsetsResponseGroup, DescribeShareGroupOffsetsResponsePartition,
    DescribeShareGroupOffsetsResponseTopic,
};
use kafka_wire::messages::{
    AlterShareGroupOffsetsRequest, AlterShareGroupOffsetsResponse, DeleteShareGroupOffsetsRequest,
    DeleteShareGroupOffsetsResponse, DescribeShareGroupOffsetsRequest,
    DescribeShareGroupOffsetsResponse, TopicName as WireTopic,
};

use crate::{Broker, Caller, Outcome, blocking};

const TOPIC_DENIED: &str = "Topic authorization failed.";
const UNKNOWN_PARTITION: &str = "This server does not host this topic-partition.";

/// What a group id is, to the share groups' admin APIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShareGroupIs {
    /// No group of any kind.
    Missing,
    /// A classic or consumer group.
    OtherKind,
    /// A share group: its members and epoch.
    Share { members: usize, epoch: i32 },
}

fn text(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

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
    /// What `group` is: a share group (in memory, or recorded), another kind, or nothing.
    pub(crate) async fn share_group_is(&self, group: &GroupId) -> ShareGroupIs {
        if let Some(d) = self.groups.describe(group).await {
            return match d.share {
                Some(v) => ShareGroupIs::Share {
                    members: v.members.len(),
                    epoch: v.epoch,
                },
                None => ShareGroupIs::OtherKind,
            };
        }
        let (life, offsets) = blocking::run(|| {
            (
                self.meta.group_life(group).ok().flatten(),
                self.meta.group_offsets(group).is_ok_and(|o| !o.is_empty()),
            )
        });
        match life {
            Some(GroupLife { share: Some(s), .. }) => ShareGroupIs::Share {
                members: 0,
                epoch: s.epoch,
            },
            Some(_) => ShareGroupIs::OtherKind,
            None if offsets => ShareGroupIs::OtherKind,
            None => ShareGroupIs::Missing,
        }
    }

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

    /// The group an admin write names, if the caller may do `op` on it and the id is valid; with what it is.
    async fn share_admin_group(
        &self,
        caller: &Caller,
        op: Operation,
        name: &str,
    ) -> Result<(GroupId, ShareGroupIs), (E, String)> {
        if !self.guard(caller).group(op, name) {
            return Err((
                E::GroupAuthorizationFailed,
                "Group authorization failed.".into(),
            ));
        }
        let Ok(group) = GroupId::parse(name) else {
            return Err((
                E::InvalidGroupId,
                "The configured groupId is invalid.".into(),
            ));
        };
        let is = self.share_group_is(&group).await;
        if let ShareGroupIs::Share { members, .. } = is
            && members > 0
        {
            return Err((E::NonEmptyGroup, "The group is not empty.".into()));
        }
        Ok((group, is))
    }

    pub(crate) async fn alter_share_group_offsets(
        &self,
        caller: &Caller,
        req: &AlterShareGroupOffsetsRequest,
    ) -> Outcome {
        let refuse = |(error, message): (E, String)| {
            let mut resp = AlterShareGroupOffsetsResponse::default();
            resp.error_code = error.code();
            resp.error_message = Some(StrBytes::from_string(message));
            Outcome::Respond(kafka_wire::api::Response::AlterShareGroupOffsets(resp))
        };
        let name = req.group_id.0.as_str();
        let (group, epoch) = match self.share_admin_group(caller, Operation::Read, name).await {
            Err(refused) => return refuse(refused),
            Ok((_, ShareGroupIs::OtherKind)) => {
                return refuse((
                    E::GroupIdNotFound,
                    format!("Group {name} is not a share group."),
                ));
            }
            Ok((group, ShareGroupIs::Share { epoch, .. })) => (group, epoch),
            Ok((group, ShareGroupIs::Missing)) => (group, 0),
        };
        let guard = self.guard(caller);
        let mut resp = AlterShareGroupOffsetsResponse::default();
        let mut writes: Vec<(TopicName, PartitionId, ShareState)> = Vec::new();
        let mut touched: Vec<Uuid> = Vec::new();
        for t in &req.topics {
            let out = if guard.topic(Operation::Read, t.topic_name.0.as_str()) {
                self.alter_share_topic(t, epoch, &mut writes, &mut touched)
            } else {
                let mut out = AlterShareGroupOffsetsResponseTopic::default();
                out.topic_name = t.topic_name.clone();
                out.partitions = t
                    .partitions
                    .iter()
                    .map(|p| {
                        altered(
                            p.partition_index,
                            Some((E::TopicAuthorizationFailed, TOPIC_DENIED)),
                        )
                    })
                    .collect();
                out
            };
            resp.responses.push(out);
        }
        if let Err(e) = self.store_share_offsets(&group, epoch, &writes) {
            tracing::error!(%group, error = %e, "a share group's offsets could not be stored");
            return refuse((
                E::KafkaStorageError,
                "The share group's offsets could not be stored.".into(),
            ));
        }
        // What is in memory is read again from the store at the next fetch.
        self.shares.forget(&group, Some(&touched));
        Outcome::Respond(kafka_wire::api::Response::AlterShareGroupOffsets(resp))
    }

    /// One topic of an AlterShareGroupOffsets the caller may make: each partition that exists gets its new start (and
    /// nothing in flight), one that does not is refused.
    fn alter_share_topic(
        &self,
        t: &AlterShareGroupOffsetsRequestTopic,
        epoch: i32,
        writes: &mut Vec<(TopicName, PartitionId, ShareState)>,
        touched: &mut Vec<Uuid>,
    ) -> AlterShareGroupOffsetsResponseTopic {
        let mut out = AlterShareGroupOffsetsResponseTopic::default();
        out.topic_name = t.topic_name.clone();
        let unknown = Some((E::UnknownTopicOrPartition, UNKNOWN_PARTITION));
        let Some(topic) = TopicName::parse(t.topic_name.0.as_str())
            .ok()
            .and_then(|n| self.meta.topic(&n))
        else {
            out.partitions = t
                .partitions
                .iter()
                .map(|p| altered(p.partition_index, unknown))
                .collect();
            return out;
        };
        out.topic_id = topic.id.to_uuid();
        touched.push(topic.id.to_uuid());
        for p in &t.partitions {
            let Some(partition) =
                PartitionId::new(p.partition_index).filter(|q| q.get() < topic.partitions.get())
            else {
                out.partitions.push(altered(p.partition_index, unknown));
                continue;
            };
            writes.push((
                topic.name.clone(),
                partition,
                ShareState {
                    state_epoch: epoch,
                    start_offset: p.start_offset,
                    batches: Vec::new(),
                },
            ));
            out.partitions.push(altered(p.partition_index, None));
        }
        out
    }

    /// The new starts, stored; a group that was not there is made, empty (Kafka's `getOrMaybeCreateShareGroup`).
    fn store_share_offsets(
        &self,
        group: &GroupId,
        epoch: i32,
        writes: &[(TopicName, PartitionId, ShareState)],
    ) -> Result<(), kafka_meta::MetaError> {
        blocking::run(|| {
            if self.meta.group_life(group)?.is_none() {
                self.meta.set_group_life(
                    group,
                    &GroupLife {
                        live: false,
                        session_ms: 0,
                        empty_since_ms: None,
                        protocol_type: "share".to_owned(),
                        consumer: None,
                        share: Some(ShareLife {
                            epoch,
                            assignment_epoch: epoch,
                        }),
                    },
                )?;
            }
            let list: Vec<kafka_meta::ShareStateWrite<'_>> =
                writes.iter().map(|(t, p, s)| (t, *p, s)).collect();
            self.meta.set_share_states(group, &list).map(drop)
        })
    }

    pub(crate) async fn delete_share_group_offsets(
        &self,
        caller: &Caller,
        req: &DeleteShareGroupOffsetsRequest,
    ) -> Outcome {
        let refuse = |(error, message): (E, String)| {
            let mut resp = DeleteShareGroupOffsetsResponse::default();
            resp.error_code = error.code();
            resp.error_message = Some(StrBytes::from_string(message));
            Outcome::Respond(kafka_wire::api::Response::DeleteShareGroupOffsets(resp))
        };
        let name = req.group_id.0.as_str();
        let group = match self
            .share_admin_group(caller, Operation::Delete, name)
            .await
        {
            Err(refused) => return refuse(refused),
            Ok((_, ShareGroupIs::Missing)) => {
                return refuse((E::GroupIdNotFound, format!("Group {name} not found.")));
            }
            Ok((_, ShareGroupIs::OtherKind)) => {
                return refuse((
                    E::GroupIdNotFound,
                    format!("Group {name} is not a share group."),
                ));
            }
            Ok((group, ShareGroupIs::Share { .. })) => group,
        };
        let guard = self.guard(caller);
        let have = blocking::run(|| self.meta.share_states(&group)).unwrap_or_default();
        let mut resp = DeleteShareGroupOffsetsResponse::default();
        let mut done = Vec::new();
        for t in &req.topics {
            if guard.topic(Operation::Read, t.topic_name.0.as_str()) {
                done.push(self.delete_share_topic(&group, &have, t));
            } else {
                let mut out = DeleteShareGroupOffsetsResponseTopic::default();
                out.topic_name = t.topic_name.clone();
                out.error_code = E::TopicAuthorizationFailed.code();
                out.error_message = Some(text(TOPIC_DENIED));
                resp.responses.push(out);
            }
        }
        resp.responses.extend(done);
        Outcome::Respond(kafka_wire::api::Response::DeleteShareGroupOffsets(resp))
    }

    /// One topic of a DeleteShareGroupOffsets the caller may make: its stored states deleted, if the group has any.
    fn delete_share_topic(
        &self,
        group: &GroupId,
        have: &[(TopicName, PartitionId, ShareState)],
        t: &DeleteShareGroupOffsetsRequestTopic,
    ) -> DeleteShareGroupOffsetsResponseTopic {
        let mut out = DeleteShareGroupOffsetsResponseTopic::default();
        out.topic_name = t.topic_name.clone();
        let Some(topic) = TopicName::parse(t.topic_name.0.as_str())
            .ok()
            .and_then(|n| self.meta.topic(&n))
        else {
            out.error_code = E::UnknownTopicOrPartition.code();
            out.error_message = Some(text(UNKNOWN_PARTITION));
            return out;
        };
        out.topic_id = topic.id.to_uuid();
        if !have.iter().any(|(n, _, _)| *n == topic.name) {
            out.error_code = E::UnknownTopicOrPartition.code();
            out.error_message = Some(text("There is no offset information to delete."));
            return out;
        }
        let deleted = blocking::run(|| {
            self.meta
                .delete_share_states(group, Some(std::slice::from_ref(&topic.name)))
        });
        if let Err(e) = deleted {
            tracing::error!(%group, error = %e, "a share group's offsets could not be deleted");
            out.error_code = E::KafkaStorageError.code();
            out.error_message = Some(text("The share group's offsets could not be deleted."));
        } else {
            self.shares.forget(group, Some(&[topic.id.to_uuid()]));
        }
        out
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

/// One partition of an AlterShareGroupOffsets answer.
fn altered(p: i32, error: Option<(E, &str)>) -> AlterShareGroupOffsetsResponsePartition {
    let mut d = AlterShareGroupOffsetsResponsePartition::default();
    d.partition_index = p;
    if let Some((e, words)) = error {
        d.error_code = e.code();
        d.error_message = Some(text(words));
    }
    d
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
