//! StreamsGroupDescribe (KIP-1071): a streams group's state, epochs, topology and members with their tasks. Kafka 4.4.0
//! (`KafkaApis.handleStreamsGroupDescribe`, `StreamsGroup.asDescribedGroup`); and StreamsGroupTopologyDescriptionUpdate
//! (KIP-1331), which a broker without a topology description plugin (Kafka's default) refuses.

use std::collections::BTreeSet;

use kafka_auth::authz::Operation;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError as E;
use kafka_wire::StrBytes;
use kafka_wire::ids::GroupId;
use kafka_wire::messages::streams_group_describe_response::{
    Assignment, DescribedGroup, Endpoint, KeyValue, Member, Subtopology, TaskIds, TaskOffset,
    TopicInfo, Topology,
};
use kafka_wire::messages::{
    GroupId as WireGroupId, StreamsGroupDescribeRequest, StreamsGroupDescribeResponse,
    StreamsGroupTopologyDescriptionUpdateRequest, StreamsGroupTopologyDescriptionUpdateResponse,
    TopicName,
};

use crate::groups::kafka_order;
use crate::groups::streams::{
    StreamsMemberView, StreamsView, Task, Tasks, by_subtopology, topology,
};
use crate::{Broker, Caller, Outcome};

/// Kafka's words for a group whose topology names a topic the caller may not see.
const HIDDEN_TOPIC: &str =
    "The described group uses topics that the client is not authorized to describe.";

/// `TopologyDescriptionStatus` NOT_STORED: asked for, and there is no plugin to have stored one.
const NOT_STORED: i8 = 1;

