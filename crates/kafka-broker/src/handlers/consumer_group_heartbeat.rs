//! ConsumerGroupHeartbeat (KIP-848): one request that joins, keeps on and leaves a consumer group, answered with
//! what the member is to own now. The checks of the request itself are Kafka's, in its order and with its words.

use kafka_storage::fs::Fs;
use kafka_wire::ResponseError as E;
use kafka_wire::StrBytes;
use kafka_wire::Uuid;
use kafka_wire::ids::{GroupId, InstanceId, MemberId};
use kafka_wire::messages::ConsumerGroupHeartbeatRequest;
use kafka_wire::messages::ConsumerGroupHeartbeatResponse;
use kafka_wire::messages::consumer_group_heartbeat_response::{Assignment, TopicPartitions};

use crate::groups::{Answer, Heartbeat, Req};
use crate::{Broker, Outcome};

/// The interval members are asked to heartbeat at. It also bounds how fast a handoff goes (a couple of intervals), so it
/// is short: this is a development broker.
const HEARTBEAT_INTERVAL_MS: i32 = 1000;
/// The longest member id, instance id, rack, topic name or pattern the broker keeps for as long as a member is one.
const MOST_BYTES: usize = 4096;

impl<F: Fs> Broker<F> {
    pub(crate) async fn consumer_group_heartbeat(
        &self,
        version: i16,
        client_id: &str,
        req: &ConsumerGroupHeartbeatRequest,
    ) -> Outcome {
        let typed = match check(version, client_id, req) {
            Ok(typed) => typed,
            Err((error, words)) => return Outcome::Respond(respond(&refusal(error, &words))),
        };
        let Ok(group) = GroupId::parse(req.group_id.0.as_str()) else {
            return Outcome::Respond(respond(&refusal(
                E::InvalidRequest,
                "the group id is not usable",
            )));
        };
        let answer = match self.groups.ask(&group, Req::ConsumerHeartbeat(typed)).await {
            Some(Answer::ConsumerHeartbeat(a)) => a,
            // The group went while this waited: it is not there.
            _ => crate::groups::HeartbeatAnswer::refused(
                E::GroupIdNotFound,
                format!("Group {group} not found."),
            ),
        };
        Outcome::Respond(respond(&answer))
    }
}

fn refusal(error: E, words: &str) -> crate::groups::HeartbeatAnswer {
    crate::groups::HeartbeatAnswer::refused(error, words)
}

type Refused = (E, String);

/// What is wrong with the request alone, in Kafka's order and words.
fn refusal_words(version: i16, r: &ConsumerGroupHeartbeatRequest) -> Option<&'static str> {
    let group = r.group_id.0.as_str();
    let instance = r.instance_id.as_ref().map(|i| i.as_str());
    // Kafka's order, and its meaning of empty: nothing but white space. A member id is asked for from version 1 (the
    // client makes it), and before that only after the first heartbeat.
    if (version >= 1 || r.member_epoch > 0 || r.member_epoch == -1)
        && r.member_id.as_str().trim().is_empty()
    {
        return Some("MemberId can't be empty.");
    }
    if group.trim().is_empty() {
        return Some("GroupId can't be empty.");
    }
    if GroupId::parse(group).is_err() {
        return Some("the group id is not usable");
    }
    if instance.is_some_and(|i| i.trim().is_empty()) {
        return Some("InstanceId can't be empty.");
    }
    if r.rack_id
        .as_ref()
        .is_some_and(|i| i.as_str().trim().is_empty())
    {
        return Some("RackId can't be empty.");
    }
    let joining = r.member_epoch == 0;
    if joining && r.rebalance_timeout_ms == -1 {
        return Some("RebalanceTimeoutMs must be provided in first request.");
    }
    if joining && r.topic_partitions.as_ref().is_none_or(|t| !t.is_empty()) {
        return Some("TopicPartitions must be empty when (re-)joining.");
    }
    if joining && r.subscribed_topic_names.is_none() && r.subscribed_topic_regex.is_none() {
        return Some(
            "Either SubscribedTopicNames or SubscribedTopicRegex must be non-null when (re-)joining.",
        );
    }
    if r.member_epoch == -2 && instance.is_none() {
        return Some("InstanceId can't be null.");
    }
    if r.member_epoch < -2 {
        return Some("MemberEpoch is invalid.");
    }
    None
}

