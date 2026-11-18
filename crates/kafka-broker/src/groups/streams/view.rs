//! A streams group as StreamsGroupDescribe, ListGroups, the offset APIs and the console see it.

use std::collections::BTreeMap;

use kafka_wire::ids::{InstanceId, MemberId};

use super::reconcile::Stage;
use super::topology::Topology;
use super::{Endpoint, StreamsGroup, Task, Tasks};
#[cfg(feature = "inspect")]
use crate::groups::machine::Millis;

/// A member as StreamsGroupDescribe shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StreamsMemberView {
    pub(crate) id: MemberId,
    pub(crate) epoch: i32,
    pub(crate) instance: Option<InstanceId>,
    pub(crate) rack: Option<String>,
    pub(crate) client_id: String,
    pub(crate) client_host: String,
    pub(crate) topology_epoch: i32,
    pub(crate) process: String,
    pub(crate) endpoint: Option<Endpoint>,
    pub(crate) client_tags: BTreeMap<String, String>,
    pub(crate) offsets: BTreeMap<Task, i64>,
    pub(crate) end_offsets: BTreeMap<Task, i64>,
    /// What it holds.
    pub(crate) assignment: Tasks,
    /// What it is to hold.
    pub(crate) target: Tasks,
    /// What it has been told to give up and has not said it has.
    pub(crate) revoking: Tasks,
    pub(crate) stage: Stage,
    /// The epoch each active task (held or being given up) was given at.
    pub(crate) given_at: BTreeMap<Task, i32>,
}

/// A streams group as StreamsGroupDescribe and ListGroups show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StreamsView {
    /// `Empty`, `NotReady`, `Assigning`, `Reconciling` or `Stable`.
    pub(crate) state: &'static str,
    pub(crate) epoch: i32,
    pub(crate) assignment_epoch: i32,
    /// The topology, its internal topics' partitions as worked out when it is ready (else as the client sent them).
    pub(crate) topology: Option<Topology>,
    /// Why the topology is not ready: a status code and its words.
    pub(crate) not_ready: Option<(i8, String)>,
    pub(crate) members: Vec<StreamsMemberView>,
}

impl StreamsGroup {
    /// The group's state (Kafka's `StreamsGroup.maybeUpdateGroupState`).
    #[must_use]
    pub(crate) fn state(&self) -> &'static str {
        if self.members.is_empty() {
            "Empty"
        } else if self
            .topology
            .as_ref()
            .is_none_or(|t| t.epoch != self.validated_epoch)
        {
            "NotReady"
        } else if self.epoch > self.assignment_epoch {
            "Assigning"
        } else if self
            .members
            .values()
            .any(|m| m.held.stage != Stage::Stable || m.held.epoch != self.assignment_epoch)
        {
            "Reconciling"
        } else {
            "Stable"
        }
    }

    /// The topology as described: when it is ready, each internal topic with the partitions worked out for it.
    fn described_topology(&self) -> Option<Topology> {
        let mut t = self.topology.clone()?;
        let Some(c) = self.configured.as_ref().filter(|c| c.ready()) else {
            return Some(t);
        };
        for (id, s) in &mut t.subtopologies {
            let Some(cs) = c.subtopologies.get(id) else { continue };
            for info in s.repartition_sources.iter_mut().chain(s.changelogs.iter_mut()) {
                if let Some((_, n)) = cs.internal.iter().find(|(name, _)| *name == info.name) {
                    info.partitions = *n;
                }
            }
        }
        Some(t)
    }

    #[must_use]
    pub(crate) fn view(&self) -> StreamsView {
        StreamsView {
            state: self.state(),
            epoch: self.epoch,
            assignment_epoch: self.assignment_epoch,
            topology: self.described_topology(),
            not_ready: self
                .configured
                .as_ref()
                .and_then(|c| c.not_ready.as_ref())
                .map(|n| (n.code, n.detail.clone())),
            members: self
                .members
                .values()
                .map(|m| StreamsMemberView {
                    id: m.id.clone(),
                    epoch: m.held.epoch,
                    instance: m.instance.clone(),
                    rack: m.rack.clone(),
                    client_id: m.client_id.clone(),
                    client_host: m.client_host.clone(),
                    topology_epoch: m.topology_epoch,
                    process: m.process.clone(),
                    endpoint: m.endpoint.clone(),
                    client_tags: m.client_tags.clone(),
                    offsets: m.offsets.clone(),
                    end_offsets: m.end_offsets.clone(),
                    assignment: m.held.tasks.clone(),
                    target: self.targets.get(&m.id).cloned().unwrap_or_default(),
                    revoking: m.held.revoking.clone(),
                    stage: m.held.stage,
                    given_at: m.held.given_at.clone(),
                })
                .collect(),
        }
    }

    /// What the console is told of the group besides what StreamsGroupDescribe says.
    #[cfg(feature = "inspect")]
    pub(crate) fn inspect(&self, now: Millis) -> crate::groups::Inspect {
        crate::groups::Inspect {
            generation: self.epoch,
            leader: None,
            rebalance: None,
            members: self
                .members
                .values()
                .map(|m| crate::groups::MemberInspect {
                    id: m.id.clone(),
                    session_ms: m.session_ms,
                    rebalance_ms: u64::try_from(m.rebalance_ms).ok(),
                    heard_ago_ms: now.0.saturating_sub(m.heard.0),
                    rejoined: true,
                })
                .collect(),
        }
    }
}