fn text(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

fn info(t: &topology::TopicInfo) -> TopicInfo {
    let mut out = TopicInfo::default();
    out.name = TopicName(text(&t.name));
    out.partitions = t.partitions;
    out.replication_factor = t.replication;
    out.topic_configs = t
        .configs
        .iter()
        .map(|(k, v)| {
            let mut kv = KeyValue::default();
            kv.key = text(k);
            kv.value = text(v);
            kv
        })
        .collect();
    out
}

/// The topology as described: subtopologies by id, their topic lists sorted.
fn topology_of(t: &topology::Topology) -> Topology {
    let mut out = Topology::default();
    out.epoch = t.epoch;
    let names = |v: &[String]| -> Vec<TopicName> {
        let sorted: BTreeSet<&String> = v.iter().collect();
        sorted.into_iter().map(|n| TopicName(text(n))).collect()
    };
    let infos = |v: &[topology::TopicInfo]| -> Vec<TopicInfo> {
        let mut sorted: Vec<&topology::TopicInfo> = v.iter().collect();
        sorted.sort_by(|a, b| a.name.cmp(&b.name));
        sorted.into_iter().map(info).collect()
    };
    out.subtopologies = Some(
        t.subtopologies
            .iter()
            .map(|(id, s)| {
                let mut sub = Subtopology::default();
                sub.subtopology_id = text(id);
                sub.source_topics = names(&s.sources);
                sub.repartition_sink_topics = names(&s.repartition_sinks);
                sub.state_changelog_topics = infos(&s.changelogs);
                sub.repartition_source_topics = infos(&s.repartition_sources);
                sub
            })
            .collect(),
    );
    out
}

fn task_ids(tasks: &BTreeSet<Task>) -> Vec<TaskIds> {
    by_subtopology(tasks)
        .into_iter()
        .map(|(sub, partitions)| {
            let mut t = TaskIds::default();
            t.subtopology_id = text(sub);
            t.partitions = partitions;
            t
        })
        .collect()
}

fn assignment(t: &Tasks) -> Assignment {
    let mut a = Assignment::default();
    a.active_tasks = task_ids(&t.active);
    a.standby_tasks = task_ids(&t.standby);
    a.warmup_tasks = task_ids(&t.warmup);
    a
}

fn offsets(o: &std::collections::BTreeMap<Task, i64>) -> Vec<TaskOffset> {
    o.iter()
        .map(|((sub, p), offset)| {
            let mut t = TaskOffset::default();
            t.subtopology_id = text(sub);
            t.partition = *p;
            t.offset = *offset;
            t
        })
        .collect()
}

fn member_of(m: &StreamsMemberView) -> Member {
    let mut out = Member::default();
    out.member_id = text(m.id.as_str());
    out.member_epoch = m.epoch;
    out.instance_id = m.instance.as_ref().map(|i| text(i.as_str()));
    out.rack_id = m.rack.as_deref().map(text);
    out.client_id = text(&m.client_id);
    out.client_host = text(&m.client_host);
    out.topology_epoch = m.topology_epoch;
    out.process_id = text(&m.process);
    out.user_endpoint = m.endpoint.as_ref().map(|e| {
        let mut ep = Endpoint::default();
        ep.host = text(&e.host);
        ep.port = e.port;
        ep
    });
    out.client_tags = m
        .client_tags
        .iter()
        .map(|(k, v)| {
            let mut kv = KeyValue::default();
            kv.key = text(k);
            kv.value = text(v);
            kv
        })
        .collect();
    out.task_offsets = offsets(&m.offsets);
    out.task_end_offsets = offsets(&m.end_offsets);
    out.assignment = assignment(&m.assignment);
    out.target_assignment = assignment(&m.target);
    out
}

/// A group as described, from its view.
fn described(name: &str, view: &StreamsView, with_description: bool) -> DescribedGroup {
    let mut out = DescribedGroup::default();
    out.group_id = WireGroupId(text(name));
    out.group_state = StrBytes::from_static_str(view.state);
    out.group_epoch = view.epoch;
    out.assignment_epoch = view.assignment_epoch;
    out.topology = view.topology.as_ref().map(topology_of);
    out.members = view.members.iter().map(member_of).collect();
    out.assignor_name = Some(StrBytes::from_static_str(
        crate::group_configs::streams::STICKY,
    ));
    if with_description {
        out.topology_description_status = NOT_STORED;
    }
    out
}

impl<F: Fs> Broker<F> {
    /// Each group named: refused (30, no words) where the caller may not DESCRIBE it, ahead of the rest in Kafka's order;
    /// one whose topology names a topic the caller may not DESCRIBE is told 29 instead of described.
    pub(crate) async fn streams_group_describe(
        &self,
        caller: &Caller,
        req: &StreamsGroupDescribeRequest,
    ) -> Outcome {
        let guard = self.guard(caller);
        let mut resp = StreamsGroupDescribeResponse::default();
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
            let mut d = self
                .describe_streams_group(&name, req.include_topology_description)
                .await;
            let hidden = d.topology.as_ref().is_some_and(|t| {
                t.subtopologies.iter().flatten().any(|s| {
                    s.source_topics
                        .iter()
                        .chain(&s.repartition_sink_topics)
                        .map(|n| n.0.as_str())
                        .chain(s.state_changelog_topics.iter().map(|i| i.name.0.as_str()))
                        .chain(
                            s.repartition_source_topics
                                .iter()
                                .map(|i| i.name.0.as_str()),
                        )
                        .any(|n| !guard.topic(Operation::Describe, n))
                })
            });
            if hidden {
                let mut out = DescribedGroup::default();
                out.group_id = d.group_id.clone();
                out.error_code = E::TopicAuthorizationFailed.code();
                out.error_message = Some(StrBytes::from_static_str(HIDDEN_TOPIC));
                d = out;
            } else if req.include_authorized_operations && d.error_code == 0 {
                d.authorized_operations = guard.group_operations(&name);
            }
            resp.groups.push(d);
        }
        Outcome::Respond(kafka_wire::api::Response::StreamsGroupDescribe(resp))
    }

    async fn describe_streams_group(&self, name: &str, with_description: bool) -> DescribedGroup {
        let refuse = |error: E, words: Option<String>| {
            let mut out = DescribedGroup::default();
            out.group_id = WireGroupId(text(name));
            out.error_code = error.code();
            out.error_message = words.map(StrBytes::from_string);
            out
        };
        if name.is_empty() {
            return refuse(E::InvalidGroupId, None);
        }
        let Ok(group) = GroupId::parse(name) else {
            return refuse(E::InvalidGroupId, None);
        };
        let view = match self.groups.describe(&group).await {
            Some(d) => d
                .streams
                .map(|v| *v)
                .ok_or_else(|| format!("Group {name} is not a streams group.")),
            None => self.kept_streams_group(&group),
        };
        match view {
            Ok(view) => described(name, &view, with_description),
            Err(words) => refuse(E::GroupIdNotFound, Some(words)),
        }
    }

    /// A group not in memory: an empty streams group the broker had before it was started again is still one; else
    /// Kafka's words for a group of another kind, or none.
    fn kept_streams_group(&self, group: &GroupId) -> Result<StreamsView, String> {
        let kept = crate::blocking::run(|| self.meta.group_life(group))
            .ok()
            .flatten();
        match kept {
            Some(kafka_meta::GroupLife {
                streams: Some(s), ..
            }) => {
                let g = crate::groups::streams::StreamsGroup::restored(
                    crate::groups::streams::stored::kept_of(&s),
                );
                Ok(g.view())
            }
            Some(_) => Err(format!("Group {group} is not a streams group.")),
            None if crate::blocking::run(|| self.meta.group_offsets(group))
                .is_ok_and(|o| !o.is_empty()) =>
            {
                Err(format!("Group {group} is not a streams group."))
            }
            None => Err(format!("Group {group} not found.")),
        }
    }

    /// StreamsGroupTopologyDescriptionUpdate: READ on the group, then UNSUPPORTED_VERSION with Kafka's words, as a 4.4.0
    /// broker without `group.streams.topology.description.plugin.class` answers it.
    pub(crate) fn streams_group_topology_description_update(
        &self,
        caller: &Caller,
        req: &StreamsGroupTopologyDescriptionUpdateRequest,
    ) -> Outcome {
        let mut resp = StreamsGroupTopologyDescriptionUpdateResponse::default();
        if self
            .guard(caller)
            .group(Operation::Read, req.group_id.0.as_str())
        {
            resp.error_code = E::UnsupportedVersion.code();
            resp.error_message = Some(StrBytes::from_static_str(
                "The broker has no streams group topology description plugin configured.",
            ));
        } else {
            resp.error_code = E::GroupAuthorizationFailed.code();
        }
        Outcome::Respond(kafka_wire::api::Response::StreamsGroupTopologyDescriptionUpdate(resp))
    }
}