/// Kafka's checks of the request alone, and the typed request.
fn check(
    version: i16,
    client_id: &str,
    r: &ConsumerGroupHeartbeatRequest,
) -> Result<Heartbeat, Refused> {
    let bad = |words: &str| Err((E::InvalidRequest, words.to_owned()));
    let group = r.group_id.0.as_str();
    let instance = r.instance_id.as_ref().map(|i| i.as_str());
    if let Some(words) = refusal_words(version, r) {
        return bad(words);
    }
    let names: Vec<String> = r
        .subscribed_topic_names
        .iter()
        .flatten()
        .map(|t| t.0.as_str().to_owned())
        .collect();
    if too_long(r, instance, &names) {
        return bad(
            "a member id, instance id, rack id, topic name or pattern is longer than this broker keeps",
        );
    }
    Ok(Heartbeat {
        group: group.to_owned(),
        member_id: MemberId::new(r.member_id.as_str()),
        version,
        epoch: r.member_epoch,
        instance: instance.map(InstanceId::new),
        rack: r.rack_id.as_ref().map(|s| s.as_str().to_owned()),
        rebalance_ms: r.rebalance_timeout_ms,
        subscribed: r.subscribed_topic_names.is_some().then_some(names),
        regex: r
            .subscribed_topic_regex
            .as_ref()
            .map(|s| s.as_str().to_owned()),
        assignor: r.server_assignor.as_ref().map(|s| s.as_str().to_owned()),
        owned: r.topic_partitions.as_ref().map(|t| {
            t.iter()
                .map(|tp| (tp.topic_id, tp.partitions.clone()))
                .collect()
        }),
        client_id: client_id.to_owned(),
    })
}

/// Whether anything the member names is longer than the broker keeps.
fn too_long(r: &ConsumerGroupHeartbeatRequest, instance: Option<&str>, names: &[String]) -> bool {
    let rack = r.rack_id.as_ref().map(|s| s.as_str());
    let regex = r.subscribed_topic_regex.as_ref().map(|s| s.as_str());
    r.member_id.len() > MOST_BYTES
        || [instance, rack, regex]
            .iter()
            .flatten()
            .any(|s| s.len() > MOST_BYTES)
        || names.iter().any(|n| n.len() > MOST_BYTES)
}

fn respond(a: &crate::groups::HeartbeatAnswer) -> kafka_wire::api::Response {
    let mut resp = ConsumerGroupHeartbeatResponse::default();
    if let Some((error, words)) = &a.error {
        resp.error_code = error.code();
        resp.error_message = Some(StrBytes::from_string(words.clone()));
    } else {
        resp.member_id = a
            .member_id
            .as_ref()
            .map(|m| StrBytes::from_string(m.to_string()));
        resp.member_epoch = a.epoch;
        // A leave is answered with no interval to keep.
        if a.epoch >= 0 {
            resp.heartbeat_interval_ms = HEARTBEAT_INTERVAL_MS;
        }
        resp.assignment = a.assignment.as_ref().map(|parts| {
            let mut out = Assignment::default();
            out.topic_partitions = by_topic(parts)
                .into_iter()
                .map(|(topic_id, partitions)| {
                    let mut tp = TopicPartitions::default();
                    tp.topic_id = topic_id;
                    tp.partitions = partitions;
                    tp
                })
                .collect();
            out
        });
    }
    kafka_wire::api::Response::ConsumerGroupHeartbeat(resp)
}

/// Partitions grouped by topic, topics in id order and partitions ascending (a set already is).
pub(super) fn by_topic(parts: &crate::groups::Partitions) -> Vec<(Uuid, Vec<i32>)> {
    let mut out: Vec<(Uuid, Vec<i32>)> = Vec::new();
    for (topic, partition) in parts {
        match out.last_mut() {
            Some((t, ps)) if t == topic => ps.push(*partition),
            _ => out.push((*topic, vec![*partition])),
        }
    }
    out
}
