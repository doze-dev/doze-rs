//! What DescribeGroups, ListGroups and DeleteGroups ask of a group.

use std::collections::BTreeSet;

use bytes::Bytes;
use kafka_wire::ResponseError;

use super::{Answer, Description, Effect, GroupMachine, MemberDescription, Millis, State, Token};
use crate::groups::subscription;

impl GroupMachine {
    pub(super) fn describe(&self, token: Token, now: Millis, out: &mut Vec<Effect>) {
        out.push(Effect::Reply(
            token,
            Answer::Describe(self.description(now)),
        ));
    }

    /// A group with members cannot be deleted; an empty one can, and the driver then lets it go.
    pub(super) fn delete(&self, token: Token, out: &mut Vec<Effect>) {
        let refused = (!self.members.is_empty()).then_some(ResponseError::NonEmptyGroup);
        out.push(Effect::Reply(token, Answer::Delete(refused)));
    }

    #[cfg_attr(not(feature = "inspect"), allow(unused_variables))]
    fn description(&self, now: Millis) -> Description {
        let chosen = self.protocol.as_deref();
        let members: Vec<MemberDescription> = self
            .members
            .values()
            .map(|m| MemberDescription {
                id: m.id.clone(),
                instance: m.instance.clone(),
                client_id: m.client_id.clone(),
                client_host: m.client_host.clone(),
                metadata: m
                    .protocols
                    .iter()
                    .find(|p| Some(p.name.as_str()) == chosen)
                    .map(|p| p.metadata.clone())
                    .unwrap_or_default(),
                assignment: m.assignment.clone(),
            })
            .collect();
        Description {
            state: self.state,
            protocol_type: self.protocol_type.clone(),
            protocol: self.protocol.clone(),
            subscribed: self.subscribed(&members),
            consumer: None,
            #[cfg(feature = "inspect")]
            inspect: self.inspect(now),
            members,
        }
    }

    /// The facts of the group the classic APIs do not say.
    #[cfg(feature = "inspect")]
    fn inspect(&self, now: Millis) -> crate::groups::Inspect {
        use crate::groups::inspect::{MemberInspect, RebalanceInspect};
        let ago = |then: Millis| now.0.saturating_sub(then.0);
        crate::groups::Inspect {
            generation: self.generation,
            leader: self.leader.clone(),
            rebalance: self.rebalance.as_ref().map(|r| RebalanceInspect {
                began_ago_ms: ago(r.began),
                remaining_ms: r.deadline.0.saturating_sub(now.0),
                timeout_ms: r.deadline.0.saturating_sub(r.began.0),
            }),
            members: self
                .members
                .values()
                .map(|m| MemberInspect {
                    id: m.id.clone(),
                    session_ms: m.session_ms,
                    rebalance_ms: Some(m.rebalance_ms),
                    heard_ago_ms: ago(m.heard),
                    rejoined: self.rebalance.is_none() || m.joined.is_some(),
                })
                .collect(),
        }
    }

    /// The topics the members subscribe to, if they are a consumer group and every member's can be read.
    fn subscribed(&self, members: &[MemberDescription]) -> Option<BTreeSet<String>> {
        if self.state == State::Empty {
            return Some(BTreeSet::new());
        }
        if self.protocol_type.as_deref() != Some("consumer") {
            return None;
        }
        let mut all = BTreeSet::new();
        for m in members {
            all.extend(subscription::topics(&Bytes::clone(&m.metadata))?);
        }
        Some(all)
    }
}
