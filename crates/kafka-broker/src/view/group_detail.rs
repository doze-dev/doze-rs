//! What the protocol hides about one group: its leader, the timeouts members asked for, when each was last heard, which
//! member a rebalance is waiting for, a consumer group's epochs, and the date the offsets sweep will take its commits.
//!
//! The group's own task answers (the same turn DescribeGroups is answered in), so the facts are one consistent look.
//! Nothing here changes a group: asking is a Describe, which no transition depends on.

use kafka_auth::authz::{operation, resource};
use kafka_storage::fs::Fs;
use kafka_wire::ids::GroupId;

use super::ConsoleView;
use crate::groups::{Description, State, View, first_expiry, members_of};
use crate::{Caller, blocking, clock};

/// Which protocol the group speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupKind {
    /// JoinGroup, SyncGroup and Heartbeat: the leader assigns.
    Classic,
    /// ConsumerGroupHeartbeat (KIP-848): the broker assigns.
    Consumer,
}

/// A rebalance running now (classic).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebalanceDetail {
    pub began_ago_ms: u64,
    pub remaining_ms: u64,
    /// The wait it was given.
    pub timeout_ms: u64,
    /// The members that have not joined again, by id.
    pub waiting_for: Vec<String>,
}

/// One member's hidden facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberDetail {
    pub id: String,
    pub instance: Option<String>,
    pub client_id: String,
    pub client_host: String,
    pub leader: bool,
    pub session_ms: u64,
    pub rebalance_ms: Option<u64>,
    pub heard_ago_ms: u64,
    pub rejoined: bool,
}

/// One committed offset and the date the sweep will take it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffsetExpiry {
    pub topic: String,
    pub partition: i32,
    pub committed_ms: i64,
    /// The first moment the sweep takes it; `None` while it will not (the group holds the topic, or may yet have members).
    pub expires_ms: Option<i64>,
}

/// The hidden facts of a group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupDetail {
    pub id: String,
    pub kind: GroupKind,
    /// Kafka's word: `Empty`, `PreparingRebalance`, `CompletingRebalance`, `Stable`, and for a consumer group `Assigning`,
    /// `Reconciling`.
    pub state: String,
    pub protocol_type: Option<String>,
    /// A classic group's chosen assignor, a consumer group's assignor.
    pub protocol: Option<String>,
    /// A classic group's generation, a consumer group's epoch.
    pub generation: i32,
    /// The epoch a consumer group's targets were computed for.
    pub assignment_epoch: Option<i32>,
    pub leader: Option<String>,
    pub rebalance: Option<RebalanceDetail>,
    pub members: Vec<MemberDetail>,
    /// When it lost its last member, where the broker recorded it.
    pub empty_since_ms: Option<i64>,
    /// `--offsets-retention`.
    pub offsets_retention_ms: i64,
    pub offsets: Vec<OffsetExpiry>,
    /// The moment of the look, milliseconds since the epoch.
    pub now_ms: i64,
}

impl<F: Fs> ConsoleView<'_, F> {
    /// The hidden facts of `group`, if `caller` may DESCRIBE it and the broker knows it (it has members, committed offsets
    /// or a record of having had members); `None` otherwise (the two are not told apart, as Kafka does not).
    pub async fn group_detail(&self, caller: &Caller, group: &str) -> Option<GroupDetail> {
        let b = self.broker;
        if !b
            .authz
            .allows(caller, resource::GROUP, group, operation::DESCRIBE)
        {
            return None;
        }
        let id = GroupId::parse(group).ok()?;
        let described = b.groups.describe(&id).await;
        let now_ms = clock::now().get();
        let (offsets, life) = blocking::run(|| {
            Ok::<_, kafka_meta::MetaError>((b.meta.group_offsets(&id)?, b.meta.group_life(&id)?))
        })
        .ok()?;
        if described.is_none() && offsets.is_empty() && life.is_none() {
            return None;
        }
        let retention_ms =
            i64::try_from(b.config.offsets_retention.as_millis()).unwrap_or(i64::MAX);
        let view = View {
            members: members_of(described.as_ref()),
            life: life.as_ref(),
            offsets: &offsets,
        };
        let firsts = first_expiry(&view, retention_ms, b.started_ms);
        let offsets = offsets
            .iter()
            .zip(firsts)
            .filter(|(o, _)| b.topic_allows(caller, o.topic.as_str(), operation::DESCRIBE))
            .map(|(o, expires_ms)| OffsetExpiry {
                topic: o.topic.as_str().to_owned(),
                partition: o.partition.get(),
                committed_ms: o.offset.committed_ms,
                expires_ms,
            })
            .collect();
        let mut detail = of(group, described.as_ref(), life.as_ref());
        detail.offsets_retention_ms = retention_ms;
        detail.offsets = offsets;
        detail.now_ms = now_ms;
        Some(detail)
    }
}

