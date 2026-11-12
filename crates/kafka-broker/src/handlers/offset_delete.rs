//! OffsetDelete: a group forgets its offsets for some partitions.

use kafka_auth::authz::operation;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::ids::{GroupId, PartitionId, TopicName};
use kafka_wire::messages::offset_delete_response::{
    OffsetDeleteResponsePartition, OffsetDeleteResponseTopic,
};
use kafka_wire::messages::{OffsetDeleteRequest, OffsetDeleteResponse};

use crate::{Broker, Caller, Outcome, blocking};

impl<F: Fs> Broker<F> {
    /// Forgets the offsets a group has for the partitions named. A group that is not there and has no offsets is not
    /// found. A topic the group's members subscribe to is refused, since they would only commit it again, and a
    /// topic that does not exist is unknown.
    ///
    /// The caller must be allowed to DELETE the group (30 at the top) and to READ each topic (29 on its partitions),
    /// before the group or the topic is looked for.
    pub(crate) async fn offset_delete(
        &self,
        caller: &Caller,
        req: &OffsetDeleteRequest,
    ) -> Outcome {
        let guard = self.guard(caller);
        let mut resp = OffsetDeleteResponse::default();
        if !guard.group(operation::DELETE, req.group_id.as_str()) {
            resp.error_code = ResponseError::GroupAuthorizationFailed.code();
            return Outcome::Respond(kafka_wire::api::Response::OffsetDelete(resp));
        }
        let Ok(group) = GroupId::parse(req.group_id.as_str()) else {
            resp.error_code = ResponseError::InvalidGroupId.code();
            return Outcome::Respond(kafka_wire::api::Response::OffsetDelete(resp));
        };
        let described = self.groups.describe(&group).await;
        let committed = blocking::run(|| self.meta.group_offsets(&group)).unwrap_or_default();
        let has_members = described.as_ref().is_some_and(|d| {
            !d.members.is_empty() || d.consumer.as_ref().is_some_and(|c| !c.members.is_empty())
        });
        if !has_members && committed.is_empty() {
            resp.error_code = ResponseError::GroupIdNotFound.code();
            return Outcome::Respond(kafka_wire::api::Response::OffsetDelete(resp));
        }
        let subscribed = described.and_then(|d| d.subscribed);
        let mut to_delete: Vec<(TopicName, PartitionId)> = Vec::new();
        let mut slots: Vec<(usize, usize)> = Vec::new();
        for (t, topic) in req.topics.iter().enumerate() {
            let mut out = OffsetDeleteResponseTopic::default();
            out.name = topic.name.clone();
            let name = TopicName::parse(topic.name.as_str()).ok();
            for (p, partition) in topic.partitions.iter().enumerate() {
                let mut answer = OffsetDeleteResponsePartition::default();
                answer.partition_index = partition.partition_index;
                let barred = !guard.topic(operation::READ, topic.name.as_str());
                let held = subscribed
                    .as_ref()
                    .is_some_and(|s| s.contains(topic.name.as_str()));
                if barred {
                    answer.error_code = ResponseError::TopicAuthorizationFailed.code();
                } else if held {
                    answer.error_code = ResponseError::GroupSubscribedToTopic.code();
                } else if let (Some(name), Some(id), true) = (
                    &name,
                    PartitionId::new(partition.partition_index),
                    name.as_ref()
                        .and_then(|n| self.meta.topic(n))
                        .is_some_and(|t| partition.partition_index < t.partitions.get()),
                ) {
                    to_delete.push((name.clone(), id));
                    slots.push((t, p));
                } else {
                    answer.error_code = ResponseError::UnknownTopicOrPartition.code();
                }
                out.partitions.push(answer);
            }
            resp.topics.push(out);
        }
        if !to_delete.is_empty()
            && let Err(e) = blocking::run(|| self.meta.delete_offsets(&group, &to_delete))
        {
            tracing::error!(%group, error = %e, "deleting offsets failed");
            for (t, p) in slots {
                if let Some(a) = resp.topics.get_mut(t).and_then(|x| x.partitions.get_mut(p)) {
                    a.error_code = ResponseError::NotCoordinator.code();
                }
            }
        }
        Outcome::Respond(kafka_wire::api::Response::OffsetDelete(resp))
    }
}
