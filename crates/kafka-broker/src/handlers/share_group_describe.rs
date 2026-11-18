//! ShareGroupDescribe (KIP-932): a share group's state, epochs and members with what each reads. Kafka 4.4.0
//! (`KafkaApis.handleShareGroupDescribe`, `GroupMetadataManager.shareGroupDescribe`).

use kafka_auth::authz::Operation;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError as E;
use kafka_wire::StrBytes;
use kafka_wire::ids::{GroupId, TopicId};
use kafka_wire::messages::share_group_describe_response::{
    Assignment, DescribedGroup, Member, TopicPartitions,
};
use kafka_wire::messages::{
    GroupId as WireGroupId, ShareGroupDescribeRequest, ShareGroupDescribeResponse, TopicName,
};

use super::consumer_group_heartbeat::by_topic;
use crate::groups::{Partitions, kafka_order};
use crate::{Broker, Caller, Outcome};

/// Kafka's words for a group the caller may not see all of (`KafkaApis.handleShareGroupDescribe`).
const HIDDEN_TOPIC: &str =
    "The group has described topic(s) that the client is not authorized to describe.";

impl<F: Fs> Broker<F> {
    /// Each group named: refused (30, no words) where the caller may not DESCRIBE it, ahead of the rest in Kafka's order;
    /// one with a member reading a topic the caller may not DESCRIBE is told 29 instead of described.
    pub(crate) async fn share_group_describe(
        &self,
        caller: &Caller,
        req: &ShareGroupDescribeRequest,
    ) -> Outcome {
        let guard = self.guard(caller);
        let mut resp = ShareGroupDescribeResponse::default();
        let mut asked: Vec<String> = Vec::new();
        for g in &req.group_ids {
            if guard.group(Operation::Describe, g.0.as_str()) {
                asked.push(g.0.as_str().to_owned());
            } else {
                let mut refused = DescribedGroup::default();
                refused.group_id = g.clone();
                refused.error_code = E::GroupAuthorizationFailed.code();
                resp.groups.push(refused);
            }
        }
        for name in kafka_order(&asked) {
            let mut described = self.describe_share_group(&name).await;
            if req.include_authorized_operations && described.error_code == 0 {
                described.authorized_operations = guard.group_operations(&name);
            }
            resp.groups.push(described);
        }
        for group in &mut resp.groups {
            let hidden = group
                .members
                .iter()
                .flat_map(|m| &m.assignment.topic_partitions)
                .any(|tp| !guard.topic(Operation::Describe, tp.topic_name.as_str()));
            if hidden {
                let mut out = DescribedGroup::default();
                out.group_id = group.group_id.clone();
                out.error_code = E::TopicAuthorizationFailed.code();
                out.error_message = Some(StrBytes::from_static_str(HIDDEN_TOPIC));
                *group = out;
            }
        }
        Outcome::Respond(kafka_wire::api::Response::ShareGroupDescribe(resp))
    }

    async fn describe_share_group(&self, name: &str) -> DescribedGroup {
        let mut out = DescribedGroup::default();
        out.group_id = WireGroupId(StrBytes::from_string(name.to_owned()));
        let refuse = |mut out: DescribedGroup, error: E, words: Option<String>| {
            out.error_code = error.code();
            out.error_message = words.map(StrBytes::from_string);
            out
        };
        if name.is_empty() {
            return refuse(out, E::InvalidGroupId, None);
        }
        let Ok(group) = GroupId::parse(name) else {
            return refuse(out, E::InvalidGroupId, None);
        };
        let Some(description) = self.groups.describe(&group).await else {
            return match self.kept_share_group(&group) {
                Ok((epoch, assignment_epoch)) => {
                    out.group_state = StrBytes::from_static_str("Empty");
                    out.group_epoch = epoch;
                    out.assignment_epoch = assignment_epoch;
                    out.assignor_name = StrBytes::from_static_str("simple");
                    out
                }
                Err(words) => refuse(out, E::GroupIdNotFound, Some(words)),
            };
        };
        let Some(view) = description.share else {
            return refuse(
                out,
                E::GroupIdNotFound,
                Some(format!("Group {name} is not a share group.")),
            );
        };
        out.group_state = StrBytes::from_static_str(view.state);
        out.group_epoch = view.epoch;
        out.assignment_epoch = view.assignment_epoch;
        out.assignor_name = StrBytes::from_static_str("simple");
        out.members = view
            .members
            .into_iter()
            .map(|m| {
                let mut member = Member::default();
                member.member_id = StrBytes::from_string(m.id.to_string());
                member.rack_id = m.rack.map(StrBytes::from_string);
                member.member_epoch = m.epoch;
                member.client_id = StrBytes::from_string(m.client_id);
                member.client_host = StrBytes::from_string(m.client_host);
                member.subscribed_topic_names = m
                    .subscribed
                    .into_iter()
                    .map(|t| TopicName(StrBytes::from_string(t)))
                    .collect();
                member.assignment = self.share_assignment(&m.assigned);
                member
            })
            .collect();
        out
    }

    /// A group not in memory: an empty share group the broker had before it was started again is still one (its epochs);
    /// else Kafka's words for a group of another kind, or none.
    fn kept_share_group(&self, group: &GroupId) -> Result<(i32, i32), String> {
        let kept = crate::blocking::run(|| self.meta.group_life(group))
            .ok()
            .flatten();
        match kept {
            Some(kafka_meta::GroupLife { share: Some(s), .. }) => Ok((s.epoch, s.assignment_epoch)),
            Some(_) => Err(format!("Group {group} is not a share group.")),
            None if crate::blocking::run(|| self.meta.group_offsets(group))
                .is_ok_and(|o| !o.is_empty()) =>
            {
                Err(format!("Group {group} is not a share group."))
            }
            None => Err(format!("Group {group} not found.")),
        }
    }

    /// Partitions as ShareGroupDescribe shows them: by topic, with its name; a topic gone is left out.
    fn share_assignment(&self, parts: &Partitions) -> Assignment {
        let mut out = Assignment::default();
        out.topic_partitions = by_topic(parts)
            .into_iter()
            .filter_map(|(topic_id, partitions)| {
                let topic = self
                    .meta
                    .topic_by_id(TopicId::from_bytes(*topic_id.as_bytes()))?;
                let mut tp = TopicPartitions::default();
                tp.topic_id = topic_id;
                tp.topic_name = TopicName(StrBytes::from_string(topic.name.to_string()));
                tp.partitions = partitions;
                Some(tp)
            })
            .collect();
        out
    }
}
