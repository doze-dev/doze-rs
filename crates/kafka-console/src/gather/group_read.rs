//! The raw reads behind the Groups pages: who the groups are, what each one's members hold, what each committed, and where
//! the logs end. Every answer is a protocol answer; a refusal or a missing answer leaves the thing out, never invents it.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use kafka_wire::api::Response;

use super::protocol;
use crate::backend::{Backend, requests};
use crate::model::{GroupMemberRow, GroupPart, GroupProtocol};

/// A group as ListGroups names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub id: String,
    pub protocol: GroupProtocol,
}

/// Every group the console may see, by id.
pub async fn listed(b: &Backend) -> Vec<Listed> {
    let Ok(Response::ListGroups(l)) = b.call(requests::list_groups()).await else {
        return Vec::new();
    };
    let mut out: Vec<Listed> = l
        .groups
        .iter()
        .map(|g| Listed {
            id: g.group_id.0.to_string(),
            protocol: if g.group_type.as_str() == "consumer" {
                GroupProtocol::Consumer
            } else {
                GroupProtocol::Classic
            },
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// A group as its members and the protocol describe it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Described {
    pub protocol: GroupProtocol,
    /// Kafka's state word.
    pub state: String,
    pub protocol_type: Option<String>,
    pub assignor: Option<String>,
    pub generation: i32,
    pub target_epoch: Option<i32>,
    pub members: Vec<GroupMemberRow>,
    /// The topics the members subscribe to, where the protocol says.
    pub subscribed: BTreeSet<String>,
}

fn parts_of(
    a: &kafka_wire::messages::consumer_group_describe_response::Assignment,
) -> Vec<GroupPart> {
    let mut out: Vec<GroupPart> = a
        .topic_partitions
        .iter()
        .flat_map(|t| {
            t.partitions.iter().map(|p| GroupPart {
                topic: t.topic_name.0.to_string(),
                partition: *p,
            })
        })
        .collect();
    out.sort();
    out
}

fn member_row(id: String, client_id: String, host: String) -> GroupMemberRow {
    GroupMemberRow {
        id,
        instance: None,
        client_id,
        host: host.strip_prefix('/').map_or(host.clone(), str::to_owned),
        leader: false,
        epoch: 0,
        session_ms: 0,
        rebalance_ms: None,
        heard_ago_ms: None,
        rejoined: true,
        holds: Vec::new(),
        target: Vec::new(),
    }
}

fn classic(g: &kafka_wire::messages::describe_groups_response::DescribedGroup) -> Described {
    let mut subscribed = BTreeSet::new();
    let members = g
        .members
        .iter()
        .map(|m| {
            let mut row = member_row(
                m.member_id.to_string(),
                m.client_id.to_string(),
                m.client_host.to_string(),
            );
            row.instance = m.group_instance_id.as_ref().map(ToString::to_string);
            row.holds = protocol::assignment(&m.member_assignment).unwrap_or_default();
            subscribed.extend(protocol::subscription(&m.member_metadata).unwrap_or_default());
            row
        })
        .collect();
    Described {
        protocol: GroupProtocol::Classic,
        state: g.group_state.to_string(),
        protocol_type: Some(g.protocol_type.to_string()).filter(|p| !p.is_empty()),
        assignor: Some(g.protocol_data.to_string()).filter(|p| !p.is_empty()),
        generation: 0,
        target_epoch: None,
        members,
        subscribed,
    }
}

fn consumer(
    g: &kafka_wire::messages::consumer_group_describe_response::DescribedGroup,
) -> Described {
    let mut subscribed = BTreeSet::new();
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
            row.holds = parts_of(&m.assignment);
            row.target = parts_of(&m.target_assignment);
            subscribed.extend(m.subscribed_topic_names.iter().map(|t| t.0.to_string()));
            row
        })
        .collect();
    Described {
        protocol: GroupProtocol::Consumer,
        state: g.group_state.to_string(),
        protocol_type: Some("consumer".to_owned()),
        assignor: Some(g.assignor_name.to_string()).filter(|p| !p.is_empty()),
        generation: g.group_epoch,
        target_epoch: Some(g.assignment_epoch),
        members,
        subscribed,
    }
}

/// These groups, described by whichever protocol knows them. A group neither describes (or the console may not describe) is
/// left out of the map.
pub async fn describe(b: &Backend, ids: &[String]) -> HashMap<String, Described> {
    let mut out = HashMap::new();
    if ids.is_empty() {
        return out;
    }
    let mut again: Vec<String> = Vec::new();
    if let Ok(Response::DescribeGroups(r)) = b.call(requests::describe_groups(ids)).await {
        for g in r.groups.iter().filter(|g| g.error_code == 0) {
            let id = g.group_id.0.to_string();
            if g.group_state.as_str() == "Dead" {
                again.push(id);
            } else {
                out.insert(id, classic(g));
            }
        }
    }
    if !again.is_empty()
        && let Ok(Response::ConsumerGroupDescribe(r)) =
            b.call(requests::consumer_group_describe(&again)).await
    {
        for g in r.groups.iter().filter(|g| g.error_code == 0) {
            out.insert(g.group_id.0.to_string(), consumer(g));
        }
    }
    out
}

/// What a group committed: `(topic, partition)` to the committed offset (never negative).
pub type Commits = BTreeMap<(String, i32), i64>;

/// The commits of these groups, one OffsetFetch per fifty. A group the broker answered with an error is left out.
pub async fn commits(b: &Backend, ids: &[String]) -> HashMap<String, Commits> {
    let mut out = HashMap::new();
    for chunk in ids.chunks(50) {
        let Ok(Response::OffsetFetch(r)) = b.call(requests::committed_of(chunk)).await else {
            continue;
        };
        for g in r.groups.iter().filter(|g| g.error_code == 0) {
            let mut c = Commits::new();
            for t in &g.topics {
                for p in t.partitions.iter().filter(|p| p.committed_offset >= 0) {
                    c.insert(
                        (t.name.0.to_string(), p.partition_index),
                        p.committed_offset,
                    );
                }
            }
            out.insert(g.group_id.0.to_string(), c);
        }
    }
    out
}

/// The partitions of every topic the console may describe (Metadata), by topic.
pub async fn partition_counts(b: &Backend) -> BTreeMap<String, usize> {
    super::meta::all(b)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|t| !t.internal)
        .map(|t| (t.name, t.partitions.len()))
        .collect()
}

/// Where each asked partition's log ends (`timestamp` -1) or starts (-2); a partition the broker would not answer is absent.
pub async fn log_bounds(
    b: &Backend,
    parts: &BTreeMap<String, BTreeSet<i32>>,
    timestamp: i64,
) -> HashMap<(String, i32), i64> {
    let mut out = HashMap::new();
    for (topic, ps) in parts {
        let asks: Vec<requests::Ask> = ps
            .iter()
            .map(|&partition| requests::Ask {
                partition,
                timestamp,
            })
            .collect();
        let Ok(Response::ListOffsets(r)) = b
            .call(requests::list_offsets(
                topic,
                &asks,
                requests::Isolation::ReadUncommitted,
            ))
            .await
        else {
            continue;
        };
        for t in &r.topics {
            for p in t.partitions.iter().filter(|p| p.error_code == 0) {
                out.insert((topic.clone(), p.partition_index), p.offset);
            }
        }
    }
    out
}
