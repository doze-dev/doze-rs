//! ConsumerGroupDescribe (KIP-848): a consumer group's state, epochs and members with what each holds and is to hold.

use kafka_storage::fs::Fs;
use kafka_wire::ResponseError as E;
use kafka_wire::StrBytes;
use kafka_wire::ids::{GroupId, TopicId};
use kafka_wire::messages::consumer_group_describe_response::{
    Assignment, DescribedGroup, Member, TopicPartitions,
};
use kafka_wire::messages::{
    ConsumerGroupDescribeRequest, ConsumerGroupDescribeResponse, GroupId as WireGroupId, TopicName,
};

use super::consumer_group_heartbeat::by_topic;
use crate::groups::{Partitions, kafka_order};
use crate::{Broker, Outcome};

/// What a client is shown as the member's address.
const CLIENT_HOST: &str = "/127.0.0.1";

impl<F: Fs> Broker<F> {
    /// Describes each group named, in the order Kafka answers (that of DescribeGroups).
    pub(crate) async fn consumer_group_describe(
        &self,
        req: &ConsumerGroupDescribeRequest,
    ) -> Outcome {
        let mut resp = ConsumerGroupDescribeResponse::default();
        let asked: Vec<String> = req
            .group_ids
            .iter()
            .map(|g| g.0.as_str().to_owned())
            .collect();
        for name in kafka_order(&asked) {
            resp.groups.push(self.describe_consumer_group(&name).await);
        }
        Outcome::Respond(kafka_wire::api::Response::ConsumerGroupDescribe(resp))
    }

    async fn describe_consumer_group(&self, name: &str) -> DescribedGroup {
        let mut out = DescribedGroup::default();
        out.group_id = WireGroupId(StrBytes::from_string(name.to_owned()));
        let refuse = |mut out: DescribedGroup, error: E, words: String| {
            out.error_code = error.code();
            out.error_message = Some(StrBytes::from_string(words));
            out
        };
        let Ok(group) = GroupId::parse(name) else {
            return refuse(
                out,
                E::InvalidGroupId,
                format!("Group {name} is not valid."),
            );
        };
        let Some(description) = self.groups.describe(&group).await else {
            return refuse(out, E::GroupIdNotFound, format!("Group {name} not found."));
        };
        let Some(view) = description.consumer else {
            // A classic group: the new API does not describe it.
            return refuse(
                out,
                E::GroupIdNotFound,
                format!("Group {name} is not a consumer group."),
            );
        };
        out.group_state = StrBytes::from_static_str(view.state);
        out.group_epoch = view.epoch;
        out.assignment_epoch = view.assignment_epoch;
        out.assignor_name = StrBytes::from_static_str(view.assignor);
        out.members = view
            .members
            .into_iter()
            .map(|m| {
                let mut member = Member::default();
                member.member_id = StrBytes::from_string(m.id.to_string());
                member.instance_id = m.instance.map(|i| StrBytes::from_string(i.to_string()));
                member.rack_id = m.rack.map(StrBytes::from_string);
                member.member_epoch = m.epoch;
                member.client_id = StrBytes::from_string(m.client_id);
                member.client_host = StrBytes::from_static_str(CLIENT_HOST);
                member.subscribed_topic_names = m
                    .subscribed
                    .into_iter()
                    .map(|t| TopicName(StrBytes::from_string(t)))
                    .collect();
                // "" when there is none, as Kafka sends it.
                member.subscribed_topic_regex = Some(StrBytes::from_string(m.regex));
                member.assignment = self.assignment_of(&m.assigned);
                member.target_assignment = self.assignment_of(&m.target);
                member.member_type = i8::from(!m.classic);
                member
            })
            .collect();
        out
    }

    /// Partitions as ConsumerGroupDescribe shows them: with the topic's name, or by id alone if it is gone.
    fn assignment_of(&self, parts: &Partitions) -> Assignment {
        let mut out = Assignment::default();
        out.topic_partitions = by_topic(parts)
            .into_iter()
            .map(|(topic_id, partitions)| {
                let mut tp = TopicPartitions::default();
                tp.topic_id = topic_id;
                if let Some(topic) = self
                    .meta
                    .topic_by_id(TopicId::from_bytes(*topic_id.as_bytes()))
                {
                    tp.topic_name = TopicName(StrBytes::from_string(topic.name.to_string()));
                }
                tp.partitions = partitions;
                tp
            })
            .collect();
        out
    }
}
