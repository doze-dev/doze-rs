//! The heartbeat's request read into the group's terms, Kafka's checks of the request alone, and the answer written back.

use std::collections::{BTreeMap, BTreeSet};

use kafka_wire::ResponseError as E;
use kafka_wire::StrBytes;
use kafka_wire::ids::{InstanceId, MemberId};
use kafka_wire::messages::streams_group_heartbeat_request::{TaskIds, TaskOffset, TopicInfo, Topology};
use kafka_wire::messages::streams_group_heartbeat_response::{
    Endpoint as WireEndpoint, EndpointToPartitions, Status, TaskIds as AnswerTasks, TopicPartition,
};
use kafka_wire::messages::{StreamsGroupHeartbeatRequest, StreamsGroupHeartbeatResponse, TopicName};

use crate::Outcome;
use crate::groups::streams::topology;
use crate::groups::streams::{Endpoint, Task, Tasks, by_subtopology, from_wire};
use crate::groups::{StreamsAnswer, StreamsHeartbeat, StreamsTiming};
use crate::handlers::Client;

/// Every topic a topology names: sources, repartition sinks and sources, changelogs (KafkaApis' `requiredTopics`).
pub(super) fn topology_names(t: &Topology) -> BTreeSet<&str> {
    t.subtopologies
        .iter()
        .flat_map(|s| {
            s.source_topics
                .iter()
                .chain(&s.repartition_sink_topics)
                .map(|n| n.0.as_str())
                .chain(s.repartition_source_topics.iter().map(|i| i.name.0.as_str()))
                .chain(s.state_changelog_topics.iter().map(|i| i.name.0.as_str()))
        })
        .collect()
}

fn blank(s: Option<&StrBytes>) -> bool {
    s.is_some_and(|s| s.as_str().trim().is_empty())
}

/// Kafka's `throwIfStreamsGroupHeartbeatRequestIsInvalid`, in its order and words.
pub(super) fn check(r: &StreamsGroupHeartbeatRequest) -> Result<(), (E, String)> {
    let bad = |words: &str| Err((E::InvalidRequest, words.to_owned()));
    if r.topology.iter().flat_map(|t| &t.subtopologies).any(|s| !s.source_topic_regex.is_empty()) {
        return bad("Regular expressions for source topics are not supported yet.");
    }
    if r.member_id.as_str().trim().is_empty() {
        return bad("MemberId can't be empty.");
    }
    if r.group_id.0.as_str().trim().is_empty() {
        return bad("GroupId can't be empty.");
    }
    if blank(r.instance_id.as_ref()) {
        return bad("InstanceId can't be empty.");
    }
    if blank(r.rack_id.as_ref()) {
        return bad("RackId can't be empty.");
    }
    if r.member_epoch == 0 {
        joining(r)?;
    } else if r.member_epoch == -2 && r.instance_id.is_none() {
        return bad("InstanceId can't be null.");
    } else if r.member_epoch < -2 {
        return bad(&format!("MemberEpoch is {}, but must be greater than or equal to -2.", r.member_epoch));
    }
    let lists = [&r.active_tasks, &r.standby_tasks, &r.warmup_tasks];
    if lists.iter().any(|l| l.is_some()) && lists.iter().any(|l| l.is_none()) {
        return bad("If one task-type is non-null, all must be non-null.");
    }
    if r.member_epoch != 0 && r.topology.is_some() {
        return bad("Topology can only be provided when (re-)joining.");
    }
    Ok(())
}

fn joining(r: &StreamsGroupHeartbeatRequest) -> Result<(), (E, String)> {
    let bad = |words: &str| Err((E::InvalidRequest, words.to_owned()));
    if r.rebalance_timeout_ms == -1 {
        return bad("RebalanceTimeoutMs must be provided in first request.");
    }
    for (list, name) in [(&r.active_tasks, "ActiveTasks"), (&r.standby_tasks, "StandbyTasks"), (&r.warmup_tasks, "WarmupTasks")] {
        if list.as_ref().is_none_or(|l| !l.is_empty()) {
            return bad(&format!("{name} must be empty when (re-)joining."));
        }
    }
    let Some(t) = &r.topology else {
        return bad("Topology must be non-null when (re-)joining.");
    };
    for c in t.subtopologies.iter().flat_map(|s| &s.state_changelog_topics) {
        if c.partitions != 0 {
            return Err((
                E::StreamsInvalidTopology,
                format!(
                    "Changelog topic {} must have an undefined partition count, but it is set to {}.",
                    c.name.0.as_str(),
                    c.partitions
                ),
            ));
        }
    }
    Ok(())
}

fn info(i: &TopicInfo) -> topology::TopicInfo {
    topology::TopicInfo {
        name: i.name.0.to_string(),
        partitions: i.partitions,
        replication: i.replication_factor,
        configs: i.topic_configs.iter().map(|kv| (kv.key.to_string(), kv.value.to_string())).collect(),
    }
}

fn topology_of(t: &Topology) -> topology::Topology {
    let names = |v: &Vec<TopicName>| v.iter().map(|n| n.0.to_string()).collect();
    topology::Topology {
        epoch: t.epoch,
        subtopologies: t
            .subtopologies
            .iter()
            .map(|s| {
                let sub = topology::Subtopology {
                    sources: names(&s.source_topics),
                    repartition_sinks: names(&s.repartition_sink_topics),
                    repartition_sources: s.repartition_source_topics.iter().map(info).collect(),
                    changelogs: s.state_changelog_topics.iter().map(info).collect(),
                    copartition: s
                        .copartition_groups
                        .iter()
                        .map(|c| topology::Copartition {
                            sources: c.source_topics.clone(),
                            repartition_sources: c.repartition_source_topics.clone(),
                        })
                        .collect(),
                };
                (s.subtopology_id.to_string(), sub)
            })
            .collect(),
    }
}

