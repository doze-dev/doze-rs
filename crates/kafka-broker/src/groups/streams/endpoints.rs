//! Where each member's tasks are, for interactive queries (Kafka 4.4.0's `EndpointToPartitionsManager`): one entry per
//! member with an endpoint, its active tasks and its standby and warm-up tasks as the partitions of the topics its
//! subtopologies read.

use std::collections::BTreeSet;

use super::Endpoint;
use super::member::Member;
use super::tasks::{Task, by_subtopology};
use super::topology::Topology;
use crate::groups::consumer::Topics;

/// One member's entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EndpointPartitions {
    pub(crate) endpoint: Endpoint,
    /// By topic, partitions in order.
    pub(crate) active: Vec<(String, Vec<i32>)>,
    pub(crate) standby: Vec<(String, Vec<i32>)>,
}

/// Task p of a subtopology is partition p of each topic it reads, where that topic has such a partition.
fn partitions(
    tasks: &BTreeSet<Task>,
    topology: &Topology,
    topics: &Topics,
) -> Vec<(String, Vec<i32>)> {
    let mut out = Vec::new();
    for (sub, ps) in by_subtopology(tasks) {
        let Some(s) = topology.subtopologies.get(sub) else {
            continue;
        };
        let inputs: BTreeSet<&str> = s.inputs().collect();
        for topic in inputs {
            let Some(count) = topics.get(topic).map(|t| t.partitions) else {
                continue;
            };
            let here: Vec<i32> = ps.iter().copied().filter(|p| *p < count).collect();
            if !here.is_empty() {
                out.push((topic.to_owned(), here));
            }
        }
    }
    out
}

fn entry(m: &Member, topology: &Topology, topics: &Topics) -> Option<EndpointPartitions> {
    let endpoint = m.endpoint.clone()?;
    let standby: BTreeSet<Task> = m
        .held
        .tasks
        .standby
        .union(&m.held.tasks.warmup)
        .cloned()
        .collect();
    Some(EndpointPartitions {
        endpoint,
        active: partitions(&m.held.tasks.active, topology, topics),
        standby: partitions(&standby, topology, topics),
    })
}

/// Every member's entry, the others in member order and `me` last.
pub(super) fn build<'a>(
    members: impl Iterator<Item = &'a Member>,
    me: &'a Member,
    topology: &Topology,
    topics: &Topics,
) -> Vec<EndpointPartitions> {
    members
        .filter(|m| m.id != me.id)
        .chain(std::iter::once(me))
        .filter_map(|m| entry(m, topology, topics))
        .collect()
}
