//! A streams group (KIP-1071) on the Groups pages: StreamsGroupDescribe's answer as the members' rows (a member reads, at
//! partition p, each topic its active task p's subtopology reads), and `ConsoleView::streams_group`'s as the page's own part.

use std::collections::{BTreeMap, BTreeSet};

use kafka_broker::{StreamsGroupView, StreamsTasks, StreamsTopic};
use kafka_wire::messages::streams_group_describe_response::{DescribedGroup, TaskIds, Topology};

use super::group_read::{Described, member_row};
use crate::model::{
    GroupPart, GroupProtocol, GroupStreamsView, StreamsInternalTopic, StreamsMemberRow,
    StreamsNotReady, StreamsRoles, StreamsSubtopologyRow, StreamsTopicKind,
};

/// The topics each subtopology's tasks read (its sources and repartition sources), by subtopology id.
fn inputs(t: Option<&Topology>) -> BTreeMap<String, Vec<String>> {
    t.and_then(|t| t.subtopologies.as_ref())
        .into_iter()
        .flatten()
        .map(|s| {
            let read = s
                .source_topics
                .iter()
                .map(|n| n.0.to_string())
                .chain(
                    s.repartition_source_topics
                        .iter()
                        .map(|i| i.name.0.to_string()),
                )
                .collect();
            (s.subtopology_id.to_string(), read)
        })
        .collect()
}

/// The partitions active tasks read.
fn parts(tasks: &[TaskIds], inputs: &BTreeMap<String, Vec<String>>) -> Vec<GroupPart> {
    let mut out: Vec<GroupPart> = tasks
        .iter()
        .flat_map(|t| {
            let topics = inputs.get(t.subtopology_id.as_str());
            t.partitions.iter().flat_map(move |p| {
                topics.into_iter().flatten().map(move |topic| GroupPart {
                    topic: topic.clone(),
                    partition: *p,
                })
            })
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

/// A streams group as StreamsGroupDescribe tells it.
#[must_use]
pub fn described(g: &DescribedGroup) -> Described {
    let inputs = inputs(g.topology.as_ref());
    let members = g
        .members
        .iter()
        .map(|m| {
            let mut row = member_row(
                m.member_id.to_string(),
                m.client_id.to_string(),
                m.client_host.to_string(),
            );
            row.instance = m.instance_id.as_ref().map(ToString::to_string);
            row.epoch = m.member_epoch;
            row.holds = parts(&m.assignment.active_tasks, &inputs);
            row.target = parts(&m.target_assignment.active_tasks, &inputs);
            row
        })
        .collect();
    Described {
        protocol: GroupProtocol::Streams,
        state: g.group_state.to_string(),
        protocol_type: Some("streams".to_owned()),
        assignor: g.assignor_name.as_ref().map(ToString::to_string),
        generation: g.group_epoch,
        target_epoch: Some(g.assignment_epoch),
        members,
        subscribed: inputs.into_values().flatten().collect::<BTreeSet<_>>(),
    }
}

/// Kafka's name of a topology status (`StreamsGroupHeartbeatResponse.Status`).
fn status_name(code: i8) -> &'static str {
    match code {
        0 => "STALE_TOPOLOGY",
        1 => "MISSING_SOURCE_TOPICS",
        2 => "INCORRECTLY_PARTITIONED_TOPICS",
        3 => "MISSING_INTERNAL_TOPICS",
        4 => "SHUTDOWN_APPLICATION",
        5 => "ASSIGNMENT_DELAYED",
        _ => "UNKNOWN",
    }
}

fn task_ids(tasks: &[(String, i32)]) -> Vec<String> {
    tasks.iter().map(|(s, p)| format!("{s}_{p}")).collect()
}

fn roles(t: &StreamsTasks) -> StreamsRoles {
    StreamsRoles {
        active: task_ids(&t.active),
        standby: task_ids(&t.standby),
        warmup: task_ids(&t.warmup),
    }
}

fn internal(
    t: &StreamsTopic,
    kind: StreamsTopicKind,
    sub: &str,
    counts: &BTreeMap<String, usize>,
) -> StreamsInternalTopic {
    StreamsInternalTopic {
        name: t.name.clone(),
        kind,
        subtopology: sub.to_owned(),
        partitions: Some(t.partitions).filter(|n| *n > 0),
        exists: counts.get(&t.name).copied(),
        configs: t.configs.clone(),
    }
}

fn names(list: &[StreamsTopic]) -> Vec<String> {
    list.iter().map(|t| t.name.clone()).collect()
}

/// The page's part from the broker's view of the group; `counts` (Metadata's partitions by topic) says which internal
/// topics exist.
#[must_use]
pub fn streams_view(v: &StreamsGroupView, counts: &BTreeMap<String, usize>) -> GroupStreamsView {
    let mut internals = Vec::new();
    for s in &v.subtopologies {
        for t in &s.repartition_sources {
            internals.push(internal(t, StreamsTopicKind::Repartition, &s.id, counts));
        }
        for t in &s.changelogs {
            internals.push(internal(t, StreamsTopicKind::Changelog, &s.id, counts));
        }
    }
    GroupStreamsView {
        state: v.state.to_owned(),
        epoch: v.epoch,
        assignment_epoch: v.assignment_epoch,
        topology_epoch: v.topology_epoch,
        not_ready: v.not_ready.as_ref().map(|(code, detail)| StreamsNotReady {
            code: *code,
            name: status_name(*code).to_owned(),
            detail: detail.clone(),
        }),
        subtopologies: v
            .subtopologies
            .iter()
            .map(|s| StreamsSubtopologyRow {
                id: s.id.clone(),
                sources: s.sources.clone(),
                repartition_sinks: s.repartition_sinks.clone(),
                repartition_sources: names(&s.repartition_sources),
                changelogs: names(&s.changelogs),
                tasks: s.tasks,
            })
            .collect(),
        internal: internals,
        members: v
            .members
            .iter()
            .map(|m| StreamsMemberRow {
                id: m.id.clone(),
                process: m.process.clone(),
                client_id: m.client_id.clone(),
                host: m.client_host.trim_start_matches('/').to_owned(),
                endpoint: m.endpoint.clone(),
                instance: m.instance.clone(),
                rack: m.rack.clone(),
                epoch: m.epoch,
                topology_epoch: m.topology_epoch,
                stage: m.stage.to_owned(),
                holds: roles(&m.assignment),
                target: roles(&m.target),
                revoking: roles(&m.revoking),
            })
            .collect(),
        live: v.live,
    }
}