fn of(group: &str, d: Option<&Description>, life: Option<&kafka_meta::GroupLife>) -> GroupDetail {
    let mut out = GroupDetail {
        id: group.to_owned(),
        kind: GroupKind::Classic,
        state: "Empty".to_owned(),
        protocol_type: life
            .map(|l| l.protocol_type.clone())
            .filter(|p| !p.is_empty()),
        protocol: None,
        generation: 0,
        assignment_epoch: None,
        leader: None,
        rebalance: None,
        members: Vec::new(),
        empty_since_ms: life.and_then(|l| l.empty_since_ms),
        offsets_retention_ms: 0,
        offsets: Vec::new(),
        now_ms: 0,
    };
    let Some(d) = d else {
        // Not in memory: a consumer group of the new protocol is still one when it was last stopped empty.
        if let Some(c) = life.and_then(|l| l.consumer.as_ref()) {
            out.kind = GroupKind::Consumer;
            out.generation = c.epoch;
            out.assignment_epoch = Some(c.assignment_epoch);
            out.protocol = Some(c.assignor.clone());
        }
        return out;
    };
    out.protocol_type = d.protocol_type.clone().or(out.protocol_type);
    out.generation = d.inspect.generation;
    out.leader = d.inspect.leader.as_ref().map(ToString::to_string);
    if let Some(c) = &d.consumer {
        out.kind = GroupKind::Consumer;
        c.state.clone_into(&mut out.state);
        out.protocol = Some(c.assignor.to_owned());
        out.assignment_epoch = Some(c.assignment_epoch);
    } else {
        out.state = state_word(d.state).to_owned();
        out.protocol = d.protocol.clone();
    }
    out.members = d.inspect.members.iter().map(|m| member(d, m)).collect();
    out.rebalance = d.inspect.rebalance.as_ref().map(|r| RebalanceDetail {
        began_ago_ms: r.began_ago_ms,
        remaining_ms: r.remaining_ms,
        timeout_ms: r.timeout_ms,
        waiting_for: d
            .inspect
            .members
            .iter()
            .filter(|m| !m.rejoined)
            .map(|m| m.id.to_string())
            .collect(),
    });
    out
}

fn member(d: &Description, m: &crate::groups::MemberInspect) -> MemberDetail {
    let (instance, client_id, client_host) = d
        .members
        .iter()
        .find(|x| x.id == m.id)
        .map(|x| {
            (
                x.instance.as_ref().map(ToString::to_string),
                x.client_id.clone(),
                x.client_host.clone(),
            )
        })
        .or_else(|| {
            d.consumer
                .as_ref()
                .and_then(|c| c.members.iter().find(|x| x.id == m.id))
                .map(|x| {
                    (
                        x.instance.as_ref().map(ToString::to_string),
                        x.client_id.clone(),
                        x.client_host.clone(),
                    )
                })
        })
        .unwrap_or_default();
    MemberDetail {
        id: m.id.to_string(),
        instance,
        client_id,
        client_host,
        leader: d.inspect.leader.as_ref() == Some(&m.id),
        session_ms: m.session_ms,
        rebalance_ms: m.rebalance_ms,
        heard_ago_ms: m.heard_ago_ms,
        rejoined: m.rejoined,
    }
}

fn state_word(s: State) -> &'static str {
    match s {
        State::Empty => "Empty",
        State::PreparingRebalance => "PreparingRebalance",
        State::CompletingRebalance => "CompletingRebalance",
        State::Stable => "Stable",
    }
}