fn tasks(l: &[TaskIds]) -> std::collections::BTreeSet<Task> {
    from_wire(l.iter().map(|t| (t.subtopology_id.as_str(), t.partitions.as_slice())))
}

fn offsets(l: Option<&Vec<TaskOffset>>) -> Option<Vec<(Task, i64)>> {
    l.map(|l| l.iter().map(|o| ((o.subtopology_id.to_string(), o.partition), o.offset)).collect())
}

/// The request in the group's terms (after [`check`], so the member id is not empty).
pub(super) fn typed(
    version: i16,
    client: &Client<'_>,
    r: &StreamsGroupHeartbeatRequest,
    timing: StreamsTiming,
) -> Option<StreamsHeartbeat> {
    let owned = match (&r.active_tasks, &r.standby_tasks, &r.warmup_tasks) {
        (Some(a), Some(s), Some(w)) => Some(Tasks {
            active: tasks(a),
            standby: tasks(s),
            warmup: tasks(w),
        }),
        _ => None,
    };
    Some(StreamsHeartbeat {
        group: r.group_id.0.to_string(),
        member_id: MemberId::new(r.member_id.as_str())?,
        epoch: r.member_epoch,
        version,
        endpoint_epoch: r.endpoint_information_epoch,
        instance: r.instance_id.as_ref().map(|i| InstanceId::new(i.as_str())),
        rack: r.rack_id.as_ref().map(ToString::to_string),
        rebalance_ms: r.rebalance_timeout_ms,
        topology: r.topology.as_ref().map(topology_of),
        owned,
        process: r.process_id.as_ref().map(ToString::to_string),
        endpoint: r.user_endpoint.as_ref().map(|e| Endpoint {
            host: e.host.to_string(),
            port: e.port,
        }),
        client_tags: r
            .client_tags
            .as_ref()
            .map(|t| t.iter().map(|kv| (kv.key.to_string(), kv.value.to_string())).collect::<BTreeMap<_, _>>()),
        offsets: offsets(r.task_offsets.as_ref()),
        end_offsets: offsets(r.task_end_offsets.as_ref()),
        shutdown: r.shutdown_application,
        client_id: client.id.to_owned(),
        client_host: client.host.to_owned(),
        timing,
    })
}

pub(super) fn respond_raw(mut resp: StreamsGroupHeartbeatResponse, error: E) -> Outcome {
    resp.error_code = error.code();
    Outcome::Respond(kafka_wire::api::Response::StreamsGroupHeartbeat(resp))
}

fn task_ids(tasks: &BTreeSet<Task>) -> Vec<AnswerTasks> {
    by_subtopology(tasks)
        .into_iter()
        .map(|(sub, partitions)| {
            let mut t = AnswerTasks::default();
            t.subtopology_id = StrBytes::from_string(sub.to_owned());
            t.partitions = partitions;
            t
        })
        .collect()
}

fn topic_partitions(list: &[(String, Vec<i32>)]) -> Vec<TopicPartition> {
    list.iter()
        .map(|(topic, partitions)| {
            let mut tp = TopicPartition::default();
            tp.topic = TopicName(StrBytes::from_string(topic.clone()));
            tp.partitions.clone_from(partitions);
            tp
        })
        .collect()
}

pub(super) fn respond(a: &StreamsAnswer) -> Outcome {
    let mut resp = StreamsGroupHeartbeatResponse::default();
    if let Some((error, words)) = &a.error {
        resp.error_message = Some(StrBytes::from_string(words.clone()));
        return respond_raw(resp, *error);
    }
    resp.member_id = a.member_id.as_ref().map_or_else(StrBytes::default, |m| StrBytes::from_string(m.to_string()));
    resp.member_epoch = a.epoch;
    resp.heartbeat_interval_ms = a.heartbeat_ms;
    resp.task_offset_interval_ms = a.task_offset_ms;
    resp.acceptable_recovery_lag = a.acceptable_lag;
    resp.status = Some(
        a.status
            .iter()
            .map(|(code, detail)| {
                let mut s = Status::default();
                s.status_code = *code;
                s.status_detail = StrBytes::from_string(detail.clone());
                s
            })
            .collect(),
    );
    if let Some(t) = &a.tasks {
        resp.active_tasks = Some(task_ids(&t.active));
        resp.standby_tasks = Some(task_ids(&t.standby));
        resp.warmup_tasks = Some(task_ids(&t.warmup));
    }
    resp.endpoint_information_epoch = a.endpoint_epoch;
    resp.partitions_by_user_endpoint = a.endpoints.as_ref().map(|list| {
        list.iter()
            .map(|e| {
                let mut out = EndpointToPartitions::default();
                let mut endpoint = WireEndpoint::default();
                endpoint.host = StrBytes::from_string(e.endpoint.host.clone());
                endpoint.port = e.endpoint.port;
                out.user_endpoint = endpoint;
                out.active_partitions = topic_partitions(&e.active);
                out.standby_partitions = topic_partitions(&e.standby);
                out
            })
            .collect()
    });
    Outcome::Respond(kafka_wire::api::Response::StreamsGroupHeartbeat(resp))
}
